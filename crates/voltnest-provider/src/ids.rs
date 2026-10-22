//! 设备标识与电量 newtype。
//!
//! `stableId` 只用最稳定的物理标识（Y-1..Y-6）：HID 不带 `mi`/`col`，
//! BLE 取配对后的 identity address，绝不使用设备名或注册表路径。

use crate::transport::Transport;
use std::fmt;
use std::str::FromStr;

/// 带 transport 前缀的稳定设备标识。
///
/// ```text
/// hid:7768:6426
/// ble:01:00:00:23:04:35
/// ```
///
/// 一旦落盘（`settings.tray.order` / `state.json`）即为用户数据键空间的一部分，
/// 改动必须伴随 `schemaVersion` 迁移（Y-2）。
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct StableId(String);

impl StableId {
    /// `hid:{vid:04x}:{pid:04x}`，小写十六进制，不含 `mi`/`col`（Y-2）。
    pub fn hid(vid: u16, pid: u16) -> Self {
        Self(format!("hid:{vid:04x}:{pid:04x}"))
    }

    /// `ble:{a:02x}:{b:02x}:...`，小写、冒号分隔（Y-4）。
    pub fn ble(mac: [u8; 6]) -> Self {
        let [a, b, c, d, e, f] = mac;
        Self(format!(
            "ble:{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{f:02x}"
        ))
    }

    /// 原始字符串。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 由前缀推导 transport。格式非法时返回 `None`（不猜测）。
    pub fn transport(&self) -> Option<Transport> {
        self.0
            .split_once(':')
            .and_then(|(p, _)| Transport::from_prefix(p))
    }

    /// 严格解析（ADR-0010 契约细节）。
    ///
    /// 只接受小写十六进制、`hid:` 后恰好两段 4 位、`ble:` 后恰好六段 2 位。
    /// **不做大小写宽容**：宽容会让 Y-3 的冲突检测漏判（同一设备出现两条大小写不同
    /// 的记录而不报警）。
    pub fn parse(raw: &str) -> Option<Self> {
        let (prefix, rest) = raw.split_once(':')?;
        let transport = Transport::from_prefix(prefix)?;
        let parts: Vec<&str> = rest.split(':').collect();
        let (want_parts, want_len) = match transport {
            Transport::Hid => (2, 4),
            Transport::Ble => (6, 2),
        };
        if parts.len() != want_parts {
            return None;
        }
        if !parts.iter().all(|p| {
            p.len() == want_len
                && p.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) {
            return None;
        }
        Some(Self(raw.to_string()))
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for StableId {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or(())
    }
}

/// 电量百分比 newtype：`> 100` 在构造期被拒绝。
///
/// `u8` 已让「负数」不可表达，因此规格里「percent < 0」这条在类型层消亡，
/// 实现侧只剩 `> 100` 与全零 payload 两种拒绝理由（ADR-0010）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Percent(u8);

impl Percent {
    /// 构造。`raw > 100` 返回 `None`，调用方必须映射为
    /// [`ErrorCode::PercentOutOfRange`](crate::ErrorCode::PercentOutOfRange)，
    /// 不得 `?` 后静默丢帧。
    pub fn new(raw: u8) -> Option<Self> {
        (raw <= 100).then_some(Self(raw))
    }

    /// 取值。
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl fmt::Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hid_id_is_lowercase_and_without_interface() {
        let id = StableId::hid(0x7768, 0x6426);
        assert_eq!(id.as_str(), "hid:7768:6426");
        assert_eq!(id.transport(), Some(Transport::Hid));
    }

    #[test]
    fn ble_id_is_colon_separated_lowercase() {
        let id = StableId::ble([0x01, 0x00, 0x00, 0x23, 0x04, 0x35]);
        assert_eq!(id.as_str(), "ble:01:00:00:23:04:35");
        assert_eq!(id.transport(), Some(Transport::Ble));
    }

    #[test]
    fn parse_accepts_canonical_forms() {
        for raw in ["hid:7768:6426", "ble:01:00:00:23:04:35"] {
            let id = StableId::parse(raw).expect("规范形式必须可解析");
            assert_eq!(id.as_str(), raw);
        }
    }

    #[test]
    fn parse_is_strict_about_case_and_width() {
        // Y-3 依赖严格解析；宽容会让冲突检测漏判。
        assert!(
            StableId::parse("hid:7768:6426:mi_01col02").is_none(),
            "不得带 mi/col"
        );
        assert!(StableId::parse("HID:7768:6426").is_none(), "前缀必须小写");
        assert!(
            StableId::parse("hid:7768:06426").is_none(),
            "段宽必须恰好 4 位"
        );
        assert!(StableId::parse("hid:776:6426").is_none());
        assert!(StableId::parse("hid:7768").is_none());
        assert!(
            StableId::parse("ble:01:00:00:23:04").is_none(),
            "BLE 必须六段"
        );
        assert!(
            StableId::parse("ble:1:0:0:23:4:35").is_none(),
            "段宽必须恰好 2 位"
        );
        assert!(StableId::parse("ble:01:00:00:23:04:3Z").is_none());
        assert!(StableId::parse("usb:7768:6426").is_none());
        assert!(StableId::parse("").is_none());
    }

    #[test]
    fn percent_rejects_out_of_range() {
        assert_eq!(Percent::new(0).map(Percent::get), Some(0));
        assert_eq!(Percent::new(100).map(Percent::get), Some(100));
        assert!(Percent::new(101).is_none());
        assert!(Percent::new(255).is_none());
    }

    #[test]
    fn from_str_matches_parse() {
        assert!("hid:7768:6426".parse::<StableId>().is_ok());
        assert!("hid:7768:6426".parse::<StableId>().unwrap().as_str() == "hid:7768:6426");
        assert!("nope".parse::<StableId>().is_err());
    }
}
