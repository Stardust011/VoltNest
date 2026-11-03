//! `settings.json`：用户资产（阈值档位、通知方式、自启、托盘顺序与隐藏列表）。
//!
//! 规则：带整数 `schemaVersion`；未知字段原样保留（`extra`）；损坏或版本过高时备份 → 默认值启动 →
//! 诊断告警，绝不阻塞启动；只存档位字符串，不存毫秒值。

use crate::SCHEMA_VERSION;
use crate::atomic::{archive_corrupt, read_optional, write_json_atomic};
use crate::paths::DataPaths;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
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
    /// 未知字段（嵌套层级同样不得丢弃）。
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
    /// 托盘顺序（`stableId` 列表）。条目不自动删除；排序时过滤不存在的 id。
    pub order: Vec<String>,
    /// 用户显式隐藏的设备。
    pub hidden: Vec<String>,
    /// 未知字段。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// 用户合并组：把多台设备呈现为一张卡片。
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MergeGroup {
    /// 主设备 `stableId`；卡片 ID 由它派生，故不可为空。
    pub primary: String,
    /// 全部成员（含主设备）。
    pub members: Vec<String>,
    /// 卡片名称，默认取合并时 HID 上报的名称。
    pub name: String,
    /// 未知字段。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// 设备管理：用户对呈现层的改动（别名、删除列表、合并组）。
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceSettings {
    /// `stableId` → 自定义名称。
    pub aliases: BTreeMap<String, String>,
    /// 用户删除的设备：不再出现在列表、卡片与托盘。
    pub removed: Vec<String>,
    /// 用户合并组。
    pub merges: Vec<MergeGroup>,
    /// 用户已分离的自动合并卡（`cardId`）：不再自动并成一张卡。
    pub separated: Vec<String>,
    /// 未知字段。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl DeviceSettings {
    /// 是否在删除列表里。
    pub fn is_removed(&self, id: &str) -> bool {
        self.removed.iter().any(|r| r == id)
    }

    /// 自定义名称（空串视为未设置）。
    pub fn alias_of(&self, id: &str) -> Option<&str> {
        self.aliases
            .get(id)
            .map(String::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }

    /// 某设备所在的合并组。
    pub fn group_of(&self, id: &str) -> Option<&MergeGroup> {
        self.merges
            .iter()
            .find(|group| group.members.iter().any(|m| m == id))
    }

    /// 改名：空白名称等于清除自定义名称。
    pub fn rename(&mut self, id: &str, name: &str) {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            self.aliases.remove(id);
        } else {
            self.aliases.insert(id.to_string(), trimmed.to_string());
        }
    }

    /// 某自动合并卡是否已被用户分离。
    pub fn is_separated(&self, card_id: &str) -> bool {
        self.separated.iter().any(|id| id == card_id)
    }

    /// 分离一张自动合并卡；返回它此前是否还并着。
    pub fn separate_auto(&mut self, card_id: &str) -> bool {
        if card_id.is_empty() || self.is_separated(card_id) {
            return false;
        }
        self.separated.push(card_id.to_string());
        true
    }

    /// 取消分离：让自动分组重新生效。
    pub fn reunite(&mut self, card_id: &str) -> bool {
        let before = self.separated.len();
        self.separated.retain(|id| id != card_id);
        self.separated.len() != before
    }

    /// 从删除列表恢复；返回它此前是否真的被删除。
    pub fn restore(&mut self, id: &str) -> bool {
        let before = self.removed.len();
        self.removed.retain(|r| r != id);
        self.removed.len() != before
    }

    /// 删除一批设备：拆散它们所在的合并组。
    pub fn remove(&mut self, ids: &[String]) {
        for id in ids {
            if !self.is_removed(id) {
                self.removed.push(id.clone());
            }
            self.merges.retain(|group| !group.members.contains(id));
        }
        self.normalize();
    }

    /// 新建合并组：任一成员已在别的合并组里则失败（不静默抢走别人的成员）。
    pub fn merge(&mut self, primary: &str, members: &[String], name: &str) -> Result<(), String> {
        let mut all = vec![primary.to_string()];
        for id in members {
            if id == primary {
                return Err("不能把设备与自身合并".to_string());
            }
            if all.contains(id) {
                continue;
            }
            all.push(id.clone());
        }
        if all.len() < 2 {
            return Err("至少需要两台设备才能合并".to_string());
        }
        for id in &all {
            if self.is_removed(id) {
                return Err(format!("设备 {id} 已被删除，请先添加"));
            }
            if let Some(group) = self.group_of(id) {
                return Err(format!(
                    "设备 {id} 已与「{}」合并，请先分离",
                    if group.name.trim().is_empty() {
                        group.primary.as_str()
                    } else {
                        group.name.as_str()
                    }
                ));
            }
        }
        let trimmed = name.trim();
        self.merges.push(MergeGroup {
            primary: primary.to_string(),
            members: all,
            name: if trimmed.is_empty() {
                primary.to_string()
            } else {
                trimmed.to_string()
            },
            extra: Map::new(),
        });
        self.normalize();
        Ok(())
    }

    /// 分离以 `primary` 为锚点的合并组；返回是否真的存在。
    pub fn separate(&mut self, primary: &str) -> bool {
        let before = self.merges.len();
        self.merges.retain(|group| group.primary != primary);
        self.merges.len() != before
    }

    /// 收敛不变量：删除列表去重，合并组至少两台、主设备在组内、成员不跨组。
    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        self.removed.retain(|id| {
            let unique = !id.is_empty() && !seen.contains(id);
            if unique {
                seen.push(id.clone());
            }
            unique
        });

        let mut claimed: Vec<String> = Vec::new();
        let mut groups: Vec<MergeGroup> = Vec::new();
        for mut group in std::mem::take(&mut self.merges) {
            let mut members: Vec<String> = Vec::new();
            for id in group.members.iter() {
                if id.is_empty()
                    || members.contains(id)
                    || claimed.contains(id)
                    || self.is_removed(id)
                {
                    continue;
                }
                members.push(id.clone());
            }
            if members.len() < 2 {
                continue;
            }
            if !members.contains(&group.primary) {
                group.primary = members[0].clone();
            }
            claimed.extend(members.iter().cloned());
            group.members = members;
            groups.push(group);
        }
        self.merges = groups;

        let mut separated: Vec<String> = Vec::new();
        self.separated.retain(|id| {
            let unique = !id.is_empty() && !separated.contains(id);
            if unique {
                separated.push(id.clone());
            }
            unique
        });
    }
}

/// `settings.json` 的完整形状。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 整数 schema 版本。
    pub schema_version: u32,
    /// 阈值档位字符串（`sensitive` / `standard` / `loose`）；存字符串以便未知值安全回落标准档。
    pub sensitivity: String,
    /// 通知设置。
    pub notifications: NotificationSettings,
    /// 开机自启：永不默认开启。
    pub autostart: bool,
    /// 启动时最小化到托盘（手动启动与开机自启共用这一项）。
    pub start_minimized: bool,
    /// 首启引导是否完成（不用「文件是否存在」判断）。
    pub onboarding_completed: bool,
    /// 托盘顺序与隐藏列表。
    pub tray: TraySettings,
    /// 设备管理（别名 / 删除 / 合并）。
    pub devices: DeviceSettings,
    /// 未知字段（读-改-写必须保留）。
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
            devices: DeviceSettings::default(),
            extra: Map::new(),
        }
    }
}

impl Settings {
    /// 损坏恢复 / 版本过高时用于重建的默认值：`onboardingCompleted = true`，避免重复弹引导。
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

    /// 读取设置。任何错误都不返回 `Err`：配置坏了也必须能启动。
    pub fn load(&self, now_ms: u64) -> LoadedSettings {
        let path = self.paths.settings_file();
        let raw = match read_optional(&path) {
            Ok(Some(bytes)) => bytes,
            // 首次运行：文件不存在不是错误（用 onboardingCompleted 判断引导）。
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

        // 版本高于当前支持 → 备份后按默认启动，不猜测解析。
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

    /// 存档 → 默认值 → 告警。
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
        assert!(!s.autostart, "自启永不默认开启");
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
        // 降级运行不得静默清空用户（或新版本）写下的字段。
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
        // 损坏 → 备份 → 默认值 → onboardingCompleted 置 true。
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

    #[test]
    fn device_settings_default_and_round_trip() {
        let (s, dir) = store();
        let mut settings = Settings::default();
        settings.devices.rename("hid:7768:6426", "  桌面鼠标 ");
        settings
            .devices
            .remove(&["ble:01:00:00:23:04:35".to_string()]);
        s.save(&settings).unwrap();

        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("\"devices\""), "{text}");
        let loaded = s.load(T).settings;
        assert_eq!(
            loaded.devices.alias_of("hid:7768:6426"),
            Some("桌面鼠标"),
            "改名要 trim"
        );
        assert!(loaded.devices.is_removed("ble:01:00:00:23:04:35"));
    }

    #[test]
    fn auto_merge_separation_round_trip() {
        let (s, dir) = store();
        let mut settings = Settings::default();
        assert!(settings.devices.separate_auto("aj159 nl"));
        assert!(
            !settings.devices.separate_auto("aj159 nl"),
            "重复分离不重复记"
        );
        assert!(settings.devices.is_separated("aj159 nl"));
        s.save(&settings).unwrap();

        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("\"separated\""), "{text}");
        assert!(s.load(T).settings.devices.is_separated("aj159 nl"));

        let mut settings = s.load(T).settings;
        assert!(settings.devices.reunite("aj159 nl"));
        assert!(!settings.devices.is_separated("aj159 nl"));
    }

    #[test]
    fn legacy_file_without_devices_section_loads() {
        let (s, dir) = store();
        fs::write(
            dir.join("settings.json"),
            r#"{"schemaVersion":1,"onboardingCompleted":true}"#,
        )
        .unwrap();
        let loaded = s.load(T);
        assert!(loaded.warning.is_none());
        assert!(loaded.settings.devices.merges.is_empty());
        assert!(loaded.settings.devices.removed.is_empty());
    }

    #[test]
    fn merge_and_separate_invariants() {
        let mut d = DeviceSettings::default();
        let a = "hid:1111:0001".to_string();
        let b = "ble:02:00:00:00:00:01".to_string();

        d.merge(&a, std::slice::from_ref(&b), "桌面").unwrap();
        assert_eq!(d.group_of(&b).map(|g| g.primary.as_str()), Some(a.as_str()));
        assert!(
            d.merge(&a, std::slice::from_ref(&b), "再来一次").is_err(),
            "重复合并应失败"
        );
        assert!(
            d.merge(&a, std::slice::from_ref(&a), "自己").is_err(),
            "自己合并自己应失败"
        );
        assert!(d.merge(&a, &[], "少一台").is_err());

        assert!(d.separate(&a));
        assert!(!d.separate(&a), "重复分离应返回 false");
        assert!(d.group_of(&a).is_none());
    }

    #[test]
    fn removing_one_member_dissolves_group() {
        let mut d = DeviceSettings::default();
        let a = "hid:1111:0001".to_string();
        let b = "ble:02:00:00:00:00:01".to_string();
        d.merge(&a, std::slice::from_ref(&b), "桌面").unwrap();

        d.remove(std::slice::from_ref(&b));
        assert!(d.merges.is_empty(), "成员被删后合并组必须解散");
        assert!(d.is_removed(&b));
        assert!(d.restore(&b));
        assert!(!d.restore(&b), "重复恢复应返回 false");
    }

    #[test]
    fn rename_with_blank_clears_alias() {
        let mut d = DeviceSettings::default();
        d.rename("hid:1111:0001", "鼠标");
        assert_eq!(d.alias_of("hid:1111:0001"), Some("鼠标"));
        d.rename("hid:1111:0001", "   ");
        assert_eq!(d.alias_of("hid:1111:0001"), None);
    }
}
