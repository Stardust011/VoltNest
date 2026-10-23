//! Provider → 宿主的事件分类：Provider 对事件做分类，宿主不做数据校验。
//!
//! 三点关键设计：① [`ValidReading::battery`] 是 `Percent`，越界值在类型上不可能出现。
//! ② [`ProviderEvent::Unreadable`] 与 [`ProviderEvent::LinkLost`] 必须分开：前者「设备还在、数据不可信」→
//! `Unavailable`，后者「链路没了」→ `Offline`；合并会让「链路消失立即 Offline」与「解析失败判 Unavailable」互相打架。
//! ③ 事件不带时间戳；宿主收到时每批取一次 `now_utc_ms()` 向下透传，Provider 完全不参与状态判定的时间基准。

use crate::classify::DeviceClassification;
use crate::error::ErrorCode;
use crate::ids::{Percent, StableId};
use crate::transport::Transport;

/// 设备出现的原始事实（一次枚举命中）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeviceFact {
    /// `stableId`。
    pub stable_id: StableId,
    /// 传输层。
    pub transport: Transport,
    /// 协议家族 ID（`ProviderProfile::family_id`）。
    pub provider: &'static str,
    /// 表查得的型号名（HID）/ GAP `0x2A00` 原文（BLE）。必须是可读型号名；
    /// HID 不得直接回填 ProductString（8 型号全同，无法区分）。
    pub display_name: String,
    /// 未经归一化的型号原文。宿主用 `normalize()` 算 `modelKey`（D4）。
    pub model_name_raw: String,
    /// 诊断用的设备路径（HID `open_path` 目标 / BLE 地址），不参与身份判定。
    pub path: Option<String>,
    /// 设备类型识别结果（含证据）。默认 [`DeviceClassification::unknown`]：证据不足即 Unknown，
    /// 不得为好看填类型。与电量采集完全解耦：本字段只影响展示与诊断，不参与状态判定。
    pub classification: DeviceClassification,
}

impl DeviceFact {
    /// 便捷构造（测试与 Provider 共用）。
    pub fn new(
        stable_id: StableId,
        transport: Transport,
        provider: &'static str,
        display_name: impl Into<String>,
        model_name_raw: impl Into<String>,
    ) -> Self {
        Self {
            stable_id,
            transport,
            provider,
            display_name: display_name.into(),
            model_name_raw: model_name_raw.into(),
            path: None,
            classification: DeviceClassification::unknown(),
        }
    }

    /// 追加诊断路径。
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// 附带设备类型识别结果。
    pub fn with_classification(mut self, classification: DeviceClassification) -> Self {
        self.classification = classification;
        self
    }
}

/// 一次有效读数。构造成功即代表「电量可信且充电语义已知或明确不可得」。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ValidReading {
    /// 设备标识。
    pub stable_id: StableId,
    /// 电量。
    pub battery: Percent,
    /// 充电状态。Profile 声明 [`ChargingSupport::Unavailable`](crate::ChargingSupport)
    /// 时必须为 `None`。
    pub charging: Option<bool>,
}

impl ValidReading {
    /// 构造。
    pub fn new(stable_id: StableId, battery: Percent, charging: Option<bool>) -> Self {
        Self {
            stable_id,
            battery,
            charging,
        }
    }

    /// 设备标识。
    pub fn stable_id(&self) -> &StableId {
        &self.stable_id
    }
}

/// 物理消失的原因，仅进诊断（不影响状态语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemovalReason {
    /// 用户拔掉接收器 / 设备断电后被系统移除。
    Unplugged,
    /// 枚举不再返回该设备。
    EnumerationGone,
    /// 会话关闭（BLE）。
    SessionClosed,
}

impl RemovalReason {
    /// 稳定字符串，进诊断页。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unplugged => "unplugged",
            Self::EnumerationGone => "enumeration_gone",
            Self::SessionClosed => "session_closed",
        }
    }
}

/// Provider 上报的事件集合。
///
/// `voltnest-core` 必须**穷尽匹配**（不得用 `_ =>` 兜底），新增变体必然编译失败。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ProviderEvent {
    /// 设备出现 → 建立设备记录，状态 `Unknown`。
    Arrived(DeviceFact),
    /// 有效读数 → 直接 `Online`，不经中间态。
    Reading(ValidReading),
    /// 设备还在但数据不可信 → `Unavailable`（保留 `lastValidAt`）。
    Unreadable {
        /// 设备标识。
        stable_id: StableId,
        /// 错误码。
        code: ErrorCode,
        /// **诊断细节**：哪条校验失败 + 被拒的原始字节（HID 坏帧）或失败上下文。
        ///
        /// 只写诊断页与日志，不参与任何状态判定——状态机看到的仍只有 `code`。
        /// `None` = 该情形无可用报文（例如打开句柄失败）。
        detail: Option<String>,
    },
    /// 链路没了 → 立即 `Offline`。
    LinkLost {
        /// 设备标识。
        stable_id: StableId,
        /// 错误码。
        code: ErrorCode,
    },
    /// 物理移除 → `Offline` + 销毁托盘图标（唯一允许销毁图标的路径）。
    Removed {
        /// 设备标识。
        stable_id: StableId,
        /// 移除原因（仅诊断）。
        reason: RemovalReason,
    },
    /// 整个 transport 不可用（线程 panic、初始化失败）→ 该 transport 全部设备转 `Offline`。
    TransportDown {
        /// 错误码。
        code: ErrorCode,
        /// 人类可读细节。
        detail: String,
    },
    /// 同一轮发现重复 `stableId` → 只告警，不建第二条记录。
    BindingConflict {
        /// 设备标识。
        stable_id: StableId,
        /// 人类可读细节。
        detail: String,
    },
}

impl ProviderEvent {
    /// 事件涉及的设备（`TransportDown` 为 `None`）。
    pub fn stable_id(&self) -> Option<&StableId> {
        match self {
            Self::Arrived(f) => Some(&f.stable_id),
            Self::Reading(r) => Some(r.stable_id()),
            Self::Unreadable { stable_id, .. }
            | Self::LinkLost { stable_id, .. }
            | Self::Removed { stable_id, .. }
            | Self::BindingConflict { stable_id, .. } => Some(stable_id),
            Self::TransportDown { .. } => None,
        }
    }

    /// 事件类别名，用于日志前缀与队列深度告警。
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Arrived(_) => "arrived",
            Self::Reading(_) => "reading",
            Self::Unreadable { .. } => "unreadable",
            Self::LinkLost { .. } => "link_lost",
            Self::Removed { .. } => "removed",
            Self::TransportDown { .. } => "transport_down",
            Self::BindingConflict { .. } => "binding_conflict",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading() -> ValidReading {
        ValidReading::new(
            StableId::hid(0x7768, 0x6426),
            Percent::new(90).unwrap(),
            Some(false),
        )
    }

    #[test]
    fn reading_keeps_identity_and_payload() {
        let r = reading();
        assert_eq!(r.stable_id().as_str(), "hid:7768:6426");
        assert_eq!(r.battery.get(), 90);
        assert_eq!(r.charging, Some(false));
    }

    #[test]
    fn every_event_exposes_kind_and_optional_id() {
        let id = StableId::hid(0x7768, 0x6426);
        let events = [
            ProviderEvent::Arrived(DeviceFact::new(
                id.clone(),
                Transport::Hid,
                "hid-vbas",
                "AJ159 NL",
                "AJ159 NL",
            )),
            ProviderEvent::Reading(reading()),
            ProviderEvent::Unreadable {
                stable_id: id.clone(),
                code: ErrorCode::MalformedFrame,
                detail: Some("cmd_mismatch(want=0x03 got=0x23) · 6B: 06 FA 23 00 5A 01".into()),
            },
            ProviderEvent::LinkLost {
                stable_id: id.clone(),
                code: ErrorCode::DeviceGone,
            },
            ProviderEvent::Removed {
                stable_id: id.clone(),
                reason: RemovalReason::Unplugged,
            },
            ProviderEvent::TransportDown {
                code: ErrorCode::Internal,
                detail: "panic".into(),
            },
            ProviderEvent::BindingConflict {
                stable_id: id.clone(),
                detail: "dup".into(),
            },
        ];
        let kinds: Vec<_> = events.iter().map(ProviderEvent::kind).collect();
        assert_eq!(
            kinds,
            [
                "arrived",
                "reading",
                "unreadable",
                "link_lost",
                "removed",
                "transport_down",
                "binding_conflict"
            ]
        );
        assert!(events[5].stable_id().is_none());
        assert_eq!(
            events[0].stable_id().map(StableId::as_str),
            Some("hid:7768:6426")
        );
    }

    #[test]
    fn removal_reason_strings_are_stable() {
        assert_eq!(RemovalReason::Unplugged.as_str(), "unplugged");
        assert_eq!(RemovalReason::EnumerationGone.as_str(), "enumeration_gone");
        assert_eq!(RemovalReason::SessionClosed.as_str(), "session_closed");
    }

    #[test]
    fn fact_path_is_optional() {
        let f = DeviceFact::new(
            StableId::ble([1, 0, 0, 0x23, 4, 0x35]),
            Transport::Ble,
            "ble-bas",
            "AJ159 NL",
            "AJ159 NL",
        );
        assert!(f.path.is_none());
        assert!(f.with_path("ble:010000230435").path.is_some());
    }

    #[test]
    fn fact_defaults_to_unknown_classification() {
        let f = DeviceFact::new(
            StableId::hid(0x7768, 0x6426),
            Transport::Hid,
            "hid-vbas",
            "AJ159 NL",
            "AJ159 NL",
        );
        assert!(f.classification.is_unknown());
        assert_eq!(f.classification.device_type, crate::DeviceType::Unknown);

        let classified = f.with_classification(DeviceClassification::new(
            crate::DeviceType::Mouse,
            crate::ClassificationMethod::VerifiedDeviceMapping,
            vec!["可核验映射".into()],
        ));
        assert_eq!(
            classified.classification.device_type,
            crate::DeviceType::Mouse
        );
        assert_eq!(classified.classification.evidence.len(), 1);
    }
}
