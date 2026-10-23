//! 开机自启：`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`。
//!
//! 永不默认开启，仅用户在引导页或设置页显式勾选才写注册项；卸载须清除；改动立即生效（AC-1/AC-5/AC-6/AC-7）。
//! 命令恒带 `--autostart`，但首屏显隐与手动启动一致，统一由「启动时最小化到托盘」决定。

use std::path::Path;

/// 注册表位置。
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// 值名。
pub const VALUE_NAME: &str = "VoltNest";

/// 生成注册表命令 `"路径" --autostart`。
///
/// 路径可能含空格（用户可改安装目录）故按需加引号；结尾多余反斜杠会转义引号，先削掉。
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

/// 当前自启命令；注册项不存在返回 `None`。
#[cfg(windows)]
pub fn current() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu.open_subkey(RUN_KEY).ok()?;
    run.get_value::<String, _>(VALUE_NAME).ok()
}

/// 非 Windows 恒为 `None`。
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

/// 非 Windows 报错，不静默假装成功。
#[cfg(not(windows))]
pub fn enable(_exe: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "自启仅支持 Windows",
    ))
}

/// 清除自启项；不存在视为成功，卸载清理可重复执行（AC-7）。
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

/// 非 Windows 空操作。
#[cfg(not(windows))]
pub fn disable() -> std::io::Result<()> {
    Ok(())
}

/// 把实际状态同步到期望状态（AC-5 双向同步的唯一入口）。
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
        // AC-6：命令恒带 --autostart，首屏显隐另由「启动时最小化」决定。
        for p in [r"C:\a b\VoltNest.exe", r"C:\a\VoltNest.exe"] {
            assert!(run_command(&PathBuf::from(p)).ends_with("--autostart"));
        }
    }

    #[test]
    fn trailing_backslash_does_not_escape_quote() {
        let cmd = run_command(&PathBuf::from("C:\\My Dir\\"));
        assert!(!cmd.contains("\\\\\""), "反斜杠转义了引号：{cmd}");
        assert!(cmd.starts_with('"'));
    }

    #[test]
    fn run_key_matches_microsoft_location() {
        assert_eq!(RUN_KEY, r"Software\Microsoft\Windows\CurrentVersion\Run");
        assert_eq!(VALUE_NAME, "VoltNest");
    }
}
