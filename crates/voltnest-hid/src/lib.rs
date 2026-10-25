//! `hid-vbas`（HID Vendor Battery Access Spec）协议家族：枚举、读取、帧解析与品牌表。
//!
//! 本 crate 只做 HID 侧 I/O 与解析，不做去重、状态判定、分组或持久化。
//!
//! # 分层：家族 == 协议形状，品牌 == 字段差异
//! 设备满足「HID 私有通道 + 命令字 + 定长报告」即属本家族；电量从哪几个字节读、是否要先发查询报文，是**品牌差异**。解析器（[`frame::decode_battery`]）与运行时（[`provider`]）只读 [`spec::BrandSpec`] 字段，无 `match brand_id` 分支；新增品牌仅追加一份 [`spec::BrandSpec`] 与型号表并在 [`brands::BRANDS`] 登记。
//!
//! # 已验证品牌
//! | 品牌 | UsagePage（读） | 命令字 | 电量下标 | 采集方式 |
//! |---|---|---|---|---|
//! | [`brands::ajazz`]（W1 / 星闪代） | `0xFFB2` | `0xFA` | payload[1] | 纯推送 |
//! | [`brands::rapoo`]（VT9 Pro） | `0xFF0B` | `0x01` | payload[0] | Feature 轮询 |
//! | [`brands::dareu`]（A950 PRO Mg） | `0xFF02` | `0x04` | payload[0] | Output 查询 |
//! | [`brands::mchose`]（A7 / M7 / A5） | `0xFF01` / `0xFFFF` | `0x06` / `0x83` | payload[9] / payload[1] | Feature 查询 |
//!
//! # 实测约束（对全部品牌适用）
//! 1. **禁止** `open(vid, pid)`：它会静默打开错误的集合（枚举顺序里第一个 `0x0C/0x01` 集合），拿到「成功但读不到」的句柄。
//! 2. 集合选择必须「VID/PID 精确 → 表内 MI_xx → UsagePage」三步。
//! 3. 声明为只写的通道（AJAZZ 的 `0xFFB1`）不得用读句柄打开。
//!
//! # 平台支持
//! - **Windows**：hidapi 走 `hid.dll`，集合记录提供 `usage_page` / `usage` / `interface_number`，行为与已验证版本一致。
//! - **Linux**：hidapi 走静态 hidraw 后端（特性 `linux-static-hidraw`），集合路径形如 `/dev/hidraw3`。
//!   1. **权限**：读写 `/dev/hidraw*` 需要当前用户被放行，发行版通常用 `plugdev` 组或 udev 规则做到；**本实现不安装任何 udev 规则**，缺权限时表现为 `open_path` 失败并上报 `access_denied`，不会静默丢数据。
//!   2. **能力**：hidraw 从报告描述符解析真实 `usage_page` / `usage`（只有 libusb 后端恒为 0），USB 总线上也提供 `interface_number`——Usage 与 MI_xx 规则在 Linux 无需退化。
//!   3. **无设备**：机器上没有 `/dev/hidraw*` 时枚举为空——不报错、不 panic、不阻塞，只打一条诊断后继续按 [`PRESENCE_POLL_MS`] 轮询。
//!   4. **构建期依赖**：hidapi 的 hidraw 后端在构建期用 pkg-config 探测 `libudev`（需要 `libudev.pc` 与 `libudev.h`，并把 C 源码静态编进来），因此构建机须安装 libudev 的开发文件：Debian / Ubuntu `sudo apt install libudev-dev`，Fedora `sudo dnf install systemd-devel`。本 crate 只声明这条依赖，**不携带、不 vendor 任何构建期垫片**；运行期只依赖系统已装的 `libudev.so.1`（发行版默认就有）。

#![forbid(unsafe_code)]
#![deny(clippy::disallowed_types)]
#![warn(missing_docs)]

pub mod brands;
pub mod frame;
pub mod spec;
pub mod table;

// 运行时（枚举 + 读取线程）依赖 hidapi：Windows 走 hid.dll，Linux 走静态 hidraw 后端。
// 其它平台没有声明 hidapi 依赖，故不编译本模块。
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod provider;

/// 出现检测间隔：固定 5s，内部常量，**不进 `settings.json`**。
pub const PRESENCE_POLL_MS: u64 = 5_000;

/// 读取超时：实测拔线 ≤120ms 报错，用它把阻塞上界压到可接受范围。
pub const READ_TIMEOUT_MS: i32 = 120;

/// 读取缓冲长度：64 字节报告 + Report ID 首字节。
pub const READ_BUFFER_LEN: usize = 65;

pub use brands::Registry;
pub use frame::{BatteryReading, FrameReject, RejectReason};
pub use spec::{BrandSpec, FrameLayout, ReplyTransport};
