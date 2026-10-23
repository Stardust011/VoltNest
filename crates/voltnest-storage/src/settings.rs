//! `settings.json`：用户资产（阈值档位、通知方式、自启、托盘顺序与隐藏列表）。
//!
//! 规则（X-1 / X-3 / X-4 / X-6 / X-7）：带整数 `schemaVersion`；未知字段读入后原样保留
//! （`#[serde(flatten)]` 收进 `extra`），降级运行不静默清空；损坏或版本过高时备份 → 默认值启动 →
//! 诊断告警，绝不阻塞启动；配置不出现任何毫秒值（Z-1），只存档位字符串。

use crate::SCHEMA_VERSION;
use crate::atomic::{archive_corrupt, read_optional, write_json_atomic};
use crate::paths::DataPaths;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;
use voltnest_provider::Sensitivity;

/// 通知方式。设置页只在这三档之间切换，不做自动降级。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationMode {
    /// Windows Toast（默认）。
    #[default]
    System,
    /// 托盘气泡。
    TrayBubble,
    /// 关闭。
    Off,
}

/// 通知设置。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    /// 通知方式。
    pub mode: NotificationMode,
    /// 低电量阈值（默认 20 / 10，可调整）。
    pub low_thresholds: [u8; 2],
    /// 未知字段（X-1：嵌套层级同样不得丢弃）。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            mode: NotificationMode::System,
            low_thresholds: [20, 10],
            extra: Map::new(),
        }
    }
}

/// 托盘设置。
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TraySettings {
    /// 托盘顺序（`stableId` 列表）。条目不自动删除；排序时过滤不存在的 id（X-7）。
    pub order: Vec<String>,
    /// 用户显式隐藏的设备（AA-4）。
    pub hidden: Vec<String>,
    /// 未知字段（X-1）。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `settings.json` 的完整形状。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 整数 schema 版本。
    pub schema_version: u32,
    /// 阈值档位字符串（`sensitive` / `standard` / `loose`）。
    ///
    /// 存字符串而不是枚举，是为了让未知值能安全回落到标准档而不是解析失败。
    pub sensitivity: String,
    /// 通知设置。
    pub notifications: NotificationSettings,
    /// 开机自启：**永不默认开启**（AC-1）。
    pub autostart: bool,
    /// 启动时最小化到托盘（AC-8：手动启动与开机自启共用这一项）。
    pub start_minimized: bool,
    /// 首启引导是否完成（AC-2：不用「文件是否存在」判断）。
    pub onboarding_completed: bool,
    /// 托盘顺序与隐藏列表。
    pub tray: TraySettings,
    /// 未知字段（X-1：读-改-写必须保留）。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            sensitivity: Sensitivity::Standard.as_str().to_string(),
            notifications: NotificationSettings::default(),
            autostart: false,
            start_minimized: false,
            onboarding_completed: false,
            tray: TraySettings::default(),
            extra: Map::new(),
        }
    }
}

impl Settings {
    /// 损坏恢复 / 版本过高时用于重建的默认值：
    /// `onboardingCompleted = true`，避免每次启动都弹一次引导（AC-3）。
    pub fn recovered() -> Self {
        Self {
            onboarding_completed: true,
            ..Self::default()
        }
    }

    /// 阈值档位（未知值回落标准档，不报错）。
    pub fn sensitivity(&self) -> Sensitivity {
        Sensitivity::parse(&self.sensitivity).unwrap_or_default()
    }

    /// 设置阈值档位。
    pub fn set_sensitivity(&mut self, sensitivity: Sensitivity) {
        self.sensitivity = sensitivity.as_str().to_string();
    }
}

/// 加载告警，宿主转成诊断页条目 + 一次 toast。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SettingsLoadWarning {
    /// 解析失败，已存档并用默认值启动。
    Corrupt {
        /// 原文件。
        path: PathBuf,
        /// 存档位置。
        backup: PathBuf,
    },
    /// `schemaVersion` 高于当前支持（用户装过更新版本）。
    VersionTooHigh {
        /// 文件里的版本号。
        found: u32,
        /// 存档位置。
        backup: PathBuf,
    },
    /// 读取失败（权限、磁盘）。
    Io {
        /// 文件路径。
        path: PathBuf,
        /// 细节。
        detail: String,
    },
}

/// 告警文案直接进诊断页，因此写成人话而不是 `Debug` 输出。
impl core::fmt::Display for SettingsLoadWarning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Corrupt { backup, .. } => write!(
                f,
                "settings.json 解析失败，已备份到 {}，用默认值启动",
                backup.display()
            ),
            Self::VersionTooHigh { found, backup } => write!(
                f,
                "settings.json 版本 {found} 高于本程序支持版本，已备份到 {}，用默认值启动",
                backup.display()
            ),
            Self::Io { path, detail } => {
                write!(f, "读取 {} 失败：{detail}", path.display())
            }
        }
    }
}

/// 加载结果：设置 + 可选告警。#[derive(Clone, Debug)]
pub struct LoadedSettings {
    /// 生效的设置。
    pub settings: Settings,
    /// 告警（不阻塞启动）。
    pub warning: Option<SettingsLoadWarning>,
}

/// `settings.json` 读写器。
#[derive(Clone, Debug)]
pub struct SettingsStore {
    paths: DataPaths,
}

impl SettingsStore {
    /// 构造。
    pub fn new(paths: DataPaths) -> Self {
        Self { paths }
    }

    /// 数据目录。
    pub fn paths(&self) -> &DataPaths {
        &self.paths
    }

    /// 读取设置。**任何**错误都不返回 `Err`：配置坏了也必须能启动。
    pub fn load(&self, now_ms: u64) -> LoadedSettings {
        let path = self.paths.settings_file();
        let raw = match read_optional(&path) {
            Ok(Some(bytes)) => bytes,
            // 首次运行：文件不存在不是错误（AC-2 用 onboardingCompleted 判断引导）。
            Ok(None) => {
                return LoadedSettings {
                    settings: Settings::default(),
                    warning: None,
                };
            }
            Err(e) => {
                return LoadedSettings {
                    settings: Settings::recovered(),
                    warning: Some(SettingsLoadWarning::Io {
                        path,
                        detail: e.to_string(),
                    }),
                };
            }
        };

        let value: Value = match serde_json::from_slice(&raw) {
            Ok(v) => v,
            Err(e) => return self.recover_corrupt(&path, now_ms, &e.to_string()),
        };

        // X-6：版本高于当前支持 → 备份后按默认启动，不猜测解析。
        let found = value
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .unwrap_or(u64::from(SCHEMA_VERSION));
        if found > u64::from(SCHEMA_VERSION) {
            let backup_target = self.paths.unsupported_settings_file(now_ms);
            let backup = archive_corrupt(&path, &backup_target).unwrap_or(backup_target);
            return LoadedSettings {
                settings: Settings::recovered(),
                warning: Some(SettingsLoadWarning::VersionTooHigh {
                    found: found as u32,
                    backup,
                }),
            };
        }

        match serde_json::from_value::<Settings>(value) {
            Ok(mut settings) => {
                // 缺失 schemaVersion 的旧文件按当前版本收编。
                settings.schema_version = SCHEMA_VERSION;
                LoadedSettings {
                    settings,
                    warning: None,
                }
            }
            Err(e) => self.recover_corrupt(&path, now_ms, &e.to_string()),
        }
    }

    /// X-4：存档 → 默认值 → 告警。
    fn recover_corrupt(&self, path: &std::path::Path, now_ms: u64, detail: &str) -> LoadedSettings {
        let backup_target = self.paths.corrupt_settings_file(now_ms);
        let backup = match archive_corrupt(path, &backup_target) {
            Ok(p) => p,
            Err(_) => backup_target,
        };
        eprintln!(
            "settings.json 损坏（{detail}），存档到 {}",
            backup.display()
        );
        LoadedSettings {
            settings: Settings::recovered(),
            warning: Some(SettingsLoadWarning::Corrupt {
                path: path.to_path_buf(),
                backup,
            }),
        }
    }

    /// 保存设置（原子替换）。
    pub fn save(&self, settings: &Settings) -> std::io::Result<()> {
        let mut copy = settings.clone();
        copy.schema_version = SCHEMA_VERSION;
        self.paths.ensure_root()?;
        write_json_atomic(&self.paths.settings_file(), &copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);
    const T: u64 = 1_800_000_000_000;

    fn store() -> (SettingsStore, PathBuf) {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("voltnest-settings-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        (SettingsStore::new(DataPaths::with_root(&dir)), dir)
    }

    #[test]
    fn defaults_are_safe() {
        let s = Settings::default();
        assert_eq!(s.schema_version, 1);
        assert!(!s.autostart, "AC-1：自启永不默认开启");
        assert!(!s.start_minimized);
        assert!(!s.onboarding_completed, "首次运行应显示引导");
        assert_eq!(s.sensitivity(), Sensitivity::Standard);
        assert_eq!(s.notifications.low_thresholds, [20, 10]);
        assert_eq!(s.notifications.mode, NotificationMode::System);
    }

    #[test]
    fn missing_file_loads_defaults_without_warning() {
        let (s, _dir) = store();
        let loaded = s.load(T);
        assert!(loaded.warning.is_none(), "首次运行不是错误");
        assert!(!loaded.settings.onboarding_completed);
    }

    #[test]
    fn round_trip_preserves_values_and_writes_schema_version() {
        let (s, dir) = store();
        let mut settings = Settings::default();
        settings.set_sensitivity(Sensitivity::Loose);
        settings.autostart = true;
        settings.onboarding_completed = true;
        settings.tray.order = vec!["hid:7768:6426".into()];
        settings.tray.hidden = vec!["ble:01:00:00:23:04:35".into()];
        settings.notifications.mode = NotificationMode::TrayBubble;
        settings.notifications.low_thresholds = [25, 15];
        s.save(&settings).unwrap();

        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("\"schemaVersion\": 1"));
        let loaded = s.load(T);
        assert!(loaded.warning.is_none());
        assert_eq!(loaded.settings, settings);
    }

    #[test]
    fn unknown_fields_survive_read_modify_write() {
        // X-1：降级运行不得静默清空用户（或新版本）写下的字段。
        let (s, dir) = store();
        fs::write(
            dir.join("settings.json"),
            r#"{
                "schemaVersion": 1,
                "sensitivity": "sensitive",
                "onboardingCompleted": true,
                "futureFeature": { "enabled": true, "level": 3 },
                "tray": { "order": ["hid:7768:6426"], "hidden": [], "futureTray": "x" }
            }"#,
        )
        .unwrap();

        let mut loaded = s.load(T).settings;
        assert_eq!(loaded.sensitivity(), Sensitivity::Sensitive);
        loaded.autostart = true;
        s.save(&loaded).unwrap();

        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("futureFeature"), "{text}");
        assert!(text.contains("\"level\": 3"), "{text}");
        assert!(text.contains("futureTray"), "{text}");
        assert!(text.contains("\"autostart\": true"), "{text}");
    }

    #[test]
    fn corrupt_file_is_archived_and_startup_continues() {
        // X-4 / AC-3：损坏 → 备份 → 默认值 → onboardingCompleted 置 true。
        let (s, dir) = store();
        fs::write(dir.join("settings.json"), b"{ this is not json").unwrap();
        let loaded = s.load(T);
        assert!(matches!(
            loaded.warning,
            Some(SettingsLoadWarning::Corrupt { .. })
        ));
        assert!(
            loaded.settings.onboarding_completed,
            "不得每次启动都重新引导"
        );
        assert!(!dir.join("settings.json").exists(), "损坏文件应被移走");
        assert!(dir.join(format!("settings.corrupt-{T}.json")).exists());
    }

    #[test]
    fn version_too_high_is_archived_not_parsed() {
        let (s, dir) = store();
        fs::write(
            dir.join("settings.json"),
            r#"{"schemaVersion": 2, "sensitivity": "loose"}"#,
        )
        .unwrap();
        let loaded = s.load(T);
        match loaded.warning {
            Some(SettingsLoadWarning::VersionTooHigh { found, ref backup }) => {
                assert_eq!(found, 2);
                assert!(backup.exists());
            }
            other => panic!("应报 VersionTooHigh，实际 {other:?}"),
        }
        assert_eq!(
            loaded.settings.sensitivity(),
            Sensitivity::Standard,
            "不猜测解析新版本"
        );
        assert!(loaded.settings.onboarding_completed);
    }

    #[test]
    fn missing_schema_version_is_adopted() {
        let (s, dir) = store();
        fs::write(dir.join("settings.json"), r#"{"sensitivity":"loose"}"#).unwrap();
        let loaded = s.load(T);
        assert!(loaded.warning.is_none());
        assert_eq!(loaded.settings.sensitivity(), Sensitivity::Loose);
        assert_eq!(loaded.settings.schema_version, 1);
    }

    #[test]
    fn unknown_sensitivity_falls_back_to_standard() {
        let (s, dir) = store();
        fs::write(
            dir.join("settings.json"),
            r#"{"schemaVersion":1,"sensitivity":"turbo"}"#,
        )
        .unwrap();
        let loaded = s.load(T);
        assert!(loaded.warning.is_none(), "未知档位不是文件损坏");
        assert_eq!(loaded.settings.sensitivity(), Sensitivity::Standard);
    }

    #[test]
    fn notification_mode_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&NotificationMode::Off).unwrap(),
            "\"off\""
        );
        assert_eq!(
            serde_json::to_string(&NotificationMode::TrayBubble).unwrap(),
            "\"trayBubble\""
        );
    }
}
