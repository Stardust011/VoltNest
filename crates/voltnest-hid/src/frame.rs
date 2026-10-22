//! 厂商通道帧解析与电量解码。
//!
//! 帧结构（`docs/ajazz-probe-report.md` §4，32/32 帧实测通过）：
//!
//! ```text
//! [0]        报告 ID = 0x06
//! [1]        cmd
//! [2]        payload 长度 len
//! [3:5]      seq（小端 u16）
//! [5:5+len]  payload
//! [5+len:]   填充 0x00
//! ```
//!
//! 所有失败必须映射为**结构化错误码**，不得静默丢帧、不得回落到 0%
//! （规格 §5 / ADR-0010 D9）。

use voltnest_provider::{ErrorCode, Percent};

/// 报告 ID。
pub const REPORT_ID: u8 = 0x06;
/// 电量状态帧的命令字。
pub const CMD_STATUS: u8 = 0xFA;
/// `cmd = 0xFA` 帧内表示「电量」的子标签。
pub const SUBTAG_BATTERY: u8 = 0x03;
/// 充电标志的取值：`0x01` 表示充电中。
pub const CHARGING_FLAG: u8 = 0x01;

/// 一次电量读数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatteryReading {
    /// 电量（已在构造期拒绝 > 100）。
    pub percent: Percent,
    /// 充电状态。缺附加字节时为 `None`，**不得猜测**。
    pub charging: Option<bool>,
}

/// 解析一帧，返回电量与充电标志。
///
/// 失败原因与错误码的对应关系是契约的一部分：
///
/// | 情形 | 错误码 |
/// |---|---|
/// | 报告不足 5 字节、报告 ID 不符、长度字段越界、payload 过短、子标签不符 | `malformed_frame` |
/// | cmd 不是 `0xFA` | `malformed_frame` |
/// | payload 全零 | `empty_payload` |
/// | 电量字节 > 100 | `percent_out_of_range` |
pub fn parse_battery(report: &[u8]) -> Result<BatteryReading, ErrorCode> {
    if report.len() < 5 {
        return Err(ErrorCode::MalformedFrame);
    }
    if report[0] != REPORT_ID {
        return Err(ErrorCode::MalformedFrame);
    }
    if report[1] != CMD_STATUS {
        return Err(ErrorCode::MalformedFrame);
    }
    let len = report[2] as usize;
    // 长度字段越界：缓冲区里没有这么多字节。
    if len < 2 || 5 + len > report.len() {
        return Err(ErrorCode::MalformedFrame);
    }
    let payload = &report[5..5 + len];
    if payload.iter().all(|b| *b == 0) {
        return Err(ErrorCode::EmptyPayload);
    }
    if payload[0] != SUBTAG_BATTERY {
        return Err(ErrorCode::MalformedFrame);
    }
    // Percent::new 的 None 必须在这里映射成错误码，不得 `?` 后 continue。
    let percent = Percent::new(payload[1]).ok_or(ErrorCode::PercentOutOfRange)?;
    Ok(BatteryReading {
        percent,
        charging: payload.get(2).map(|b| *b == CHARGING_FLAG),
    })
}

/// 诊断用：报告尾部填充是否全零。
///
/// 实测帧的填充恒为 0x00，但**不**作为拒绝条件 —— 把填充当校验会让一个
/// 无害的固件小改动直接变成「设备不可读」。
pub fn padding_all_zero(report: &[u8]) -> bool {
    if report.len() < 5 {
        return false;
    }
    let len = report[2] as usize;
    let tail = 5usize.saturating_add(len);
    report
        .get(tail..)
        .map(|rest| rest.iter().all(|b| *b == 0))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 实测帧：`06 fa 03 00 00 | 03 5a 00`（AJ159 NL，90%）。
    const REAL_FRAME: [u8; 8] = [0x06, 0xFA, 0x03, 0x00, 0x00, 0x03, 0x5A, 0x00];

    fn buf_with(payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0u8; 65];
        v[0] = REPORT_ID;
        v[1] = CMD_STATUS;
        v[2] = payload.len() as u8;
        v[5..5 + payload.len()].copy_from_slice(payload);
        v
    }

    #[test]
    fn parses_real_push_frame() {
        let r = parse_battery(&REAL_FRAME).expect("实测帧必须解析成功");
        assert_eq!(r.percent.get(), 90);
        assert_eq!(r.charging, Some(false));
    }

    #[test]
    fn parses_64_byte_report_with_padding() {
        let mut buf = [0u8; 65];
        buf[..REAL_FRAME.len()].copy_from_slice(&REAL_FRAME);
        assert!(padding_all_zero(&buf));
        assert_eq!(parse_battery(&buf).unwrap().percent.get(), 90);
    }

    #[test]
    fn real_charging_and_unplugged_frames() {
        // 2026-10-22 实测：插线 90→100 且标志 0x01；拔线 100 且标志 0x00。
        let charging = [0x06, 0xFA, 0x03, 0x00, 0x00, 0x03, 0x64, 0x01];
        assert_eq!(parse_battery(&charging).unwrap().charging, Some(true));
        let unplugged = [0x06, 0xFA, 0x03, 0x00, 0x00, 0x03, 0x64, 0x00];
        assert_eq!(parse_battery(&unplugged).unwrap().charging, Some(false));
    }

    #[test]
    fn truncated_reports_are_malformed() {
        assert_eq!(parse_battery(&[]), Err(ErrorCode::MalformedFrame));
        assert_eq!(parse_battery(&[0x06]), Err(ErrorCode::MalformedFrame));
        assert_eq!(
            parse_battery(&[0x06, 0xFA, 0x03, 0x00]),
            Err(ErrorCode::MalformedFrame),
            "不足 5 字节头"
        );
        // 截掉 payload 尾部。
        assert_eq!(
            parse_battery(&REAL_FRAME[..6]),
            Err(ErrorCode::MalformedFrame)
        );
    }

    #[test]
    fn length_out_of_bounds_is_malformed() {
        let mut bad = REAL_FRAME;
        bad[2] = 0x40; // 声明 64 字节 payload，但缓冲只有 8 字节
        assert_eq!(parse_battery(&bad), Err(ErrorCode::MalformedFrame));

        let mut zero_len = REAL_FRAME;
        zero_len[2] = 0x00;
        assert_eq!(parse_battery(&zero_len), Err(ErrorCode::MalformedFrame));
    }

    #[test]
    fn wrong_report_id_or_cmd_is_malformed() {
        let mut bad = REAL_FRAME;
        bad[0] = 0x07;
        assert_eq!(parse_battery(&bad), Err(ErrorCode::MalformedFrame));

        let mut bad = REAL_FRAME;
        bad[1] = 0x5A; // 电量值被错放在 cmd 位
        assert_eq!(parse_battery(&bad), Err(ErrorCode::MalformedFrame));
    }

    #[test]
    fn all_zero_payload_is_empty_payload() {
        assert_eq!(
            parse_battery(&[0u8; 65]),
            Err(ErrorCode::MalformedFrame),
            "全零报告连 cmd 都不对"
        );
        let frame = buf_with(&[0x00, 0x00, 0x00]);
        assert_eq!(parse_battery(&frame), Err(ErrorCode::EmptyPayload));
    }

    #[test]
    fn percent_over_100_is_out_of_range() {
        // 101 = 0x65, 110 = 0x6E, 255 = 0xFF
        for bad in [101u8, 110, 255] {
            let frame = buf_with(&[SUBTAG_BATTERY, bad, 0x00]);
            assert_eq!(
                parse_battery(&frame),
                Err(ErrorCode::PercentOutOfRange),
                "bad={bad}"
            );
        }
    }

    #[test]
    fn percent_boundaries_are_accepted() {
        for good in [0u8, 1, 50, 99, 100] {
            let frame = buf_with(&[SUBTAG_BATTERY, good, 0x00]);
            assert_eq!(parse_battery(&frame).unwrap().percent.get(), good);
        }
    }

    #[test]
    fn wrong_subtag_is_malformed() {
        let frame = buf_with(&[0x01, 0x64, 0x00]);
        assert_eq!(parse_battery(&frame), Err(ErrorCode::MalformedFrame));
    }

    #[test]
    fn missing_charging_byte_is_unknown_not_false() {
        let frame = buf_with(&[SUBTAG_BATTERY, 0x5A]);
        assert_eq!(parse_battery(&frame).unwrap().charging, None);
    }

    #[test]
    fn unknown_charging_flag_values_are_not_charging() {
        // 只有 0x01 表示充电；其它值一律按「未充电」处理，不猜测语义。
        let frame = buf_with(&[SUBTAG_BATTERY, 0x5A, 0x02]);
        assert_eq!(parse_battery(&frame).unwrap().charging, Some(false));
    }

    #[test]
    fn padding_detection_is_diagnostic_only() {
        let mut noisy = [0u8; 65];
        noisy[..REAL_FRAME.len()].copy_from_slice(&REAL_FRAME);
        noisy[63] = 0xAA;
        assert!(!padding_all_zero(&noisy));
        // 填充脏了，但电量仍然可用。
        assert_eq!(parse_battery(&noisy).unwrap().percent.get(), 90);
    }
}
