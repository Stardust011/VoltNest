//! 通用帧解析：**完全由 [`BrandSpec`] 驱动**的电量解码（新增品牌不改这里）。
//!
//! 所有失败必须映射为**结构化错误码**，不得静默丢帧、不得回落到 0%。错误码与情形的对应关系：
//!
//! | 情形 | 错误码 |
//! |---|---|
//! | 报告短于 `min_report_len`、报告 ID 不符、长度字段越界、payload 过短、子标签不符、命令字不符 | `malformed_frame` |
//! | payload 全零 | `empty_payload` |
//! | 电量字节 > 100 | `percent_out_of_range` |
//!
//! # 诊断：错误码只是一个词
//! `malformed_frame` 有六种成因，仅看错误码分不清「报告 ID 不符」与「payload 装不下」。故解析器同时给 [`FrameReject`]：哪一校验失败、期望与实际各是多少、原始字节是什么。它只进诊断页与日志（[`FrameReject::detail`]），不参与状态判定——状态机仍只看 [`ErrorCode`]。

use crate::spec::BrandSpec;
use voltnest_provider::{DIAG_BYTES, ErrorCode, Percent, hex_bytes};

/// 一次电量读数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatteryReading {
    /// 电量（已在构造期拒绝 > 100）。
    pub percent: Percent,
    /// 充电状态。协议不提供、或缺附加字节时为 `None`，**不得猜测**。
    pub charging: Option<bool>,
}

/// 帧被拒的**具体环节**（诊断用）。
///
/// 与 [`ErrorCode`] 是两层：错误码是给状态机与 `lastError` 的稳定标识，
/// 本枚举是给人看的排查线索。一个 `MalformedFrame` 对应其中一种成因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RejectReason {
    /// 报告长度不足 `min_report_len`。
    ReportTooShort {
        /// 品牌要求的最小报告长度。
        min: usize,
    },
    /// 报告 ID 与品牌声明不符（`got` 为 `None` 表示连首字节都没有）。
    ReportIdMismatch {
        /// 期望值。
        want: u8,
        /// 实际值。
        got: Option<u8>,
    },
    /// 命令字不符。
    CmdMismatch {
        /// 期望值。
        want: u8,
        /// 实际值。
        got: Option<u8>,
    },
    /// 长度字段所在下标越过缓冲末尾。
    LengthFieldMissing {
        /// 品牌声明的长度字段下标。
        index: usize,
    },
    /// 声明的 payload 长度装不下必选字段。
    DeclaredPayloadTooShort {
        /// 帧内声明的长度。
        declared: usize,
        /// 品牌要求的最小 payload。
        min: usize,
    },
    /// 声明的 payload 越过缓冲末尾（长度字段被写坏时的典型表现）。
    DeclaredPayloadPastEnd {
        /// 帧内声明的长度。
        declared: usize,
        /// 声明的 payload 结束下标。
        end: usize,
        /// 实际可用长度。
        len: usize,
    },
    /// 子标签不符。
    SubtagsMismatch {
        /// 期望值。
        want: u8,
        /// 实际值。
        got: Option<u8>,
    },
    /// 电量字段下标越过 payload（长度检查通过后的兜底）。
    PercentMissing {
        /// 品牌声明的电量下标。
        index: usize,
    },
    /// payload 全零。
    EmptyPayload,
    /// 电量字节 > 100。
    PercentOutOfRange {
        /// 实际读到的字节。
        raw: u8,
    },
}

impl RejectReason {
    /// 稳定字符串（不含参数），用于日志与 grep。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReportTooShort { .. } => "report_too_short",
            Self::ReportIdMismatch { .. } => "report_id_mismatch",
            Self::CmdMismatch { .. } => "cmd_mismatch",
            Self::LengthFieldMissing { .. } => "length_field_missing",
            Self::DeclaredPayloadTooShort { .. } => "declared_payload_too_short",
            Self::DeclaredPayloadPastEnd { .. } => "declared_payload_past_end",
            Self::SubtagsMismatch { .. } => "subtag_mismatch",
            Self::PercentMissing { .. } => "percent_missing",
            Self::EmptyPayload => "empty_payload",
            Self::PercentOutOfRange { .. } => "percent_out_of_range",
        }
    }
}

impl std::fmt::Display for RejectReason {
    /// `cmd_mismatch(want=0x03 got=0x23)`：参数写在同一行，诊断页可直接读。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())?;
        match self {
            Self::ReportTooShort { min } => write!(f, "(min={min})"),
            Self::ReportIdMismatch { want, got }
            | Self::CmdMismatch { want, got }
            | Self::SubtagsMismatch { want, got } => {
                write!(f, "(want={want:#04X} got={})", hex_opt(*got))
            }
            Self::LengthFieldMissing { index } => write!(f, "(index={index})"),
            Self::DeclaredPayloadTooShort { declared, min } => {
                write!(f, "(declared={declared} min={min})")
            }
            Self::DeclaredPayloadPastEnd { declared, end, len } => {
                write!(f, "(declared={declared} end={end} len={len})")
            }
            Self::PercentMissing { index } => write!(f, "(index={index})"),
            Self::EmptyPayload => Ok(()),
            Self::PercentOutOfRange { raw } => write!(f, "(raw={raw})"),
        }
    }
}

fn hex_opt(value: Option<u8>) -> String {
    value.map_or_else(|| "none".to_string(), |b| format!("{b:#04X}"))
}

/// 一次具名的拒绝：错误码 + 成因 + 原始字节。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FrameReject {
    /// 稳定错误码（写 `Snapshot.lastError`）。
    pub code: ErrorCode,
    /// 具体成因（只进诊断）。
    pub reason: RejectReason,
    /// 被拒的原始报告（完整保留；格式化时再截断）。
    pub report: Vec<u8>,
}

impl FrameReject {
    /// 诊断 detail：`cmd_mismatch(want=0x03 got=0x23) · 8B: 06 FA 03 …`。
    ///
    /// 长度写在字节前面：`3B: 06 FA 03` 能立刻看出「报告被读短了」，
    /// 而只有 hex 时还得自己数。
    pub fn detail(&self) -> String {
        format!(
            "{} · {}B: {}",
            self.reason,
            self.report.len(),
            hex_bytes(&self.report, DIAG_BYTES)
        )
    }
}

/// 按品牌描述解析一帧，返回电量与充电标志。
///
/// 只回错误码：状态机与 `lastError` 只需要它，新增成因不得新增错误码。
pub fn decode_battery(spec: &BrandSpec, report: &[u8]) -> Result<BatteryReading, ErrorCode> {
    decode_battery_detailed(spec, report).map_err(|reject| reject.code)
}

/// 同 [`decode_battery`]，但失败时给出**可复核的成因与原始字节**（诊断页用）。
pub fn decode_battery_detailed(
    spec: &BrandSpec,
    report: &[u8],
) -> Result<BatteryReading, FrameReject> {
    let layout = &spec.frame;
    // 每次拒绝都带上原始报告：诊断页要的是「被拒的那一帧长什么样」。
    let original_report = report;
    let reject = |reason: RejectReason| FrameReject {
        code: reason_code(reason),
        reason,
        report: original_report.to_vec(),
    };

    // 1) 头部完整 + 报告 ID。
    if report.len() < spec.min_report_len() {
        return Err(reject(RejectReason::ReportTooShort {
            min: spec.min_report_len(),
        }));
    }
    if let Some(id) = layout.report_id
        && report.first() != Some(&id)
    {
        return Err(reject(RejectReason::ReportIdMismatch {
            want: id,
            got: report.first().copied(),
        }));
    }

    // MCHOSE 等协议只反转报告 ID 之后的内容；诊断报告仍保留原始字节。
    let normalized_report;
    let report = if spec.reply_inverted {
        normalized_report = report
            .iter()
            .enumerate()
            .map(|(index, byte)| {
                if layout.report_id.is_some() && index == 0 {
                    *byte
                } else {
                    *byte ^ 0xFF
                }
            })
            .collect::<Vec<_>>();
        normalized_report.as_slice()
    } else {
        report
    };

    // 2) 命令字。
    if report.get(layout.cmd_index) != Some(&spec.battery.cmd) {
        return Err(reject(RejectReason::CmdMismatch {
            want: spec.battery.cmd,
            got: report.get(layout.cmd_index).copied(),
        }));
    }

    // 3) payload 长度：字段声明 / 固定长度两种模式。
    let payload_len = match layout.len_index {
        Some(index) => {
            let Some(&raw) = report.get(index) else {
                return Err(reject(RejectReason::LengthFieldMissing { index }));
            };
            raw as usize
        }
        None => layout.fixed_payload_len,
    };
    let end = layout.payload_offset.saturating_add(payload_len);
    // 长度字段越界（声明比缓冲长）与 payload 过短（装不下必选字段）都算坏帧，
    // 但成因不同：前者是「长度字段写坏」，后者是「布局不符」，诊断必须分开。
    if payload_len < spec.min_payload_len() {
        return Err(reject(RejectReason::DeclaredPayloadTooShort {
            declared: payload_len,
            min: spec.min_payload_len(),
        }));
    }
    if end > report.len() {
        return Err(reject(RejectReason::DeclaredPayloadPastEnd {
            declared: payload_len,
            end,
            len: report.len(),
        }));
    }
    let payload = match report.get(layout.payload_offset..end) {
        Some(p) => p,
        None => {
            return Err(reject(RejectReason::DeclaredPayloadPastEnd {
                declared: payload_len,
                end,
                len: report.len(),
            }));
        }
    };

    // 4) 全零 payload 是「设备在场但没数据」，与坏帧分开上报。
    if payload.iter().all(|b| *b == 0) {
        return Err(reject(RejectReason::EmptyPayload));
    }

    // 5) 子标签（若协议有）。
    if let Some((index, want)) = spec.battery.subtag
        && payload.get(index) != Some(&want)
    {
        return Err(reject(RejectReason::SubtagsMismatch {
            want,
            got: payload.get(index).copied(),
        }));
    }

    // 6) 电量。Percent::new 的 None 必须在这里映射成错误码，不得 `?` 后 continue。
    let raw = payload
        .get(spec.battery.percent_index)
        .copied()
        .ok_or_else(|| {
            reject(RejectReason::PercentMissing {
                index: spec.battery.percent_index,
            })
        })?;
    let percent = if spec.clamp_percent {
        Percent::new(raw.min(100)).expect("截断后的电量必定在 0..=100")
    } else {
        Percent::new(raw).ok_or_else(|| reject(RejectReason::PercentOutOfRange { raw }))?
    };

    // 7) 充电标志：协议不提供 → None；字节缺失 → None（不猜测）。
    let charging = spec.battery.charging.and_then(|field| {
        payload
            .get(field.index)
            .map(|b| field.true_values.contains(b))
    });

    Ok(BatteryReading { percent, charging })
}

/// 成因 → 错误码。**一个 `MalformedFrame` 对应多种成因**，映射只在这里写一次。
const fn reason_code(reason: RejectReason) -> ErrorCode {
    match reason {
        RejectReason::EmptyPayload => ErrorCode::EmptyPayload,
        RejectReason::PercentOutOfRange { .. } => ErrorCode::PercentOutOfRange,
        _ => ErrorCode::MalformedFrame,
    }
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
        transport: crate::spec::ReplyTransport::Push,
        reply_inverted: false,
        clamp_percent: false,
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
                true_values: &[0x02],
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

    // ---------- 诊断：成因与原始字节 ----------

    /// 带长度字段的合成描述：`[2]` 是 payload 长度，payload 从 `[4]` 开始。
    const SYNTHETIC_LEN: BrandSpec = BrandSpec {
        frame: FrameLayout {
            report_id: Some(0x11),
            cmd_index: 1,
            len_index: Some(2),
            payload_offset: 4,
            fixed_payload_len: 0,
        },
        ..SYNTHETIC
    };

    #[test]
    fn each_rejection_names_its_own_reason() {
        // 成因必须逐条可辨：六种 `malformed_frame` 在诊断页上不能再长得一样。
        let too_short = decode_battery_detailed(&SYNTHETIC, &[0x11, 0x22]).unwrap_err();
        assert_eq!(too_short.reason, RejectReason::ReportTooShort { min: 5 });

        let wrong_id =
            decode_battery_detailed(&SYNTHETIC, &[0x99, 0x22, 0x00, 0x00, 0x37, 0x02]).unwrap_err();
        assert_eq!(
            wrong_id.reason,
            RejectReason::ReportIdMismatch {
                want: 0x11,
                got: Some(0x99)
            }
        );

        let wrong_cmd =
            decode_battery_detailed(&SYNTHETIC, &[0x11, 0x23, 0x00, 0x00, 0x37, 0x02]).unwrap_err();
        assert_eq!(
            wrong_cmd.reason,
            RejectReason::CmdMismatch {
                want: 0x22,
                got: Some(0x23)
            }
        );

        // 固定长度布局 + 短报告：长度不是「声明得小」，而是「声明越过末尾」。
        let past_end =
            decode_battery_detailed(&SYNTHETIC, &[0x11, 0x22, 0x00, 0x00, 0x37]).unwrap_err();
        assert_eq!(
            past_end.reason,
            RejectReason::DeclaredPayloadPastEnd {
                declared: 2,
                end: 6,
                len: 5
            }
        );

        // 长度字段被写坏成 0：payload 装不下必选字段。
        let zero_len =
            decode_battery_detailed(&SYNTHETIC_LEN, &[0x11, 0x22, 0x00, 0x00, 0x37, 0x02])
                .unwrap_err();
        assert_eq!(
            zero_len.reason,
            RejectReason::DeclaredPayloadTooShort {
                declared: 0,
                min: 1
            }
        );

        let out_of_range =
            decode_battery_detailed(&SYNTHETIC, &[0x11, 0x22, 0x00, 0x00, 0x65, 0x02]).unwrap_err();
        assert_eq!(
            out_of_range.reason,
            RejectReason::PercentOutOfRange { raw: 0x65 }
        );
    }

    #[test]
    fn subtag_mismatch_is_its_own_reason() {
        let mut spec = SYNTHETIC;
        spec.battery.subtag = Some((1, 0x03));
        let err =
            decode_battery_detailed(&spec, &[0x11, 0x22, 0x00, 0x00, 0x37, 0x02]).unwrap_err();
        assert_eq!(
            err.reason,
            RejectReason::SubtagsMismatch {
                want: 0x03,
                got: Some(0x02)
            }
        );
    }

    #[test]
    fn reject_detail_carries_reason_and_raw_bytes() {
        let err =
            decode_battery_detailed(&SYNTHETIC, &[0x11, 0x23, 0x00, 0x00, 0x37, 0x02]).unwrap_err();
        assert_eq!(err.code, ErrorCode::MalformedFrame);
        assert_eq!(
            err.detail(),
            "cmd_mismatch(want=0x22 got=0x23) · 6B: 11 23 00 00 37 02"
        );
    }

    #[test]
    fn empty_payload_detail_keeps_zero_bytes() {
        let err =
            decode_battery_detailed(&SYNTHETIC, &[0x11, 0x22, 0x00, 0x00, 0x00, 0x00]).unwrap_err();
        assert_eq!(err.code, ErrorCode::EmptyPayload);
        assert_eq!(err.reason, RejectReason::EmptyPayload);
        assert!(
            err.detail()
                .starts_with("empty_payload · 6B: 11 22 00 00 00 00"),
            "{}",
            err.detail()
        );
    }
}
