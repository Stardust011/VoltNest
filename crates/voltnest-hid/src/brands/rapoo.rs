//! 品牌：RAPOO VT9 Pro（参考 OpenMouse mouse-protocol 的 feature 状态块）。
//!
//! `0xFF0B:0x0104` 的 feature report `0x2A` 含电量；该路径不提供可核验的充电标志。

use crate::spec::{BatteryFields, BrandSpec, FrameLayout, ReplyTransport};

/// RAPOO VT9 Pro（第一代）的协议描述。
pub const SPEC: BrandSpec = BrandSpec {
    id: "rapoo",
    label: "RAPOO VT9 Pro（第一代）",
    usage_page: 0xFF0B,
    usage_page_write: None,
    transport: ReplyTransport::FeaturePoll { reply_len: 65 },
    reply_inverted: false,
    clamp_percent: false,
    frame: FrameLayout {
        report_id: Some(0x2A),
        cmd_index: 1,
        len_index: None,
        payload_offset: 9,
        fixed_payload_len: 1,
    },
    battery: BatteryFields {
        cmd: 0x01,
        subtag: None,
        percent_index: 0,
        charging: None,
    },
    request: None,
    expected_interval_ms: 30_000,
    device_table_json: include_str!("../../data/vbas/rapoo.json"),
};

#[cfg(test)]
mod tests {
    use super::SPEC;
    use crate::frame::decode_battery;

    #[test]
    fn parses_feature_status_block() {
        let mut report = [0u8; 65];
        report[..11].copy_from_slice(&[0x2A, 0x01, 0, 0, 4, 0x21, 0x50, 0, 0, 98, 0]);
        assert_eq!(decode_battery(&SPEC, &report).unwrap().percent.get(), 98);
        assert_eq!(decode_battery(&SPEC, &report).unwrap().charging, None);
    }
}
