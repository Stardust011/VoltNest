//! 通用帧解析：**完全由 [`BrandSpec`] 驱动**的电量解码。
//!
//! 本模块不认识任何品牌：字段位置、命令字、子标签、充电标志取值全部来自
//! 品牌描述。新增品牌不需要改这里（ADR-0011）。
//!
//! 所有失败必须映射为**结构化错误码**，不得静默丢帧、不得回落到 0%
//! （规格 §5 / ADR-0010 D9）。错误码与情形的对应关系是契约：
//!
//! | 情形 | 错误码 |
//! |---|---|
//! | 报告短于 `min_report_len`、报告 ID 不符、长度字段越界、payload 过短、子标签不符、命令字不符 | `malformed_frame` |
//! | payload 全零 | `empty_payload` |
//! | 电量字节 > 100 | `percent_out_of_range` |

use crate::spec::BrandSpec;
use voltnest_provider::{ErrorCode, Percent};

/// 一次电量读数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatteryReading {
    /// 电量（已在构造期拒绝 > 100）。
    pub percent: Percent,
    /// 充电状态。协议不提供、或缺附加字节时为 `None`，**不得猜测**。
    pub charging: Option<bool>,
}

/// 按品牌描述解析一帧，返回电量与充电标志。
pub fn decode_battery(spec: &BrandSpec, report: &[u8]) -> Result<BatteryReading, ErrorCode> {
    let layout = &spec.frame;

    // 1) 头部完整 + 报告 ID。
    if report.len() < spec.min_report_len() {
        return Err(ErrorCode::MalformedFrame);
    }
    if let Some(id) = layout.report_id
        && report.first() != Some(&id)
    {
        return Err(ErrorCode::MalformedFrame);
    }

    // 2) 命令字。
    if report.get(layout.cmd_index) != Some(&spec.battery.cmd) {
        return Err(ErrorCode::MalformedFrame);
    }

    // 3) payload 长度：字段声明 / 固定长度两种模式。
    let payload_len = match layout.len_index {
        Some(index) => {
            let Some(&raw) = report.get(index) else {
                return Err(ErrorCode::MalformedFrame);
            };
            raw as usize
        }
        None => layout.fixed_payload_len,
    };
    let end = layout.payload_offset.saturating_add(payload_len);
    // 长度字段越界（声明比缓冲长）与 payload 过短（装不下必选字段）都算坏帧。
    if payload_len < spec.min_payload_len() || end > report.len() {
        return Err(ErrorCode::MalformedFrame);
    }
    let payload = match report.get(layout.payload_offset..end) {
        Some(p) => p,
        None => return Err(ErrorCode::MalformedFrame),
    };

    // 4) 全零 payload 是「设备在场但没数据」，与坏帧分开上报。
    if payload.iter().all(|b| *b == 0) {
        return Err(ErrorCode::EmptyPayload);
    }

    // 5) 子标签（若协议有）。
    if let Some((index, want)) = spec.battery.subtag
        && payload.get(index) != Some(&want)
    {
        return Err(ErrorCode::MalformedFrame);
    }

    // 6) 电量。Percent::new 的 None 必须在这里映射成错误码，不得 `?` 后 continue。
    let raw = payload
        .get(spec.battery.percent_index)
        .copied()
        .ok_or(ErrorCode::MalformedFrame)?;
    let percent = Percent::new(raw).ok_or(ErrorCode::PercentOutOfRange)?;

    // 7) 充电标志：协议不提供 → None；字节缺失 → None（不猜测）。
    let charging = spec
        .battery
        .charging
        .and_then(|field| payload.get(field.index).map(|b| *b == field.true_value));

    Ok(BatteryReading { percent, charging })
}

/// 诊断用：报告尾部填充是否全零。
///
/// 实测帧的填充恒为 0x00，但**不**作为拒绝条件 —— 把填充当校验会让一个
/// 无害的固件小改动直接变成「设备不可读」。
pub fn padding_all_zero(spec: &BrandSpec, report: &[u8]) -> bool {
    let layout = &spec.frame;
    let payload_len = match layout.len_index {
        Some(index) => match report.get(index) {
            Some(&raw) => raw as usize,
            None => return false,
        },
        None => layout.fixed_payload_len,
    };
    let tail = layout.payload_offset.saturating_add(payload_len);
    report
        .get(tail..)
        .map(|rest| rest.iter().all(|b| *b == 0))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{BatteryFields, BrandSpec, ChargingField, FrameLayout};

    /// 与任何真实品牌都不同的合成描述：验证解析器只吃字段、不含品牌分支。
    ///
    /// 布局：`[0] = 0x11`（无长度字段）→ `[1]=cmd 0x22` → `[2..4]` 填充
    /// → `[4..]` payload：`[0]=电量`、`[1]=充电标志(0x02=充电中)`。
    const SYNTHETIC: BrandSpec = BrandSpec {
        id: "synthetic",
        label: "合成品牌",
        usage_page: 0xFFC0,
        usage_page_write: None,
        frame: FrameLayout {
            report_id: Some(0x11),
            cmd_index: 1,
            len_index: None,
            payload_offset: 4,
            fixed_payload_len: 2,
        },
        battery: BatteryFields {
            cmd: 0x22,
            subtag: None,
            percent_index: 0,
            charging: Some(ChargingField {
                index: 1,
                true_value: 0x02,
            }),
        },
        request: None,
        expected_interval_ms: 20_000,
        device_table_json: crate::brands::ajazz::SPEC.device_table_json,
    };

    #[test]
    fn decodes_fixed_length_layout_without_subtag() {
        let r = decode_battery(&SYNTHETIC, &[0x11, 0x22, 0x00, 0x00, 0x37, 0x02]).unwrap();
        assert_eq!(r.percent.get(), 55);
        assert_eq!(r.charging, Some(true));
    }

    #[test]
    fn fixed_length_payload_shorter_than_declared_is_malformed() {
        assert_eq!(
            decode_battery(&SYNTHETIC, &[0x11, 0x22, 0x00, 0x00, 0x37]),
            Err(ErrorCode::MalformedFrame)
        );
    }

    #[test]
    fn padding_detection_works_without_length_field() {
        let mut buf = vec![0u8; 8];
        buf[..6].copy_from_slice(&[0x11, 0x22, 0x00, 0x00, 0x37, 0x02]);
        assert!(padding_all_zero(&SYNTHETIC, &buf));
        buf[7] = 0x01;
        assert!(!padding_all_zero(&SYNTHETIC, &buf));
    }

    #[test]
    fn brand_without_charging_always_reports_unknown() {
        let mut spec = SYNTHETIC;
        spec.battery.charging = None;
        let r = decode_battery(&spec, &[0x11, 0x22, 0x00, 0x00, 0x37, 0x02]).unwrap();
        assert_eq!(r.charging, None, "协议不提供充电状态时不得猜测");
    }

    #[test]
    fn wrong_cmd_is_malformed_for_every_brand() {
        assert_eq!(
            decode_battery(&SYNTHETIC, &[0x11, 0x23, 0x00, 0x00, 0x37, 0x02]),
            Err(ErrorCode::MalformedFrame)
        );
    }

    #[test]
    fn percent_out_of_range_is_structural() {
        assert_eq!(
            decode_battery(&SYNTHETIC, &[0x11, 0x22, 0x00, 0x00, 0x65, 0x02]),
            Err(ErrorCode::PercentOutOfRange)
        );
    }
}
