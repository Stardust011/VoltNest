//! 平台适配：开机自启、单实例唤醒、AUMID 兜底注册。
//!
//! Windows 走注册表 HKCU 的 Run 项（自启）、当前用户 Local 命名空间的互斥体 + 命名事件（单实例）、
//! HKCU 的 AppUserModelId 项（AUMID 兜底）。
//! Linux 走 XDG Autostart 的 $XDG_CONFIG_HOME/autostart/voltnest.desktop（自启）、
//! Unix domain socket（单实例）；通知归属由 freedesktop 通知服务按 .desktop 的 Name/Icon 决定，
//! 没有 AUMID 这个开关，故为成功返回的空操作。
//!
//! 只放平台 API，不含业务状态与 UI 规则；时间戳由调用方传入，本 crate 不读时钟。

#![warn(missing_docs)]

pub mod aumid;
pub mod autostart;
pub mod single_instance;

/// 应用 AUMID：Windows Toast 与开始菜单快捷方式共用；Linux 无此概念，通知应用名取自 .desktop。
pub const APP_USER_MODEL_ID: &str = "VoltNest.Desktop";

/// 自启命令行参数：带此参数启动一律静默进托盘。
pub const AUTOSTART_FLAG: &str = "--autostart";
