//! VoltNest Provider 契约。
//!
//! 本 crate 只包含**纯类型与常量**，是 HID / BLE crate 与宿主（`voltnest-core`、
//! `src-tauri`）之间唯一的契约面（ADR-0010）。它不含任何 I/O、线程、平台 API，
//! 也不含状态机。
//!
//! 两条最容易写错的规则在这里被类型消灭，而不是靠评审记住：
//!
//! - [`Percent`] 是 newtype，`> 100` 在构造期被拒绝 → 状态机不可能收到越界电量。
//! - [`ProviderProfile::charging`] 为 [`ChargingSupport::Unavailable`] 时，
//!   [`ProviderProfile::sanitize_charging`] 会把任何 `Some(bool)` 压成 `None`
//!   → BLE 不会被渲染成「未充电」（PC-5）。

#![forbid(unsafe_code)]
#![deny(clippy::disallowed_types)]
#![warn(missing_docs)]

pub mod clock;
pub mod error;
pub mod event;
pub mod ids;
pub mod profile;
pub mod testing;
pub mod transport;

pub use error::ErrorCode;
pub use event::{DeviceFact, ProviderEvent, RemovalReason, ValidReading};
pub use ids::{Percent, StableId};
pub use profile::{
    BLE_BAS, Cadence, ChargingSupport, HID_VBAS, PollPolicy, ProfileError, ProviderProfile,
    Sensitivity, Thresholds, validate_registry,
};
pub use transport::Transport;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

/// 协议家族 ID：`hid-vbas`（HID Vendor Battery Access Spec）。
///
/// 同时是日志前缀、诊断分组键与 `Snapshot.provider` 值。家族层面**不含品牌**：
/// 任何走 HID 私有协议报电量的设备都归入本家族，品牌差异由
/// `voltnest-hid` 内的品牌描述（字段位置 + 解析规则）表达，见 ADR-0011。
pub const FAMILY_HID_VBAS: &str = "hid-vbas";

/// 协议家族 ID：`ble-bas`。
pub const FAMILY_BLE_BAS: &str = "ble-bas";

/// Provider 的统一入口（ADR-0010 D1）。
///
/// 实现者只上报**原始事实**：出现、原始读数、结构化错误、链路状态。
/// 去重、冲突仲裁、`modelKey` 归一化、分组、时间戳、持久化全部属于宿主。
pub trait Provider: Send + 'static {
    /// 编译期声明的能力与节奏。
    fn profile(&self) -> &'static ProviderProfile;

    /// 阻塞运行，直到 `stop` 置位。
    ///
    /// 返回前必须释放全部句柄与 GATT 订阅；实现内部不得调用 `std::process::exit`。
    fn run(self: Box<Self>, tx: Sender<ProviderEvent>, stop: Arc<AtomicBool>);
}
