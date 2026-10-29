//! VoltNest 持久化：`settings.json`（用户资产）与 `state.json`（可丢弃缓存）。
//!
//! 两个文件刻意分开（X）：合并会让最高频写入去碰最有价值的数据——断电、磁盘满、杀软锁文件都会
//! 一次同时毁掉阈值、隐藏列表和托盘顺序。
//! 所有写入走原子替换：`.tmp` → `fsync` → `rename`。损坏恢复不阻塞启动：
//! 备份 → 用默认值启动 → 诊断页告警。
//!
//! 第三个文件 `manual_devices.json`（见 [`manual`]）同样是用户资产：用户手动添加的设备与
//! 它绑定的抓包报告，删了不会自己长回来，故不放进可丢弃的 `state.json`。

#![forbid(unsafe_code)]
#![deny(clippy::disallowed_types)]
#![warn(missing_docs)]

pub mod atomic;
pub mod manual;
pub mod paths;
pub mod settings;
pub mod state;

pub use manual::{
    LoadedManualDevices, MANUAL_SCHEMA_VERSION, MAX_MANUAL_DEVICES, ManualDevice, ManualDeviceFile,
    ManualDeviceStore, ManualSample,
};
pub use paths::DataPaths;
pub use settings::{
    LoadedSettings, NotificationMode, NotificationSettings, Settings, SettingsLoadWarning,
    SettingsStore, TraySettings,
};
pub use state::{DeviceStateEntry, NotificationStateEntry, StateFile, StateStore, StateThrottle};

/// 两个文件共用的 schema 版本（整数）。
pub const SCHEMA_VERSION: u32 = 1;

/// `state.json` 写入节流。
pub const STATE_THROTTLE_MS: u64 = 5_000;

/// 按 `stableId` 索引的条目上限。超出时淘汰最久未出现项。
pub const MAX_INDEXED_ENTRIES: usize = 32;
