//! 数据根目录与文件名约定。
//!
//! 两个平台各按自己的「应用数据」惯例解析（见 [`DataPaths::root_from`]）：
//!
//! ```text
//! Windows                         Linux
//! %APPDATA%\VoltNest\             $XDG_DATA_HOME/VoltNest/
//! ├─ settings.json                ├─ settings.json
//! ├─ state.json                   ├─ state.json
//! ├─ manual_devices.json          ├─ manual_devices.json
//! ├─ reports/                     ├─ reports/
//! ├─ logs/                        ├─ logs/
//! └─ toast-icon.png               └─ toast-icon.png
//! ```
//!
//! Linux 侧 `$XDG_DATA_HOME` 缺失时用 `$HOME/.local/share/VoltNest`。
//!
//! 除「HOME 与平台变量都缺失」这一最后兜底外，根目录恒为绝对路径，与进程工作目录无关——
//! 从桌面启动器启动时工作目录可能是 `/`，此处不能依赖它。

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// 应用自己的目录名（两平台共用）。
const APP_DIR: &str = "VoltNest";

/// 真实环境查询：[`DataPaths::resolve`] 交给纯逻辑的那一半。
fn env_lookup(key: &str) -> Option<OsString> {
    std::env::var_os(key)
}

/// 应用数据目录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataPaths {
    root: PathBuf,
}

impl DataPaths {
    /// 解析数据根目录：Windows 为 `%APPDATA%\VoltNest`，Linux 为 `$XDG_DATA_HOME/VoltNest`
    /// （缺省 `$HOME/.local/share/VoltNest`）。
    pub fn resolve() -> Self {
        Self::root_from(env_lookup)
    }

    /// 由环境查询函数推导数据根目录（纯函数，便于测试）。
    ///
    /// - Windows：`%APPDATA%\VoltNest`；`%APPDATA%` 缺失或为空时走最后兜底。
    /// - Linux：`$XDG_DATA_HOME/VoltNest`，但仅当该变量非空且是**绝对路径**（XDG 规范：
    ///   相对路径视为未设置）；否则 `$HOME/.local/share/VoltNest`。
    /// - 最后兜底：上面都拿不到（HOME 缺失等环境不完整的情形）时用当前目录下的 `.voltnest`。
    ///   **这只是兜底，不是产品行为**，正常桌面会话不会走到这里。
    pub fn root_from(lookup: impl Fn(&str) -> Option<OsString>) -> Self {
        let root = platform_root(&lookup).unwrap_or_else(|| PathBuf::from(".voltnest"));
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

    /// `manual_devices.json`（用户手动添加的设备）。
    pub fn manual_devices_file(&self) -> PathBuf {
        self.root.join("manual_devices.json")
    }

    /// `reports/`：生成上报数据时产出的 issue 报告。
    pub fn reports_dir(&self) -> PathBuf {
        self.root.join("reports")
    }

    /// `logs/`。
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// 损坏存档：`settings.corrupt-<UTC毫秒>.json`。时间戳由调用方传入，本 crate 不读时钟。
    pub fn corrupt_settings_file(&self, now_ms: u64) -> PathBuf {
        self.root.join(format!("settings.corrupt-{now_ms}.json"))
    }

    /// 版本过高存档：`settings.unsupported-<UTC毫秒>.json`。
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

/// Windows 数据根：`%APPDATA%\VoltNest`。
///
/// 不变式：拼接形状恒为 `<APPDATA>\VoltNest`；`%APPDATA%` 缺失或为空才返回 `None`。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_root(lookup: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let base = lookup("APPDATA").filter(|value| !value.is_empty())?;
    Some(PathBuf::from(base).join(APP_DIR))
}

/// Linux 数据根：`$XDG_DATA_HOME/VoltNest`，缺省 `$HOME/.local/share/VoltNest`。
///
/// XDG Base Directory 规范要求 `$XDG_DATA_HOME` 是绝对路径，相对路径按未设置处理；空值同理。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_root(lookup: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(base) = lookup("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        let base = PathBuf::from(base);
        if base.is_absolute() {
            return Some(base.join(APP_DIR));
        }
    }
    let home = lookup("HOME").filter(|value| !value.is_empty())?;
    Some(
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join(APP_DIR),
    )
}

/// 当前平台的数据根（不含最后兜底）。
#[cfg(target_os = "windows")]
fn platform_root(lookup: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    windows_root(lookup)
}

/// 当前平台的数据根（不含最后兜底）。
#[cfg(target_os = "linux")]
fn platform_root(lookup: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    linux_root(lookup)
}

/// 其它平台暂无定义的数据根，取最后兜底。
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn platform_root(_lookup: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 固定环境的假查询函数：只认给出的键值对。
    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[cfg(target_os = "linux")]
    fn home_tree() -> PathBuf {
        Path::new("/home/u")
            .join(".local")
            .join("share")
            .join(APP_DIR)
    }

    #[test]
    fn file_names_are_fixed() {
        let p = DataPaths::with_root("C:/tmp/voltnest");
        assert!(p.settings_file().ends_with("settings.json"));
        assert!(p.state_file().ends_with("state.json"));
        assert!(p.manual_devices_file().ends_with("manual_devices.json"));
        assert!(p.reports_dir().ends_with("reports"));
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

    /// Linux 路径语义依赖宿主平台的 `Path::is_absolute`：Windows 上 `/data` 不算绝对路径，
    /// 这组断言只在 Linux 有意义。
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_root_prefers_absolute_xdg_data_home() {
        let root = linux_root(&env_of(&[("XDG_DATA_HOME", "/data"), ("HOME", "/home/u")]));
        assert_eq!(root, Some(Path::new("/data").join(APP_DIR)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_root_ignores_relative_xdg_data_home() {
        // XDG 规范：相对路径视为未设置，于是回落 HOME。
        let root = linux_root(&env_of(&[
            ("XDG_DATA_HOME", "rel/data"),
            ("HOME", "/home/u"),
        ]));
        assert_eq!(root, Some(home_tree()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_root_falls_back_to_home() {
        let root = linux_root(&env_of(&[("HOME", "/home/u")]));
        assert_eq!(root, Some(home_tree()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_root_ignores_empty_variables() {
        let root = linux_root(&env_of(&[("XDG_DATA_HOME", ""), ("HOME", "")]));
        assert_eq!(root, None);
    }

    #[test]
    fn root_without_any_environment_is_last_resort() {
        let paths = DataPaths::root_from(env_of(&[]));
        assert_eq!(paths.root(), Path::new(".voltnest"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn resolve_root_is_wired_to_linux_branch() {
        let xdg = DataPaths::root_from(env_of(&[("XDG_DATA_HOME", "/data"), ("HOME", "/home/u")]));
        assert_eq!(xdg.root(), Path::new("/data").join(APP_DIR).as_path());
        let home = DataPaths::root_from(env_of(&[("HOME", "/home/u")]));
        assert_eq!(home.root(), home_tree().as_path());
    }

    #[test]
    fn windows_root_keeps_appdata_shape() {
        let appdata = Path::new(r"C:\Users\me\AppData\Roaming");
        let root = windows_root(&env_of(&[("APPDATA", r"C:\Users\me\AppData\Roaming")]));
        assert_eq!(root, Some(appdata.join(APP_DIR)));
        assert_eq!(windows_root(&env_of(&[("APPDATA", "")])), None);
        assert_eq!(windows_root(&env_of(&[])), None);
    }

    #[test]
    fn windows_root_ignores_linux_variables() {
        // Linux 变量不该左右 Windows 分支：只有 HOME / XDG_DATA_HOME 时仍返回 None。
        assert_eq!(
            windows_root(&env_of(&[("XDG_DATA_HOME", "/data"), ("HOME", "/home/u")])),
            None
        );
    }
}
