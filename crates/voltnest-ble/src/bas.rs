//! Battery Service / GAP 特征 UUID 与解析。

use voltnest_provider::{ErrorCode, Percent};

/// Battery Service。
pub const SERVICE_BATTERY: u16 = 0x180F;
/// Battery Level（READ + NOTIFY）。
pub const CHAR_BATTERY_LEVEL: u16 = 0x2A19;
/// Generic Access Service，设备名来源。
pub const SERVICE_GENERIC_ACCESS: u16 = 0x1800;
/// GAP Device Name。
pub const CHAR_GAP_DEVICE_NAME: u16 = 0x2A00;
/// GAP Appearance：**只用于设备类型识别**，不参与电量与状态判定。
pub const CHAR_GAP_APPEARANCE: u16 = 0x2A01;
/// Battery Level Status：**只用于诊断展示**，失真，不进入状态机。
pub const CHAR_BATTERY_LEVEL_STATUS: u16 = 0x2BED;

/// 解析 GAP `0x2A01` Appearance：16 位枚举（`category << 6 | subcategory`），小端序传输。
/// 只有恰好两个字节才算有效；长度不符即 `None`，绝不补默认值。只做长度与字节序的机械解析，
/// 语义映射集中在 [`voltnest_provider::classify`]，让「标准数值」与「应用层分类」两件事分开可核对。
pub fn parse_appearance(bytes: &[u8]) -> Option<u16> {
    match bytes.len() {
        2 => Some(u16::from_le_bytes([bytes[0], bytes[1]])),
        _ => None,
    }
}

/// 解析 `0x2A19` 读数：`[0x64]` → 100%，`[0x00]` → 0%。
///
/// 单字节 `0x00` 是合法的 0% 而非「全零 payload」；HID 侧「全零 payload 判不可读」针对三字节 payload，
/// 套用在此会把真没电的设备判成不可读。长度 > 1 且全零才按不可读处理。
pub fn parse_battery_level(bytes: &[u8]) -> Result<Percent, ErrorCode> {
    if bytes.is_empty() {
        return Err(ErrorCode::EmptyPayload);
    }
    if bytes.len() > 1 {
        if bytes.iter().all(|b| *b == 0) {
            return Err(ErrorCode::EmptyPayload);
        }
        // 标准特征只有 1 字节；多余字节说明读错了特征或布局变化，不猜。
        return Err(ErrorCode::MalformedFrame);
    }
    Percent::new(bytes[0]).ok_or(ErrorCode::PercentOutOfRange)
}

/// 解析 GAP `0x2A00` 设备名：去尾部 NUL 与首尾空白。
/// `model_name_raw` 原文由 Provider 上报，`modelKey` 归一化只在宿主做；此处只为避免填充字节进入展示名。
pub fn parse_device_name(bytes: &[u8]) -> Option<String> {
    let text: String = bytes
        .iter()
        .copied()
        .take_while(|b| *b != 0)
        .map(|b| b as char)
        .collect();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_boundary_values() {
        assert_eq!(parse_battery_level(&[0]).unwrap().get(), 0);
        assert_eq!(parse_battery_level(&[1]).unwrap().get(), 1);
        assert_eq!(parse_battery_level(&[90]).unwrap().get(), 90);
        assert_eq!(parse_battery_level(&[100]).unwrap().get(), 100);
    }

    #[test]
    fn zero_is_valid_not_empty() {
        // 单字节 0x00 是 0%，不得判成「全零 payload」。
        assert_eq!(parse_battery_level(&[0x00]).unwrap().get(), 0);
    }

    #[test]
    fn rejects_out_of_range() {
        for bad in [101u8, 200, 255] {
            assert_eq!(
                parse_battery_level(&[bad]),
                Err(ErrorCode::PercentOutOfRange)
            );
        }
    }

    #[test]
    fn rejects_empty_and_multi_byte() {
        assert_eq!(parse_battery_level(&[]), Err(ErrorCode::EmptyPayload));
        assert_eq!(
            parse_battery_level(&[0x00, 0x00]),
            Err(ErrorCode::EmptyPayload)
        );
        assert_eq!(
            parse_battery_level(&[0x5A, 0x00]),
            Err(ErrorCode::MalformedFrame)
        );
    }

    #[test]
    fn name_drops_nul_padding() {
        // 实测：0x2A00 = "AJ159 NL" + 补零。
        let raw = b"AJ159 NL\0\0\0\0\0\0\0\0";
        assert_eq!(parse_device_name(raw).as_deref(), Some("AJ159 NL"));
    }

    #[test]
    fn name_trims_whitespace_and_rejects_blank() {
        assert_eq!(
            parse_device_name(b"  AJ159 NL \0").as_deref(),
            Some("AJ159 NL")
        );
        assert_eq!(parse_device_name(b"\0\0"), None);
        assert_eq!(parse_device_name(b"   \0"), None);
        assert_eq!(parse_device_name(&[]), None);
    }

    #[test]
    fn uuids_match_standard() {
        assert_eq!(SERVICE_BATTERY, 0x180F);
        assert_eq!(CHAR_BATTERY_LEVEL, 0x2A19);
        assert_eq!(SERVICE_GENERIC_ACCESS, 0x1800);
        assert_eq!(CHAR_GAP_DEVICE_NAME, 0x2A00);
        assert_eq!(CHAR_GAP_APPEARANCE, 0x2A01);
        assert_eq!(CHAR_BATTERY_LEVEL_STATUS, 0x2BED);
    }

    #[test]
    fn appearance_is_little_endian_and_length_checked() {
        // HID Mouse 0x03C2 → 线上字节 [0xC2, 0x03]。
        assert_eq!(parse_appearance(&[0xC2, 0x03]), Some(0x03C2));
        assert_eq!(parse_appearance(&[0x00, 0x00]), Some(0x0000));
        assert_eq!(parse_appearance(&[0x41, 0x08]), Some(0x0841));
        // 长度不符：不补零、不截断。
        assert_eq!(parse_appearance(&[]), None);
        assert_eq!(parse_appearance(&[0xC2]), None);
        assert_eq!(parse_appearance(&[0xC2, 0x03, 0x00]), None);
    }
}
