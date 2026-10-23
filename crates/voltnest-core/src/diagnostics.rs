//! 诊断事件环形缓冲。
//!
//! 诊断页要能回答「系统吞了 / 我们没发」「为什么这台设备没出现」这类问题，
//! 因此每条事件都带稳定代码、时间戳与可选设备 ID（AB-5 / O-2 / D5）。

use std::collections::VecDeque;

/// 诊断级别，对应诊断页的红条 / 黄条 / 普通行 / 折叠的调试行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagLevel {
    /// 调试信息：讲「哪些字段存在但没参与判定」这类排查细节，前端默认隐藏。
    ///
    /// 与 [`Self::Info`] 的分界是**结论是否需要它**：`Info` 放要用户知情的结论
    /// 与原因，`Debug` 放逐条枚举噪音（例如一台复合 HID 报了 7 个集合，其中 5 个
    /// 本项目不支持）。级别越低噪声越大，不会挤掉关键行。
    Debug,
    /// 普通信息。
    Info,
    /// 需要用户知情但不阻塞。
    Warn,
    /// 采集不可用等硬故障。
    Error,
}

impl DiagLevel {
    /// 稳定字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// 一条诊断事件。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiagEvent {
    /// UTC 毫秒。
    pub at_ms: u64,
    /// 级别。
    pub level: DiagLevel,
    /// 稳定代码（如 `clock_backward`、`PROFILE_CONFLICT`）。
    pub code: &'static str,
    /// 相关设备（`stableId`）。
    pub device_id: Option<String>,
    /// 人类可读细节。
    pub detail: String,
}

/// 固定在 `capacity` 条的环形缓冲。写满后丢最旧的。
#[derive(Debug)]
pub struct Diagnostics {
    events: VecDeque<DiagEvent>,
    capacity: usize,
}

impl Diagnostics {
    /// 默认保留 200 条。
    pub fn new() -> Self {
        Self::with_capacity(200)
    }

    /// 指定容量。
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity.min(4096)),
            capacity: capacity.max(1),
        }
    }

    /// 记录一条。
    pub fn push(
        &mut self,
        at_ms: u64,
        level: DiagLevel,
        code: &'static str,
        device_id: Option<String>,
        detail: impl Into<String>,
    ) {
        if self.events.len() == self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(DiagEvent {
            at_ms,
            level,
            code,
            device_id,
            detail: detail.into(),
        });
    }

    /// 最近的在前。
    pub fn recent(&self) -> Vec<DiagEvent> {
        self.events.iter().rev().cloned().collect()
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 是否有 `Error` 级别事件（诊断页红条）。
    pub fn has_error(&self) -> bool {
        self.events.iter().any(|e| e.level == DiagLevel::Error)
    }

    /// 清空（用户点「清除诊断」）。
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_newest_first_and_respects_capacity() {
        let mut d = Diagnostics::with_capacity(3);
        for i in 0..5 {
            d.push(i, DiagLevel::Info, "test", None, format!("e{i}"));
        }
        assert_eq!(d.len(), 3);
        let recent = d.recent();
        assert_eq!(recent[0].at_ms, 4);
        assert_eq!(recent[2].at_ms, 2, "最旧的两条必须被丢弃");
    }

    #[test]
    fn error_flag_tracks_level() {
        let mut d = Diagnostics::new();
        assert!(!d.has_error());
        d.push(0, DiagLevel::Warn, "w", None, "warn");
        assert!(!d.has_error());
        d.push(
            0,
            DiagLevel::Error,
            "e",
            Some("hid:7768:6426".into()),
            "boom",
        );
        assert!(d.has_error());
        assert_eq!(d.recent()[0].code, "e");
        d.clear();
        assert!(d.is_empty());
    }
}
