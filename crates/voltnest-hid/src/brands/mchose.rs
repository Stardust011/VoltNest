//! 品牌：MCHOSE 多代 HID 电量协议。
//!
//! 0x3837/0x5253 使用 `0xFF01` feature 通道，命令体逐字节取反；
//! 0x2023 A5 Pro Max 使用 `0xFFFF` 的独立 feature 协议。

use crate::spec::{
    BatteryFields, BrandSpec, ChargingField, FrameLayout, ReplyTransport, RequestTemplate,
};

const MCHOSE_REQUEST_PAYLOAD: [u8; 19] = [0; 19];
const A5_REQUEST_PREFIX: [u8; 5] = [0, 0, 2, 2, 0];

/// MCHOSE A7 V2 等 0x3837 设备。
pub const A7_SPEC: BrandSpec = BrandSpec {
    id: "mchose-a7",
    label: "MCHOSE A7 V2",
    usage_page: 0xFF01,
    usage_page_write: Some(0xFF01),
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
        cmd: 0x06,
        subtag: None,
        percent_index: 9,
        charging: Some(ChargingField {
            index: 10,
            true_values: &[0x01],
        }),
    },
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
    }),
    expected_interval_ms: 30_000,
    device_table_json: include_str!("../../data/vbas/mchose_a7.json"),
};

/// MCHOSE M7/L7/A7/K7 等 0x5253 设备。
pub const M7_SPEC: BrandSpec = BrandSpec {
    id: "mchose-m7",
    label: "MCHOSE M7/L7 系列",
    usage_page: 0xFF01,
    usage_page_write: Some(0xFF01),
    transport: ReplyTransport::FeatureQuery { reply_len: 21 },
    reply_inverted: true,
    clamp_percent: false,
    frame: A7_SPEC.frame,
    battery: A7_SPEC.battery,
    request: A7_SPEC.request,
    expected_interval_ms: 30_000,
    device_table_json: include_str!("../../data/vbas/mchose_m7.json"),
};

/// MCHOSE A5 Pro Max 第一代设备。
pub const A5_SPEC: BrandSpec = BrandSpec {
    id: "mchose-a5",
    label: "MCHOSE A5 Pro Max",
    usage_page: 0xFFFF,
    usage_page_write: Some(0xFFFF),
    transport: ReplyTransport::FeatureQuery { reply_len: 65 },
    reply_inverted: false,
    clamp_percent: true,
    frame: FrameLayout {
        report_id: Some(0x00),
        cmd_index: 6,
        len_index: None,
        payload_offset: 7,
        fixed_payload_len: 2,
    },
    battery: BatteryFields {
        cmd: 0x83,
        subtag: None,
        percent_index: 1,
        charging: Some(ChargingField {
            index: 0,
            true_values: &[0x01],
        }),
    },
    request: Some(RequestTemplate {
        usage_page: 0xFFFF,
        cmd: 0x83,
        prefix: &A5_REQUEST_PREFIX,
        report_len: 65,
        cmd_index: 6,
        payload_offset: 7,
        payload: &[],
        interval_ms: Some(30_000),
        inverted: false,
    }),
    expected_interval_ms: 30_000,
    device_table_json: include_str!("../../data/vbas/mchose_a5.json"),
};

#[cfg(test)]
mod tests {
    use super::{A5_SPEC, A7_SPEC, M7_SPEC};
    use crate::frame::decode_battery;

    #[test]
    fn parses_inverted_mchose_status() {
        let plain = [0x11, 0x06, 0x53, 0x52, 0x31, 0, 2, 5, 7, 0, 9, 64, 0];
        let mut report = [0u8; 21];
        report[0] = 0x11;
        for (dst, byte) in report[1..].iter_mut().zip(plain[1..].iter()) {
            *dst = *byte ^ 0xFF;
        }
        let reading = decode_battery(&A7_SPEC, &report).unwrap();
        assert_eq!(reading.percent.get(), 64);
        assert_eq!(reading.charging, Some(false));
        assert!(M7_SPEC.validate().is_ok());
    }

    #[test]
    fn clamps_a5_level() {
        let mut report = [0u8; 65];
        report[6] = 0x83;
        report[7] = 1;
        report[8] = 120;
        assert_eq!(decode_battery(&A5_SPEC, &report).unwrap().percent.get(), 100);
    }
}
