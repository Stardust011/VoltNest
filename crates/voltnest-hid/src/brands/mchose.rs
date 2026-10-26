//! 品牌：MCHOSE。
//!
//! 单一 spec 覆盖全部 MCHOSE 型号：`0xFF01` feature 通道、命令体逐字节取反。

use crate::spec::{
    BatteryFields, BrandSpec, ChargingField, FrameLayout, ReplyTransport, RequestTemplate,
};

const MCHOSE_REQUEST_PAYLOAD: [u8; 19] = [0; 19];

/// MCHOSE 全系设备。
pub const SPEC: BrandSpec = BrandSpec {
    id: "mchose",
    label: "MCHOSE",
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
    device_table_json: include_str!("../../data/mappings/mchose.json"),
};

#[cfg(test)]
mod tests {
    use super::SPEC;
    use crate::frame::decode_battery;
    use crate::table::DeviceTable;

    #[test]
    fn parses_inverted_mchose_status() {
        let plain = [0x11, 0x06, 0x53, 0x52, 0x31, 0, 2, 5, 7, 0, 9, 64, 0];
        let mut report = [0u8; 21];
        report[0] = 0x11;
        for (dst, byte) in report[1..].iter_mut().zip(plain[1..].iter()) {
            *dst = *byte ^ 0xFF;
        }
        let reading = decode_battery(&SPEC, &report).unwrap();
        assert_eq!(reading.percent.get(), 64);
        assert_eq!(reading.charging, Some(false));
        assert!(SPEC.validate().is_ok());
    }

    #[test]
    fn device_table_covers_all_vendor_ids() {
        let table = DeviceTable::parse(SPEC.device_table_json).unwrap();
        for vid in [0x3837, 0x5253, 0x41E4, 0x19F5, 0x0E6A, 0x291D] {
            assert!(table.known_vid(vid), "缺少 VID {vid:04X}");
        }
    }
}
