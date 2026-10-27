//! 设备记录：每个 transport 一条，永不合并。
//!
//! 记录持有身份、读数、状态与新鲜度；呈现分组只读取它，不改这里的任何字段。

use crate::state::DeviceState;
use voltnest_provider::{DeviceClassification, ErrorCode, Percent, StableId, Transport};

/// 单台设备在一次「出现」里最多记住多少种坏帧（见 [`DeviceRecord::unreadable_kinds`]）。
///
/// 兜住「每帧字节都不同」的极端情况（如帧内带计数器）：达上限后新种类不再留痕，
/// 宁可少一条诊断，也不让环形缓冲被刷空。
pub const UNREADABLE_KINDS_MAX: usize = 32;

/// 一个 transport 上的一台设备。
#[derive(Clone, Debug)]
pub struct DeviceRecord {
    /// 稳定标识。
    pub stable_id: StableId,
    /// 传输层。
    pub transport: Transport,
    /// 协议家族 ID。
    pub family_id: &'static str,
    /// 表查得的型号名（HID）/ GAP `0x2A00` 原文（BLE），由 Provider 上报。
    pub display_name: String,
    /// 未归一化的型号原文。
    pub model_name_raw: String,
    /// 宿主计算的归一化键。`None` = 查表未命中 / 名称为空。
    pub model_key: Option<String>,
    /// 设备类型识别结果（含证据）。默认 `Unknown`：不参与状态判定，只影响展示与诊断；
    /// 重启后未收到 `Arrived` 前恒为 `Unknown`（不落盘）。
    pub classification: DeviceClassification,
    /// 当前有效电量。`None` 表示当前无有效读数，**绝不**用 0 代替。
    pub battery: Option<Percent>,
    /// 上次已知电量，仅用于启动后的副行展示。
    pub last_known_battery: Option<Percent>,
    /// 上次已知电量的时间戳。
    pub last_known_at_ms: Option<u64>,
    /// 充电状态。`None` = 未知（BLE 恒为 `None`）。
    pub charging: Option<bool>,
    /// 状态。
    pub state: DeviceState,
    /// 唯一退化依据。
    pub last_valid_at_ms: Option<u64>,
    /// 最近一次结构化错误。
    pub last_error: Option<ErrorCode>,
    /// 本次「出现」以来记录过的坏帧诊断文本（错误码 + 成因 + 原始字节）。
    ///
    /// 只用于去重：坏帧可能每 20ms 一条，重复内容会挤掉其它线索。按「不同种类各留一条」而非
    /// 「与上一条比」——同一设备常轮流发几种非电量报文，只比上一条挡不住交替刷屏。
    /// 读到有效帧不清空（好坏交替同理），仅设备重新出现（`Offline` → `Unknown`）才清空。
    pub unreadable_kinds: Vec<String>,
    /// 物理存在。只由 `Arrived` / `Removed` 改变，不随 `state` 变化。
    pub present: bool,
    /// 诊断用设备路径。
    pub path: Option<String>,
    /// 首次发现时间。
    pub first_seen_at_ms: u64,
    /// 最近一次出现在枚举中的时间。
    pub last_seen_at_ms: u64,
    /// 冲突告警（保留先发现者，不建第二条）。
    pub conflict: Option<String>,
}

impl DeviceRecord {
    /// 新建一条 `Unknown` 记录。
    pub fn new(
        stable_id: StableId,
        transport: Transport,
        family_id: &'static str,
        display_name: impl Into<String>,
        model_name_raw: impl Into<String>,
        now_ms: u64,
    ) -> Self {
        let raw = model_name_raw.into();
        let model_key = crate::normalize::normalize(&raw);
        Self {
            stable_id,
            transport,
            family_id,
            display_name: display_name.into(),
            model_name_raw: raw,
            model_key,
            classification: DeviceClassification::unknown(),
            battery: None,
            last_known_battery: None,
            last_known_at_ms: None,
            charging: None,
            state: DeviceState::Unknown,
            last_valid_at_ms: None,
            last_error: None,
            unreadable_kinds: Vec::new(),
            present: true,
            path: None,
            first_seen_at_ms: now_ms,
            last_seen_at_ms: now_ms,
            conflict: None,
        }
    }

    /// 距最后一次有效读数的毫秒数；从未有有效读数时为 `None`。
    ///
    /// 用 `saturating_sub`：系统时间后跳时不得下溢成天文数字。
    pub fn age_ms(&self, now_ms: u64) -> Option<u64> {
        self.last_valid_at_ms.map(|t| now_ms.saturating_sub(t))
    }

    /// 该记录是否还该有托盘图标（只由物理存在决定）。
    pub fn has_tray_icon(&self, hidden: bool) -> bool {
        self.present && !hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> DeviceRecord {
        DeviceRecord::new(
            StableId::hid(0x7768, 0x6426),
            Transport::Hid,
            "hid-vbas",
            "AJ159 NL",
            "AJ159 NL\0",
            1_000,
        )
    }

    #[test]
    fn new_record_is_unknown_without_battery() {
        let r = rec();
        assert_eq!(r.state, DeviceState::Unknown);
        assert!(r.battery.is_none(), "不得把「没读到」渲染成 0%");
        assert_eq!(r.model_key.as_deref(), Some("aj159 nl"));
        assert!(r.present);
    }

    #[test]
    fn age_uses_saturating_sub() {
        let mut r = rec();
        assert_eq!(r.age_ms(5_000), None);
        r.last_valid_at_ms = Some(10_000);
        assert_eq!(r.age_ms(15_000), Some(5_000));
        // 系统时间后跳：不得下溢。
        assert_eq!(r.age_ms(1), Some(0));
    }

    #[test]
    fn tray_icon_follows_presence_not_state() {
        let mut r = rec();
        r.state = DeviceState::Offline;
        assert!(r.has_tray_icon(false), "Offline 但物理存在仍保留图标");
        assert!(!r.has_tray_icon(true));
        r.present = false;
        assert!(!r.has_tray_icon(false), "物理消失才销毁");
    }
}
