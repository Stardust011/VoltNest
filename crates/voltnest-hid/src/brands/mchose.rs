//! 品牌：迈从 MCHOSE。
//!
//! 依据：官方 M HUB 网页驱动（`mchose.com.cn`，`assets/purify.es-*.js` + `assets/index-*.js`，
//! 2026-10 线上版本）的反混淆阅读。**官方按设备类别走四条互不相同的通道**，本模块据此拆 spec：
//!
//! | 类别 | 官方通道 | 本模块 |
//! |---|---|---|
//! | 鼠标 | 连接时一次 feature 查询 `11 06`（应答逐字节取反）+ 之后只靠实时帧刷新 | [`MOUSE_WIRED`] / [`MOUSE_WIRELESS`]（仅监听帧布局不同） |
//! | 键盘 · CZ/GLW 族 | 标准页 `0x0001`：`55 05 …` 读 64 字节功能配置块，电量在 `payload[32]` | [`KEYBOARD_GLW`] |
//! | 键盘 · `dJe` 老族 | 有线 feature `06 87`（520B）、2.4G output `13 4A` + 校验和 + 多包重组 | [`KEYBOARD_DJE_WIRED`] / [`KEYBOARD_DJE_WIRELESS`] |
//! | 键盘 · CZ/QHW 族 | `0xFF80`：报告 `0x4D` + 7 字节头 + 2 字节命令（查询 `0x0904` / 推送 `0x0700`）+ 尾校验 | [`KEYBOARD_QHW`] |
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
    BatteryFields, BrandSpec, ChargingField, FrameLayout, ListenSpec, NotifySpec, Reassembly,
    ReplyTransport, RequestTemplate,
};
use crate::table::Entry;

const MCHOSE_REQUEST_PAYLOAD: [u8; 19] = [0; 19];

/// 键盘（有线）查询载荷：官方 `wiredCommand` 去掉报告 ID `0x06` 与命令字 `0x87` 之后的部分。
///
/// 官方模板是 `` `06 87 00 00 01 00 02 00 ${"00 ".repeat(512).trim()}` ``：`0x06` 是报告 ID
/// （`readCommand` 里用 `wiredCommand.slice(3)` 砍掉它，因为 WebHID 的 `sendFeatureReport`
/// 把报告 ID 作为独立入参），`0x87` 是命令，后面 6 字节是命令参数，再补 512 个 0。
const KEYBOARD_DJE_WIRED_PAYLOAD: [u8; 6] = [0x00, 0x00, 0x01, 0x00, 0x02, 0x00];

/// 键盘（2.4G）查询载荷：官方 `wirelessCommand` 去掉报告 ID `0x13` 与命令字 `0x4A` 之后的部分，
/// 末尾追加官方 XOR 校验和。
///
/// 官方 `tJe` = 「各字节十六进制求和」⇒ `(0x13 + 0x4A + 0x01) & 0xFF = 0x5E`。求和**包含报告 ID**，
/// 这一点容易看漏（`tJe(o)` 的入参是带 `13` 的完整命令串）。载荷共 18 字节：
/// `01 00×16 5E`，加上报告 ID 与命令字正好 20 字节一条 output report。
const KEYBOARD_DJE_WIRELESS_PAYLOAD: [u8; 18] = {
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
///
/// # 节奏：连接时问一次，之后只订阅
/// 官方鼠标页的初始化是「发一条 `0x11 0x06` 拿 `getBatteryMode deviceInfo`」，
/// 随后 `ck.getNotifier(id).subscribe(..)` 订阅实时帧，只在 `command===10 && type===5`
/// 时采用电量（`q3e` 解析，有线 `[0]=0x0A [1]=type [2]=电量 [3]=bit4 满电 + bit4 充电`）。
/// **没有周期查询** —— 因此本 spec 的 `interval_ms` 为 `None`，不按节奏唤醒设备。
///
/// 与键盘的差别值得记一笔：这一族的页面 `0xFF01` / `0xFF0B` 与官方过滤器
/// （`[65281, 65291].includes(usagePage)`）**一致**，所以鼠标从来没有键盘那种
/// 「枚举不到」的问题；问题只在采集节奏上。
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
        cmd_high: None,
        subtag: None,
        percent_index: 9,
        percent_mask: 0xFF,
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
    notify: None,
    request: Some(RequestTemplate {
        usage_page: 0xFF01,
        cmd: 0x06,
        cmd_high: None,
        prefix: &[],
        report_len: 21,
        cmd_index: 1,
        payload_offset: 2,
        payload: &MCHOSE_REQUEST_PAYLOAD,
        // **不周期查询**：官方鼠标页只在连接时发一次这条命令，之后只订阅实时帧
        // （`q3e` 解析 `command===10 && type===5`，`getBatteryMode deviceInfo` 只打一次日志）。
        // `None` = 会话建立时问一次，之后由运行时在「读数快过期」时探活（见 `runtime::Reader`）。
        interval_ms: None,
        inverted: true,
        // 官方 `LG()` 在命令字回显不符时最多重读 6 次（600ms 窗口）。
        retry: 6,
    }),
    // 该固件无独立确认位约定：命令字回显（取自应答 `[1]` 取反）就是确认。
    ack: None,
    fallback_report_id: None,
    expected_interval_ms: 30_000,
    device_table_file: "mchose.json",
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
        cmd_high: None,
        subtag: None,
        percent_index: 0,
        percent_mask: 0xFF,
        charging: Some(ChargingField {
            mask: 0xFF,
            // 2=放电 3=充电 4=满电 26=休眠（休眠时电量字段无意义，按错误态拒绝）。
            index: 1,
            true_values: &[0x03, 0x04],
            error_from: Some(0x1A),
        }),
    },
    listen: None,
    notify: None,
    request: Some(RequestTemplate {
        usage_page: 0,
        cmd: 0x65,
        cmd_high: None,
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
    device_table_file: "mchose.json",
    variants: &[],
    route: None,
};

/// 走另一种帧的音频型号（官方 `QKt` 表：`V9 PRO 2` / `R9`）。
const ALT_AUDIO_MODELS: [&str; 2] = ["V9 PRO 2", "R9"];

/// 键盘（GLW 族）：CZ 驱动（`cz_main.js`）里没声明 `manufacturer` 的全部键盘，缺省即这一族。
///
/// G75 V2 就在这一族里（其配置对象没有 `manufacturer` 字段 → 官方 `It()` 返回 `"glw"`）。
///
/// # 通道
/// 官方 `hidFilters:[{usagePage:1,usage:0},{usagePage:65280,usage:1,boot:!0}]` ——
/// **正常模式挂在标准 Generic Desktop 页（`0x0001`）上**，`0xFF00` 那条带 `boot:!0`，
/// 只在设备切成升级 PID（G75 V2：`0x2022`）时生效。此前的 `0xFF01` 是鼠标 `LG()` 通道的页面，
/// 键鼠两族用错页的后果一样：`select_targets` 零命中，设备连 `Arrived` 都不发。
///
/// # 集合
/// 标准页 `0x0001` 上并列多个集合（键盘 `1/6`、消费类 `1/12`、鼠标 `1/2`），GLW 私有协议在 `1/0`；
/// 故 `usage` 钉死为 `0`，只作用于主页面。
///
/// # 电量（没有 `getBattery` 这种一问一答）
/// 官方在 GLW 族里**没有独立的电量命令**：`getPerformanceInfo()` → `getFuncConfig(profile)`，
/// 读一整块 64 字节的功能配置，电量与充电标志压在同一个字节里
/// （`xe()`：`rf_battery:127&e[32]`, `isCharging:e[32]>>7&1`）。
///
/// 查询帧由 `CmdQueueItem` 拼：`55 <cmd> 00 <xor> <size> <lo> <hi> 00 <data…>`；
/// `getFuncConfig` = `_simpleGetCommand(5, 64*profile, 64)`，按 56 字节一段发多包 ——
/// 首包即「偏移 0、长度 56」，而电量在第 33 字节，单包就够，故这里只发首包。
///
/// 校验字节取 `size/lo/hi/0` 的折叠值；本例 `size=56`、`lo=hi=0`，异或与求和同为 `0x38`，
/// 两种折叠方式给出同一个字节，故校验算法存疑处不影响这条报文。
///
/// # 未接入：`FA FB 07 <电量> <充电>` 通知帧
/// 设备会主动推电量，但官方对 GLW 族把 `featSupport.AllowUpdateBattery` 缺省成 `false`
/// （只有 `qhw` 缺省 `true`），`_proxyEvent` 收到这类事件会**直接丢弃**并打日志。既然官方
/// 自己不用这条通道，本项目也不拿它当读数来源 —— 少一条没有官方背书的解析路径。
///
/// # 静态分析到此为止的一处（需实机确认）
/// **应答是否带报告 ID 槽**：本模块用 `decode_battery`/`matches_cmd` 的位移容错覆盖两种形态，
/// 无需再改布局；如果两种都不中，诊断会给出 `cmd_mismatch(want=0x05 got=…)`，按实际字节补一眼即可。
pub const KEYBOARD_GLW: BrandSpec = BrandSpec {
    id: "mchose-keyboard-glw",
    label: "MCHOSE（键盘 · CZ/GLW 族）",
    usage_page: 0x0001,
    // 官方过滤器第二条：`0xFF00/0x0001`。正常模式用不到，但 boot 模式下设备只在它上面有集合。
    usage_page_alt: Some(0xFF00),
    // 只打开 GLW 私有集合（`1/0`），不落到键盘 `1/6`。
    usage: Some(0),
    usage_page_write: Some(0x0001),
    reassembly: None,
    transport: ReplyTransport::InterruptQuery,
    reply_inverted: false,
    clamp_percent: false,
    frame: FrameLayout {
        // 线上无编号报告：官方 `CmdQueueItem` 未指定 reportId 时取缺省 `0`，即无编号报告。
        report_id: None,
        // 应答 `AA <cmd> …`：`[0]` 是期望标志、`[1]` 才是命令字。
        cmd_index: 1,
        // `[4]` = 本包含有的数据长度（官方 `_isMatch` 用它判「回包属于哪一问」）。
        len_index: Some(4),
        // 头部 8 字节：`AA cmd ? ? size lo hi 00`。
        payload_offset: 8,
        fixed_payload_len: 0,
    },
    battery: BatteryFields {
        cmd: Some(0x05),
        cmd_high: None,
        subtag: None,
        // 功能配置块的第 33 字节：低 7 位电量、最高位充电。
        percent_index: 32,
        percent_mask: 0x7F,
        charging: Some(ChargingField {
            mask: 0x80,
            index: 32,
            true_values: &[0x80],
            error_from: None,
        }),
    },
    listen: None,
    notify: None,
    request: Some(RequestTemplate {
        usage_page: 0x0001,
        cmd: 0x05,
        cmd_high: None,
        // `[0]` 是 hidapi 对**无编号报告**要求的报告 ID 槽（必须是 `0x00`），`[1]` 才是官方
        // `NORMAL_OPTIONS.flag = 0x55`。少写这个 0，hidapi 会把 `0x55` 当成报告 ID，整帧被设备丢弃。
        prefix: &[0x00, 0x55],
        // 官方 `packetLength` 缺省 64：整包发满，尾部补 0。
        report_len: 64,
        cmd_index: 2,
        payload_offset: 3,
        payload: &KEYBOARD_GLW_PAYLOAD,
        interval_ms: Some(30_000),
        inverted: false,
        // 官方 `getFuncConfig` 走队列重发，不按「一问一答」重问；重发会插进设备正在处理的队列。
        retry: 0,
    }),
    ack: None,
    fallback_report_id: None,
    expected_interval_ms: 30_000,
    device_table_file: "mchose.json",
    variants: &[],
    route: None,
};

/// GLW 族查询载荷（命令字之后的部分）：`00 <xor> <size> <lo> <hi> 00`。
///
/// `size = 56` 是官方把 64 字节块按 56 字节切包后的首包长度，`lo = hi = 0` 即从块首读。
const KEYBOARD_GLW_PAYLOAD: [u8; 6] = {
    let size = 56u8;
    // 折叠值：`size ^ lo ^ hi ^ 0`（本例恰与求和同值）。
    let checksum = size;
    [0x00, checksum, size, 0x00, 0x00, 0x00]
};

/// 键盘（CZ/QHW 族）：CZ 配置里显式声明 `manufacturer:"qhw"` 的两台 —— G98 V3（`0x2025`）与
/// K87 V3（`0x2023`），以及它们共用的 2.4G 接收器 PID（`0x1015` / `0x1017`）。
///
/// # 通道
/// 官方给这两台的 `hidFilters` 是 `[{usagePage:65408,usage:1},{usagePage:65408,usage:1,boot:!0}]`
/// —— **`0xFF80/0x01`**，与 GLW 族的 `0x0001`、`dJe` 族的 `0xFF01` 都不同。通道类是
/// `Channel:QhwKeyboard:Loop`，`reportId` 缺省取 `Ge.REPORT_ID = 77 = 0x4D`。
///
/// # 帧（`Ge` 队列项，7 字节头 + 尾校验）
/// ```text
/// [0] 0x4D          报告 ID（hidapi 要求写在首字节）
/// [1] version = 1   `Ge` 构造期写死
/// [2] flags   = 3   同上
/// [3] length        data 的字节数
/// [4] cmd 低字节    `hJ(command) = [command & 0xFF, command >> 8]`（小端）
/// [5] cmd 高字节
/// [6] bizCode = 0   `Ge.BIZ_CODE`
/// [7] sequence      自增序号（本模块固定 0，理由见下）
/// [8..8+length]     data
/// [8+length]        校验和 = 从 `[2]`（flags）起逐字节异或到 data 末尾
/// ```
/// 官方 `toHeaderBytes()` 拼 `[version, flags, length, ...hJ(command), bizCode, sequence]`，
/// 校验和是 `d.slice(1).reduce(xor)` —— 入参 `d` 是**不含报告 ID**的帧，`slice(1)` 跳过
/// `version`，故校验从 flags 起算。`length` 为 0 时这条电量查询的校验和恒为 `0x0E`。
///
/// # 电量
/// `getBattery()` = `_simpleGet(2308, [])` → `{chargeStatus: toChargingStatus(data[0]),
/// batteryLevel: data[1]}`；`toChargingStatus`：`0` 未充电 / `1` 充电中 / `2` 已充满 / 其它 `unknown`。
/// 与 GLW 族「读 64 字节功能块」完全不同 —— 这一族是真正的一问一答。
///
/// # 序号固定为 0
/// 官方 `obtainSequenceId()` 每发一条命令自增，用来对齐回包；但它的匹配逻辑
/// （`Ge._isMatch`）只比 `length`、`_parseData` 也不校验序号，设备侧无从证明会拒绝序号 0。
/// 本项目一次会话只发同一条查询，固定 0 即可；要接多命令时再引入计数。
///
/// # 主动上报（本族的常态读数来源）
/// `featSupport.AllowUpdateBattery` / `Has1PercentPrecisionBattery` 对 qhw 族**缺省为 `true`**
/// （GLW 族缺省 `false`），所以官方在 qhw 上确实消费 "`updateBattery`" 事件。
///
/// 但这一族**没有** GLW 那种 `FA FB 07 <电量> <充电>` 的纯电量通知帧：电量随配置块一起推，
/// 命令 `0x0700`（`MANUAL_REPORT`），官方 `parseManualReportBytes` 拆 23 字节后取
/// `n[2]`（走 `toChargingStatus`）与 `n[3]`（`batteryLevel`），再与缓存 diff 才发事件。
/// 所以本 spec 的 `notify` 是 `0x0700` + `data[2]/data[3]`，与 `getBattery`(0x0904) 的
/// `data[0]/data[1]` **不是同一套下标** —— 混用会读出一个稳定但无关的数字。
///
/// 有了推送就不必按节奏轮询：`RequestTemplate::interval_ms` 置 `None`，只在会话建立时问一次；
/// 之后由运行时在「读数静默超过 `expected_interval_ms`」时探活（`runtime::lazy_probe_due`），
/// 探活也没回应就让读数自然老化，由引擎按阈值链判 `stale` / `unavailable`。
///
/// # 设备完全不推送时会怎样（静态分析无法排除）
/// 那时探活成为唯一的读数来源，表现为「每 `expected_interval_ms` 问一次」，即退化成轮询 ——
/// 功能仍正确，只是省电收益没有兑现。有实机后应先确认：静置不动时是否收到 `0x0700` 帧。
///
/// # 静态分析到此为止（需实机确认）
/// 报文按 `Ge.NORMAL_MAX_PACKET_LENGTH = 31` 补满 32 字节发出，而官方 WebHID 只发实际长度
/// （本条 9 字节，剩下的由浏览器按报告描述符补）。若该集合的输出报告描述符短于 32 字节，
/// hidapi 会拒绝写入，表现为「查询永远没应答、也没有错误」—— 运行时的 output 写失败是
/// **静默忽略**的（`let _ = dev.write(..)`）。遇到这种情况把 `report_len` 改成 9 即可。
pub const KEYBOARD_QHW: BrandSpec = BrandSpec {
    id: "mchose-keyboard-qhw",
    label: "MCHOSE（键盘 · CZ/QHW 族）",
    usage_page: 0xFF80,
    // 官方过滤器两条都是 0xFF80，只是 `usage:1` 与 `usage:1, boot:true` 之分 —— 同一个页面。
    usage_page_alt: None,
    usage: None,
    usage_page_write: Some(0xFF80),
    reassembly: None,
    transport: ReplyTransport::InterruptQuery,
    reply_inverted: false,
    clamp_percent: false,
    frame: FrameLayout {
        report_id: Some(0x4D),
        // 命令字在 `[4]`（`[1]` version、`[2]` flags、`[3]` length 之后）。
        cmd_index: 4,
        len_index: Some(3),
        // 头部 8 字节：`4D 01 03 <len> <lo> <hi> <biz> <seq>`。
        payload_offset: 8,
        // 长度取自 `[3]`，固定值不参与（`validate` 只在 `len_index` 为 `None` 时校验它）。
        fixed_payload_len: 0,
    },
    battery: BatteryFields {
        // 命令 2308 = 0x0904，线上小端写作 `04 09`。
        cmd: Some(0x04),
        cmd_high: Some(0x09),
        subtag: None,
        // `getBattery` 取 `data[0]` 状态、`data[1]` 电量。
        percent_index: 1,
        percent_mask: 0xFF,
        charging: Some(ChargingField {
            // 三态整字节比较：0 未充电 / 1 充电中 / 2 已充满。`2`（充满）不算「正在充电」，
            // 与官方 `toChargingStatus` 的语义一致 —— 充满时插着线，但充电已停止。
            mask: 0xFF,
            index: 0,
            true_values: &[0x01],
            // 官方把 3 及以上叫 `unknown` 而不是错误态：电量字段仍可能有效，不整帧作废。
            error_from: None,
        }),
    },
    listen: None,
    // 设备主动推 `MANUAL_REPORT`（命令 0x0700）时更新电量：整块配置里 `data[2]` 三态充电、
    // `data[3]` 电量（官方 `parseManualReportBytes` 拆 23 字节，`toChargingStatus(n[2])` + `n[3]`）。
    notify: Some(NotifySpec {
        cmd: 0x00,
        cmd_high: Some(0x07),
        percent_index: 3,
        percent_mask: 0xFF,
        charging: Some(ChargingField {
            mask: 0xFF,
            index: 2,
            true_values: &[0x01],
            error_from: None,
        }),
    }),
    request: Some(RequestTemplate {
        usage_page: 0xFF80,
        cmd: 0x04,
        cmd_high: Some(0x09),
        // `[1..4)`：version / flags / length(=0，本条命令不带参数)。
        prefix: &[0x01, 0x03, 0x00],
        // 报告 ID + 31 字节报文（`Ge.NORMAL_MAX_PACKET_LENGTH = 31`：7 头 + 23 数据 + 1 校验）。
        report_len: 32,
        cmd_index: 4,
        // 命令字之后紧跟 `bizCode` `sequence` `校验和` —— 本条命令不带参数，校验和就是帧尾，
        // 故从 `[6]` 起一律算 payload（与应答侧 `payload_offset = 8` 不同：那边 `[8]` 起才是 data）。
        payload_offset: 6,
        payload: &KEYBOARD_QHW_PAYLOAD,
        // **不周期查询**：键盘处理一条电量命令要唤醒固件，而推送是设备自己决定何时发。
        // `None` = 会话建立时问一次，之后由运行时在「读数快过期」时探活（见 `runtime::Reader`）。
        interval_ms: None,
        inverted: false,
        // 官方 `_simpleGet` 由队列重发，不按「一问一答」重问；重发会插进设备正在处理的队列。
        retry: 0,
    }),
    ack: None,
    fallback_report_id: None,
    expected_interval_ms: 30_000,
    device_table_file: "mchose.json",
    variants: &[],
    route: None,
};

/// QHW 族查询载荷（命令字之后的部分）：`<bizCode> <sequence> <xor>`。
///
/// 校验和按官方算法实算而不是抄一个常数，将来改 `prefix` 时不会悄悄发出一帧校验错的报文。
const KEYBOARD_QHW_PAYLOAD: [u8; 3] = {
    let biz = 0u8;
    let seq = 0u8;
    // 与官方 `toHeaderBytes` 覆盖范围一致：flags · length · cmdLo · cmdHi · bizCode · sequence。
    let checksum = checksum::xor8(&[0x03, 0x00, 0x04, 0x09, biz, seq]);
    [biz, seq, checksum]
};

/// 键盘（`dJe` 老族，有线）：官方 `dJe.isWired` 分支 —— 520 字节 feature 报告，报告 ID `0x06`，命令 `0x87`。
///
/// 这一族是网页驱动里 **purify 那一套**（`getBattery.wiredCommand = "06 87 …"`），覆盖
/// G87 / K99 / G98 / UT98 / Z75 / X75 / GX87 / K87 等上一代型号，页面挂在 `0xFF01`。
/// 新一代键盘（含 G75 V2）走 [`KEYBOARD_GLW`]，两族的帧与页面都不同。
///
/// 应答 `slice(2)` 后按 `bae(true)` 解析：跳 6 字节 → 电量 → 状态（高半字节满电、低半字节充电）。
/// 官方 `readCommand` **不校验命令字回显**（与鼠标 `LG()` 不同），故 `battery.cmd: None`：
/// 多校验一条没有证据的字节，只会在别的固件版本上把可用读数判成坏帧。
pub const KEYBOARD_DJE_WIRED: BrandSpec = BrandSpec {
    id: "mchose-keyboard-dje-wired",
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
        cmd_high: None,
        subtag: None,
        percent_index: 6,
        percent_mask: 0xFF,
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
    notify: None,
    request: Some(RequestTemplate {
        usage_page: 0xFF01,
        cmd: 0x87,
        cmd_high: None,
        prefix: &[],
        // 报告 ID `0x06` + 519 字节载荷 = 官方那条 520 字节特征报告。
        report_len: 520,
        cmd_index: 1,
        payload_offset: 2,
        payload: &KEYBOARD_DJE_WIRED_PAYLOAD,
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
    device_table_file: "mchose.json",
    variants: &[],
    route: None,
};

/// 键盘（`dJe` 老族，2.4G）：官方 `dJe` 无线分支 —— 输出报告 `0x13` + 命令 `0x4A`，应答是多条输入报告。
///
/// 应答包布局（数据窗口，`[0]` 是命令字）：`[0]=0x4A [1]=总包数 [2]=序号 [3]=保留 [4..-1)=载荷
/// [-1]=校验和`。凑齐全部序号后把载荷按序拼起来，`[0]=电量`、`[1]=状态`。
pub const KEYBOARD_DJE_WIRELESS: BrandSpec = BrandSpec {
    id: "mchose-keyboard-dje-wireless",
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
        cmd_high: None,
        subtag: None,
        percent_index: 0,
        percent_mask: 0xFF,
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
    notify: None,
    request: Some(RequestTemplate {
        usage_page: 0xFF01,
        cmd: 0x4A,
        cmd_high: None,
        prefix: &[],
        // 报告 ID `0x13` + 19 字节载荷 = 20 字节 output report。
        report_len: 20,
        cmd_index: 1,
        payload_offset: 2,
        payload: &KEYBOARD_DJE_WIRELESS_PAYLOAD,
        interval_ms: Some(30_000),
        inverted: false,
        // 无线是「发一次 + 多包应答」，重发会打断正在拼的那一轮。
        retry: 0,
    }),
    ack: None,
    fallback_report_id: None,
    expected_interval_ms: 30_000,
    device_table_file: "mchose.json",
    variants: &[],
    route: None,
};

/// 迈从自己的 `driver + deviceType + mode` 分派。
///
/// 官方把键盘分成**三套互不兼容的协议**，判据不在 VID:PID 上：
///
/// | 族 | 官方判据 | 通道 |
/// |---|---|---|
/// | `glw` | CZ 设备配置里 `manufacturer` 缺省 | 标准页 `0x0001`，命令 `0x05` 读 64 字节块 |
/// | `dje` | 不在 CZ 配置表里（purify 老驱动） | `0xFF01`，有线 feature `06 87` / 2.4G `13 4A` |
/// | `qhw` | CZ 配置里显式 `manufacturer:"qhw"`（G98 V3 `0x2025`、K87 V3 `0x2023`） | `0xFF80`，报告 `0x4D` + 7 字节头 + 变长序号 |
///
/// 分派依据是型号表里的 `driver` 字段 —— 由官方 bundle 抓取时写入，与雷柏用 `driver` 分派同一套做法。
/// 缺字段一律 `None`（不采集），不退回任一默认族：猜错族的表现是「设备在列表里但永远读不出」，
/// 比不显示更难查。
fn route(entry: &Entry) -> Option<&'static BrandSpec> {
    let wireless = entry.mode.eq_ignore_ascii_case("2.4G");
    match entry.device_type.as_str() {
        // 三族键盘的 HID 页面、帧头、命令字全都不同，必须按 `driver` 分派。
        "keyboard" => match entry.declared_driver() {
            Some("glw") => Some(&KEYBOARD_GLW),
            Some("dje") => Some(if wireless {
                &KEYBOARD_DJE_WIRELESS
            } else {
                &KEYBOARD_DJE_WIRED
            }),
            // `qhw`：报告 `0x4D` + 7 字节头（含 2 字节命令与自增序号）+ 尾校验。
            Some("qhw") => Some(&KEYBOARD_QHW),
            // 没登记 `driver` 的键盘（型号表缺字段）：不采集，也不退回任一默认族去猜。
            _ => None,
        },
        // 键盘与鼠标是**两套互不兼容的协议**：键盘走 CZ / `dJe` 家族，鼠标走 `LG` 家族
        // （`11 06` 且应答逐字节取反）。
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
        &KEYBOARD_GLW,
        &KEYBOARD_QHW,
        &KEYBOARD_DJE_WIRED,
        &KEYBOARD_DJE_WIRELESS,
    ],
    route: Some(route),
    ..MOUSE_WIRED
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{self, decode_battery, decode_listen};
    use crate::table::DeviceTable;

    fn table() -> DeviceTable {
        crate::mappings::load_table(SPEC.device_table_file).unwrap()
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
        assert_eq!(
            decode_listen(&MOUSE_WIRED, &bare).unwrap().charging,
            Some(false)
        );
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
        let query = AUDIO
            .request
            .expect("音频必须声明查询模板")
            .build(Some(0x55));
        assert_eq!(query.len(), 64);
        assert_eq!(&query[..3], &[0x55, 0x65, 0x01]);
        assert!(query[3..].iter().all(|b| *b == 0));
    }

    /// 分类分派：键盘按 `driver` 走三族之一；鼠标走 `LG` 族；音频走 0x55/0x65；未接入的返回 `None`。
    #[test]
    fn route_dispatches_by_driver_and_device_type() {
        let table = table();
        let spec_of = |vid, pid| SPEC.for_device(table.lookup(vid, pid).unwrap());

        // G75 V2（2.4G 接收器）：官方 CZ/GLW 族 —— 标准页 0x0001，命令 0x05 读功能配置块。
        let glw = spec_of(0x3837, 0x3033).expect("G75 V2 的 2.4G PID 必须能分派出协议");
        assert_eq!(glw.id, KEYBOARD_GLW.id, "G75 V2 2.4G 属 CZ/GLW 族");
        assert_eq!(glw.usage_page, 0x0001, "GLW 族挂在标准 Generic Desktop 页");
        assert_eq!(glw.usage_page_alt, Some(0xFF00), "boot 模式下用 0xFF00");
        assert_eq!(glw.request.unwrap().cmd, 0x05, "读功能配置块用命令 0x05");
        assert!(glw.listen.is_none(), "官方对 GLW 族缺省丢弃电量通知");

        // G75 V2（有线）与 Ace 60 同族。
        for (vid, pid) in [(0x3837, 0x2021), (0x19F5, 0xFC30)] {
            assert_eq!(spec_of(vid, pid).unwrap().id, KEYBOARD_GLW.id);
        }

        // G98 V3 / K87 V3 及其共用的 2.4G 接收器是 CZ/QHW 族：0xFF80 页面 + 报告 0x4D。
        for (vid, pid) in [(0x3837, 0x2025), (0x3837, 0x2023), (0x3837, 0x1015)] {
            let qhw = spec_of(vid, pid).expect("QHW 族必须能分派出协议");
            assert_eq!(qhw.id, KEYBOARD_QHW.id, "0x{vid:04X}:0x{pid:04X} 属 QHW 族");
            assert_eq!(qhw.usage_page, 0xFF80, "QHW 族挂在 0xFF80");
            assert_eq!(qhw.frame.report_id, Some(0x4D));
            let req = qhw.request.expect("QHW 族必须声明查询模板");
            assert_eq!(
                (req.cmd, req.cmd_high),
                (0x04, Some(0x09)),
                "电量命令 2308 = 0x0904，小端上线"
            );
        }

        // 老族（`dJe`）：不在 CZ 配置表里的键盘 PID 保持原有行为。
        let dje = spec_of(0x3837, 0x2019).expect("老族键盘必须仍可分派");
        assert_eq!(dje.id, KEYBOARD_DJE_WIRED.id);
        assert_eq!(dje.frame.report_id, Some(0x06));
        assert_eq!(dje.request.unwrap().cmd, 0x87);

        // 鼠标（2.4G）：A7 Pro —— 仍是鼠标那套 0x11/0x06，不受键盘分派改动影响。
        let wireless = spec_of(0x3837, 0x0010).unwrap();
        assert_eq!(wireless.frame.report_id, Some(0x11), "鼠标不走键盘通道");
        assert!(wireless.reassembly.is_none());
        assert_eq!(
            wireless.listen.unwrap().percent_index,
            5,
            "无线监听电量在 [5]"
        );

        let headset = spec_of(0x3837, 0x6001).unwrap();
        assert_eq!(headset.frame.report_id, Some(0x55));
        assert_eq!(headset.battery.cmd, Some(0x65));

        // 官方走另一种帧的两个型号：不采集。
        for (vid, pid) in [(0x3837, 0x6011), (0x3837, 0x6013)] {
            assert!(
                spec_of(vid, pid).is_none(),
                "0x{vid:04X}:0x{pid:04X} 需要 Gd/Mpe 帧，未接入"
            );
        }

        // 扩展坞无电量。
        assert!(spec_of(0x3837, 0x1012).is_none());
    }

    // ---- 键盘（CZ/GLW 族，含 G75 V2）----

    /// GLW 应答：64 字节整包，`AA 05 … size=56 lo=0 hi=0`，载荷从 `[8]` 起、电量在载荷 `[32]`。
    ///
    /// 电量与充电共用一个字节（官方 `rf_battery:127&e[32]`、`isCharging:e[32]>>7&1`）。
    fn glw_reply(percent: u8, charging: bool) -> Vec<u8> {
        let mut report = vec![0u8; 64];
        report[0] = 0xAA;
        report[1] = 0x05;
        report[4] = 56;
        report[8 + 32] = if charging { percent | 0x80 } else { percent };
        report
    }

    #[test]
    fn parses_glw_block_reply() {
        let idle = decode_battery(&KEYBOARD_GLW, &glw_reply(73, false)).unwrap();
        assert_eq!(idle.percent.get(), 73);
        assert_eq!(idle.charging, Some(false));

        // 充电时最高位被置上：不取位会读成 201 并被判成越界 —— 充电场景反而没有读数。
        let charging = decode_battery(&KEYBOARD_GLW, &glw_reply(73, true)).unwrap();
        assert_eq!(charging.percent.get(), 73);
        assert_eq!(charging.charging, Some(true));
    }

    /// 电量掩码是这条通道的命门：同一字节低 7 位电量、最高位充电。
    #[test]
    fn glw_percent_ignores_charging_bit() {
        assert_eq!(KEYBOARD_GLW.battery.percent_mask, 0x7F);
        assert_eq!(KEYBOARD_GLW.battery.percent_of(0x80 | 42), 42);
        assert_eq!(KEYBOARD_GLW.battery.percent_of(100), 100);
        // 取位后仍越界（>100）才判错：0x7F = 127 是坏数据，不是 100。
        let mut bad = glw_reply(0, false);
        bad[8 + 32] = 0x7F;
        assert_eq!(
            decode_battery(&KEYBOARD_GLW, &bad),
            Err(voltnest_provider::ErrorCode::PercentOutOfRange)
        );
    }

    /// 命令字不符的帧（例如键盘敲击报告）不得当成电量。
    #[test]
    fn glw_ignores_foreign_command() {
        let mut report = glw_reply(50, false);
        report[1] = 0x0A;
        assert_eq!(
            decode_battery(&KEYBOARD_GLW, &report),
            Err(voltnest_provider::ErrorCode::MalformedFrame)
        );
    }

    /// 查询报文与官方 `CmdQueueItem` 拼出的首包逐字节一致：
    /// `00 55 05 00 38 38 00 00 00` + 补 0 到 64。
    ///
    /// `[0]=0x00` 是 hidapi 对无编号报告要求的报告 ID 槽，`[1]=0x55` 才是官方 flag；
    /// 官方下发的是 `55 05 00 38 38 00 00 00`（WebHID 的 `sendReport(0, data)` 自己补槽）。
    #[test]
    fn glw_request_matches_official_template() {
        let req = KEYBOARD_GLW.request.expect("GLW 族必须声明查询模板");
        let query = req.build(None);
        assert_eq!(query.len(), 64, "官方 packetLength 缺省 64");
        assert_eq!(
            &query[..9],
            &[0x00, 0x55, 0x05, 0x00, 0x38, 0x38, 0x00, 0x00, 0x00],
            "报告 ID 槽 + 官方 flag + 命令 + 校验 + size + lo + hi + 保留"
        );
        assert!(query[9..].iter().all(|b| *b == 0), "尾部补 0");
        // 官方 getFuncConfig 走队列，不按一问一答重问。
        assert_eq!(req.retry, 0);
    }

    /// 应答首字节可能是平台补的报告 ID 槽：两种形态都要能解出来。
    ///
    /// hidapi 在带编号的集合上会把 ID 放在 `[0]`；官方解析器的输入则不含 ID。
    /// 这条容错是**唯一**能覆盖两种拓扑的地方，缺了它，位移一字节的应答会被静默丢掉。
    #[test]
    fn glw_reply_tolerates_leading_report_id() {
        let plain = glw_reply(64, false);
        assert_eq!(
            decode_battery(&KEYBOARD_GLW, &plain).unwrap().percent.get(),
            64
        );

        let mut shifted = vec![0x07];
        shifted.extend_from_slice(&plain);
        let reading = decode_battery(&KEYBOARD_GLW, &shifted).expect("位移一字节的应答也要能解");
        assert_eq!(reading.percent.get(), 64);

        // 前置过滤同样要放行：否则帧在进解析器之前就被静默丢弃，没有任何错误可查。
        assert!(frame::matches_cmd(&KEYBOARD_GLW, &shifted));
        assert!(frame::matches_cmd(&KEYBOARD_GLW, &plain));
    }

    // ---- 键盘（CZ/QHW 族，G98 V3 / K87 V3）----

    /// QHW 应答：`4D 01 03 <len> 04 09 00 <seq> <charge> <percent> <xor>`。
    ///
    /// 与官方 `parseHeaderBytes` 的字段顺序逐字节对齐（报告 ID 在 `[0]` 是 hidapi 的形态）。
    fn qhw_reply(charge: u8, percent: u8) -> Vec<u8> {
        let mut report = vec![
            0x4D, 0x01, 0x03, 0x02, 0x04, 0x09, 0x00, 0x00, charge, percent,
        ];
        let checksum = report[2..].iter().fold(0u8, |acc, b| acc ^ b);
        report.push(checksum);
        report
    }

    /// `getBattery` 的两个字段：`data[0]` 是三态充电状态、`data[1]` 才是电量。
    #[test]
    fn qhw_reply_maps_charge_tristate_and_percent() {
        // 未充电 / 充电中 / 已充满。`2`（充满）不报「正在充电」—— 插着线但充电已停止。
        for (raw, charging) in [(0u8, false), (1, true), (2, false)] {
            let reading = decode_battery(&KEYBOARD_QHW, &qhw_reply(raw, 63)).unwrap();
            assert_eq!(reading.percent.get(), 63, "电量在 data[1]");
            assert_eq!(reading.charging, Some(charging), "状态 {raw} 的充电判定");
        }
    }

    /// 低字节相同、高字节不同的是**别的命令**：只比低字节会把「读信息」的应答当电量解析。
    #[test]
    fn qhw_rejects_foreign_command_sharing_the_low_byte() {
        let mut report = qhw_reply(1, 50);
        report[5] = 0x0A;
        assert!(
            !frame::matches_cmd(&KEYBOARD_QHW, &report),
            "前置过滤必须按双字节命令拒绝"
        );
        match frame::decode_battery_detailed(&KEYBOARD_QHW, &report) {
            Err(reject) => assert_eq!(
                reject.reason,
                frame::RejectReason::CmdHighMismatch {
                    want: 0x09,
                    got: Some(0x0A),
                }
            ),
            Ok(reading) => panic!("别的命令被当成电量：{reading:?}"),
        }
    }

    /// 查询报文逐字节对齐官方 `Ge` 队列项：7 字节头 + 尾校验，尾部补 0 到整包。
    #[test]
    fn qhw_request_matches_official_template() {
        let req = KEYBOARD_QHW.request.expect("QHW 族必须声明查询模板");
        let query = req.build(KEYBOARD_QHW.frame.report_id);
        assert_eq!(query.len(), 32, "报告 ID + 31 字节报文");
        assert_eq!(
            &query[..9],
            &[0x4D, 0x01, 0x03, 0x00, 0x04, 0x09, 0x00, 0x00, 0x0E],
            "头 7 字节 + bizCode + sequence + 校验和 0x0E"
        );
        assert!(query[9..].iter().all(|b| *b == 0), "尾部补 0");
        // 校验和取 flags 起算（官方 `d.slice(1)` 跳过 version），不是整帧异或：
        // 3 ^ 4 ^ 9 = 0x0E，整帧（含 version）异或会得到 0x0F。
        assert_eq!(KEYBOARD_QHW_PAYLOAD[2], 0x0E, "校验和按官方算法实算");
    }

    /// qhw 的电量推送：`MANUAL_REPORT`（命令 `0x0700`）整块配置里 `data[2]` 充电、`data[3]` 电量。
    ///
    /// 官方 `parseManualReportBytes` 把 23 字节拆成配置字段：`n[2]` 走 `toChargingStatus`、
    /// `n[3]` 就是 `batteryLevel`。这与 `getBattery`(0x0904) 的 `data[0]/data[1]` 是**两套下标**。
    fn qhw_notify(charge: u8, percent: u8) -> Vec<u8> {
        let mut report = vec![0x4D, 0x01, 0x03, 23, 0x00, 0x07, 0x00, 0x00];
        let mut data = [0u8; 23];
        // `n[0..2]` 是官方 `Uh([a,i])` 拼出的双字节字段，取非零值以证明它不参与电量。
        data[0] = 0x37;
        data[1] = 0x38;
        data[2] = charge;
        data[3] = percent;
        report.extend_from_slice(&data);
        let checksum = report[2..].iter().fold(0u8, |acc, b| acc ^ b);
        report.push(checksum);
        report
    }

    #[test]
    fn qhw_notify_reads_charge_and_percent_from_manual_report() {
        for (raw, charging) in [(0u8, false), (1, true), (2, false)] {
            let reading = frame::decode_notify(&KEYBOARD_QHW, &qhw_notify(raw, 47))
                .unwrap_or_else(|| panic!("状态 {raw} 的推送帧必须能解析"));
            assert_eq!(reading.percent.get(), 47, "电量在 data[3]");
            assert_eq!(reading.charging, Some(charging), "状态 {raw} 的充电判定");
        }
    }

    /// 推送帧与应答帧的命令字不同，两条通道不得互相串味。
    ///
    /// 串味的后果不是坏帧而是**看起来正常的错值**：`0x0700` 的 `data[0]` 是配置块首字节，
    /// 当成电量会读出一个稳定但无关的数字。
    #[test]
    fn qhw_notify_and_reply_do_not_cross_decode() {
        let pushed = qhw_notify(1, 47);
        assert!(
            frame::decode_notify(&KEYBOARD_QHW, &pushed).is_some(),
            "推送帧必须由推送通道解析"
        );
        assert!(
            decode_battery(&KEYBOARD_QHW, &pushed).is_err(),
            "推送帧不得被当成查询应答"
        );
        assert!(
            !frame::matches_cmd(&KEYBOARD_QHW, &pushed),
            "查询通道的前置过滤必须拒掉推送帧"
        );

        let reply = qhw_reply(1, 63);
        assert!(
            frame::decode_notify(&KEYBOARD_QHW, &reply).is_none(),
            "应答帧不得被当成推送"
        );
    }

    /// 鼠标同样不周期查询：官方只在连接时发一次 `0x11 0x06`，之后订阅实时帧。
    ///
    /// 有线和 2.4G 共用同一份 `request`（`MOUSE_WIRELESS` 用 `..MOUSE_WIRED` 展开），
    /// 但两边的监听帧布局不同，两个都要检查。
    #[test]
    fn mouse_does_not_poll_periodically() {
        for spec in [&MOUSE_WIRED, &MOUSE_WIRELESS] {
            let req = spec.request.expect("鼠标必须声明连接时的查询");
            assert_eq!(req.interval_ms, None, "{} 不应周期查询", spec.label);
            assert!(spec.listen.is_some(), "{} 必须声明实时上报帧", spec.label);
            assert_eq!(req.cmd, 0x06, "{} 的连接时查询仍是 11 06", spec.label);
        }
    }

    /// 推送为主的协议不再按节奏轮询：常态下零查询，由设备自己推。
    #[test]
    fn qhw_does_not_poll_periodically() {
        let req = KEYBOARD_QHW.request.expect("QHW 族仍需要首帧查询");
        assert_eq!(
            req.interval_ms, None,
            "周期查询会唤醒固件耗电，qhw 改为「连接时问一次 + 越界时探活」"
        );
        assert!(KEYBOARD_QHW.notify.is_some(), "纯监听必须声明推送帧");
    }

    #[test]
    fn qhw_spec_validates() {
        assert!(KEYBOARD_QHW.validate().is_ok(), "QHW 族描述不合法");
    }

    // ---- 键盘（官方 `dJe` 老族）----

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
        let reading = decode_battery(&KEYBOARD_DJE_WIRED, &keyboard_wired_reply(73, 0x00)).unwrap();
        assert_eq!(reading.percent.get(), 73);
        assert_eq!(reading.charging, Some(false));

        let charging =
            decode_battery(&KEYBOARD_DJE_WIRED, &keyboard_wired_reply(73, 0x01)).unwrap();
        assert_eq!(charging.charging, Some(true));
    }

    /// 状态字节两个半字节各一义：高半字节满电、低半字节充电。
    /// 整字节比较会在 `0x11`（满电且充电）时漏判 —— `mask` 就是为这一位加的。
    #[test]
    fn keyboard_charge_low_nibble_ignores_full_flag() {
        let full_and_charging =
            decode_battery(&KEYBOARD_DJE_WIRED, &keyboard_wired_reply(100, 0x11)).unwrap();
        assert_eq!(full_and_charging.percent.get(), 100);
        assert_eq!(
            full_and_charging.charging,
            Some(true),
            "满电且充电（0x11）的低半字节仍是 1"
        );

        let full_only =
            decode_battery(&KEYBOARD_DJE_WIRED, &keyboard_wired_reply(100, 0x10)).unwrap();
        assert_eq!(full_only.charging, Some(false));
    }

    /// 全零应答（设备在场但没数据）：只能判成 `EmptyPayload`，不得降级为「0% 未充电」。
    #[test]
    fn keyboard_all_zero_reply_is_empty_payload_not_zero_percent() {
        let reading = decode_battery(&KEYBOARD_DJE_WIRED, &keyboard_wired_reply(0, 0x00));
        assert!(
            matches!(reading, Err(voltnest_provider::ErrorCode::EmptyPayload)),
            "全零应答不代表 0%"
        );
    }

    /// 有线查询报文必须与官方 520 字节模板逐字节一致：`06 87 00 00 01 00 02 00` + 512×0。
    #[test]
    fn keyboard_wired_request_matches_official_template() {
        let req = KEYBOARD_DJE_WIRED.request.expect("键盘必须声明查询模板");
        let query = req.build(Some(0x06));
        assert_eq!(query.len(), 520, "官方特征报告是 520 字节");
        assert_eq!(
            &query[..8],
            &[0x06, 0x87, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00]
        );
        assert!(query[8..].iter().all(|b| *b == 0x00));
        // 官方有线读是一次「发 + 收」，不重问。
        assert_eq!(req.retry, 0);
    }

    /// 无线查询报文：`13 4A 01 00×16 5E` —— 与官方「命令串 + XOR 校验和」逐字节一致。
    #[test]
    fn keyboard_wireless_request_matches_official_template() {
        let req = KEYBOARD_DJE_WIRELESS
            .request
            .expect("无线键盘必须声明查询模板");
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
        for spec in [&KEYBOARD_GLW, &KEYBOARD_DJE_WIRED, &KEYBOARD_DJE_WIRELESS] {
            assert!(spec.validate().is_ok(), "{} 结构不合法", spec.label);
        }
        // GLW 族的电量与充电同字节、靠掩码分开：这条不变量必须真的被校验器接受。
        assert_eq!(KEYBOARD_GLW.battery.percent_index, 32);
        assert_eq!(KEYBOARD_GLW.battery.charging.unwrap().index, 32);
    }
}
