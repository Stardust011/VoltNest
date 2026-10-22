//! 数据根目录与文件名约定（规格 §11）。
//!
//! ```text
//! %APPDATA%\VoltNest\
//! ├─ settings.json
//! ├─ state.json
//! └─ logs/
//! ```

use std::path::{Path, PathBuf};

/// 应用数据目录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataPaths {
    root: PathBuf,
}

impl DataPaths {
    /// 解析固定数据根目录：`%APPDATA%\VoltNest\`。
    ///
    /// 非 Windows 或环境变量缺失时退化为「当前目录下的 .voltnest」，
    /// 仅为让测试与交叉编译可用；产品行为只在 Windows 定义。
    pub fn resolve() -> Self {
        let root = match std::env::var_os("APPDATA") {
            Some(base) if !base.is_empty() => PathBuf::from(base).join("VoltNest"),
            _ => PathBuf::from(".voltnest"),
        };
        Self { root }
    }

    /// 测试与便携场景：指定根目录。
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 数据根目录。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `settings.json`。
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// `state.json`。
    pub fn state_file(&self) -> PathBuf {
        self.root.join("state.json")
    }

    /// `logs/`。
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// 损坏存档：`settings.corrupt-<UTC毫秒>.json`（X-4）。
    ///
    /// 时间戳由调用方传入：本 crate 不读时钟（时间唯一出入点在宿主）。
    pub fn corrupt_settings_file(&self, now_ms: u64) -> PathBuf {
        self.root.join(format!("settings.corrupt-{now_ms}.json"))
    }

    /// 版本过高存档：`settings.unsupported-<UTC毫秒>.json`（X-6）。
    pub fn unsupported_settings_file(&self, now_ms: u64) -> PathBuf {
        self.root
            .join(format!("settings.unsupported-{now_ms}.json"))
    }

    /// 损坏的 `state.json` 存档。
    pub fn corrupt_state_file(&self, now_ms: u64) -> PathBuf {
        self.root.join(format!("state.corrupt-{now_ms}.json"))
    }

    /// 确保目录存在。
    pub fn ensure_root(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_fixed() {
        let p = DataPaths::with_root("C:/tmp/voltnest");
        assert!(p.settings_file().ends_with("settings.json"));
        assert!(p.state_file().ends_with("state.json"));
        assert!(p.logs_dir().ends_with("logs"));
        assert!(p.root().ends_with("voltnest"));
    }

    #[test]
    fn corrupt_names_carry_timestamp() {
        let p = DataPaths::with_root("C:/tmp/voltnest");
        assert!(
            p.corrupt_settings_file(1_800_000_000_000)
                .ends_with("settings.corrupt-1800000000000.json")
        );
        assert!(
            p.unsupported_settings_file(7)
                .ends_with("settings.unsupported-7.json")
        );
        assert!(p.corrupt_state_file(7).ends_with("state.corrupt-7.json"));
    }

    #[test]
    fn resolve_uses_appdata_when_present() {
        // 只验证拼接形状，不依赖本机是否设置了 APPDATA。
        let paths = DataPaths::resolve();
        assert!(paths.root().ends_with("VoltNest") || paths.root().ends_with(".voltnest"));
    }
}
