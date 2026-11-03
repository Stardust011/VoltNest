//! `manual_devices.json`：用户手动添加的设备（自动识别失败时的兜底）。
//!
//! 与 `state.json` 分开：前者是用户资产，后者是可随时删除的运行时缓存；本文件损坏时先存档再用空表启动。
//! 记录的是「用户声称这台设备存在」，不参与状态机，也不产生电量读数（[`ManualDevice::report_path`]）。

use crate::atomic::{archive_corrupt, read_optional, write_json_atomic};
use crate::paths::DataPaths;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// 本文件的 schema 版本（与 `settings.json` 的版本号各管各的）。
pub const MANUAL_SCHEMA_VERSION: u32 = 1;

/// 记录条数上限；超出时淘汰最早添加的记录。
pub const MAX_MANUAL_DEVICES: usize = 64;

/// 一次采样：同一集合、某一时刻录到的一段数据，以及当时驱动 / 系统显示的电量。
///
/// 电量是采样的一部分（每次采样各记各的），不是设备级属性。
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ManualSample {
    /// 驱动 / 系统界面当时显示的电量（`None` = 驱动没显示，不能编一个数）。
    pub battery_percent: Option<u8>,
    /// 该次录到的帧数。
    pub frames: usize,
    /// 录制完成时间（UTC 毫秒）。
    pub captured_at_ms: u64,
    /// 未知字段。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// 一台手动添加的设备。
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ManualDevice {
    /// 标识，与设备 `stableId` 同构（`hid:7768:6426`）：同一 VID:PID 只保留一条记录。
    pub id: String,
    /// 用户可改的显示名（默认取 HID 产品名）。
    pub name: String,
    /// 厂商 ID。
    pub vid: u16,
    /// 产品 ID。
    pub pid: u16,
    /// 抓包时选中的集合的 UsagePage。
    pub usage_page: u16,
    /// 抓包时选中的集合的 Usage。
    pub usage: u16,
    /// 抓包时选中的集合路径（设备换口就会变，只说明当时抓的是哪个集合）。
    pub path: String,
    /// HID 产品名（用户改名后仍能追溯原始值）。
    pub product: Option<String>,
    /// 序列号（同型号多只设备时唯一可靠的区分）。
    pub serial: Option<String>,
    /// 用户补充的说明。
    pub note: String,
    /// 首次添加时间（UTC 毫秒）。
    pub added_at_ms: u64,
    /// 最近一次抓包时间（UTC 毫秒）。
    pub last_capture_at_ms: Option<u64>,
    /// 全部采样的帧数之和（冗余字段：读列表时不必再遍历 `samples`）。
    pub captured_frames: usize,
    /// 采样记录（按录制顺序；每条自带驱动显示的电量）。
    pub samples: Vec<ManualSample>,
    /// 报告文件路径（`reports/` 下）。报告内容以文件为准，不在本文件里存几千帧字节。
    pub report_path: Option<String>,
    /// 未知字段（读-改-写必须原样保留）。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `manual_devices.json` 的完整形状。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ManualDeviceFile {
    /// 整数 schema 版本。
    pub schema_version: u32,
    /// 设备列表（按添加时间倒序由界面负责，本结构不排序）。
    pub devices: Vec<ManualDevice>,
    /// 未知字段。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ManualDeviceFile {
    fn default() -> Self {
        Self {
            schema_version: MANUAL_SCHEMA_VERSION,
            devices: Vec::new(),
            extra: Map::new(),
        }
    }
}

impl ManualDeviceFile {
    /// 按 `id` 新增或覆盖；覆盖时保留原有的 `added_at_ms`。
    pub fn upsert(&mut self, mut device: ManualDevice) {
        match self.devices.iter_mut().find(|known| known.id == device.id) {
            Some(slot) => {
                device.added_at_ms = slot.added_at_ms;
                *slot = device;
            }
            None => self.devices.push(device),
        }
        self.prune();
    }

    /// 删除一条记录；返回是否真的删掉了。
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.devices.len();
        self.devices.retain(|device| device.id != id);
        before != self.devices.len()
    }

    /// 按 `id` 查一条记录。
    pub fn get(&self, id: &str) -> Option<&ManualDevice> {
        self.devices.iter().find(|device| device.id == id)
    }

    /// 超过 [`MAX_MANUAL_DEVICES`] 时丢掉最早添加的记录（防止文件无限增长）。
    fn prune(&mut self) {
        if self.devices.len() <= MAX_MANUAL_DEVICES {
            return;
        }
        self.devices.sort_by_key(|device| device.added_at_ms);
        let overflow = self.devices.len() - MAX_MANUAL_DEVICES;
        self.devices.drain(..overflow);
    }
}

/// 加载结果：数据 + 需要告知用户的问题（界面转成一条提示，不阻塞启动）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LoadedManualDevices {
    /// 解析出的数据（失败时是空表）。
    pub file: ManualDeviceFile,
    /// 原文件损坏时的存档路径。
    pub corrupt_backup: Option<std::path::PathBuf>,
    /// 版本高于当前支持时的存档路径。
    pub unsupported_backup: Option<std::path::PathBuf>,
}

/// `manual_devices.json` 读写。
#[derive(Clone, Debug)]
pub struct ManualDeviceStore {
    paths: DataPaths,
}

impl ManualDeviceStore {
    /// 绑定数据目录。
    pub fn new(paths: DataPaths) -> Self {
        Self { paths }
    }

    /// 读取文件。文件不存在 / 损坏 / 版本过高都不算致命：前两者返回空表，
    /// 损坏与版本过高额外返回存档路径，由调用方提示。
    pub fn load(&self, now_ms: u64) -> LoadedManualDevices {
        let path = self.paths.manual_devices_file();
        let Some(bytes) = read_optional(&path).ok().flatten() else {
            return LoadedManualDevices {
                file: ManualDeviceFile::default(),
                corrupt_backup: None,
                unsupported_backup: None,
            };
        };

        match serde_json::from_slice::<ManualDeviceFile>(&bytes) {
            Ok(file) if file.schema_version > MANUAL_SCHEMA_VERSION => {
                let target = self
                    .paths
                    .root()
                    .join(format!("manual_devices.unsupported-{now_ms}.json"));
                let backup = archive_corrupt(&path, &target).ok();
                LoadedManualDevices {
                    file: ManualDeviceFile::default(),
                    corrupt_backup: None,
                    unsupported_backup: backup,
                }
            }
            Ok(file) => LoadedManualDevices {
                file,
                corrupt_backup: None,
                unsupported_backup: None,
            },
            Err(_) => {
                let target = self
                    .paths
                    .root()
                    .join(format!("manual_devices.corrupt-{now_ms}.json"));
                let backup = archive_corrupt(&path, &target).ok();
                LoadedManualDevices {
                    file: ManualDeviceFile::default(),
                    corrupt_backup: backup,
                    unsupported_backup: None,
                }
            }
        }
    }

    /// 原子写入整个文件。
    pub fn save(&self, file: &ManualDeviceFile) -> std::io::Result<()> {
        write_json_atomic(&self.paths.manual_devices_file(), file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn temp_paths() -> DataPaths {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("voltnest-manual-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        DataPaths::with_root(root)
    }

    fn device(id: &str, added_at_ms: u64) -> ManualDevice {
        ManualDevice {
            id: id.to_string(),
            name: format!("设备 {id}"),
            vid: 0x7768,
            pid: 0x6426,
            usage_page: 0xFFB2,
            usage: 0x0001,
            path: r"\\?\hid#vid_7768".to_string(),
            product: Some("AJ159 NL".to_string()),
            serial: None,
            note: "滚动滚轮".to_string(),
            added_at_ms,
            last_capture_at_ms: Some(added_at_ms + 1_000),
            captured_frames: 132,
            samples: vec![ManualSample {
                battery_percent: Some(80),
                frames: 132,
                captured_at_ms: added_at_ms + 1_000,
                extra: Map::new(),
            }],
            report_path: Some("reports/x.md".to_string()),
            extra: Map::new(),
        }
    }

    #[test]
    fn round_trip_keeps_every_field() {
        let paths = temp_paths();
        let store = ManualDeviceStore::new(paths.clone());
        let mut file = ManualDeviceFile::default();
        file.upsert(device("hid:7768:6426", 1_000));

        store.save(&file).expect("写盘成功");
        let loaded = store.load(0);
        assert!(loaded.corrupt_backup.is_none());
        assert_eq!(loaded.file.devices.len(), 1);
        let saved = &loaded.file.devices[0];
        assert_eq!(saved.id, "hid:7768:6426");
        assert_eq!(saved.product.as_deref(), Some("AJ159 NL"));
        assert_eq!(saved.captured_frames, 132);
        assert_eq!(saved.samples.len(), 1);
        assert_eq!(saved.samples[0].battery_percent, Some(80));
        assert_eq!(saved.samples[0].frames, 132);
        assert_eq!(saved.report_path.as_deref(), Some("reports/x.md"));
        assert!(paths.manual_devices_file().is_file());
    }

    #[test]
    fn upsert_replaces_but_keeps_first_added_time() {
        let mut file = ManualDeviceFile::default();
        file.upsert(device("hid:7768:6426", 1_000));
        let mut again = device("hid:7768:6426", 9_000);
        again.name = "改过的名字".to_string();
        again.captured_frames = 200;
        file.upsert(again);

        assert_eq!(file.devices.len(), 1, "同 id 覆盖而不是追加");
        assert_eq!(file.devices[0].name, "改过的名字");
        assert_eq!(file.devices[0].captured_frames, 200);
        assert_eq!(file.devices[0].added_at_ms, 1_000, "首次添加时间不得被改掉");
    }

    #[test]
    fn remove_reports_whether_it_did_anything() {
        let mut file = ManualDeviceFile::default();
        file.upsert(device("hid:7768:6426", 1_000));
        assert!(file.remove("hid:7768:6426"));
        assert!(!file.remove("hid:7768:6426"));
        assert!(file.devices.is_empty());
        assert!(file.get("hid:7768:6426").is_none());
    }

    #[test]
    fn prune_keeps_the_newest_entries() {
        let mut file = ManualDeviceFile::default();
        for index in 0..(MAX_MANUAL_DEVICES + 5) {
            file.upsert(device(&format!("hid:0000:{index:04x}"), index as u64));
        }
        assert_eq!(file.devices.len(), MAX_MANUAL_DEVICES);
        assert_eq!(
            file.devices.first().map(|d| d.added_at_ms),
            Some(5),
            "最早添加的 5 条被淘汰"
        );
    }

    /// 损坏文件不能让用户的手动设备「消失得无声无息」：必须先存档再空表启动。
    #[test]
    fn corrupt_file_is_archived_and_loads_empty() {
        let paths = temp_paths();
        std::fs::create_dir_all(paths.root()).unwrap();
        std::fs::write(paths.manual_devices_file(), b"{ not json").unwrap();

        let store = ManualDeviceStore::new(paths.clone());
        let loaded = store.load(1_700_000_000_000);
        assert!(loaded.file.devices.is_empty());
        let backup = loaded.corrupt_backup.expect("必须留下存档");
        assert!(backup.is_file(), "存档内容必须真的在：{backup:?}");
        assert!(
            backup
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("manual_devices.corrupt-")
        );
        assert!(!paths.manual_devices_file().exists(), "坏文件已被移走");
    }

    /// 装过更新版本的用户降级运行：不解析新格式，存档后从空表开始。
    #[test]
    fn future_schema_is_archived_not_reinterpreted() {
        let paths = temp_paths();
        std::fs::create_dir_all(paths.root()).unwrap();
        std::fs::write(
            paths.manual_devices_file(),
            br#"{"schemaVersion": 99, "devices": []}"#,
        )
        .unwrap();

        let loaded = ManualDeviceStore::new(paths.clone()).load(7);
        assert!(loaded.file.devices.is_empty());
        assert!(
            loaded
                .unsupported_backup
                .as_ref()
                .is_some_and(|p| p.is_file())
        );
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let loaded = ManualDeviceStore::new(temp_paths()).load(0);
        assert!(loaded.file.devices.is_empty());
        assert!(loaded.corrupt_backup.is_none());
        assert!(loaded.unsupported_backup.is_none());
        assert_eq!(loaded.file.schema_version, MANUAL_SCHEMA_VERSION);
    }

    /// 未知字段必须原样保留，否则降级运行会悄悄丢字段。
    #[test]
    fn unknown_fields_survive_a_round_trip() {
        let paths = temp_paths();
        std::fs::create_dir_all(paths.root()).unwrap();
        std::fs::write(
            paths.manual_devices_file(),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "futureTop": true,
                "devices": [{
                    "id": "hid:7768:6426",
                    "name": "x",
                    "vid": 30568,
                    "pid": 25638,
                    "futureDevice": "keep"
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        let store = ManualDeviceStore::new(paths.clone());
        let loaded = store.load(0);
        assert_eq!(
            loaded.file.extra.get("futureTop"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            loaded.file.devices[0].extra.get("futureDevice"),
            Some(&serde_json::json!("keep"))
        );

        store.save(&loaded.file).unwrap();
        let text = std::fs::read_to_string(paths.manual_devices_file()).unwrap();
        assert!(text.contains("futureTop"), "{text}");
        assert!(text.contains("futureDevice"), "{text}");
    }
}
