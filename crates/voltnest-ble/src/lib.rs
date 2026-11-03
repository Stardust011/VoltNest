//! `ble-bas` 协议家族：标准 BLE Battery Service 的电量读取。电量取 `0x180F` 的 `0x2A19`（READ + NOTIFY），
//! 设备名只读 GAP `0x2A00`，读取一律 Uncached；充电状态协议层不可得，恒为 `Unknown`。
//!
//! Windows（WinRT）与 Linux（bluez/dbus）后端共用同一套协议与调度逻辑，仅「枚举来源」「`connect()` 幂等性」
//! 「清理链路」按平台分支。Linux 侧需 bluetoothd 运行、系统 dbus 可用，且当前用户可访问（`bluetooth` 组或
//! polkit 授权）；后端缺席时上报一次 `ProviderEvent::TransportDown` 后结束线程，不 panic、不空转。

#![forbid(unsafe_code)]
#![deny(clippy::disallowed_types)]
#![warn(missing_docs)]

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
compile_error!(
    "voltnest-ble 仅支持 Windows 与 Linux；新增平台需先按 runtime 模块文档的复核表逐条核对 btleplug 后端语义"
);

pub mod bas;
pub mod policy;
pub mod runtime;

/// 建链超时。
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

/// I/O 并行执行器数。
pub const IO_WORKERS: usize = 2;

/// 连续失败多少次后进入退避。
pub const FAILURES_BEFORE_BACKOFF: u32 = 3;

/// 退避上限。
pub const BACKOFF_CAP_MS: u64 = 10 * 60_000;

/// 后端初始化（`Manager::new` / 适配器枚举）的总超时；超时按不可用上报。
pub const INIT_TIMEOUT_MS: u64 = 5_000;

/// 连续多少轮巡检失败才判定「后端整体不可用」并上报 `TransportDown`。
pub const ENUM_FAILURES_BEFORE_DOWN: u32 = 3;

/// 判定「后端整体不可用」前，连续失败必须至少持续这么久。
pub const ENUM_FAILURES_MIN_WINDOW_MS: u64 = 60_000;

/// 连续多少轮枚举不到才判定设备消失。
pub const PRESENCE_MISSES_BEFORE_REMOVED: u32 = 3;

/// 断链去抖窗口：事件到达后先等这么久再判定，等的是系统自动重连。
pub const LINK_LOSS_DEBOUNCE_MS: u64 = 1_500;

/// 去抖窗口到期后，用一次 `is_connected()` 复核，避免把「已经重连上」误判成断链。
pub const LINK_CONFIRM_TIMEOUT_MS: u64 = 500;

/// 事件流结束（后端重启、dbus 断流）后的重订阅间隔。
pub const EVENT_RESUBSCRIBE_DELAY_MS: u64 = 2_000;
