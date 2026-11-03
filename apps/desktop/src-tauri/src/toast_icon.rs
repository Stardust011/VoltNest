//! Windows Toast 图标落盘。`appLogoOverride` 只认磁盘图片、不认 exe 内嵌资源，故把 128×128 PNG 幂等写到数据目录。图标缺失非致命：Toast 照发。
//!
//! 平台归属：Windows 落盘并缓存；其他平台 `init` 是空操作，直接返回 `None`（图标由 `.desktop` 解析）。

#[cfg(any(windows, test))]
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::sync::OnceLock;

/// 打包进来的应用图标（128×128 PNG，与托盘/窗口图标同源）。仅 Windows 落盘用。
#[cfg(any(windows, test))]
const ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");

/// 落盘后的文件名（`%APPDATA%\VoltNest\toast-icon.png`）。
#[cfg(any(windows, test))]
const FILE_NAME: &str = "toast-icon.png";

/// 进程内缓存：`path()` 要能在任意线程直接取，不再重复读盘。
#[cfg(windows)]
static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Windows：幂等落盘并缓存路径；返回 `None` 表示写不进去（调用方继续走无图标路径）。
#[cfg(windows)]
pub fn init(root: &Path) -> Option<PathBuf> {
    CACHE.get_or_init(|| write(root).ok()).clone()
}

/// 其他平台：不落盘、不缓存，直接 `None` —— 落盘物在那里没有任何消费方。
#[cfg(not(windows))]
pub fn init(_root: &Path) -> Option<PathBuf> {
    None
}

/// 已落盘的图标路径（`init` 之前调用得到 `None`）。仅 Windows Toast 消费。
#[cfg(windows)]
pub fn path() -> Option<&'static Path> {
    CACHE.get().and_then(|cached| cached.as_deref())
}

/// 内容不同才写。`test` 下也编译，便于直接测落盘契约。
#[cfg(any(windows, test))]
fn write(root: &Path) -> std::io::Result<PathBuf> {
    let target = root.join(FILE_NAME);
    if fs::read(&target).is_ok_and(|existing| existing == ICON_PNG) {
        return Ok(target);
    }
    fs::write(&target, ICON_PNG)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_icon_is_a_png() {
        assert_eq!(
            ICON_PNG[..8],
            [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        );
    }

    #[test]
    fn writing_is_idempotent_and_self_healing() {
        let dir = std::env::temp_dir().join("voltnest-toast-icon-test");
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join(FILE_NAME);
        let _ = fs::remove_file(&target);

        assert_eq!(write(&dir).unwrap(), target);
        let first = fs::read(&target).unwrap();
        fs::write(&target, b"broken").unwrap();
        assert_eq!(write(&dir).unwrap(), target);
        assert_eq!(fs::read(&target).unwrap(), first);
    }

    /// 非 Windows：`init` 是真空操作 —— 不写盘、不缓存，数据目录里不留 Windows 资产。
    #[cfg(not(windows))]
    #[test]
    fn init_does_not_touch_the_disk_off_windows() {
        let dir = std::env::temp_dir().join("voltnest-toast-icon-noop-test");
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join(FILE_NAME);
        let _ = std::fs::remove_file(&target);

        assert_eq!(init(&dir), None, "无消费方就不要返回路径");
        assert_eq!(init(&dir), None, "重复调用同样返回 None（也不缓存）");
        assert!(
            !target.exists(),
            "非 Windows 上不能让数据目录出现 toast-icon.png"
        );
    }

    /// Windows：`init` 仍要落盘并返回真实路径（那张 PNG 就是 Toast 头图）。
    #[cfg(windows)]
    #[test]
    fn init_writes_the_icon_on_windows() {
        let dir = std::env::temp_dir().join("voltnest-toast-icon-init-test");
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join(FILE_NAME);
        let _ = std::fs::remove_file(&target);

        assert_eq!(init(&dir), Some(target.clone()));
        assert!(target.exists(), "Windows 上必须真的落盘");
    }
}
