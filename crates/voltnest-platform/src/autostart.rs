//! 开机自启：`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`。
//!
//! 硬性规则（AC-1 / AC-5 / AC-6 / AC-7）：
//!
//! - **永不默认开启**，只有用户在引导或设置页显式勾选才写注册项；
//! - 命令固定带 `--autostart`（仅用于日志与诊断定位，不改变首屏行为）；
//!   首屏显隐与手动启动一致，统一由「启动时最小化到托盘」决定；
//! - 卸载必须清除该项；
//! - 设置页改动立即生效，不做「重启后生效」。

use std::path::Path;

/// 注册表位置。
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// 值名。
pub const VALUE_NAME: &str = "VoltNest";

/// 把可执行路径转成注册表命令：`"C:\...\VoltNest.exe" --autostart`。
///
/// 引号是必须的：Program Files 之外的路径也可能含空格（用户可改安装目录）。
/// 多个反斜杠结尾时只保留一个，避免 `"C:\dir\\"` 把引号转义掉。
pub fn run_command(exe: &Path) -> String {
    let mut path = exe.display().to_string();
    if !path.contains(' ') && !path.contains('\t') {
        return format!("{path} {}", crate::AUTOSTART_FLAG);
    }
    while path.ends_with('\\') && !path.ends_with("\\\\") {
        path.pop();
    }
    format!("\"{path}\" {}", crate::AUTOSTART_FLAG)
}

/// 读取当前自启命令。注册项不存在返回 `None`。
#[cfg(windows)]
pub fn current() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu.open_subkey(RUN_KEY).ok()?;
    run.get_value::<String, _>(VALUE_NAME).ok()
}

/// 读取当前自启命令（非 Windows 恒为 `None`）。
#[cfg(not(windows))]
pub fn current() -> Option<String> {
    None
}

/// 是否已启用。
pub fn is_enabled() -> bool {
    current().is_some()
}

/// 写入自启项。
#[cfg(windows)]
pub fn enable(exe: &Path) -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE)?;
    run.set_value(VALUE_NAME, &run_command(exe))
}

/// 写入自启项（非 Windows 直接报错，避免静默假装成功）。
#[cfg(not(windows))]
pub fn enable(_exe: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "自启仅在 Windows 上实现",
    ))
}

/// 清除自启项。项不存在视为成功（幂等，AC-7 卸载清理要能重复执行）。
#[cfg(windows)]
pub fn disable() -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = match hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE) {
        Ok(k) => k,
        Err(_) => return Ok(()),
    };
    match run.delete_value(VALUE_NAME) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(std::io::Error::other(e.to_string())),
    }
}

/// 清除自启项（非 Windows 空操作）。
#[cfg(not(windows))]
pub fn disable() -> std::io::Result<()> {
    Ok(())
}

/// 让实际状态与期望状态一致（AC-5 双向同步的实现点）。
pub fn set_enabled(exe: &Path, enabled: bool) -> std::io::Result<()> {
    if enabled { enable(exe) } else { disable() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn command_quotes_paths_with_spaces() {
        let cmd = run_command(&PathBuf::from(r"C:\Program Files\VoltNest\VoltNest.exe"));
        assert_eq!(
            cmd,
            "\"C:\\Program Files\\VoltNest\\VoltNest.exe\" --autostart"
        );
    }

    #[test]
    fn command_without_spaces_stays_unquoted() {
        let cmd = run_command(&PathBuf::from(r"C:\Tools\VoltNest.exe"));
        assert_eq!(cmd, "C:\\Tools\\VoltNest.exe --autostart");
    }

    #[test]
    fn command_always_carries_autostart_flag() {
        // AC-6：命令恒定带 --autostart 标记（首屏显隐另由「启动时最小化」决定）。
        for p in [r"C:\a b\VoltNest.exe", r"C:\a\VoltNest.exe"] {
            assert!(run_command(&PathBuf::from(p)).ends_with("--autostart"));
        }
    }

    #[test]
    fn trailing_backslash_does_not_escape_quote() {
        let cmd = run_command(&PathBuf::from("C:\\My Dir\\"));
        assert!(!cmd.contains("\\\\\""), "结尾反斜杠会把引号转义掉：{cmd}");
        assert!(cmd.starts_with('"'));
    }

    #[test]
    fn run_key_matches_microsoft_location() {
        assert_eq!(RUN_KEY, r"Software\Microsoft\Windows\CurrentVersion\Run");
        assert_eq!(VALUE_NAME, "VoltNest");
    }
}
