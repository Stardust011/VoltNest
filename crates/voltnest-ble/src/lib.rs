//! `ble-bas` 协议家族：标准 BLE Battery Service 的电量读取。实测结论：
//! 服务 `0x180F` 的 `0x2A19`（READ + NOTIFY）；设备名必须读 GAP `0x2A00`，禁用 `BluetoothLEDevice.Name` /
//! `properties().local_name`（Windows 注册表缓存，用户可改名）；读取一律 `Uncached`，缓存值不得进状态机；
//! 充电状态协议层不可得（`0x2BF0`/`0x2BEA` 未实现，`0x2BED` 失真）→ 恒为 Unknown。

#![forbid(unsafe_code)]
#![deny(clippy::disallowed_types)]
#![warn(missing_docs)]

pub mod bas;
pub mod policy;

#[cfg(target_os = "windows")]
pub mod provider;

/// 建链超时：实测已配对无链路的设备 `connect()` 要 7.97s 才返回，
/// 必须自带 4s 超时，否则会拖住整个刷新周期。
pub const CONNECT_TIMEOUT_MS: u64 = 4_000;

/// 服务发现超时。
pub const DISCOVER_TIMEOUT_MS: u64 = 3_000;

/// 单次读取超时。
pub const READ_TIMEOUT_MS: u64 = 2_000;

/// 无 `0x180F` 的负缓存 TTL：30 分钟。
pub const NEGATIVE_CACHE_MS: u64 = 30 * 60_000;

/// 跟踪上限：超出只在诊断页列出、不建链。
pub const MAX_TRACKED: usize = 8;

/// 全局同时建链上限。
pub const MAX_CONCURRENT: usize = 2;

/// I/O 并行执行器数：`btleplug` 单线程巡检最坏 8×9s=72s > 60s 周期，故 I/O 须由两个并行执行器承担。
pub const IO_WORKERS: usize = 2;

/// 连续失败多少次后进入退避。
pub const FAILURES_BEFORE_BACKOFF: u32 = 3;

/// 退避上限。
pub const BACKOFF_CAP_MS: u64 = 10 * 60_000;
