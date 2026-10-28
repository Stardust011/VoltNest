//! `ble-bas` 协议家族：标准 BLE Battery Service 的电量读取。实测结论：
//! 服务 `0x180F` 的 `0x2A19`（READ + NOTIFY）；设备名必须读 GAP `0x2A00`，禁用 `BluetoothLEDevice.Name` /
//! `properties().local_name`（Windows 注册表缓存，用户可改名）；读取一律 `Uncached`，缓存值不得进状态机；
//! 充电状态协议层不可得（`0x2BF0`/`0x2BEA` 未实现，`0x2BED` 失真）→ 恒为 Unknown。
//!
//! # 平台
//! `runtime` 在 Windows（WinRT 后端）与 Linux（bluez/dbus 后端）上都编译，二者共用同一套协议与调度逻辑，
//! 只有「枚举来源」「`connect()` 幂等性」「清理链路」三处按平台分支，见 `runtime` 模块文档的复核表。
//!
//! # Linux（bluez）运行前提
//! - 必须有 **bluetoothd** 在运行、系统 dbus 可用：`Manager::new()` 只连系统总线，`manager.adapters()`
//!   才真正问 bluetoothd 要适配器，因此 `bluetoothd` 缺席时的失败点在适配器枚举而不是 `Manager::new()`。
//! - 当前用户**需在 `bluetooth` 组**，或者能被 **polkit** 授权（否则 dbus 返回 `AccessDenied`，
//!   这里映射成 `ErrorCode::AccessDenied`）。
//! - **无适配器**（容器 / WSL2 / rfkill 关闭）：runtime 上报一次 `ProviderEvent::TransportDown`
//!   （码 + 人类可读 detail），线程结束，由宿主 supervisor 按 5s/15s/60s 退避重启，连续 5 次失败标记不可用——
//!   不 panic、不空转、不伪造空读数。
//! - 只跟随已连接设备（`Device1.Connected`），不主动拉起配对/连接；失败清理时也只断开自己建立的链路。

#![forbid(unsafe_code)]
#![deny(clippy::disallowed_types)]
#![warn(missing_docs)]

// 只实现了 Windows 与 Linux 两套后端语义（见 runtime 模块文档的复核表）。
// 其它平台必须显式失败：静默套用 bluez 假设会得到「能编译、行为错」的运行时。
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
compile_error!(
    "voltnest-ble 仅支持 Windows 与 Linux；新增平台需先按 runtime 模块文档的复核表逐条核对 btleplug 后端语义"
);

pub mod bas;
pub mod policy;
pub mod runtime;

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

/// 后端初始化（`Manager::new` / 适配器枚举）的总超时。
///
/// dbus 上的一次方法调用可能停在「等待服务激活」上很久（bluez 由 dbus 按需拉起），
/// 而初始化跑在采集线程里、不能长时间占着不报状态：超时就按不可用上报，交给 supervisor 退避重启。
pub const INIT_TIMEOUT_MS: u64 = 5_000;

/// 连续多少轮巡检失败才判定「后端整体不可用」并上报 `TransportDown`。
///
/// 单轮失败常见的是一次 dbus 抖动或设备刚好消失，立刻拆掉整个 transport 会让所有设备闪断；
/// 连续失败说明后端真的不在了（bluetoothd 退出、适配器被拔出），此时才交给 supervisor 退避重启。
pub const ENUM_FAILURES_BEFORE_DOWN: u32 = 3;

/// 判定「后端整体不可用」前，连续失败必须至少持续这么久。
///
/// 出现检测周期是 5s，只数轮数等于「15s 抖动就拆 transport」；加上时间窗后与旧的
/// 「3 轮 × 60s」等价，轮数只用来过滤掉零星失败。
pub const ENUM_FAILURES_MIN_WINDOW_MS: u64 = 60_000;

/// 连续多少轮枚举不到才判定设备消失。
///
/// WinRT 的「已连接」选择器里，查 GATT 服务超时的设备会被当成「无服务」而从结果里消失；
/// 单轮漏报很常见，立即判 `Removed` 会让图标闪一下。3 轮 × 5s ≈ 15s 后才算真消失。
pub const PRESENCE_MISSES_BEFORE_REMOVED: u32 = 3;

/// 断链去抖窗口：事件到达后先等这么久再判定，等的是「系统自动重连」。
///
/// HID 鼠标断电/休眠会被系统立刻重连，若立即上报 `LinkLost`，托盘图标会闪一下 Offline 又回 Online。
pub const LINK_LOSS_DEBOUNCE_MS: u64 = 1_500;

/// 去抖窗口到期后，用一次 `is_connected()` 复核，避免把「已经重连上」误判成断链。
pub const LINK_CONFIRM_TIMEOUT_MS: u64 = 500;

/// 事件流结束（后端重启、dbus 断流）后的重订阅间隔。
pub const EVENT_RESUBSCRIBE_DELAY_MS: u64 = 2_000;
