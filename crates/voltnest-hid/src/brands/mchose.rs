//! 品牌：迈从 MCHOSE。
//!
//! 依据：官方 M HUB 网页驱动（`mchose.com.cn`，`assets/purify.es-*.js` + `assets/index-*.js`，
//! 2026-10 线上版本）的反混淆阅读。**官方按设备类别走四条互不相同的通道**，本模块据此拆 spec：
//!
//! | 类别 | 官方通道 | 本模块 |
//! |---|---|---|
//! | 鼠标 | feature 查询 `11 06`（应答逐字节取反）+ 输入报告实时刷新 | [`MOUSE_WIRED`] / [`MOUSE_WIRELESS`]（仅监听帧布局不同） |
//! | 键盘 | `dJe` 家族：有线 feature `06 87`（520B）、2.4G output `13 4A` + 校验和 + 多包重组 | [`KEYBOARD_WIRED`] / [`KEYBOARD_WIRELESS`] |
//! | 耳机 / 音箱 | 输出报告 `0x55` + 命令 `0x65`，应答是输入报告 | [`AUDIO`] |
//! | G 系列鼠标 | 客户端注入 iframe `#gSeries`，官方在 `LG()` / `isG3Mouse()` 里直接跳过 WebHID | 仍按键鼠通道读（官方不走这条，是否应答 `0x06` 待实测；不按名称猜排除） |
//! | 扩展坞 / 未知 | 无电量上报 | 不采集 |
//!
//! # 证据（字节级）
//! 1. 查询报文 `jat = "11 06 " + 19×"00"`，发送前报告 ID 之后逐字节取反 — `LG()`：
//!    `order.slice(3).split(" ").map(x => "0x"+x ^ 255)`。
//! 2. 应答解析器 `Pat`（binary-parser，输入是应答去掉前 2 字节并逐字节取反）：
//!    `uint16 vid · uint16 pid · uint32 fwVersion · bit3 connectMode · bit1 connectStatus ·
//!    bit4 inReserved · uint8 batteryLevel · uint8 chargeStatus`。
//!    → payload 偏移：`[0..2)` vid、`[2..4)` pid、`[4..8)` fwVersion、`[8]` 连接态位域、
//!    `[9]` 电量、`[10]` 充电状态。官方只在 `chargeStatus === 1` 时显示「充电中」。
//! 3. 实时上报解析器 `q3e(isWireless)`：`uint8 command` →（无线再 3 字节 space）→
//!    `uint8 type · uint8 batteryLevel · bit4 fullStatus · bit4 chargeStatus`；
//!    消费者只在 `command === 10 && type === 5` 时采用，`fullStatus` 非零则电量按 100 显示。
//!    → 有线 `[0]=0x0A [1]=type [2]=电量 [3]=状态`；无线 `[0]=0x0A [1..4)=空 [4]=type [5]=电量 [6]=状态`。
//! 4. 音频：`D8t()` 发 `sendReport(0x55, [0x65, 0x01, 61×0x00])`，输入报告 `0x55` 以 `0x65` 开头，
//!    `data[1]=电量`、`data[2]=状态`（`2=放电 3=充电 4=满电 26=休眠`）。
//! 5. 键盘命令表 `Rpe`（`new Zpe(Rpe)` → `my`）：
//!    `getBattery.wiredCommand = "06 87 00 00 01 00 02 00" + 512×00`（报告 ID `0x06`、命令 `0x87`）、
//!    `wirelessCommand = "13 4a 01" + 16×00`（报告 ID `0x13`、命令 `0x4A`）。
//!    `dJe.readCommand` 有线走 `sendFeatureReport(0x06, cmd.slice(3))` + `receiveFeatureReport(0x06)`，
//!    应答 `slice(2)` 后按 `bae(true)` = 跳 6 字节 → 电量 → `bit4 fullStatus · bit4 chargeStatus`；
//!    无线走 `sendReport(0x13, 命令+校验和)`，应答由 `handleCommandResponse` 按
//!    `commonParser{commandParams,total,current}` 逐包拼接，`t.slice(4,-1)` 才是载荷，
//!    拼完按 `bae(false)` = 电量 → 状态。
//! 6. 集合：官方 WebHID 过滤器（`Z1e` / `R1e`）对同一型号同时声明 `usagePage 0xFF01` 与 `0xFF0B`。
//!
//! # 未接入（有据可查但需另做）
//! - **键盘的 520 字节特征报告未经实机验证**：官方模板是「报告 ID + 519 字节」，超过了
//!   常见的 64 字节特征报告上限，能否 `send_feature_report` 取决于该集合在报告描述符里
//!   声明的长度；`hidapi` 若拒绝，会表现为发送失败 → 整轮不可读（不会读出错值）。
//! - **音频例外型号 `V9 PRO 2` / `R9`**：官方 `v6(productName)` 命中后走 `HJe()` 的另一种帧
//!   （`Gd/Mpe`：`FJe=0xAA` 起始、`Zc=4` 命令位、XOR 校验和），电量在
//!   `[sleepState, (charging<<7)|percent]`（`Epe/Gpe`）。需要另做一套带校验和的帧，
//!   [`route`] 对这两个型号返回 `None`：不拿 0x65 帧去猜。
//! - **`canGetBattery` 门控**：官方按产品目录的 `showBattery` / `enableBatteryVersion`
//!   （对比接收器固件版本 `0xAA` 特征报告）决定是否查电量。目录字段来自后端 `fetchData`，
//!   型号表里没有，故本项目对音频设备一律尝试查询：不应答即上报不可读，不假装有数据。

use crate::brands::checksum;
use crate::spec::{
    BatteryFields, BrandSpec, ChargingField, FrameLayout, ListenSpec, Reassembly, ReplyTransport,
    RequestTemplate,
};
use crate::table::Entry;

const MCHOSE_REQUEST_PAYLOAD: [u8; 19] = [0; 19];

/// 键盘（有线）查询载荷：官方 `wiredCommand` 去掉报告 ID `0x06` 与命令字 `0x87` 之后的部分。
///
/// 官方模板是 `` `06 87 00 00 01 00 02 00 ${"00 ".repeat(512).trim()}` ``：`0x06` 是报告 ID
/// （`readCommand` 里用 `wiredCommand.slice(3)` 砍掉它，因为 WebHID 的 `sendFeatureReport`
/// 把报告 ID 作为独立入参），`0x87` 是命令，后面 6 字节是命令参数，再补 512 个 0。
const KEYBOARD_WIRED_PAYLOAD: [u8; 6] = [0x00, 0x00, 0x01, 0x00, 0x02, 0x00];

/// 键盘（2.4G）查询载荷：官方 `wirelessCommand` 去掉报告 ID `0x13` 与命令字 `0x4A` 之后的部分，
/// 末尾追加官方 XOR 校验和。
///
/// 官方 `tJe` = 「各字节十六进制求和」⇒ `(0x13 + 0x4A + 0x01) & 0xFF = 0x5E`。求和**包含报告 ID**，
/// 这一点容易看漏（`tJe(o)` 的入参是带 `13` 的完整命令串）。载荷共 18 字节：
/// `01 00×16 5E`，加上报告 ID 与命令字正好 20 字节一条 output report。
const KEYBOARD_WIRELESS_PAYLOAD: [u8; 18] = {
    let mut payload = [0u8; 18];
    payload[0] = 0x01;
    payload[17] = checksum::sum8(&[0x13, 0x4A, payload[0]]);
    payload
};

/// 音频查询载荷：`[0x65, 0x01, 61×0x00]` 去掉命令字后的部分（命令字由模板写入）。
const AUDIO_REQUEST_PAYLOAD: [u8; 62] = {
    let mut payload = [0u8; 62];
    payload[0] = 0x01;
    payload
};

/// 鼠标（有线）：`0xFF01` feature 查询，应答取反，输入报告实时刷新。
pub const MOUSE_WIRED: BrandSpec = BrandSpec {
    id: "mchose-mouse-wired",
    label: "MCHOSE（鼠标 · 有线）",
    usage_page: 0xFF01,
    // 官方 WebHID 过滤器对同一型号同时声明 `0xFF01` / `0xFF0B`（`R1e`）。
    usage_page_alt: Some(0xFF0B),
    usage: None,
    usage_page_write: Some(0xFF01),
    reassembly: None,
    transport: ReplyTransport::FeatureQuery { reply_len: 21 },
    reply_inverted: true,
    clamp_percent: false,
    frame: FrameLayout {
        report_id: Some(0x11),
        cmd_index: 1,
        len_index: None,
        payload_offset: 2,
        fixed_payload_len: 11,
    },
    battery: BatteryFields {
        cmd: Some(0x06),
        subtag: None,
        percent_index: 9,
        charging: Some(ChargingField {
            mask: 0xFF,
            index: 10,
            true_values: &[0x01],
            error_from: None,
        }),
    },
    // 官方 `q3e(true)`：command → type → 电量 → 状态。
    listen: Some(ListenSpec {
        cmd: 0x0A,
        subtag_index: 1,
        subtag_value: 5,
        percent_index: 2,
        status_index: 3,
        charging_low_nibble: 0x01,
    }),
    request: Some(RequestTemplate {
        usage_page: 0xFF01,
        cmd: 0x06,
        prefix: &[],
        report_len: 21,
        cmd_index: 1,
        payload_offset: 2,
        payload: &MCHOSE_REQUEST_PAYLOAD,
        interval_ms: Some(30_000),
        inverted: true,
        // 官方 `LG()` 在命令字回显不符时最多重读 6 次（600ms 窗口）。
        retry: 6,
    }),
    // 该固件无独立确认位约定：命令字回显（取自应答 `[1]` 取反）就是确认。
    ack: None,
    fallback_report_id: None,
    expected_interval_ms: 30_000,
    device_table_json: include_str!("../../data/mappings/mchose.json"),
    variants: &[],
    route: None,
};

/// 鼠标（2.4G）：查询与解析与有线完全一致，只有实时上报帧前面多 3 字节。
pub const MOUSE_WIRELESS: BrandSpec = BrandSpec {
    id: "mchose-mouse-wireless",
    label: "MCHOSE（鼠标 · 2.4G）",
    listen: Some(ListenSpec {
        cmd: 0x0A,
        subtag_index: 4,
        subtag_value: 5,
        percent_index: 5,
        status_index: 6,
        charging_low_nibble: 0x01,
    }),
    ..MOUSE_WIRED
};

/// 耳机 / 音箱：输出报告 `0x55` + 命令 `0x65`，应答走输入报告。
///
/// `usage_page: 0` 是通配 —— 官方用 `collections[].outputReports[].reportId === 0x55`
/// 选集合，hidapi 的枚举结果里没有报告 ID 清单。
pub const AUDIO: BrandSpec = BrandSpec {
    id: "mchose-audio",
    label: "MCHOSE（耳机 / 音箱）",
    usage_page: 0,
    usage_page_alt: None,
    usage: None,
    usage_page_write: Some(0),
    reassembly: None,
    transport: ReplyTransport::InterruptQuery,
    reply_inverted: false,
    clamp_percent: false,
    frame: FrameLayout {
        report_id: Some(0x55),
        cmd_index: 1,
        len_index: None,
        payload_offset: 2,
        fixed_payload_len: 2,
    },
    battery: BatteryFields {
        cmd: Some(0x65),
        subtag: None,
        percent_index: 0,
        charging: Some(ChargingField {
            mask: 0xFF,
            // 2=放电 3=充电 4=满电 26=休眠（休眠时电量字段无意义，按错误态拒绝）。
            index: 1,
            true_values: &[0x03, 0x04],
            error_from: Some(0x1A),
        }),
    },
    listen: None,
    request: Some(RequestTemplate {
        usage_page: 0,
        cmd: 0x65,
        prefix: &[],
        // 报告 ID + 63 字节载荷（`[0x65, 0x01, 61×0x00]`）。
        report_len: 64,
        cmd_index: 1,
        payload_offset: 2,
        payload: &AUDIO_REQUEST_PAYLOAD,
        interval_ms: Some(30_000),
        inverted: false,
        // 音频设备不重发：官方只在充电状态变化后 32s 补查一次，重发会把休眠中的耳机叫醒。
        retry: 0,
    }),
    ack: None,
    fallback_report_id: None,
    expected_interval_ms: 30_000,
    device_table_json: include_str!("../../data/mappings/mchose.json"),
    variants: &[],
    route: None,
};

/// 走另一种帧的音频型号（官方 `QKt` 表：`V9 PRO 2` / `R9`）。
const ALT_AUDIO_MODELS: [&str; 2] = ["V9 PRO 2", "R9"];

/// 键盘（有线）：官方 `dJe.isWired` 分支 —— 520 字节 feature 报告，报告 ID `0x06`，命令 `0x87`。
///
/// 应答 `slice(2)` 后按 `bae(true)` 解析：跳 6 字节 → 电量 → 状态（高半字节满电、低半字节充电）。
/// 官方 `readCommand` **不校验命令字回显**（与鼠标 `LG()` 不同），故 `battery.cmd: None`：
/// 多校验一条没有证据的字节，只会在别的固件版本上把可用读数判成坏帧。
pub const KEYBOARD_WIRED: BrandSpec = BrandSpec {
    id: "mchose-keyboard-wired",
    label: "MCHOSE（键盘 · 有线）",
    usage_page: 0xFF01,
    usage_page_alt: Some(0xFF0B),
    usage: None,
    usage_page_write: Some(0xFF01),
    reassembly: None,
    transport: ReplyTransport::FeatureQuery { reply_len: 520 },
    reply_inverted: false,
    clamp_percent: false,
    frame: FrameLayout {
        report_id: Some(0x06),
        cmd_index: 1,
        len_index: None,
        payload_offset: 2,
        // 6 字节头部 + 电量 + 状态：整段一起算 payload，0% 且未充电时才不会被误判成「空应答」。
        fixed_payload_len: 8,
    },
    battery: BatteryFields {
        cmd: None,
        subtag: None,
        percent_index: 6,
        charging: Some(ChargingField {
            // 状态字节低半字节 = chargeStatus，高半字节是 fullStatus，故按 0x0F 取位。
            mask: 0x0F,
            index: 7,
            true_values: &[0x01],
            error_from: None,
        }),
    },
    // 同一套实时上报帧（官方键盘页的消费者同样判 `command===10 && type===5`）。
    listen: Some(ListenSpec {
        cmd: 0x0A,
        subtag_index: 1,
        subtag_value: 5,
        percent_index: 2,
        status_index: 3,
        charging_low_nibble: 0x01,
    }),
    request: Some(RequestTemplate {
        usage_page: 0xFF01,
        cmd: 0x87,
        prefix: &[],
        // 报告 ID `0x06` + 519 字节载荷 = 官方那条 520 字节特征报告。
        report_len: 520,
        cmd_index: 1,
        payload_offset: 2,
        payload: &KEYBOARD_WIRED_PAYLOAD,
        interval_ms: Some(30_000),
        inverted: false,
        // 官方有线读是一次「发 + 收」，没有重问；重问只会拖长无响应设备的占用时间。
        retry: 0,
    }),
    ack: None,
    // 官方 `OE` 表里 6 个型号（G98 Pro / G98 V2 / UT98 / Z75 / Z75S / G87 V2）有线报告 ID 是
    // `0x09` 而不是 `0x06`。型号名对不上型号表，故不在 route 里按名字猜，交给运行时「主 → 备」探测。
    fallback_report_id: Some(0x09),
    expected_interval_ms: 30_000,
    device_table_json: include_str!("../../data/mappings/mchose.json"),
    variants: &[],
    route: None,
};

/// 键盘（2.4G）：官方 `dJe` 无线分支 —— 输出报告 `0x13` + 命令 `0x4A`，应答是多条输入报告。
///
/// 应答包布局（数据窗口，`[0]` 是命令字）：`[0]=0x4A [1]=总包数 [2]=序号 [3]=保留 [4..-1)=载荷
/// [-1]=校验和`。凑齐全部序号后把载荷按序拼起来，`[0]=电量`、`[1]=状态`。
pub const KEYBOARD_WIRELESS: BrandSpec = BrandSpec {
    id: "mchose-keyboard-wireless",
    label: "MCHOSE（键盘 · 2.4G）",
    usage_page: 0xFF01,
    usage_page_alt: Some(0xFF0B),
    usage: None,
    usage_page_write: Some(0xFF01),
    reassembly: Some(Reassembly {
        // 官方 `currentOrder = parseInt(wirelessCommand.split(" ")[1], 16)` = `0x4A`。
        cmd: 0x4A,
        total_index: 1,
        seq_index: 2,
        payload_offset: 4,
        // 官方 `t.slice(4, -1)`：最后一个字节是校验和，不进载荷。
        trailer_len: 1,
        timeout_ms: 10_000,
    }),
    transport: ReplyTransport::InterruptQuery,
    reply_inverted: false,
    clamp_percent: false,
    // 组装结果没有报告 ID / 命令字头部，故 payload 从 0 起算；报告 ID 仍声明，
    // 因为它是这条通道收发报文用的 ID（运行时按它过滤输入报告）。
    frame: FrameLayout {
        report_id: Some(0x13),
        cmd_index: 1,
        len_index: None,
        payload_offset: 2,
        fixed_payload_len: 2,
    },
    battery: BatteryFields {
        cmd: None,
        subtag: None,
        percent_index: 0,
        charging: Some(ChargingField {
            // 低半字节 = chargeStatus，高半字节是 fullStatus（查询结果里官方不使用满电标志，
            // 见 `frame::decode_assembled` 的说明）。
            mask: 0x0F,
            index: 1,
            true_values: &[0x01],
            error_from: None,
        }),
    },
    // 2.4G 的实时帧前面多 3 字节 `space0`（与鼠标一致）。
    listen: Some(ListenSpec {
        cmd: 0x0A,
        subtag_index: 4,
        subtag_value: 5,
        percent_index: 5,
        status_index: 6,
        charging_low_nibble: 0x01,
    }),
    request: Some(RequestTemplate {
        usage_page: 0xFF01,
        cmd: 0x4A,
        prefix: &[],
        // 报告 ID `0x13` + 19 字节载荷 = 20 字节 output report。
        report_len: 20,
        cmd_index: 1,
        payload_offset: 2,
        payload: &KEYBOARD_WIRELESS_PAYLOAD,
        interval_ms: Some(30_000),
        inverted: false,
        // 无线是「发一次 + 多包应答」，重发会打断正在拼的那一轮。
        retry: 0,
    }),
    ack: None,
    fallback_report_id: None,
    expected_interval_ms: 30_000,
    device_table_json: include_str!("../../data/mappings/mchose.json"),
    variants: &[],
    route: None,
};

/// 迈从自己的 `deviceType + mode` 分派。
///
/// 官方按「设备类别 + 是否 2.4G 接收器」选数据源与帧布局（`pJe(productName)` 判无线），
/// 型号表里的 `deviceType` / `mode` 正好是这两项。
fn route(entry: &Entry) -> Option<&'static BrandSpec> {
    let wireless = entry.mode.eq_ignore_ascii_case("2.4G");
    match entry.device_type.as_str() {
        // 键盘与鼠标是**两套互不兼容的协议**：键盘走 `dJe` 家族（有线 `06 87` / 2.4G `13 4A`），
        // 鼠标走 `LG` 家族（`11 06` 且应答逐字节取反）。
        "keyboard" => Some(if wireless {
            &KEYBOARD_WIRELESS
        } else {
            &KEYBOARD_WIRED
        }),
        "mouse" => Some(if wireless {
            &MOUSE_WIRELESS
        } else {
            &MOUSE_WIRED
        }),
        "headset" | "speaker" => {
            let name = entry.name.as_deref().unwrap_or_default();
            if ALT_AUDIO_MODELS.contains(&name) {
                // 另一种帧（Gd/Mpe + XOR 校验和）未接入：不拿 0x65 帧去猜。
                None
            } else {
                Some(&AUDIO)
            }
        }
        // 扩展坞等无电量上报。
        _ => None,
    }
}

/// 注册表登记的迈从协议描述（实际设备由 [`route`] 分派）。
pub const SPEC: BrandSpec = BrandSpec {
    id: "mchose",
    label: "MCHOSE（按设备类型分派）",
    variants: &[
        &MOUSE_WIRED,
        &MOUSE_WIRELESS,
        &AUDIO,
        &KEYBOARD_WIRED,
        &KEYBOARD_WIRELESS,
    ],
    route: Some(route),
    ..MOUSE_WIRED
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{decode_battery, decode_listen};
    use crate::table::DeviceTable;

    fn table() -> DeviceTable {
        DeviceTable::parse(SPEC.device_table_json).unwrap()
    }

    /// 有线应答：`[0]=0x11 [1]=~cmd [2..]=~payload`，电量 `payload[9]`、充电 `payload[10]`。
    fn wired_reply(percent: u8, charge: u8) -> Vec<u8> {
        let mut plain = vec![0u8; 13];
        plain[0] = 0x11;
        plain[1] = 0x06;
        plain[11] = percent;
        plain[12] = charge;
        let mut report = plain.clone();
        for byte in report.iter_mut().skip(1) {
            *byte ^= 0xFF;
        }
        report
    }

    #[test]
    fn parses_inverted_mchose_status() {
        let reading = decode_battery(&MOUSE_WIRED, &wired_reply(64, 0x00)).unwrap();
        assert_eq!(reading.percent.get(), 64);
        assert_eq!(reading.charging, Some(false));
        assert!(SPEC.validate().is_ok());
    }

    #[test]
    fn charge_status_means_charging_only_when_one() {
        let charging = decode_battery(&MOUSE_WIRED, &wired_reply(42, 0x01)).unwrap();
        assert_eq!(
            charging.charging,
            Some(true),
            "官方只在 chargeStatus===1 显示充电中"
        );
    }

    #[test]
    fn device_table_covers_all_vendor_ids() {
        let table = table();
        for vid in [0x3837, 0x5253, 0x41E4, 0x19F5, 0x0E6A, 0x291D] {
            assert!(table.known_vid(vid), "缺少 VID {vid:04X}");
        }
    }

    /// 官方 `LG()` 在命令字不符时最多重读 6 次。
    #[test]
    fn battery_query_retries_like_official_driver() {
        let req = MOUSE_WIRED.request.expect("键鼠必须声明查询模板");
        assert_eq!(req.retry, 6);
        assert_eq!(req.attempts(), 7);
    }

    /// 有线实时上报：`0A <type=5> <电量> <满电|充电>`；hidapi 会在首字节放报告 ID。
    #[test]
    fn decodes_wired_push_frame() {
        let with_id = [0x11, 0x0A, 0x05, 0x37, 0x01];
        let r = decode_listen(&MOUSE_WIRED, &with_id).expect("有线电量帧必须解析成功");
        assert_eq!(r.percent.get(), 55);
        assert_eq!(r.charging, Some(true));

        // 不带报告 ID 时同样成立（官方解析器的输入就是这种形态）。
        let bare = [0x0A, 0x05, 0x37, 0x00];
        assert_eq!(decode_listen(&MOUSE_WIRED, &bare).unwrap().charging, Some(false));
    }

    /// 无线实时上报：多 3 字节 `space0`，type/电量/状态整体后移。
    #[test]
    fn decodes_wireless_push_frame() {
        let frame = [0x11, 0x0A, 0x00, 0x00, 0x00, 0x05, 0x2A, 0xE1];
        let r = decode_listen(&MOUSE_WIRELESS, &frame).expect("无线电量帧必须解析成功");
        assert_eq!(
            r.percent.get(),
            100,
            "fullStatus 高半字节非零 → 官方显示 100"
        );
        assert_eq!(r.charging, Some(true), "低半字节 1 = 充电中");
    }

    #[test]
    fn ignores_frames_that_are_not_battery() {
        // 鼠标位移报告：首字节不是 0x0A。
        assert!(decode_listen(&MOUSE_WIRED, &[0x02, 0x01, 0x00, 0x00]).is_none());
        // 命令字对上但 type 不是 5（官方同款判断）。
        assert!(decode_listen(&MOUSE_WIRED, &[0x0A, 0x02, 0x37, 0x00]).is_none());
    }

    /// 音频：应答 `55 65 <电量> <状态>`，状态 3/4 视为充电，26 = 休眠按错误态拒绝。
    #[test]
    fn decodes_audio_reply() {
        let frame = [0x55, 0x65, 0x41, 0x02];
        let r = decode_battery(&AUDIO, &frame).unwrap();
        assert_eq!(r.percent.get(), 65);
        assert_eq!(r.charging, Some(false));

        let charging = [0x55, 0x65, 0x41, 0x03];
        assert_eq!(
            decode_battery(&AUDIO, &charging).unwrap().charging,
            Some(true)
        );
        let full = [0x55, 0x65, 0x64, 0x04];
        assert_eq!(decode_battery(&AUDIO, &full).unwrap().charging, Some(true));

        let asleep = [0x55, 0x65, 0x00, 0x1A];
        assert!(
            decode_battery(&AUDIO, &asleep).is_err(),
            "休眠帧的电量字段没有意义，不得当成 0%"
        );
    }

    #[test]
    fn audio_request_matches_official_payload() {
        let query = AUDIO.request.expect("音频必须声明查询模板").build(Some(0x55));
        assert_eq!(query.len(), 64);
        assert_eq!(&query[..3], &[0x55, 0x65, 0x01]);
        assert!(query[3..].iter().all(|b| *b == 0));
    }

    /// 分类分派：键盘/鼠标各走自己那套协议；音频走 0x55/0x65；例外型号与扩展坞不采集。
    #[test]
    fn route_dispatches_by_device_type_and_mode() {
        let table = table();
        // 键盘（有线）：Ace 60 —— 官方 `dJe` 有线通道，报告 ID 0x06。
        let wired = SPEC
            .for_device(table.lookup(0x19F5, 0xFC30).unwrap())
            .unwrap();
        assert_eq!(wired.frame.report_id, Some(0x06), "键盘有线用 06 报告");
        assert_eq!(wired.frame.cmd_index, 1);
        assert_eq!(wired.request.unwrap().cmd, 0x87);
        assert_eq!(wired.listen.unwrap().percent_index, 2, "有线监听电量在 [2]");

        // 键盘（2.4G）：G98 V3 / K87 V3 共用的接收器 PID —— 官方无线通道，报告 ID 0x13。
        let wireless_keyboard = SPEC
            .for_device(table.lookup(0x3837, 0x1015).unwrap())
            .unwrap();
        assert_eq!(wireless_keyboard.frame.report_id, Some(0x13));
        assert_eq!(wireless_keyboard.request.unwrap().cmd, 0x4A);
        assert!(wireless_keyboard.reassembly.is_some(), "2.4G 键盘是多包应答");

        // 鼠标（2.4G）：A7 Pro —— 仍是鼠标那套 0x11/0x06。
        let wireless = SPEC
            .for_device(table.lookup(0x3837, 0x0010).unwrap())
            .unwrap();
        assert_eq!(wireless.frame.report_id, Some(0x11), "鼠标不走键盘通道");
        assert!(wireless.reassembly.is_none());
        assert_eq!(
            wireless.listen.unwrap().percent_index,
            5,
            "无线监听电量在 [5]"
        );

        let headset = SPEC
            .for_device(table.lookup(0x3837, 0x6001).unwrap())
            .unwrap();
        assert_eq!(headset.frame.report_id, Some(0x55));
        assert_eq!(headset.battery.cmd, Some(0x65));

        // 官方走另一种帧的两个型号：不采集。
        for (vid, pid) in [(0x3837, 0x6011), (0x3837, 0x6013)] {
            let entry = table.lookup(vid, pid).unwrap();
            assert!(
                SPEC.for_device(entry).is_none(),
                "0x{vid:04X}:0x{pid:04X} 需要 Gd/Mpe 帧，未接入"
            );
        }

        // 扩展坞无电量。
        assert!(
            SPEC.for_device(table.lookup(0x3837, 0x1012).unwrap())
                .is_none()
        );
    }

    // ---- 键盘（官方 `dJe` 家族）----

    /// 有线应答：`[0]=0x06 [1]=0x87` + 6 字节头部 + 电量 `[8]` + 状态 `[9]`，其余补 0 到 520。
    fn keyboard_wired_reply(percent: u8, status: u8) -> Vec<u8> {
        let mut report = vec![0u8; 520];
        report[0] = 0x06;
        report[1] = 0x87;
        report[8] = percent;
        report[9] = status;
        report
    }

    #[test]
    fn parses_keyboard_wired_reply() {
        let reading = decode_battery(&KEYBOARD_WIRED, &keyboard_wired_reply(73, 0x00)).unwrap();
        assert_eq!(reading.percent.get(), 73);
        assert_eq!(reading.charging, Some(false));

        let charging = decode_battery(&KEYBOARD_WIRED, &keyboard_wired_reply(73, 0x01)).unwrap();
        assert_eq!(charging.charging, Some(true));
    }

    /// 状态字节两个半字节各一义：高半字节满电、低半字节充电。
    /// 整字节比较会在 `0x11`（满电且充电）时漏判 —— `mask` 就是为这一位加的。
    #[test]
    fn keyboard_charge_low_nibble_ignores_full_flag() {
        let full_and_charging =
            decode_battery(&KEYBOARD_WIRED, &keyboard_wired_reply(100, 0x11)).unwrap();
        assert_eq!(full_and_charging.percent.get(), 100);
        assert_eq!(
            full_and_charging.charging,
            Some(true),
            "满电且充电（0x11）的低半字节仍是 1"
        );

        let full_only = decode_battery(&KEYBOARD_WIRED, &keyboard_wired_reply(100, 0x10)).unwrap();
        assert_eq!(full_only.charging, Some(false));
    }

    /// 全零应答（设备在场但没数据）：只能判成 `EmptyPayload`，不得降级为「0% 未充电」。
    #[test]
    fn keyboard_all_zero_reply_is_empty_payload_not_zero_percent() {
        let reading = decode_battery(&KEYBOARD_WIRED, &keyboard_wired_reply(0, 0x00));
        assert!(
            matches!(reading, Err(voltnest_provider::ErrorCode::EmptyPayload)),
            "全零应答不代表 0%"
        );
    }

    /// 有线查询报文必须与官方 520 字节模板逐字节一致：`06 87 00 00 01 00 02 00` + 512×0。
    #[test]
    fn keyboard_wired_request_matches_official_template() {
        let req = KEYBOARD_WIRED.request.expect("键盘必须声明查询模板");
        let query = req.build(Some(0x06));
        assert_eq!(query.len(), 520, "官方特征报告是 520 字节");
        assert_eq!(&query[..8], &[0x06, 0x87, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00]);
        assert!(query[8..].iter().all(|b| *b == 0x00));
        // 官方有线读是一次「发 + 收」，不重问。
        assert_eq!(req.retry, 0);
    }

    /// 无线查询报文：`13 4A 01 00×16 5E` —— 与官方「命令串 + XOR 校验和」逐字节一致。
    #[test]
    fn keyboard_wireless_request_matches_official_template() {
        let req = KEYBOARD_WIRELESS.request.expect("无线键盘必须声明查询模板");
        let query = req.build(Some(0x13));
        assert_eq!(query.len(), 20, "报告 ID + 19 字节载荷");
        assert_eq!(&query[..3], &[0x13, 0x4A, 0x01]);
        assert_eq!(*query.last().unwrap(), 0x5E, "校验和");
        assert!(
            query[3..19].iter().all(|b| *b == 0x00),
            "中间必须是 16 个 0"
        );

        // 校验和的来历：官方 `tJe` 对**含报告 ID** 的整条命令逐字节求和。
        let sum = query.iter().map(|b| *b as u32).sum::<u32>();
        assert_eq!(
            (sum - 0x5E) & 0xFF,
            0x5E,
            "报告 ID + 命令 + 载荷之和的低字节就是校验和"
        );
    }

    #[test]
    fn keyboard_specs_validate() {
        for spec in [&KEYBOARD_WIRED, &KEYBOARD_WIRELESS] {
            assert!(spec.validate().is_ok(), "{} 结构不合法", spec.label);
        }
    }
}
