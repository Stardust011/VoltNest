//! `hid-nearlink-ajazz` 协议家族：枚举、读取、帧解析与型号表。
//!
//! 本 crate 只负责 HID 侧的 I/O 与解析，不做去重、状态判定、分组或持久化
//! （ADR-0010 §3.3 / D6）。型号差异只更新 `data` 里的表，不新增 crate。
//!
//! 协议事实来自本机实测（`docs/ajazz-probe-report.md`、`docs/hid-rust-verification.md`）：
//!
//! - 读取通道 `UsagePage = 0xFFB2`（MI_01 / Col02），报告至少 64 字节；
//! - `0xFFB1` 是只写通道，读句柄会持续报错 → v1 不打开；
//! - 电量来自 `cmd = 0xFA` 的推送帧，约 30s 一次，通常两帧一组；
//! - `0xFFB3` 在验证设备上无数据 → v1 不实现。

#![forbid(unsafe_code)]
#![deny(clippy::disallowed_types)]
#![warn(missing_docs)]

pub mod frame;
pub mod table;

#[cfg(target_os = "windows")]
pub mod provider;

/// 电量读取通道的 UsagePage。
pub const USAGE_PAGE_BATTERY: u16 = 0xFFB2;
/// 只写命令通道的 UsagePage（v1 不打开，P-4 禁止向它发任何报文）。
pub const USAGE_PAGE_WRITE_ONLY: u16 = 0xFFB1;
/// 辅助通道（验证设备上无数据，v1 不实现）。
pub const USAGE_PAGE_AUX: u16 = 0xFFB3;

/// 出现检测间隔：固定 5s，内部常量，**不进 `settings.json`**（P-2）。
pub const PRESENCE_POLL_MS: u64 = 5_000;

/// 读取超时：实测拔线 ≤120ms 报错，用它把阻塞上界压到可接受范围（P-1 / D6）。
pub const READ_TIMEOUT_MS: i32 = 120;

/// 读取缓冲长度：64 字节报告 + Report ID 首字节。
pub const READ_BUFFER_LEN: usize = 65;

/// 型号表（`ajazz_device_map.json`）在编译期嵌入。
pub mod data {
    /// 官方驱动 `config.xml` 导出的快照，由 `tools/probe/dump_driver_config.py` 生成。
    ///
    /// 用 `include_str!` 而非运行时读文件：安装目录不需要携带数据文件，
    /// 也不会与二进制版本分叉（ADR-0010 D7）。
    pub const AJAZZ_DEVICE_MAP: &str = include_str!("../data/ajazz_device_map.json");
}
