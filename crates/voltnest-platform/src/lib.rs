//! Windows 平台适配：开机自启（HKCU Run）、单实例互斥体、AUMID 兜底注册。
//!
//! 本 crate 只放**平台 API**，不含业务状态与 UI 规则（规格 §3.2）。
//! 所有时间戳由调用方传入，本 crate 不读时钟。

#![warn(missing_docs)]

pub mod aumid;
pub mod autostart;
pub mod single_instance;

/// 应用 AUMID：Windows Toast 与开始菜单快捷方式共用。
pub const APP_USER_MODEL_ID: &str = "VoltNest.Desktop";

/// 自启命令行参数：该路径**永远静默进托盘**（AC-6）。
pub const AUTOSTART_FLAG: &str = "--autostart";
