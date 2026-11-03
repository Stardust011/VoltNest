//! `hid-vbas`（HID Vendor Battery Access Spec）协议家族：枚举、读取、帧解析与品牌表。
//!
//! 本 crate 只做 HID 侧 I/O 与解析，不做去重、状态判定、分组或持久化。
//!
//! # 分层：家族 == 协议形状，品牌 == 字段差异
//! 设备满足「HID 私有通道 + 命令字 + 定长报告」即属本家族；电量从哪几个字节读、是否要先发查询报文，是**品牌差异**。解析器（[`frame::decode_battery`]）与运行时（[`runtime`]）只读 [`spec::BrandSpec`] 字段，无 `match brand_id` 分支；新增品牌仅追加一份 [`spec::BrandSpec`] 与型号表并在 [`brands::BRANDS`] 登记。
//!
//! # 型号表：运行时从文件系统加载
//! 型号表（`data/mappings/*.json`）**不再**编译期嵌入：进程首次枚举前由 [`mappings`] 定位目录、
//! 扫描并解析全部 `*.json`，再按 [`spec::BrandSpec::device_table_file`] 交给对应品牌。因此
//! **安装包必须随二进制带上型号表目录**（Tauri 打包见 `bundle.resources`），定位顺序：
//! `VOLTNEST_HID_MAPPINGS_DIR` 环境变量 → 可执行文件同级 / 上一级（`mappings`、`data/mappings`、
//! `resources/mappings`、macOS `Resources/mappings`、`share/voltnest/mappings`、Linux
//! `lib/<productName>/mappings`）→ 源码树
//! `CARGO_MANIFEST_DIR/data/mappings`（开发期）。目录找不到时 [`brands::Registry::embedded`] 直接
//! panic 并在信息里列出全部已尝试路径；需要优雅降级时用 [`brands::Registry::try_embedded`]。
//!
//! 单份型号表读不出或解析失败**只跳过对应品牌**（记入 [`brands::Registry::skipped`]，并在
//! `try_embedded` 里打一条 `voltnest-hid:` 诊断），其它品牌照常采集；只有「一个品牌都不可用」
//! 才算部署事故（`RegistryError::NoUsableBrand`）。
//!
//! # 已验证品牌
//! | 品牌 | 接入的协议数 | 分派依据 | 通道 / 采集方式 |
//! |---|---|---|---|
//! | [`brands::ajazz`]（W1 / 星闪代） | 1 | 固定（单协议） | `0xFFB2` 纯监听 |
//! | [`brands::dareu`] | 3（匠盟 / 自研 Tg / 海思 Hs） | 型号表 `fwType` | `0xFF02` output 查询 · `0x0C` feature 查询 |
//! | [`brands::mchose`] | 5（鼠标有线/2.4G、键盘有线/2.4G、音频） | 型号表 `deviceType` + `mode` | `0xFF01`（备用 `0xFF0B`）feature 查询 · output 多包查询 · 通配 `0x55` |
//! | [`brands::rapoo`] | 4（nrf54L / CH584 / LM20RT / 维盛 AP） | 型号表 `driver` | `0xFF00` 纯监听 · `0xFF1C` output 查询 |
//!
//! **字段级事实（电量下标、命令字、充电取值）只写在各自模块的文档里**，这里不再抄第二份：
//! 抄一份就会漂移一次（本表上一版还写着雷柏 `0xFF0B` / feature 轮询，与四家族分派早已不符）。
//! 每个品牌的变体都必须登记进 [`spec::BrandSpec::variants`]，注册表在首次枚举前逐份校验。
//!
//! # 实测约束（对全部品牌适用）
//! 1. **禁止** `open(vid, pid)`：它会静默打开错误的集合（枚举顺序里第一个 `0x0C/0x01` 集合），拿到「成功但读不到」的句柄。
//! 2. 集合选择必须「VID/PID 精确 → 品牌声明的 UsagePage」两步；型号表只按 `VID:PID` 提供型号（统一数据规范见 [`table`]），不含接口号。
//! 3. 声明为只写的通道（AJAZZ 的 `0xFFB1`）不得用读句柄打开。
//!
//! # 平台支持
//! - **Windows**：hidapi 走 `hid.dll`，集合记录提供 `usage_page` / `usage`，行为与已验证版本一致。
//! - **Linux**：hidapi 走静态 hidraw 后端（特性 `linux-static-hidraw`），集合路径形如 `/dev/hidraw3`。
//!   1. **权限**：读写 `/dev/hidraw*` 需要当前用户被放行，发行版通常用 `plugdev` 组或 udev 规则做到；**本实现不安装任何 udev 规则**，缺权限时表现为 `open_path` 失败并上报 `access_denied`，不会静默丢数据。
//!   2. **能力**：hidraw 从报告描述符解析真实 `usage_page` / `usage`（只有 libusb 后端恒为 0）——集合选择的两项判据在 Linux 无需退化。
//!   3. **无设备**：机器上没有 `/dev/hidraw*` 时枚举为空——不报错、不 panic、不阻塞，只打一条诊断后继续按 [`PRESENCE_POLL_MS`] 轮询。
//!   4. **构建期依赖**：hidapi 的 hidraw 后端在构建期用 pkg-config 探测 `libudev`（需要 `libudev.pc` 与 `libudev.h`，并把 C 源码静态编进来），因此构建机须安装 libudev 的开发文件：Debian / Ubuntu `sudo apt install libudev-dev`，Fedora `sudo dnf install systemd-devel`。本 crate 只声明这条依赖，**不携带、不 vendor 任何构建期垫片**；运行期只依赖系统已装的 `libudev.so.1`（发行版默认就有）。

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

/// 出现检测间隔：固定 5s，内部常量，**不进 `settings.json`**。
pub const PRESENCE_POLL_MS: u64 = 5_000;

/// 读取超时：实测拔线 ≤120ms 报错，用它把阻塞上界压到可接受范围。
pub const READ_TIMEOUT_MS: i32 = 120;

/// 读取缓冲长度：64 字节报告 + Report ID 首字节。
pub const READ_BUFFER_LEN: usize = 65;

/// 一轮查询内两次尝试之间的间隔（官方 `retryTime` 是即时重问，这里留出设备处理命令的时间）。
pub const RETRY_GAP_MS: u64 = 100;

/// 监听通道上等设备自发上报的读取超时：足够短以便按时发起下一次查询，又足够长以避开忙轮询。
pub const LISTEN_READ_TIMEOUT_MS: i32 = 100;

pub use brands::Registry;
pub use frame::{BatteryReading, FrameReject, RejectReason};
pub use spec::{BrandSpec, FrameLayout, ReplyTransport};
