//! 结构化错误码。字符串形式对外可见，改名属破坏性变更。

use std::fmt;

/// Provider 上报的结构化错误码。宿主只做分类，不重新校验数据。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ErrorCode {
    /// HID 句柄失效 / 设备从系统移除。
    DeviceGone,
    /// `read_timeout` 到期、BLE `connect` 4s 超时。
    IoTimeout,
    /// 被官方驱动独占。
    AccessDenied,
    /// 长度 / cmd / 布局不符。
    MalformedFrame,
    /// 全零 payload。
    EmptyPayload,
    /// 电量字节 > 100。
    PercentOutOfRange,
    /// BLE 无 `0x180F`（负缓存的触发条件）。
    ServiceMissing,
    /// GATT 缓存命中被拒（读取一律 `Uncached`）。
    CachedValueRejected,
    /// `0x2A19` 订阅失败。
    NotifySubscribeFailed,
    /// Provider 内部错误（不含设备语义）。
    Internal,
}

impl ErrorCode {
    /// 稳定字符串形式，写入日志、诊断页与 `Snapshot.lastError`。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeviceGone => "device_gone",
            Self::IoTimeout => "io_timeout",
            Self::AccessDenied => "access_denied",
            Self::MalformedFrame => "malformed_frame",
            Self::EmptyPayload => "empty_payload",
            Self::PercentOutOfRange => "percent_out_of_range",
            Self::ServiceMissing => "service_missing",
            Self::CachedValueRejected => "cached_value_rejected",
            Self::NotifySubscribeFailed => "notify_subscribe_failed",
            Self::Internal => "internal",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 字符串对外可见：既有值不得改动，只能新增变体。
    #[test]
    fn stable_strings() {
        let pairs = [
            (ErrorCode::DeviceGone, "device_gone"),
            (ErrorCode::IoTimeout, "io_timeout"),
            (ErrorCode::AccessDenied, "access_denied"),
            (ErrorCode::MalformedFrame, "malformed_frame"),
            (ErrorCode::EmptyPayload, "empty_payload"),
            (ErrorCode::PercentOutOfRange, "percent_out_of_range"),
            (ErrorCode::ServiceMissing, "service_missing"),
            (ErrorCode::CachedValueRejected, "cached_value_rejected"),
            (ErrorCode::NotifySubscribeFailed, "notify_subscribe_failed"),
            (ErrorCode::Internal, "internal"),
        ];
        for (code, text) in pairs {
            assert_eq!(code.as_str(), text);
            assert_eq!(code.to_string(), text);
        }
    }
}
