//! 设备状态：五种，且只有五种（规格 §5）。
//!
//! `Unknown` 与 `Unavailable` UI 必须可区分（E）：前者从未拿到数据，后者曾经拿到、当前拿不到。

use serde::{Deserialize, Serialize};

/// 设备状态。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceState {
    /// 尚未收到有效读数（含重启回填后的初始态）。
    Unknown,
    /// 最近收到有效读数。
    Online,
    /// 超过 `staleAfter` 未收到有效读数。
    Stale,
    /// 曾有有效读数，但当前读取失败。
    Unavailable,
    /// 链路消失或超过 `offlineAfter`。
    Offline,
}

impl DeviceState {
    /// 落盘 / 快照字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Online => "online",
            Self::Stale => "stale",
            Self::Unavailable => "unavailable",
            Self::Offline => "offline",
        }
    }

    /// L2 分组谓词之一（§9.2）：`Online` / `Stale` 仍有可展示读数；`Unavailable` 数据不可信，算非活跃。
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Online | Self::Stale)
    }

    /// 严格解析字符串，非小写枚举值返回 `None`。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "unknown" => Some(Self::Unknown),
            "online" => Some(Self::Online),
            "stale" => Some(Self::Stale),
            "unavailable" => Some(Self::Unavailable),
            "offline" => Some(Self::Offline),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_round_trip() {
        for s in [
            DeviceState::Unknown,
            DeviceState::Online,
            DeviceState::Stale,
            DeviceState::Unavailable,
            DeviceState::Offline,
        ] {
            assert_eq!(DeviceState::parse(s.as_str()), Some(s));
        }
        assert_eq!(DeviceState::parse("Online"), None);
    }

    #[test]
    fn only_online_and_stale_are_active() {
        assert!(DeviceState::Online.is_active());
        assert!(DeviceState::Stale.is_active());
        assert!(!DeviceState::Unknown.is_active());
        assert!(!DeviceState::Unavailable.is_active());
        assert!(!DeviceState::Offline.is_active());
    }

    #[test]
    fn serializes_lowercase() {
        let json = serde_json::to_string(&DeviceState::Unavailable).unwrap();
        assert_eq!(json, "\"unavailable\"");
    }
}
