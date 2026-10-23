//! VoltNest Provider 契约：纯类型与常量，是 HID / BLE 与宿主（`voltnest-core`、`src-tauri`）之间唯一的契约面（ADR-0010）。
//! 不含 I/O、线程、平台 API 与状态机。
//!
//! 两条最易写错的规则由类型消灭而非靠评审记住：
//! [`Percent`] 是 newtype，`> 100` 在构造期拒绝 → 状态机不可能收到越界电量；
//! [`ProviderProfile::charging`] 为 [`ChargingSupport::Unavailable`] 时，
//! [`ProviderProfile::sanitize_charging`] 把任何 `Some(bool)` 压成 `None` → BLE 不会被渲染成未充电（PC-5）。
//!
//! 设备类型识别（[`classify`]）同样是纯类型 + 纯函数：Provider 只把实际取到的标准字段填进
//! [`HidEvidence`] / [`BleEvidence`]，判定规则与冲突处理不依赖 I/O，也不影响电量采集。

#![forbid(unsafe_code)]
#![deny(clippy::disallowed_types)]
#![warn(missing_docs)]

pub mod classify;
pub mod clock;
pub mod diag;
pub mod error;
pub mod event;
pub mod ids;
pub mod profile;
pub mod testing;
pub mod transport;

pub use classify::{
    BleEvidence, ClassificationMethod, DeviceClassification, DeviceType, HidCollection,
    HidEvidence, VerifiedMapping, appearance_type, classify_ble, classify_hid, usage_type,
};
pub use diag::{DIAG_BYTES, hex_bytes};
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
/// 同时是日志前缀、诊断分组键与 `Snapshot.provider` 值。家族层面不含品牌：
/// 任何走 HID 私有协议报电量的设备都归入此处，品牌差异由 `voltnest-hid` 内的品牌描述表达（ADR-0011）。
pub const FAMILY_HID_VBAS: &str = "hid-vbas";

/// 协议家族 ID：`ble-bas`。
pub const FAMILY_BLE_BAS: &str = "ble-bas";

/// Provider 统一入口（ADR-0010 D1）：实现者只上报原始事实（出现、读数、结构化错误、链路状态）。
/// 去重、冲突仲裁、归一化、分组、时间戳、持久化全属宿主。
pub trait Provider: Send + 'static {
    /// 编译期声明的能力与节奏。
    fn profile(&self) -> &'static ProviderProfile;

    /// 阻塞运行，直到 `stop` 置位。
    ///
    /// 返回前必须释放全部句柄与 GATT 订阅；实现内部不得调用 `std::process::exit`。
    fn run(self: Box<Self>, tx: Sender<ProviderEvent>, stop: Arc<AtomicBool>);
}
