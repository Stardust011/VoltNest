//! 通用帧解析：完全由 [`BrandSpec`] 驱动，不含品牌分支。
//!
//! 所有失败映射为结构化错误码：payload 全零 → `empty_payload`，电量字节 > 100 →
//! `percent_out_of_range`，其余校验失败 → `malformed_frame`。具体成因由 [`FrameReject`]
//! 给出，只进诊断，不参与状态判定。

use crate::spec::{BatteryFields, BrandSpec, ListenSpec, ListenStatus};
use std::time::{Duration, Instant};
use voltnest_provider::{DIAG_BYTES, ErrorCode, Percent, hex_bytes};

/// 一次电量读数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatteryReading {
    /// 电量（构造期已保证 ≤ 100）。
    pub percent: Percent,
    /// 充电状态；协议不提供时为 `None`。
    pub charging: Option<bool>,
}

/// 帧被拒的具体环节（诊断用）。
///
/// [`ErrorCode`] 是给状态机的稳定标识，本枚举是给人看的排查线索。
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
    /// 双字节命令字的高字节不符。
    CmdHighMismatch {
        /// 期望值。
        want: u8,
        /// 实际值。
        got: Option<u8>,
    },
    /// 应答确认位不符（设备还没处理完该命令，或回的是别的命令）。
    AckMismatch {
        /// 期望值（已与掩码对齐）。
        want: u8,
        /// 实际字节（未掩码，便于人工核对）。
        got: Option<u8>,
    },
    /// 设备自报错误态（状态值达到品牌声明的错误起点）。
    StatusError {
        /// 实际状态字节。
        raw: u8,
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
            Self::CmdHighMismatch { .. } => "cmd_high_mismatch",
            Self::AckMismatch { .. } => "ack_mismatch",
            Self::StatusError { .. } => "status_error",
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
    /// 稳定词 + 参数，如 `cmd_mismatch(want=0x03 got=0x23)`。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())?;
        match self {
            Self::ReportTooShort { min } => write!(f, "(min={min})"),
            Self::ReportIdMismatch { want, got }
            | Self::CmdMismatch { want, got }
            | Self::CmdHighMismatch { want, got }
            | Self::AckMismatch { want, got }
            | Self::SubtagsMismatch { want, got } => {
                write!(f, "(want={want:#04X} got={})", hex_opt(*got))
            }
            Self::StatusError { raw } => write!(f, "(status={raw:#04X})"),
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

/// 取 `index` 处的字节并按 [`BrandSpec::reply_inverted`] 归一化。
///
/// 报告 ID 不参与取反。
fn normalized_byte(spec: &BrandSpec, index: usize, byte: u8) -> u8 {
    let keep = spec.frame.report_id.is_some() && index == 0;
    if spec.reply_inverted && !keep {
        byte ^ 0xFF
    } else {
        byte
    }
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
/// 失败只返回错误码；需要成因与原始字节时用 [`decode_battery_detailed`]。
pub fn decode_battery(spec: &BrandSpec, report: &[u8]) -> Result<BatteryReading, ErrorCode> {
    decode_battery_detailed(spec, report).map_err(|reject| reject.code)
}

/// 同 [`decode_battery`]，但失败时给出成因与原始字节（诊断用）。
///
/// 协议未声明报告 ID 时，首字节可能含 hidapi 补的报告 ID 槽，故两个偏移各试一次；
/// 声明了报告 ID 的协议不走这条容错。
pub fn decode_battery_detailed(
    spec: &BrandSpec,
    report: &[u8],
) -> Result<BatteryReading, FrameReject> {
    if spec.frame.report_id.is_some() {
        return decode_battery_at(spec, &spec.battery, spec.ack, report, report);
    }
    let mut first: Option<FrameReject> = None;
    for shift in 0..=1usize {
        let Some(window) = report.get(shift..) else {
            continue;
        };
        match decode_battery_at(spec, &spec.battery, spec.ack, window, report) {
            Ok(reading) => return Ok(reading),
            Err(reject) => {
                if first.is_none() {
                    first = Some(reject);
                }
            }
        }
    }
    Err(first.unwrap_or_else(|| FrameReject {
        code: ErrorCode::MalformedFrame,
        reason: RejectReason::ReportTooShort {
            min: spec.min_report_len(),
        },
        report: report.to_vec(),
    }))
}

/// 在给定窗口上解析一帧；`original` 仅用于诊断（保留未位移的原始报告）。
///
/// `fields` 由调用方选定：应答帧传 [`BrandSpec::battery`]，推送帧传
/// [`crate::spec::NotifySpec::fields`]；两条通道共用同一套 [`crate::spec::FrameLayout`]。
fn decode_battery_at(
    spec: &BrandSpec,
    fields: &BatteryFields,
    ack: Option<crate::spec::AckField>,
    report: &[u8],
    original: &[u8],
) -> Result<BatteryReading, FrameReject> {
    let layout = &spec.frame;
    let reject = |reason: RejectReason| FrameReject {
        code: reason_code(reason),
        reason,
        report: original.to_vec(),
    };

    let min_report_len = layout.payload_offset + fields.min_payload_len();
    if report.len() < min_report_len {
        return Err(reject(RejectReason::ReportTooShort {
            min: min_report_len,
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

    let normalized_report;
    let report = if spec.reply_inverted {
        normalized_report = report
            .iter()
            .enumerate()
            .map(|(index, byte)| normalized_byte(spec, index, *byte))
            .collect::<Vec<_>>();
        normalized_report.as_slice()
    } else {
        report
    };

    if let Some(ack) = ack {
        let raw = report.get(ack.index).copied();
        if raw.map(|b| b & ack.mask) != Some(ack.value) {
            return Err(reject(RejectReason::AckMismatch {
                want: ack.value,
                got: raw,
            }));
        }
    }

    if let Some(cmd) = fields.cmd {
        let got = report.get(layout.cmd_index).copied();
        if got != Some(cmd) {
            return Err(reject(RejectReason::CmdMismatch { want: cmd, got }));
        }
        if let Some(high) = fields.cmd_high {
            let got = report.get(layout.cmd_index + 1).copied();
            if got != Some(high) {
                return Err(reject(RejectReason::CmdHighMismatch { want: high, got }));
            }
        }
    }

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
    if payload_len < fields.min_payload_len() {
        return Err(reject(RejectReason::DeclaredPayloadTooShort {
            declared: payload_len,
            min: fields.min_payload_len(),
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

    if payload.iter().all(|b| *b == 0) {
        return Err(reject(RejectReason::EmptyPayload));
    }

    if let Some((index, want)) = fields.subtag
        && payload.get(index) != Some(&want)
    {
        return Err(reject(RejectReason::SubtagsMismatch {
            want,
            got: payload.get(index).copied(),
        }));
    }

    if let Some(field) = fields.charging
        && let Some(&raw) = payload.get(field.index)
        && field.is_error(raw)
    {
        return Err(reject(RejectReason::StatusError { raw }));
    }

    let raw = payload.get(fields.percent_index).copied().ok_or_else(|| {
        reject(RejectReason::PercentMissing {
            index: fields.percent_index,
        })
    })?;
    let raw = fields.percent_of(raw);
    let percent = if spec.clamp_percent {
        Percent::new(raw.min(100)).expect("截断后的电量必定在 0..=100")
    } else {
        Percent::new(raw).ok_or_else(|| reject(RejectReason::PercentOutOfRange { raw }))?
    };

    let charging = fields
        .charging
        .and_then(|field| payload.get(field.index).map(|b| field.is_charging(*b)));

    Ok(BatteryReading { percent, charging })
}

/// 成因 → 错误码。一个 `MalformedFrame` 对应多种成因，映射只在这里写一次。
const fn reason_code(reason: RejectReason) -> ErrorCode {
    match reason {
        RejectReason::EmptyPayload => ErrorCode::EmptyPayload,
        RejectReason::PercentOutOfRange { .. } => ErrorCode::PercentOutOfRange,
        _ => ErrorCode::MalformedFrame,
    }
}

/// 解析一帧设备主动推送的电量（帧头与应答同构，但不校验确认位）。
///
/// 返回 `None` 表示这一帧不是电量推送帧，调用方应静默丢弃。
pub fn decode_notify(spec: &BrandSpec, report: &[u8]) -> Option<BatteryReading> {
    let notify = spec.notify?;
    decode_battery_at(spec, &notify.fields(), None, report, report).ok()
}

/// 监听通道上一帧的解析结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListenEvent {
    /// 电量 + 可选充电状态。
    Reading(BatteryReading),
    /// 只有充电状态变化。
    Charging(bool),
}

/// 解析一帧设备自发上报的电量。
///
/// 返回 `None` 表示这一帧不是电量上报，调用方应静默丢弃。首字节可能含报告 ID 槽，
/// 故偏移 0 与 1 各试一次。
pub fn decode_listen(spec: &BrandSpec, report: &[u8]) -> Option<ListenEvent> {
    let frames = spec.listen?;
    for offset in [0usize, 1] {
        let Some(window) = report.get(offset..) else {
            continue;
        };
        for frame in frames {
            if let Some(event) = decode_listen_frame(frame, window) {
                return Some(event);
            }
        }
    }
    None
}

/// 单条监听模板的匹配与解码；字段下标相对窗口起点。
fn decode_listen_frame(frame: &ListenSpec, window: &[u8]) -> Option<ListenEvent> {
    if window.len() < frame.min_len() || window[0] != frame.cmd {
        return None;
    }
    if let Some(second) = frame.cmd2
        && window[1] != second
    {
        return None;
    }
    if window[frame.subtag_index] != frame.subtag_value {
        return None;
    }
    let status = window[frame.status_index];
    match frame.status {
        ListenStatus::Nibbles {
            charging_low_nibble,
        } => {
            let (index, mask) = frame.percent?;
            let raw = window[index] & mask;
            let percent = if status >> 4 != 0 { 100u8 } else { raw };
            Some(ListenEvent::Reading(BatteryReading {
                percent: Percent::new(percent)?,
                charging: Some((status & 0x0F) == charging_low_nibble),
            }))
        }
        ListenStatus::Gate { charging, usable } => {
            let charging = charging.contains(&status);
            let Some((index, mask)) = frame.percent else {
                return Some(ListenEvent::Charging(charging));
            };
            if !usable.contains(&status) {
                return None;
            }
            Some(ListenEvent::Reading(BatteryReading {
                percent: Percent::new(window[index] & mask)?,
                charging: Some(charging),
            }))
        }
    }
}

/// 该帧是不是本协议的电量帧（只按命令字判定，不解析字段）。
///
/// 用于监听 / output 通道的前置过滤，其它帧直接丢，不进 `lastError`。未声明报告 ID
/// 的协议同样允许首字节是平台补的报告 ID 槽。
pub fn matches_cmd(spec: &BrandSpec, report: &[u8]) -> bool {
    let Some(cmd) = spec.battery.cmd else {
        return true;
    };
    let index = spec.frame.cmd_index;
    let hit = |base: usize| {
        let at = base + index;
        let low = report.get(at).copied().map(|byte| {
            if base == 0 {
                normalized_byte(spec, at, byte)
            } else {
                byte
            }
        });
        if low != Some(cmd) {
            return false;
        }
        match spec.battery.cmd_high {
            Some(high) => report.get(at + 1).copied() == Some(high),
            None => true,
        }
    };
    if spec.frame.report_id.is_none() {
        return (0..=1usize).any(hit);
    }
    hit(0)
}

/// 诊断用：报告尾部填充是否全零。不作为拒绝条件。
pub fn padding_all_zero(spec: &BrandSpec, report: &[u8]) -> bool {
    let layout = &spec.frame;
    let Some(payload_len) = layout.payload_len(report) else {
        return false;
    };
    let tail = layout.payload_offset.saturating_add(payload_len);
    report
        .get(tail..)
        .map(|rest| rest.iter().all(|b| *b == 0))
        .unwrap_or(false)
}

/// 多包应答的拼接器：把拆成 N 条输入报告的一条应答拼回一条。
///
/// 仅 [`BrandSpec::reassembly`] 非空的协议会用到。生命周期按「一次查询」算：凑齐 `total`
/// 个序号即产出结果；超过 `timeout_ms` 未凑齐则丢弃半包。
#[derive(Debug, Default)]
pub struct Reassembler {
    parts: Vec<u8>,
    total: usize,
    next_seq: usize,
    deadline: Option<Instant>,
}

impl Reassembler {
    /// 空拼接器（等待第一条包）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 丢弃当前未完成的组装（新一次查询发出前调用）。
    pub fn reset(&mut self) {
        self.parts.clear();
        self.total = 0;
        self.next_seq = 0;
        self.deadline = None;
    }

    /// 一次应答是否已凑齐（诊断用）。
    pub fn is_complete(&self) -> bool {
        self.total > 0 && self.next_seq == self.total
    }

    /// 喂一条输入报告。返回 `Some` 表示这次应答已凑齐。
    ///
    /// 只认命令字对上的包；序号不符的包直接丢弃。
    pub fn feed(&mut self, spec: &BrandSpec, report: &[u8], now: Instant) -> Option<Vec<u8>> {
        let reasm = spec.reassembly?;
        if let Some(deadline) = self.deadline
            && now > deadline
        {
            self.reset();
        }
        let window = (0..=1usize).find_map(|base| {
            let w = report.get(base..)?;
            (w.len() > reasm.min_packet_len() && w.first() == Some(&reasm.cmd)).then_some(w)
        })?;
        let total = *window.get(reasm.total_index)? as usize;
        let seq = *window.get(reasm.seq_index)? as usize;
        if total == 0 || seq >= total {
            return None;
        }
        if seq == 0 {
            self.parts.clear();
            self.total = total;
            self.next_seq = 0;
            self.deadline = Some(now + Duration::from_millis(reasm.timeout_ms));
        }
        if self.total != total || seq != self.next_seq {
            return None;
        }
        let end = window.len().saturating_sub(reasm.trailer_len);
        let chunk = window.get(reasm.payload_offset..end)?;
        self.parts.extend_from_slice(chunk);
        self.next_seq += 1;
        if !self.is_complete() {
            return None;
        }
        let assembled = std::mem::take(&mut self.parts);
        self.reset();
        Some(assembled)
    }
}

/// 解析组装完成的应答（[`BrandSpec::reassembly`] 声明的通道）。
///
/// 不做「满电按 100」的改写；字段位置取自 [`BrandSpec::battery`]。
pub fn decode_assembled(spec: &BrandSpec, assembled: &[u8]) -> Option<BatteryReading> {
    spec.reassembly?;
    if assembled.len() < spec.min_payload_len() {
        return None;
    }
    let raw = *assembled.get(spec.battery.percent_index)?;
    let percent = Percent::new(spec.battery.percent_of(raw))?;
    let charging = spec
        .battery
        .charging
        .and_then(|field| assembled.get(field.index).map(|b| field.is_charging(*b)));
    Some(BatteryReading { percent, charging })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{BatteryFields, BrandSpec, ChargingField, FrameLayout, ReplyTransport};

    /// 合成描述：`[0]=0x11` → `[1]=cmd 0x22` → `[2..4]` 填充 → `[4..]` payload（电量、充电标志）。
    const SYNTHETIC: BrandSpec = BrandSpec {
        id: "synthetic",
        label: "合成品牌",
        name_from_product: false,
        usage_page: 0xFFC0,
        usage_page_alt: None,
        usage: None,
        usage_page_write: None,
        reassembly: None,
        transport: crate::spec::ReplyTransport::Listen,
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
            cmd: Some(0x22),
            cmd_high: None,
            subtag: None,
            percent_index: 0,
            percent_mask: 0xFF,
            charging: Some(ChargingField {
                mask: 0xFF,
                index: 1,
                true_values: &[0x02],
                error_from: None,
            }),
        },
        listen: None,
        notify: None,
        request: None,
        block_offset: None,
        profile_probe: None,
        ack: None,
        fallback_report_id: None,
        expected_interval_ms: 20_000,
        device_table_file: crate::brands::ajazz::SPEC.device_table_file,
        variants: &[],
        route: None,
    };

    #[test]
    fn decodes_fixed_length_layout_without_subtag() {
        let r = decode_battery(&SYNTHETIC, &[0x11, 0x22, 0x00, 0x00, 0x37, 0x02]).unwrap();
        assert_eq!(r.percent.get(), 55);
        assert_eq!(r.charging, Some(true));
    }

    // ---- 多包重组（对应迈从键盘 2.4G 通道）----

    /// 合成多包描述：`[0]=0x4A` `[1]=总包数` `[2]=序号` `[3]=保留` `[4..]=载荷` `[-1]=校验和`。
    const REASSEMBLED: BrandSpec = BrandSpec {
        id: "synthetic-reasm",
        label: "合成多包品牌",
        name_from_product: false,
        transport: ReplyTransport::InterruptQuery,
        reassembly: Some(crate::spec::Reassembly {
            cmd: 0x4A,
            total_index: 1,
            seq_index: 2,
            payload_offset: 4,
            trailer_len: 1,
            timeout_ms: 5_000,
        }),
        frame: FrameLayout {
            report_id: Some(0x33),
            cmd_index: 0,
            len_index: None,
            payload_offset: 1,
            fixed_payload_len: 2,
        },
        battery: BatteryFields {
            cmd: None,
            cmd_high: None,
            subtag: None,
            percent_index: 0,
            percent_mask: 0xFF,
            charging: Some(ChargingField {
                mask: 0x0F,
                index: 1,
                true_values: &[0x01],
                error_from: None,
            }),
        },
        notify: None,
        request: Some(crate::spec::RequestTemplate {
            usage_page: 0xFFC0,
            cmd: 0x4A,
            cmd_high: None,
            prefix: &[],
            report_len: 6,
            cmd_index: 1,
            payload_offset: 2,
            payload: &[0x01],
            interval_ms: Some(1_000),
            inverted: false,
            retry: 0,
        }),
        ..SYNTHETIC
    };

    /// 造一条应答包；`with_id` 控制是否带报告 ID。
    fn packet(total: u8, seq: u8, payload: &[u8], with_id: bool) -> Vec<u8> {
        let mut p = Vec::new();
        if with_id {
            p.push(0x33);
        }
        p.extend_from_slice(&[0x4A, total, seq, 0x00]);
        p.extend_from_slice(payload);
        p.push(0x5A); // 校验和，不进载荷
        p
    }

    /// 组装进度可查可清。
    #[test]
    fn reassembler_reports_and_clears_progress() {
        let t0 = Instant::now();
        let mut r = Reassembler::new();
        assert!(!r.is_complete(), "空拼接器不算完成");

        assert!(
            r.feed(&REASSEMBLED, &packet(2, 0, &[0x3C, 0x01], true), t0)
                .is_none()
        );
        assert!(!r.is_complete(), "只收到一半不得算完成");

        r.reset();
        assert!(!r.is_complete());
        assert!(
            r.feed(&REASSEMBLED, &packet(2, 1, &[0x00, 0x00], true), t0)
                .is_none(),
            "半包已丢弃，孤立的尾包拼不出结果"
        );

        assert!(
            r.feed(&REASSEMBLED, &packet(2, 0, &[0x3C, 0x01], true), t0)
                .is_none()
        );
        assert!(
            r.feed(&REASSEMBLED, &packet(2, 1, &[0x00, 0x00], true), t0)
                .is_some()
        );
        assert!(!r.is_complete());
    }

    #[test]
    fn assembles_multi_packet_answer() {
        let mut r = Reassembler::new();
        let t0 = Instant::now();
        assert!(
            r.feed(&REASSEMBLED, &packet(2, 0, &[0x3C, 0x01], true), t0)
                .is_none(),
            "第一包不够，必须等第二包"
        );
        let done = r
            .feed(&REASSEMBLED, &packet(2, 1, &[0x00, 0x00], true), t0)
            .expect("凑齐两包后必须产出组装结果");
        assert_eq!(done, vec![0x3C, 0x01, 0x00, 0x00]);
        let reading = decode_assembled(&REASSEMBLED, &done).unwrap();
        assert_eq!(reading.percent.get(), 60);
        assert_eq!(reading.charging, Some(true), "低半字节 1 = 充电");
    }

    #[test]
    fn assembles_without_report_id_too() {
        let mut r = Reassembler::new();
        let t0 = Instant::now();
        r.feed(&REASSEMBLED, &packet(1, 0, &[0x50, 0x00], false), t0);
        // 单包应答：第一包就该完成。
        let mut r = Reassembler::new();
        let done = r
            .feed(&REASSEMBLED, &packet(1, 0, &[0x50, 0x00], false), t0)
            .expect("单包应答必须立即完成");
        assert_eq!(
            decode_assembled(&REASSEMBLED, &done).unwrap().percent.get(),
            80
        );
    }

    #[test]
    fn ignores_packets_with_foreign_command() {
        let mut r = Reassembler::new();
        let mut p = packet(1, 0, &[0x50, 0x00], true);
        p[1] = 0x0A; // 监听帧（电量上报）也走这条输入通道
        assert!(r.feed(&REASSEMBLED, &p, Instant::now()).is_none());
    }

    #[test]
    fn out_of_order_packet_is_dropped() {
        let mut r = Reassembler::new();
        let t0 = Instant::now();
        assert!(
            r.feed(&REASSEMBLED, &packet(2, 0, &[0x3C, 0x00], true), t0)
                .is_none()
        );
        assert!(
            r.feed(&REASSEMBLED, &packet(2, 2, &[0x00, 0x00], true), t0)
                .is_none()
        );
        let done = r.feed(&REASSEMBLED, &packet(2, 1, &[0x00, 0x00], true), t0);
        assert!(done.is_some(), "丢掉乱序包后仍应能凑齐");
    }

    #[test]
    fn sequence_restart_discards_previous_packet() {
        let mut r = Reassembler::new();
        let t0 = Instant::now();
        r.feed(&REASSEMBLED, &packet(2, 0, &[0xFF, 0x00], true), t0);
        let done = r
            .feed(&REASSEMBLED, &packet(2, 0, &[0x3C, 0x00], true), t0)
            .is_none();
        assert!(done);
        let done = r
            .feed(&REASSEMBLED, &packet(2, 1, &[0x00, 0x00], true), t0)
            .unwrap();
        assert_eq!(done, vec![0x3C, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn half_packet_is_dropped_after_timeout() {
        let mut r = Reassembler::new();
        let t0 = Instant::now();
        r.feed(&REASSEMBLED, &packet(2, 0, &[0xFF, 0x00], true), t0);
        let late = t0 + Duration::from_secs(6);
        assert!(
            r.feed(&REASSEMBLED, &packet(2, 1, &[0x00, 0x00], true), late)
                .is_none()
        );
    }

    #[test]
    fn truncated_packet_is_ignored() {
        let mut r = Reassembler::new();
        assert!(
            r.feed(&REASSEMBLED, &[0x33, 0x4A, 0x01], Instant::now())
                .is_none()
        );
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

    /// 确认位不符时整帧不可用，不得当成 0% 电量。
    #[test]
    fn missing_ack_marks_frame_malformed() {
        let mut spec = SYNTHETIC;
        spec.ack = Some(crate::spec::AckField {
            index: 1,
            mask: 0x0F,
            value: 0x02,
        });
        let report = [0x11, 0x20, 0x00, 0x00, 0x50, 0x02];
        match decode_battery_detailed(&spec, &report) {
            Err(reje) => assert_eq!(
                reje.reason,
                RejectReason::AckMismatch {
                    want: 0x02,
                    got: Some(0x20),
                }
            ),
            Ok(reading) => panic!("确认位不匹配却被当成有效读数：{reading:?}"),
        }
        // 确认位对上则照常出读数。
        let ok = [0x11, 0x22, 0x00, 0x00, 0x50, 0x02];
        assert!(decode_battery_detailed(&spec, &ok).is_ok());
    }

    /// 状态字节达到错误态起点时整帧作废。
    #[test]
    fn status_error_discards_whole_frame() {
        let mut spec = SYNTHETIC;
        spec.battery.charging = Some(ChargingField {
            index: 1,
            mask: 0xFF,
            true_values: &[0x02],
            error_from: Some(0x03),
        });
        match decode_battery_detailed(&spec, &[0x11, 0x22, 0x00, 0x00, 0x50, 0x03]) {
            Err(reje) => assert_eq!(reje.reason, RejectReason::StatusError { raw: 0x03 }),
            Ok(reading) => panic!("错误态下不得给出读数：{reading:?}"),
        }
    }

    /// 布局自相矛盾（长度字段落在缓冲之外）时报诊断而不是 panic。
    #[test]
    fn length_field_outside_report_is_rejected() {
        let mut spec = SYNTHETIC;
        spec.frame.len_index = Some(5);
        spec.frame.payload_offset = 2;
        let report = [0x11, 0x22, 0x50, 0x02, 0x00];
        match decode_battery_detailed(&spec, &report) {
            Err(reje) => assert_eq!(reje.reason, RejectReason::LengthFieldMissing { index: 5 }),
            Ok(reading) => panic!("缺长度字段却解析成功：{reading:?}"),
        }
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

    // ---------- 命令字前置过滤 ----------

    /// 未声明命令字的协议放行一切帧；声明了就只认命令字对上的帧，短帧不得 panic。
    #[test]
    fn matches_cmd_filters_frames_by_declared_command() {
        let mut spec = SYNTHETIC;
        spec.battery.cmd = None;
        assert!(matches_cmd(&spec, &[]), "无命令字声明的协议一律放行");

        assert!(matches_cmd(&SYNTHETIC, &[0x11, 0x22, 0x00]));
        assert!(!matches_cmd(&SYNTHETIC, &[0x11, 0x23, 0x00]));
        assert!(
            !matches_cmd(&SYNTHETIC, &[0x11]),
            "命令字下标越过缓冲属于「不是电量帧」，不是错误"
        );
    }

    /// 成因文本是对外契约（日志按它 grep）。
    #[test]
    fn rejection_text_is_stable_for_logs() {
        let cases = [
            (
                RejectReason::ReportTooShort { min: 7 },
                "report_too_short(min=7)",
            ),
            (
                RejectReason::ReportIdMismatch {
                    want: 0x06,
                    got: Some(0x07),
                },
                "report_id_mismatch(want=0x06 got=0x07)",
            ),
            (
                RejectReason::CmdMismatch {
                    want: 0x22,
                    got: None,
                },
                "cmd_mismatch(want=0x22 got=none)",
            ),
            (
                RejectReason::CmdHighMismatch {
                    want: 0x09,
                    got: Some(0x08),
                },
                "cmd_high_mismatch(want=0x09 got=0x08)",
            ),
            (
                RejectReason::AckMismatch {
                    want: 0x02,
                    got: Some(0x20),
                },
                "ack_mismatch(want=0x02 got=0x20)",
            ),
            (
                RejectReason::StatusError { raw: 0x03 },
                "status_error(status=0x03)",
            ),
            (
                RejectReason::LengthFieldMissing { index: 2 },
                "length_field_missing(index=2)",
            ),
            (
                RejectReason::DeclaredPayloadTooShort {
                    declared: 0,
                    min: 2,
                },
                "declared_payload_too_short(declared=0 min=2)",
            ),
            (
                RejectReason::DeclaredPayloadPastEnd {
                    declared: 9,
                    end: 14,
                    len: 6,
                },
                "declared_payload_past_end(declared=9 end=14 len=6)",
            ),
            (
                RejectReason::SubtagsMismatch {
                    want: 0x03,
                    got: None,
                },
                "subtag_mismatch(want=0x03 got=none)",
            ),
            (
                RejectReason::PercentMissing { index: 1 },
                "percent_missing(index=1)",
            ),
            (RejectReason::EmptyPayload, "empty_payload"),
            (
                RejectReason::PercentOutOfRange { raw: 0x65 },
                "percent_out_of_range(raw=101)",
            ),
        ];
        for (reason, want) in cases {
            assert_eq!(reason.to_string(), want);
            assert_eq!(reason.as_str(), want.split('(').next().unwrap());
        }
    }
}
