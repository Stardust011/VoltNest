//! VoltNest 核心：设备注册表、状态机、归一化、分组、通知策略与快照。
//!
//! 本 crate 是**纯逻辑**：不读系统时间（`now_ms` 全部由宿主传入）、不建线程、
//! 不做文件 I/O、不调用任何平台 API（规格 §5.2 / K-1..K-5）。
//!
//! 宿主是设备状态的唯一所有者：Provider 只上报原始事实，去重、`modelKey` 归一化、
//! 跨 transport 分组、状态判定、时间戳与持久化都在这里完成。

#![forbid(unsafe_code)]
#![deny(clippy::disallowed_types)]
#![warn(missing_docs)]

pub mod diagnostics;
pub mod engine;
pub mod humanize;
pub mod normalize;
pub mod notify;
pub mod record;
pub mod snapshot;
pub mod state;

pub use diagnostics::{DiagEvent, DiagLevel, Diagnostics};
pub use engine::{Engine, EngineConfig, SeedEntry, Transition};
pub use humanize::humanize_ago;
pub use normalize::normalize;
pub use notify::{LowBatteryAlert, NotificationBook, NotificationConfig, NotificationEntry};
pub use record::Record;
pub use snapshot::{CardDto, DeviceDto, Snapshot, TrayDto};
pub use state::DeviceState;
