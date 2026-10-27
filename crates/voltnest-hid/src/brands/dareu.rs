//! 品牌：达尔优 Dareu（VID `0x260D`）。
//!
//! 达尔优同一张型号表下混着**互不兼容的固件家族**，网页驱动按型号表的 `fwType` 造不同的
//! HID 类（`device.js:419-489`）。本模块用 [`crate::spec::RouteFn`] 钩子把这份分派留在
//! 品牌内部 —— 通用层只问「这个设备用哪套协议」，不认任何厂商字段：
//!
//! | `fwType` | 家族 | 通道 | 报文 |
//! |---|---|---|---|
//! | `0x02` | 匠盟 Jm（`jmdevice.js`） | `0xFF02` output 查询 + 输入报告 | 17B（1B ID + 16B body） |
//! | `0x00` / `0x06` | 自研 Tg（`tgdevice.js`） | `0x0C` feature 查询 | 64B |
//! | `0x01` | 海思 Hs（`tgdevice.js:3980` 继承 Tg） | `0x0C` feature 查询 | 64B，报告 ID `0x0A` |
//!
//! 其余家族（`0x04` / `0x05` 合盈 OHID 等）未验证，[`route`] 返回 `None`：宁可不采集，
//! 也不拿别的家族指令去猜。
//!
//! 证据：`C:\Users\HT_Code\rea-dr-analysis\js\device\*.js`（网页驱动 v1.3.26，与线上一致）。

use crate::brands::checksum;
use crate::spec::{
    AckField, BatteryFields, BrandSpec, ChargingField, FrameLayout, ReplyTransport, RequestTemplate,
};
use crate::table::Entry;

/// 匠盟查询报文载荷：末字节 `[15]` 是厂商校验和。
///
/// `CheckSum(buffer)`（`jmdevice.js:1449`）= `0x55 - Σ(buffer[0..15])`，其中 `buffer[0..2]`
/// 是 `0x08`（报告 ID）+ `0x04`（命令），故结果为 `0x55 - 0x0C = 0x49`。**必须带校验和**：
/// 官方每条 JM 命令都写 `pkt[16]`，缺了设备按坏帧丢弃，表现是「查询永远无应答」。
const JM_QUERY_PAYLOAD: [u8; 15] = {
    let mut payload = [0u8; 15];
    payload[14] = checksum::neg_sum8(&[0x08, 0x04]);
    payload
};

/// 匠盟（`fwType 0x02`）：报告 ID `0x08`，17 字节帧，输出报告查询 + 输入报告应答。
///
/// 应答 body（去掉报告 ID 后）`[5]` = `Level`、`[6]` = `Status`（`jmdevice.js:2187-2191`；
/// 官方把 `MaxLevel` / `Critical` 硬编码为 100，所以 `Level` 本身就是百分比）。
pub const JM: BrandSpec = BrandSpec {
    id: "dareu-jm",
    label: "达尔优 · 匠盟固件（fwType 0x02）",
    usage_page: 0xFF02,
    usage_page_alt: None,
    usage: None,
    usage_page_write: Some(0xFF02),
    reassembly: None,
    transport: ReplyTransport::InterruptQuery,
    reply_inverted: false,
    clamp_percent: false,
    frame: FrameLayout {
        report_id: Some(0x08),
        cmd_index: 1,
        len_index: None,
        payload_offset: 6,
        fixed_payload_len: 2,
    },
    battery: BatteryFields {
        // 匠盟回显命令字：`GetReport` 只接受 `body[0] == 0x04` 的应答（`jmdevice.js:1490`）。
        cmd: Some(0x04),
        subtag: None,
        // payload[0] = body[5] = Level；payload[1] = body[6] = Status。
        percent_index: 0,
        charging: Some(ChargingField {
            mask: 0xFF,
            index: 1,
            true_values: &[0x01, 0x02],
            // 0x00 非充电 / 0x01 普通充 / 0x02 快充 / 0x03 及以上错误态。
            error_from: Some(0x03),
        }),
    },
    listen: None,
    request: Some(RequestTemplate {
        usage_page: 0xFF02,
        cmd: 0x04,
        prefix: &[],
        report_len: 17,
        cmd_index: 1,
        payload_offset: 2,
        payload: &JM_QUERY_PAYLOAD,
        interval_ms: Some(30_000),
        inverted: false,
        // 官方每条命令只发一次，无重试约定。
        retry: 0,
    }),
    // 匠盟靠回显的命令字 + 报告 ID 判定，无独立确认位约定。
    ack: None,
    fallback_report_id: None,
    expected_interval_ms: 30_000,
    device_table_file: "dareu.json",
    variants: &[],
    route: None,
};

/// 自研 Tg（`fwType 0x00`、磁轴 `0x06`）：64 字节 feature 查询，`0x0C` 通道。
///
/// 帧体：`[0]` 状态（应答 `& 0x0F == 0x02` 表示命令已完成）、`[1]` 长度、`[2]` 类 `0x07`、
/// `[3]` 命令 `0x80`（`PWR_CMD_BAT_STATUS | GET_CMD`）、`[6..]` payload =
/// `Status / Level / MaxLevel / Critical`（`tgdevice.js:2521-2533`）。
///
/// 报告 ID 随连接方式变化：有线 `0x00`、2.4G 接收器 `0x07`（`device.js:1109-1113`）。
/// 枚举结果里没有 `WirelessFlag`，故主用 `0x00`、备用 `0x07`，由运行时按「主 → 备」试。
pub const TG: BrandSpec = BrandSpec {
    id: "dareu-tg",
    label: "达尔优 · 自研固件（fwType 0x00 / 0x06）",
    usage_page: 0x000C,
    usage_page_alt: None,
    usage: None,
    usage_page_write: Some(0x000C),
    reassembly: None,
    transport: ReplyTransport::FeatureQuery { reply_len: 65 },
    reply_inverted: false,
    clamp_percent: false,
    frame: FrameLayout {
        report_id: Some(0x00),
        cmd_index: 4,
        len_index: None,
        payload_offset: 7,
        fixed_payload_len: 4,
    },
    battery: BatteryFields {
        // 官方对 Tg 应答只校验确认位，不校验命令回显（`tgdevice.js:1836`）。
        cmd: None,
        subtag: None,
        // payload[0] = Status，payload[1] = Level。
        percent_index: 1,
        charging: Some(ChargingField {
            mask: 0xFF,
            index: 0,
            true_values: &[0x01, 0x02],
            error_from: Some(0x03),
        }),
    },
    listen: None,
    request: Some(RequestTemplate {
        usage_page: 0x000C,
        cmd: 0x80,
        // body[0]=TargetId(0) / body[1]=HDR_SIZE(0x03) / body[2]=HDR_CLASS(0x07)。
        prefix: &[0x00, 0x03, 0x07],
        report_len: 65,
        cmd_index: 4,
        payload_offset: 7,
        payload: &[],
        interval_ms: Some(30_000),
        inverted: false,
        // 官方每条命令只发一次，无重试约定。
        retry: 0,
    }),
    ack: Some(AckField {
        index: 1,
        mask: 0x0F,
        value: 0x02,
    }),
    fallback_report_id: Some(0x07),
    expected_interval_ms: 30_000,
    device_table_file: "dareu.json",
    variants: &[],
    route: None,
};

/// 海思（`fwType 0x01`）：协议与自研 Tg 完全一致（`HsMouseHidDevice` 只是 `TgMouseHidDevice`
/// 的空子类，`tgdevice.js:3980-3994`），唯一差别是报告 ID 固定 `0x0A`（`device.js:1116-1119`）。
///
/// 因此这里只覆盖**四处**：标识、展示名、报告 ID、以及「不换 ID 重问」（`0x0A` 是固定值，
/// 没有 `0x07` 那种接收器备用 ID）。其余字段整体继承 [`TG`] —— 改动 Tg 的字段时海思自动跟随，
/// 这正是官方「空子类」的事实对应物；写成第二份完整字面量则会随固件升级悄悄分叉。
pub const HS: BrandSpec = BrandSpec {
    id: "dareu-hs",
    label: "达尔优 · 海思固件（fwType 0x01）",
    frame: FrameLayout {
        report_id: Some(0x0A),
        ..TG.frame
    },
    fallback_report_id: None,
    ..TG
};

/// 达尔优自己的 `fwType → 协议` 分派。
///
/// **只有达尔优的型号表带 `fwType`**，所以这张表只存在于本模块内；其它品牌 `route: None`，
/// 通用层那侧永远走不到这里。
fn route(entry: &Entry) -> Option<&'static BrandSpec> {
    match entry.declared_fw_type() {
        Some(0x02) => Some(&JM),
        // 磁轴 `0x06` 与自研 `0x00` 共用同一套 Tg 指令与报告 ID 选择（`device.js:1109,460-470`）。
        Some(0x00) | Some(0x06) => Some(&TG),
        Some(0x01) => Some(&HS),
        // `0x04` / `0x05`（合盈 OHID）与未知家族：未验证，不采集。
        _ => None,
    }
}

/// 注册表登记的达尔优协议描述（匠盟为字段默认项，实际由 [`route`] 按 `fwType` 分派）。
///
/// `id` 保持品牌级 `dareu`（日志 / 诊断里的品牌身份）；单套协议的精确标识在
/// [`JM`] / [`TG`] / [`HS`] 上。
pub const SPEC: BrandSpec = BrandSpec {
    id: "dareu",
    label: "达尔优 Dareu（按 fwType 分派）",
    variants: &[&JM, &TG, &HS],
    route: Some(route),
    ..JM
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brands::Registry;
    use crate::frame::{BatteryReading, RejectReason, decode_battery, decode_battery_detailed};
    use crate::table::Entry;
    use voltnest_provider::{ErrorCode, HID_VBAS, Percent};

    /// 匠盟查询帧：`08 04 00 … 49`（末字节为厂商校验和）。
    #[test]
    fn jm_query_carries_vendor_checksum() {
        let query = JM.request.expect("匠盟必须声明查询报文").build(Some(0x08));
        assert_eq!(query.len(), 17);
        assert_eq!(query[0], 0x08, "报告 ID");
        assert_eq!(query[1], 0x04, "命令字");
        assert_eq!(query[16], 0x49, "0x55 - (0x08 + 0x04)");
        assert!(
            query[2..16].iter().all(|b| *b == 0),
            "中间必须全零：{:02X?}",
            &query[2..16]
        );
    }

    /// 匠盟应答：body[5]=Level、body[6]=Status → 报告 `[6]` / `[7]`。
    #[test]
    fn jm_parses_level_and_status() {
        let mut report = [0u8; 17];
        report[..2].copy_from_slice(&[0x08, 0x04]);
        report[6] = 0x56; // Level = 86
        report[7] = 0x02; // 快充
        let reading = decode_battery(&JM, &report).unwrap();
        assert_eq!(reading.percent.get(), 86);
        assert_eq!(reading.charging, Some(true));
    }

    /// 状态 ≥0x03 是错误态：整帧不可用，不得降级成「未充电 + 某个电量」。
    #[test]
    fn jm_error_status_rejects_frame() {
        for status in [0x03u8, 0x05, 0xFF] {
            let mut report = [0u8; 17];
            report[..2].copy_from_slice(&[0x08, 0x04]);
            report[6] = 0x56;
            report[7] = status;
            let err = decode_battery_detailed(&JM, &report).unwrap_err();
            assert_eq!(err.code, ErrorCode::MalformedFrame);
            assert_eq!(err.reason, RejectReason::StatusError { raw: status });
        }
    }

    /// 自研/海思应答：`[0]`=报告 ID、`[1]`=确认位、`[3]`=类、`[4]`=命令、`[7..11]`=payload。
    fn tg_report(report_id: u8, ack: u8, status: u8, level: u8) -> [u8; 65] {
        let mut report = [0u8; 65];
        report[0] = report_id;
        report[1] = ack;
        report[2] = 0x03; // HDR_SIZE
        report[3] = 0x07; // CLASS_POWER
        report[4] = 0x80; // PWR_CMD_BAT_STATUS | GET_CMD
        report[7] = status;
        report[8] = level;
        report[9] = 100; // MaxLevel
        report[10] = 100; // Critical
        report
    }

    #[test]
    fn tg_parses_status_and_level() {
        let reading = decode_battery(&TG, &tg_report(0x00, 0x02, 0x01, 0x37)).unwrap();
        assert_eq!(reading.percent.get(), 0x37);
        assert_eq!(reading.charging, Some(true));

        let reading = decode_battery(&TG, &tg_report(0x00, 0x02, 0x00, 0x64)).unwrap();
        assert_eq!(reading.percent.get(), 100);
        assert_eq!(reading.charging, Some(false));
    }

    /// 2.4G 接收器用报告 ID `0x07`：解析器只认主 ID，备用 ID 由运行时换 ID 重问。
    #[test]
    fn tg_declares_dongle_report_id_as_fallback() {
        assert_eq!(
            decode_battery(&TG, &tg_report(0x00, 0x02, 0x01, 0x37)),
            Ok(BatteryReading {
                percent: Percent::new(0x37).unwrap(),
                charging: Some(true),
            })
        );
        assert_eq!(
            decode_battery(&TG, &tg_report(0x07, 0x02, 0x01, 0x37)),
            Err(ErrorCode::MalformedFrame),
            "主 ID 0x00 已锁定语义：0x07 必须由运行时换 ID 后重问"
        );
        assert_eq!(TG.fallback_report_id, Some(0x07));
    }

    /// 确认位不是 0x02 = 命令还没处理完，不能当读数。
    #[test]
    fn tg_requires_ack_nibble() {
        let err = decode_battery_detailed(&TG, &tg_report(0x00, 0x00, 0x00, 0x37)).unwrap_err();
        assert_eq!(
            err.reason,
            RejectReason::AckMismatch {
                want: 0x02,
                got: Some(0x00)
            }
        );
    }

    /// 海思用报告 ID `0x0A`，其余与 Tg 同构。
    #[test]
    fn hs_uses_report_id_0a() {
        assert_eq!(
            decode_battery(&HS, &tg_report(0x0A, 0x02, 0x01, 0x50))
                .unwrap()
                .percent
                .get(),
            0x50
        );
        assert_eq!(
            decode_battery(&HS, &tg_report(0x00, 0x02, 0x01, 0x50)),
            Err(ErrorCode::MalformedFrame),
            "海思不认 0x00"
        );
    }

    /// 构造一条只带 `fwType` 的型号表条目（分派只读这一个字段）。
    fn entry_with_fw(fw: Option<u8>) -> Entry {
        super::super::test_support::entry_with("fwType", fw.map(|v| serde_json::json!(v)))
    }

    /// 分派只认达尔优自己的 `fwType`；未验证家族返回 `None`（不采集，不猜）。
    #[test]
    fn route_dispatches_by_fw_type() {
        assert_eq!(
            route(&entry_with_fw(Some(0x02))).map(|s| s.id),
            Some("dareu-jm")
        );
        assert_eq!(
            route(&entry_with_fw(Some(0x00))).map(|s| s.id),
            Some("dareu-tg")
        );
        assert_eq!(
            route(&entry_with_fw(Some(0x06))).map(|s| s.id),
            Some("dareu-tg")
        );
        assert_eq!(
            route(&entry_with_fw(Some(0x01))).map(|s| s.id),
            Some("dareu-hs")
        );
        assert!(
            route(&entry_with_fw(Some(0x04))).is_none(),
            "合盈 OHID 未接入"
        );
        assert!(route(&entry_with_fw(Some(0x05))).is_none());
        assert!(
            route(&entry_with_fw(None)).is_none(),
            "型号表缺 fwType 时不猜协议"
        );
    }

    /// 三套协议都必须通过构造期校验，且落在家族能力预算内。
    #[test]
    fn all_dareu_specs_validate() {
        for spec in [&JM, &TG, &HS, &SPEC] {
            assert!(spec.validate().is_ok(), "{} 非法", spec.id);
        }
        assert!(SPEC.validate_against(&HID_VBAS).is_ok());
    }

    /// 通用层只读钩子：其它品牌（`route: None`）行为不变，达尔优按 `fwType` 分派。
    #[test]
    fn for_device_honours_route_hook() {
        assert_eq!(
            SPEC.for_device(&entry_with_fw(Some(0x02))).map(|s| s.id),
            Some("dareu-jm")
        );
        assert_eq!(
            SPEC.for_device(&entry_with_fw(Some(0x01))).map(|s| s.id),
            Some("dareu-hs")
        );
        let ajazz: &'static BrandSpec = &crate::brands::ajazz::SPEC;
        for fw in [None, Some(0x00), Some(0x02)] {
            assert_eq!(
                ajazz.for_device(&entry_with_fw(fw)).map(|s| s.id),
                Some("ajazz"),
                "无钩子的品牌必须原样返回自己"
            );
        }
    }

    /// 型号表里的 `fwType` 必须能被解析出来。
    #[test]
    fn table_exposes_fw_type() {
        let reg = Registry::embedded();
        let (_, entry) = reg.lookup(0x260D, 0x1117).expect("A950 PRO Mg 必须在表内");
        assert_eq!(entry.declared_fw_type(), Some(0x02), "A950 PRO Mg = 匠盟");
        let (_, entry) = reg.lookup(0x260D, 0x0030).expect("Z82 必须在表内");
        assert_eq!(entry.declared_fw_type(), Some(0x00), "Z82 = 自研 Tg");
    }
}
