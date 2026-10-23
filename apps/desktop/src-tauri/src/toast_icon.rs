//! Toast 图标落盘。Windows 的 `appLogoOverride` 与 AUMID 的 `IconUri` 只认磁盘真实图片、不认 exe 内嵌资源；安装目录运行时只读，故把打包的 128×128 PNG 幂等落到数据目录。图标缺失非致命：Toast 照发，头图回落 AUMID 解析结果。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 打包进来的应用图标（128×128 PNG，与托盘/窗口图标同源）。
const ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");

/// 落盘后的文件名（`%APPDATA%\VoltNest\toast-icon.png`）。
const FILE_NAME: &str = "toast-icon.png";

/// 进程内缓存：`path()` 要能在任意线程直接取，不再重复读盘。
static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// 幂等落盘并缓存路径；返回 `None` 表示写不进去（调用方继续走无图标路径）。
pub fn init(root: &Path) -> Option<PathBuf> {
    CACHE.get_or_init(|| write(root).ok()).clone()
}

/// 已落盘的图标路径（`init` 之前调用得到 `None`）。
pub fn path() -> Option<&'static Path> {
    CACHE.get().and_then(|cached| cached.as_deref())
}

/// 内容不同才写，避免每次启动都碰一次磁盘。
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
        // 内容被外力破坏后（用户/清理工具动过），下一次启动要能自己修回来。
        fs::write(&target, b"broken").unwrap();
        assert_eq!(write(&dir).unwrap(), target);
        assert_eq!(fs::read(&target).unwrap(), first);
    }
}
