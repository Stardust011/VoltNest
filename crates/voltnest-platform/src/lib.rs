//! 平台适配：开机自启、单实例唤醒、AUMID 兜底注册。
//!
//! Windows 走 HKCU 注册表 Run / AppUserModelId 项与 Local 命名空间互斥体 + 命名事件；
//! Linux 走 XDG Autostart、Unix domain socket，通知归属由 `.desktop` 决定（AUMID 为空操作）。
//! 只放平台 API，不含业务状态与 UI 规则；时间戳由调用方传入。

#![warn(missing_docs)]

pub mod aumid;
pub mod autostart;
pub mod single_instance;

/// 应用 AUMID：Windows Toast 与开始菜单快捷方式共用；Linux 无此概念，通知应用名取自 .desktop。
pub const APP_USER_MODEL_ID: &str = "VoltNest.Desktop";

/// 自启命令行参数：带此参数启动一律静默进托盘。
pub const AUTOSTART_FLAG: &str = "--autostart";
