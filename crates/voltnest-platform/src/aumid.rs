//! AUMID 注册：让 Windows Toast 显示「VoltNest」与应用图标，而非 `powershell.exe`。
//!
//! 解析途径：开始菜单快捷方式携带 `System.AppUserModel.ID`（正常路径），或
//! `HKCU\Software\Classes\AppUserModelId\<AUMID>` 下的 `DisplayName` + `IconUri`（兜底，须成对写）。
//! Linux 无 AUMID 概念，应用名与图标由 `.desktop` 决定，本模块只有空操作。

use std::path::Path;

/// AUMID 的注册表位置。
pub const AUMID_KEY: &str = r"Software\Classes\AppUserModelId\VoltNest.Desktop";

/// 通知里显示的应用名（仅 Windows 注册表项用到）。
#[cfg(windows)]
const DISPLAY_NAME_VALUE: &str = "DisplayName";

/// 通知里显示的应用图标（`file:///` URI，指向真实存在的图片；仅 Windows 注册表项用到）。
#[cfg(windows)]
const ICON_URI_VALUE: &str = "IconUri";

/// 幂等注册，每次启动调用一次。
///
/// `icon` 须为绝对路径且文件已存在；`None` 会清掉旧 `IconUri`。
#[cfg(windows)]
pub fn register(display_name: &str, icon: Option<&Path>) -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey_with_flags(AUMID_KEY, KEY_READ | KEY_WRITE)?;
    key.set_value(DISPLAY_NAME_VALUE, &display_name)?;
    match icon {
        Some(path) => key.set_value(ICON_URI_VALUE, &file_uri(path)),
        None => match key.delete_value(ICON_URI_VALUE) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        },
    }
}

/// 非 Windows（含 Linux）：成功返回的空操作。
///
/// Linux 上应用名与图标由 freedesktop 通知服务按 `.desktop` 解析，无需预注册。
#[cfg(not(windows))]
pub fn register(_display_name: &str, _icon: Option<&Path>) -> std::io::Result<()> {
    Ok(())
}

/// 路径 → `IconUri` 期望的 `file:///` 形式（正斜杠，空格转义）。
pub fn file_uri(path: &Path) -> String {
    let mut text = path.to_string_lossy().replace('\\', "/");
    if !text.starts_with('/') {
        text.insert(0, '/');
    }
    format!("file://{text}").replace(' ', "%20")
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

/// 非 Windows 空操作。
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

    #[test]
    fn icon_uri_is_a_forward_slash_file_url() {
        let uri = file_uri(Path::new(
            r"C:\Users\John Doe\AppData\Roaming\VoltNest\toast-icon.png",
        ));
        assert_eq!(
            uri,
            "file:///C:/Users/John%20Doe/AppData/Roaming/VoltNest/toast-icon.png"
        );
        assert!(!uri.contains('\\'));
    }

    #[cfg(windows)]
    #[test]
    fn register_is_idempotent() {
        let icon = std::env::temp_dir().join("voltnest-toast-icon.png");
        register("VoltNest", Some(&icon)).unwrap();
        register("VoltNest", Some(&icon)).unwrap();
        assert!(is_registered());

        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(AUMID_KEY)
            .unwrap();
        let name: String = key.get_value(DISPLAY_NAME_VALUE).unwrap();
        let uri: String = key.get_value(ICON_URI_VALUE).unwrap();
        assert_eq!(name, "VoltNest");
        assert_eq!(uri, file_uri(&icon));
    }
}
