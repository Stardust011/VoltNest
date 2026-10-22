//! AUMID 注册：让 Windows Toast 显示「VoltNest」而不是 `tauri-app.exe`。
//!
//! 正常路径是安装器创建的开始菜单快捷方式携带 AUMID；运行时在 HKCU 幂等写入
//! `AppUserModelId` 作为**兜底**（规格 §13）。卸载时应删除该项。

/// AUMID 的注册表位置。
pub const AUMID_KEY: &str = r"Software\Classes\AppUserModelId\VoltNest.Desktop";

/// 幂等注册（每次启动调用一次）。
#[cfg(windows)]
pub fn register(display_name: &str) -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey_with_flags(AUMID_KEY, KEY_READ | KEY_WRITE)?;
    key.set_value("DisplayName", &display_name)
}

/// 非 Windows：空操作。
#[cfg(not(windows))]
pub fn register(_display_name: &str) -> std::io::Result<()> {
    Ok(())
}

/// 是否已注册（诊断页展示）。
#[cfg(windows)]
pub fn is_registered() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(AUMID_KEY)
        .is_ok()
}

/// 非 Windows 恒为 false。
#[cfg(not(windows))]
pub fn is_registered() -> bool {
    false
}

/// 卸载清理。
#[cfg(windows)]
pub fn unregister() -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match hkcu.delete_subkey_all(AUMID_KEY) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// 非 Windows：空操作。
#[cfg(not(windows))]
pub fn unregister() -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aumid_path_is_under_classes() {
        assert!(AUMID_KEY.starts_with(r"Software\Classes\AppUserModelId\"));
        assert!(AUMID_KEY.ends_with("VoltNest.Desktop"));
        assert_eq!(
            AUMID_KEY,
            format!(
                r"Software\Classes\AppUserModelId\{}",
                crate::APP_USER_MODEL_ID
            )
        );
    }

    #[cfg(windows)]
    #[test]
    fn register_is_idempotent() {
        register("VoltNest").unwrap();
        register("VoltNest").unwrap();
        assert!(is_registered());
    }
}
