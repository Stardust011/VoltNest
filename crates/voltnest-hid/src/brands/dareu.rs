//! 品牌：达尔优 Dareu A950 PRO Mg（Jm 协议）。
//!
//! 报告 ID `0x08` 的 16 字节协议帧来自 OpenMouse mouse-protocol 的 TM265 取证；
//! 查询校验和由厂商帧固定为 `0x49`，这里通过 RequestTemplate 发送完整报告。

use crate::spec::{
    BatteryFields, BrandSpec, ChargingField, FrameLayout, ReplyTransport, RequestTemplate,
};

/// 达尔优 A950 PRO Mg 的协议描述。
pub const SPEC: BrandSpec = BrandSpec {
    id: "dareu",
    label: "达尔优 A950 PRO Mg",
    usage_page: 0xFF02,
    usage_page_write: Some(0xFF02),
    transport: ReplyTransport::InterruptQuery,
    reply_inverted: false,
    clamp_percent: false,
    frame: FrameLayout {
        report_id: Some(0x08),
        cmd_index: 1,
        len_index: None,
        payload_offset: 6,
        fixed_payload_len: 2,
    },
    battery: BatteryFields {
        cmd: 0x04,
        subtag: None,
        percent_index: 0,
        charging: Some(ChargingField {
            index: 1,
            true_values: &[0x01, 0x02],
        }),
    },
    request: Some(RequestTemplate {
        usage_page: 0xFF02,
        cmd: 0x04,
        prefix: &[],
        report_len: 17,
        cmd_index: 1,
        payload_offset: 2,
        payload: &[],
        interval_ms: Some(30_000),
        inverted: false,
    }),
    expected_interval_ms: 30_000,
    device_table_json: include_str!("../../data/vbas/dareu.json"),
};

#[cfg(test)]
mod tests {
    use super::SPEC;
    use crate::frame::decode_battery;

    #[test]
    fn parses_battery_and_charging_status() {
        let mut report = [0u8; 17];
        report[..8].copy_from_slice(&[0x08, 0x04, 0, 0, 0, 0, 86, 1]);
        let reading = decode_battery(&SPEC, &report).unwrap();
        assert_eq!(reading.percent.get(), 86);
        assert_eq!(reading.charging, Some(true));
    }
}
