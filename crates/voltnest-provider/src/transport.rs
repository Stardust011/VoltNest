//! 传输层枚举。

/// 设备接入的物理传输层。`stableId` 的前缀与之一一对应。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Transport {
    /// 2.4G / NearLink 接收器，走 HID 厂商通道。
    Hid,
    /// 标准 BLE Battery Service。
    Ble,
}

impl Transport {
    /// 小写字符串形式，用作 `stableId` 前缀与 `Snapshot.transport`。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hid => "hid",
            Self::Ble => "ble",
        }
    }

    /// 用户可见的链路名称。展示措辞归宿主，但链路词汇是协议事实，由本类型统一提供以避免两处漂移。
    pub const fn display_label(self) -> &'static str {
        match self {
            Self::Hid => "2.4G",
            Self::Ble => "蓝牙",
        }
    }

    /// 解析 `stableId` 前缀。
    pub fn from_prefix(prefix: &str) -> Option<Self> {
        match prefix {
            "hid" => Some(Self::Hid),
            "ble" => Some(Self::Ble),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_stable() {
        assert_eq!(Transport::Hid.as_str(), "hid");
        assert_eq!(Transport::Ble.as_str(), "ble");
        assert_eq!(Transport::Hid.display_label(), "2.4G");
        assert_eq!(Transport::Ble.display_label(), "蓝牙");
    }

    #[test]
    fn prefix_round_trip() {
        for t in [Transport::Hid, Transport::Ble] {
            assert_eq!(Transport::from_prefix(t.as_str()), Some(t));
        }
        assert_eq!(Transport::from_prefix("usb"), None);
        assert_eq!(Transport::from_prefix("HID"), None);
    }
}
