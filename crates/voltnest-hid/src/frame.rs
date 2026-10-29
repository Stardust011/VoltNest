//! 通用帧解析：**完全由 [`BrandSpec`] 驱动**的电量解码（新增品牌不改这里）。
//!
//! 所有失败必须映射为**结构化错误码**，不得静默丢帧、不得回落到 0%。错误码与情形的对应关系：
//!
//! | 情形 | 错误码 |
//! |---|---|
//! | 报告短于 `min_report_len`、报告 ID 不符、确认位不符、命令字不符、长度字段越界、payload 过短、子标签不符、状态错误态 | `malformed_frame` |
//! | payload 全零 | `empty_payload` |
//! | 电量字节 > 100 | `percent_out_of_range` |
//!
//! # 诊断：错误码只是一个词
//! `malformed_frame` 有六种成因，仅看错误码分不清「报告 ID 不符」与「payload 装不下」。故解析器同时给 [`FrameReject`]：哪一校验失败、期望与实际各是多少、原始字节是什么。它只进诊断页与日志（[`FrameReject::detail`]），不参与状态判定——状态机仍只看 [`ErrorCode`]。
//!
//! # 三条解析入口，互不替代
//! | 入口 | 用于 | 认不出的帧 |
//! |---|---|---|
//! | [`decode_battery`] | 单帧应答（feature 查询 / output 查询） | 报错并进 `lastError` |
//! | [`decode_listen`] | 监听通道上的自发上报帧（混着位移、媒体键等大量无关帧） | 静默丢弃 |
//! | [`decode_assembled`] | 多包应答经 [`Reassembler`] 拼合之后 | 返回 `None` |

use crate::spec::BrandSpec;
use std::time::{Duration, Instant};
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
    /// `cmd_mismatch(want=0x03 got=0x23)`：参数写在同一行，诊断页可直接读。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())?;
        match self {
            Self::ReportTooShort { min } => write!(f, "(min={min})"),
            Self::ReportIdMismatch { want, got }
            | Self::CmdMismatch { want, got }
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
/// 报告 ID 不参与取反（官方只反转 ID 之后的内容）—— 这条规则在这里写一次，
/// 全帧归一化与单字节比对共用，避免两处各写一份条件。
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
///
/// # 无编号报告：两种首字节都要试
/// 协议没声明报告 ID 时（迈从 CZ/GLW 族），hidapi 是否在首字节补一个报告 ID 槽，取决于该集合的
/// 报告描述符与平台 —— 静态无法判定。官方解析器的输入是 WebHID 的 `report.data`（**不含** ID），
/// 而 hidapi 在带编号的集合上会把 ID 放在 `[0]`。两种形态都试一次，命中哪个用哪个；
/// 判据与 [`decode_listen`] 相同，误判只可能来自「ID 恰好等于命令字且长度字段也自洽」。
///
/// 声明了报告 ID 的协议不走这条容错：那时 ID 是**已知**的硬约束，放宽只会让坏帧蒙混过关。
pub fn decode_battery_detailed(
    spec: &BrandSpec,
    report: &[u8],
) -> Result<BatteryReading, FrameReject> {
    if spec.frame.report_id.is_some() {
        return decode_battery_at(spec, report, report);
    }
    let mut first: Option<FrameReject> = None;
    for shift in 0..=1usize {
        let Some(window) = report.get(shift..) else {
            continue;
        };
        match decode_battery_at(spec, window, report) {
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
fn decode_battery_at(
    spec: &BrandSpec,
    report: &[u8],
    original: &[u8],
) -> Result<BatteryReading, FrameReject> {
    let layout = &spec.frame;
    // 每次拒绝都带上原始报告：诊断页要的是「被拒的那一帧长什么样」。
    let reject = |reason: RejectReason| FrameReject {
        code: reason_code(reason),
        reason,
        report: original.to_vec(),
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
            .map(|(index, byte)| normalized_byte(spec, index, *byte))
            .collect::<Vec<_>>();
        normalized_report.as_slice()
    } else {
        report
    };

    // 2) 应答确认位：官方驱动靠它判定「设备已处理该命令」（`tgdevice.js:1836`）。
    //    缺这条校验时，一个「设备还没准备好」的应答会被当成 0% 电量上报。
    if let Some(ack) = spec.ack {
        let raw = report.get(ack.index).copied();
        if raw.map(|b| b & ack.mask) != Some(ack.value) {
            return Err(reject(RejectReason::AckMismatch {
                want: ack.value,
                got: raw,
            }));
        }
    }

    // 3) 命令字（协议回显时才有）。
    if let Some(cmd) = spec.battery.cmd
        && report.get(layout.cmd_index) != Some(&cmd)
    {
        return Err(reject(RejectReason::CmdMismatch {
            want: cmd,
            got: report.get(layout.cmd_index).copied(),
        }));
    }

    // 4) payload 长度：字段声明 / 固定长度两种模式。
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

    // 5) 全零 payload 是「设备在场但没数据」，与坏帧分开上报。
    if payload.iter().all(|b| *b == 0) {
        return Err(reject(RejectReason::EmptyPayload));
    }

    // 6) 子标签（若协议有）。
    if let Some((index, want)) = spec.battery.subtag
        && payload.get(index) != Some(&want)
    {
        return Err(reject(RejectReason::SubtagsMismatch {
            want,
            got: payload.get(index).copied(),
        }));
    }

    // 7) 错误态：设备自报异常时电量字段没有意义，整帧不可用 ——
    //    不得降级成「未充电 + 某个电量值」（官方语义：状态 ≥0x03 为错误态）。
    if let Some(field) = spec.battery.charging
        && let Some(&raw) = payload.get(field.index)
        && field.is_error(raw)
    {
        return Err(reject(RejectReason::StatusError { raw }));
    }

    // 8) 电量。Percent::new 的 None 必须在这里映射成错误码，不得 `?` 后 continue。
    let raw = payload
        .get(spec.battery.percent_index)
        .copied()
        .ok_or_else(|| {
            reject(RejectReason::PercentMissing {
                index: spec.battery.percent_index,
            })
        })?;
    // 一个字节塞两个字段的协议先取位：迈从 GLW 键盘 `0x80|pct`，整字节读会判成越界。
    let raw = spec.battery.percent_of(raw);
    let percent = if spec.clamp_percent {
        Percent::new(raw.min(100)).expect("截断后的电量必定在 0..=100")
    } else {
        Percent::new(raw).ok_or_else(|| reject(RejectReason::PercentOutOfRange { raw }))?
    };

    // 9) 充电标志：协议不提供 → None；字节缺失 → None（不猜测）。
    //    一位一义的状态字节要先按 `mask` 取位再比较：迈从键盘把「满电」放在高半字节、
    //    把「充电中」放在低半字节，整字节比较会在 `0x11`（满电且充电）时漏判。
    let charging = spec
        .battery
        .charging
        .and_then(|field| payload.get(field.index).map(|b| field.is_charging(*b)));

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

/// 解析一帧**设备自发上报**（宿主未提问）的电量。
///
/// 返回 `None` 表示「这一帧不是电量上报」（鼠标位移、媒体键、或别的命令），调用方应静默丢弃 ——
/// 输入通道上无关帧占绝大多数，把它们报成 `Unreadable` 只会把诊断页刷满、把 `lastError` 写脏。
///
/// 偏移容错：官方解析器的输入是 WebHID 的 `report.data`（**不含**报告 ID），而 hidapi 的
/// `read_timeout` 在报告带编号时把报告 ID 放在首字节。两种都试，命中哪个用哪个 ——
/// 首字节必须等于 `cmd`，误判成电量的概率只来自「报告 ID 恰好等于 cmd 且子标签也对上」。
pub fn decode_listen(spec: &BrandSpec, report: &[u8]) -> Option<BatteryReading> {
    let listen = spec.listen?;
    for offset in [0usize, 1] {
        let Some(window) = report.get(offset..) else {
            continue;
        };
        if window.len() < listen.min_len() {
            continue;
        }
        if window[0] != listen.cmd || window[listen.subtag_index] != listen.subtag_value {
            continue;
        }
        let raw = window[listen.percent_index];
        let status = window[listen.status_index];
        // 官方：高半字节 `fullStatus` 非零即满电，直接把电量按 100 上报。
        let percent = if status >> 4 != 0 { 100u8 } else { raw };
        let percent = Percent::new(percent)?;
        let charging = (status & 0x0F) == listen.charging_low_nibble;
        return Some(BatteryReading {
            percent,
            charging: Some(charging),
        });
    }
    None
}

/// 该帧是不是本协议的电量帧（按命令字判定，不解析字段）。
///
/// 监听通道 / output 查询通道用它做前置过滤：只有命令字对上的帧才进解析器，
/// 其它帧直接丢，不进 `lastError`。
///
/// 无编号报告的协议同样允许首字节是平台补的报告 ID 槽（与 [`decode_battery_detailed`] 一致）：
/// 否则「位移一字节」的应答会在前置过滤就被静默丢掉 —— 设备在列表里却永远没有读数、也没有错误。
pub fn matches_cmd(spec: &BrandSpec, report: &[u8]) -> bool {
    let Some(cmd) = spec.battery.cmd else {
        return true;
    };
    let index = spec.frame.cmd_index;
    if spec.frame.report_id.is_none() {
        // 位移后的窗口里，`normalized_byte` 的取反规则不再适用（它是按绝对下标写的），
        // 故取反协议只认未位移的形态。
        return (0..=1usize).any(|base| {
            let at = base + index;
            report.get(at).copied().map(|byte| {
                if base == 0 {
                    normalized_byte(spec, at, byte)
                } else {
                    byte
                }
            }) == Some(cmd)
        });
    }
    report
        .get(index)
        .copied()
        .map(|byte| normalized_byte(spec, index, byte))
        == Some(cmd)
}

/// 诊断用：报告尾部填充是否全零。
///
/// 实测帧的填充恒为 0x00，但**不**作为拒绝条件 —— 把填充当校验会让一个
/// 无害的固件小改动直接变成「设备不可读」。
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

/// 多包应答的拼接器：把设备拆成 N 条输入报告的一条应答拼回一条。
///
/// 只有 [`BrandSpec::reassembly`] 非空的协议会用到它（迈从键盘的 2.4G 通道）。
/// 生命周期按「一次查询」算：查询发出后喂包，凑齐 `total` 个序号即产出组装结果；
/// 超过 `timeout_ms` 没凑齐就丢弃半包 —— 半包拼出来的电量比没有电量更糟。
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

    /// 喂一条输入报告。返回 `Some(组装结果)` 表示这次应答已凑齐。
    ///
    /// 只认命令字对上的包；序号不符的包直接丢（官方同样只接受 `current === expected`，
    /// 迟到/重发的包会造成重复字节，比丢一包更难查）。
    pub fn feed(&mut self, spec: &BrandSpec, report: &[u8], now: Instant) -> Option<Vec<u8>> {
        let reasm = spec.reassembly?;
        // 半包超时：先丢，再按「这条包属于新一次应答」继续。
        if let Some(deadline) = self.deadline
            && now > deadline
        {
            self.reset();
        }
        // 与监听帧同一套容错：数据窗口可能带报告 ID（hidapi）也可能不带（官方解析器）。
        let window = (0..=1usize).find_map(|base| {
            let w = report.get(base..)?;
            (w.len() > reasm.min_packet_len() && w.first() == Some(&reasm.cmd)).then_some(w)
        })?;
        let total = *window.get(reasm.total_index)? as usize;
        let seq = *window.get(reasm.seq_index)? as usize;
        if total == 0 || seq >= total {
            return None;
        }
        // seq 回到 0 = 设备重新开始一轮应答，丢弃旧半包。
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
/// **与监听帧不同，这里不做「满电按 100」的改写**：官方键盘页拿到查询结果后把 `fullStatus`
/// 显式置 0、直接显示 `batteryLevel`（`s.value=0, s.value&&(e.value=100)` 永远不成立），
/// 满电标志只在实时上报那条路径生效。字段位置取自 [`BrandSpec::battery`]，没有第二套下标。
pub fn decode_assembled(spec: &BrandSpec, assembled: &[u8]) -> Option<BatteryReading> {
    spec.reassembly?;
    if assembled.len() < spec.min_payload_len() {
        return None;
    }
    let raw = *assembled.get(spec.battery.percent_index)?;
    let percent = Percent::new(spec.battery.percent_of(raw))?;
    // 状态字节只有协议声明了充电标志时才读：没声明就当作「未知」而不是「未充电」。
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

    /// 与任何真实品牌都不同的合成描述：验证解析器只吃字段、不含品牌分支。
    ///
    /// 布局：`[0] = 0x11`（无长度字段）→ `[1]=cmd 0x22` → `[2..4]` 填充
    /// → `[4..]` payload：`[0]=电量`、`[1]=充电标志(0x02=充电中)`。
    const SYNTHETIC: BrandSpec = BrandSpec {
        id: "synthetic",
        label: "合成品牌",
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
        request: None,
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

    /// 合成多包描述：窗口 `[0]=0x4A(命令) [1]=总包数 [2]=序号 [3]=保留 [4..]=载荷 [-1]=校验和`，
    /// 组装结果 `[0]=电量 [1]=状态`（高半字节满电、低半字节充电）。
    const REASSEMBLED: BrandSpec = BrandSpec {
        id: "synthetic-reasm",
        label: "合成多包品牌",
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
        request: Some(crate::spec::RequestTemplate {
            usage_page: 0xFFC0,
            cmd: 0x4A,
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

    /// 造一条应答包；`with_id` 控制是否带报告 ID（hidapi 与官方解析器两种形态）。
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

    /// 组装进度必须可查、可清：诊断页看 is_complete，新一轮查询前 reset。
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

        // 主动丢弃半包后，紧接着的后续包不得接着拼。
        r.reset();
        assert!(!r.is_complete());
        assert!(
            r.feed(&REASSEMBLED, &packet(2, 1, &[0x00, 0x00], true), t0)
                .is_none(),
            "半包已丢弃，孤立的尾包拼不出结果"
        );

        // 设备从 seq 0 重新开始才恢复。
        assert!(
            r.feed(&REASSEMBLED, &packet(2, 0, &[0x3C, 0x01], true), t0)
                .is_none()
        );
        assert!(
            r.feed(&REASSEMBLED, &packet(2, 1, &[0x00, 0x00], true), t0)
                .is_some()
        );
        // 完成后自动复位，下一轮从头开始。
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
        // 序号 2 越界（total=2 合法序号只有 0/1）：丢弃，不破坏已收到的第一包。
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
        // 设备重新开始一轮：旧的半包必须丢，否则会拼出 [0xFF, 0x00, ...]。
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
        // 超过 timeout_ms（5s）后到达的第二包：半包已过期，必须丢弃而不是拼上去。
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

    /// 应答确认位不足时整帧不可用：设备「还没准备好」的应答不是 0% 电量。
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

    /// 状态字节达到错误态起点时整帧作废，不得降级成「未充电 + 某个电量」。
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

    /// 成因文本是对外契约（日志按它 grep）：稳定词 + 同一行的参数。
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
