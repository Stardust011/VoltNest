//! `hid-vbas`（HID Vendor Battery Access Spec）协议家族：枚举、读取、帧解析与品牌表。
//!
//! 只做 HID 侧 I/O 与解析，不做去重、状态判定、分组或持久化。协议形状（HID 私有通道 + 命令字 +
//! 定长报告）为家族共用，字段下标与查询方式为品牌差异：每个品牌一份 [`spec::BrandSpec`]，解析器
//! 与运行时只读字段、无 `match brand_id` 分支。
//!
//! 型号表（`data/mappings/*.json`）在运行时由 [`mappings`] 定位并加载（定位顺序见该模块），
//! 因此安装包必须随二进制带上该目录。

#![forbid(unsafe_code)]
#![deny(clippy::disallowed_types)]
#![warn(missing_docs)]

pub mod brands;
pub mod frame;
pub mod hex;
pub mod mappings;
pub mod probe;
pub mod spec;
pub mod table;

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod runtime;

/// 出现检测间隔（毫秒）：固定 5s，内部常量，不进 `settings.json`。
pub const PRESENCE_POLL_MS: u64 = 5_000;

/// 读取超时（毫秒）：限制单次读取的阻塞上界。
pub const READ_TIMEOUT_MS: i32 = 120;

/// 读取缓冲长度：64 字节报告 + 1 字节 Report ID。
pub const READ_BUFFER_LEN: usize = 65;

/// 一轮查询内两次尝试之间的间隔（毫秒）。
pub const RETRY_GAP_MS: u64 = 100;

/// 监听通道上等待设备自发上报的读取超时（毫秒）。
pub const LISTEN_READ_TIMEOUT_MS: i32 = 100;

pub use brands::Registry;
pub use frame::{BatteryReading, FrameReject, RejectReason};
pub use spec::{BrandSpec, FrameLayout, ReplyTransport};
