//! 品牌：AJAZZ（W1 / 星闪代，VID `0x7768`）。
//!
//! 协议事实来自本机实测（`docs/vbas-probe-report.md`、`docs/hid-rust-verification.md`）：
//!
//! - 读取通道 `UsagePage = 0xFFB2`（MI_01 / Col02），报告至少 64 字节；
//! - `0xFFB1` 是只写通道，读句柄会持续报错 → v1 不打开；
//! - 电量来自 `cmd = 0xFA` 的推送帧，约 30s 一次，通常两帧一组：
//!
//! ```text
//! [0]        报告 ID = 0x06
//! [1]        cmd = 0xFA
//! [2]        payload 长度 len
//! [3:5]      seq（小端 u16）
//! [5:5+len]  payload：[0]=子标签 0x03  [1]=电量  [2]=充电标志（可选）
//! [5+len:]   填充 0x00
//! ```
//!
//! - `0xFFB3` 在验证设备上无数据 → v1 不实现。
//!
//! `request: None` 是**实测结论**，不是缺省：该设备纯推送，不发任何报文也能
//! 拿到电量（P-3 / P-4）。若将来某个固件只在被查询时才回帧，把这里的
//! `request` 换成一份 [`crate::spec::RequestTemplate`] 即可，运行时无需改动。

use crate::spec::{BatteryFields, BrandSpec, ChargingField, FrameLayout};

/// AJAZZ（W1 / 星闪代）的协议描述。
pub const SPEC: BrandSpec = BrandSpec {
    id: "ajazz",
    label: "AJAZZ（W1 / 星闪代）",
    usage_page: 0xFFB2,
    usage_page_write: Some(0xFFB1),
    frame: FrameLayout {
        report_id: Some(0x06),
        cmd_index: 1,
        len_index: Some(2),
        payload_offset: 5,
        fixed_payload_len: 3,
    },
    battery: BatteryFields {
        cmd: 0xFA,
        subtag: Some((0, 0x03)),
        percent_index: 1,
        charging: Some(ChargingField {
            index: 2,
            true_value: 0x01,
        }),
    },
    // 纯推送：不向设备发任何报文。
    request: None,
    expected_interval_ms: 30_000,
    device_table_json: include_str!("../../data/vbas/ajazz.json"),
};

#[cfg(test)]
mod tests {
    use crate::frame::{decode_battery, padding_all_zero};
    use voltnest_provider::ErrorCode;

    /// 实测帧：`06 fa 03 00 00 | 03 5a 00`（AJ159 NL，90%）。
    const REAL_FRAME: [u8; 8] = [0x06, 0xFA, 0x03, 0x00, 0x00, 0x03, 0x5A, 0x00];

    fn buf_with(payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0u8; 65];
        v[0] = 0x06;
        v[1] = 0xFA;
        v[2] = payload.len() as u8;
        v[5..5 + payload.len()].copy_from_slice(payload);
        v
    }

    #[test]
    fn parses_real_push_frame() {
        let r = decode_battery(&super::SPEC, &REAL_FRAME).expect("实测帧必须解析成功");
        assert_eq!(r.percent.get(), 90);
        assert_eq!(r.charging, Some(false));
    }

    #[test]
    fn parses_64_byte_report_with_padding() {
        let mut buf = [0u8; 65];
        buf[..REAL_FRAME.len()].copy_from_slice(&REAL_FRAME);
        assert!(padding_all_zero(&super::SPEC, &buf));
        assert_eq!(
            decode_battery(&super::SPEC, &buf).unwrap().percent.get(),
            90
        );
    }

    #[test]
    fn real_charging_and_unplugged_frames() {
        // 2026-10-22 实测：插线 90→100 且标志 0x01；拔线 100 且标志 0x00。
        let charging = [0x06, 0xFA, 0x03, 0x00, 0x00, 0x03, 0x64, 0x01];
        assert_eq!(
            decode_battery(&super::SPEC, &charging).unwrap().charging,
            Some(true)
        );
        let unplugged = [0x06, 0xFA, 0x03, 0x00, 0x00, 0x03, 0x64, 0x00];
        assert_eq!(
            decode_battery(&super::SPEC, &unplugged).unwrap().charging,
            Some(false)
        );
    }

    #[test]
    fn truncated_reports_are_malformed() {
        assert_eq!(
            decode_battery(&super::SPEC, &[]),
            Err(ErrorCode::MalformedFrame)
        );
        assert_eq!(
            decode_battery(&super::SPEC, &[0x06]),
            Err(ErrorCode::MalformedFrame)
        );
        assert_eq!(
            decode_battery(&super::SPEC, &[0x06, 0xFA, 0x03, 0x00]),
            Err(ErrorCode::MalformedFrame),
            "不足 5 字节头"
        );
        // 截掉 payload 尾部。
        assert_eq!(
            decode_battery(&super::SPEC, &REAL_FRAME[..6]),
            Err(ErrorCode::MalformedFrame)
        );
    }

    #[test]
    fn length_out_of_bounds_is_malformed() {
        let mut bad = REAL_FRAME;
        bad[2] = 0x40; // 声明 64 字节 payload，但缓冲只有 8 字节
        assert_eq!(
            decode_battery(&super::SPEC, &bad),
            Err(ErrorCode::MalformedFrame)
        );

        let mut zero_len = REAL_FRAME;
        zero_len[2] = 0x00;
        assert_eq!(
            decode_battery(&super::SPEC, &zero_len),
            Err(ErrorCode::MalformedFrame)
        );
    }

    #[test]
    fn wrong_report_id_or_cmd_is_malformed() {
        let mut bad = REAL_FRAME;
        bad[0] = 0x07;
        assert_eq!(
            decode_battery(&super::SPEC, &bad),
            Err(ErrorCode::MalformedFrame)
        );

        let mut bad = REAL_FRAME;
        bad[1] = 0x5A; // 电量值被错放在 cmd 位
        assert_eq!(
            decode_battery(&super::SPEC, &bad),
            Err(ErrorCode::MalformedFrame)
        );
    }

    #[test]
    fn all_zero_payload_is_empty_payload() {
        assert_eq!(
            decode_battery(&super::SPEC, &[0u8; 65]),
            Err(ErrorCode::MalformedFrame),
            "全零报告连 cmd 都不对"
        );
        let frame = buf_with(&[0x00, 0x00, 0x00]);
        assert_eq!(
            decode_battery(&super::SPEC, &frame),
            Err(ErrorCode::EmptyPayload)
        );
    }

    #[test]
    fn percent_over_100_is_out_of_range() {
        for bad in [101u8, 110, 255] {
            let frame = buf_with(&[0x03, bad, 0x00]);
            assert_eq!(
                decode_battery(&super::SPEC, &frame),
                Err(ErrorCode::PercentOutOfRange),
                "bad={bad}"
            );
        }
    }

    #[test]
    fn percent_boundaries_are_accepted() {
        for good in [0u8, 1, 50, 99, 100] {
            let frame = buf_with(&[0x03, good, 0x00]);
            assert_eq!(
                decode_battery(&super::SPEC, &frame).unwrap().percent.get(),
                good
            );
        }
    }

    #[test]
    fn wrong_subtag_is_malformed() {
        let frame = buf_with(&[0x01, 0x64, 0x00]);
        assert_eq!(
            decode_battery(&super::SPEC, &frame),
            Err(ErrorCode::MalformedFrame)
        );
    }

    #[test]
    fn missing_charging_byte_is_unknown_not_false() {
        let frame = buf_with(&[0x03, 0x5A]);
        assert_eq!(decode_battery(&super::SPEC, &frame).unwrap().charging, None);
    }

    #[test]
    fn unknown_charging_flag_values_are_not_charging() {
        // 只有 0x01 表示充电；其它值一律按「未充电」处理，不猜测语义。
        let frame = buf_with(&[0x03, 0x5A, 0x02]);
        assert_eq!(
            decode_battery(&super::SPEC, &frame).unwrap().charging,
            Some(false)
        );
    }

    #[test]
    fn padding_detection_is_diagnostic_only() {
        let mut noisy = [0u8; 65];
        noisy[..REAL_FRAME.len()].copy_from_slice(&REAL_FRAME);
        noisy[63] = 0xAA;
        assert!(!padding_all_zero(&super::SPEC, &noisy));
        // 填充脏了，但电量仍然可用。
        assert_eq!(
            decode_battery(&super::SPEC, &noisy).unwrap().percent.get(),
            90
        );
    }

    #[test]
    fn device_table_covers_verified_generation() {
        let table = crate::table::DeviceTable::parse(super::SPEC.device_table_json).unwrap();
        assert_eq!(table.len(), 16, "8 型号 × 2 模式");
        assert_eq!(table.source_file, "config.xml");
        assert_eq!(table.source_version, "1.0.7.6");
        assert_eq!(table.name(0x7768, 0x6426), Some("AJ159 NL"));
        assert_eq!(table.name(0x7768, 0x6425), Some("AJ159 NL P"));
        assert_eq!(table.name(0x7768, 0x6427), None, "6427 不存在，必须 miss");
        let usb = table.lookup(0x7768, 0x6D26).unwrap();
        let g24 = table.lookup(0x7768, 0x6426).unwrap();
        assert_eq!(usb.name, g24.name);
        assert_ne!(usb.mode, g24.mode);
        let names: std::collections::BTreeSet<_> = table
            .entries()
            .iter()
            .map(|(_, e)| e.name.clone())
            .collect();
        assert_eq!(names.len(), 8, "8 个型号");
    }
}
