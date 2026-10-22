//! `state.json`：可丢弃缓存（设备最后已知电量、时间、路径、通知冷却与 `armed`）。
//!
//! 规则（X-3 / X-5 / X-7）：
//!
//! - 写入节流 5s；退出、隐藏到托盘、状态退化时由宿主强制 flush；
//! - **随时删除、功能不受影响**：只失去启动回填（有自动化测试，不靠人工记得）；
//! - 按 `stableId` 的条目超过 32 条时淘汰最久未出现项（Y-5 的地址轮换孤儿）。

use crate::SCHEMA_VERSION;
use crate::atomic::{archive_corrupt, read_optional, write_json_atomic};
use crate::paths::DataPaths;
use crate::{MAX_INDEXED_ENTRIES, STATE_THROTTLE_MS};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// 单个 `(device, threshold)` 的通知状态（AB-4 / AB-5）。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationStateEntry {
    /// 阈值。
    pub threshold: u8,
    /// 是否已武装。
    pub armed: bool,
    /// 上次通知时间。
    pub last_notified_at_ms: Option<u64>,
    /// 上次通知结果。
    pub last_result: Option<String>,
}

impl Default for NotificationStateEntry {
    fn default() -> Self {
        Self {
            threshold: 0,
            armed: true,
            last_notified_at_ms: None,
            last_result: None,
        }
    }
}

/// 一台设备的回填数据。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceStateEntry {
    /// `stableId`。
    pub stable_id: String,
    /// `hid` / `ble`。
    pub transport: String,
    /// 显示名快照（仅诊断与展示，Provider 上报才是真值，D8）。
    pub display_name: String,
    /// 型号原文快照。
    pub model_name_raw: String,
    /// 上次已知电量（只走副行展示，W-9）。
    pub last_known_battery: Option<u8>,
    /// 上次已知电量时间。
    pub last_known_at_ms: Option<u64>,
    /// 诊断路径（Y-6）。
    pub last_path: Option<String>,
    /// 首次发现时间。
    pub first_seen_at_ms: Option<u64>,
    /// 最近出现时间。
    pub last_seen_at_ms: Option<u64>,
    /// 通知状态。
    pub notifications: Vec<NotificationStateEntry>,
    /// 未知字段（X-1）。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for DeviceStateEntry {
    fn default() -> Self {
        Self {
            stable_id: String::new(),
            transport: String::new(),
            display_name: String::new(),
            model_name_raw: String::new(),
            last_known_battery: None,
            last_known_at_ms: None,
            last_path: None,
            first_seen_at_ms: None,
            last_seen_at_ms: None,
            notifications: Vec::new(),
            extra: Map::new(),
        }
    }
}

/// `state.json` 的完整形状。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StateFile {
    /// 整数 schema 版本。
    pub schema_version: u32,
    /// 设备条目。
    pub devices: Vec<DeviceStateEntry>,
    /// 未知字段（X-1）。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for StateFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            devices: Vec::new(),
            extra: Map::new(),
        }
    }
}

impl StateFile {
    /// 新增或替换一条设备状态。
    pub fn upsert(&mut self, entry: DeviceStateEntry) {
        match self
            .devices
            .iter_mut()
            .find(|d| d.stable_id == entry.stable_id)
        {
            Some(slot) => *slot = entry,
            None => self.devices.push(entry),
        }
    }

    /// 按 `lastSeenAt` 淘汰最久未出现的条目，保留 `limit` 条。
    ///
    /// 这是 Y-5「地址轮换产生的 orphan 交给 X-7 的 32 条上限」的落地。
    pub fn prune(&mut self, limit: usize) {
        if self.devices.len() <= limit {
            return;
        }
        self.devices.sort_by(|a, b| {
            b.last_seen_at_ms
                .unwrap_or(0)
                .cmp(&a.last_seen_at_ms.unwrap_or(0))
                .then_with(|| a.stable_id.cmp(&b.stable_id))
        });
        self.devices.truncate(limit);
        // 恢复稳定顺序，避免每次保存都抖动文件内容。
        self.devices.sort_by(|a, b| a.stable_id.cmp(&b.stable_id));
    }

    /// 按 `stableId` 查条目。
    pub fn get(&self, stable_id: &str) -> Option<&DeviceStateEntry> {
        self.devices.iter().find(|d| d.stable_id == stable_id)
    }
}

/// `state.json` 读写器。
#[derive(Clone, Debug)]
pub struct StateStore {
    paths: DataPaths,
}

impl StateStore {
    /// 构造。
    pub fn new(paths: DataPaths) -> Self {
        Self { paths }
    }

    /// 加载。文件不存在、损坏、版本过高都返回空状态（X-5：删除后功能必须正常）。
    pub fn load(&self, now_ms: u64) -> StateFile {
        let path = self.paths.state_file();
        let raw = match read_optional(&path) {
            Ok(Some(bytes)) => bytes,
            _ => return StateFile::default(),
        };
        match serde_json::from_slice::<StateFile>(&raw) {
            Ok(mut state) => {
                state.schema_version = SCHEMA_VERSION;
                state.prune(MAX_INDEXED_ENTRIES);
                state
            }
            Err(e) => {
                // 缓存坏了不值得打扰用户：存档 + 空状态，诊断由宿主补一条。
                let target = self.paths.corrupt_state_file(now_ms);
                let _ = archive_corrupt(&path, &target);
                eprintln!("VoltNest: state.json 损坏（{e}），已丢弃缓存");
                StateFile::default()
            }
        }
    }

    /// 保存（原子替换 + 上限裁剪）。
    pub fn save(&self, state: &StateFile) -> std::io::Result<()> {
        let mut copy = state.clone();
        copy.schema_version = SCHEMA_VERSION;
        copy.prune(MAX_INDEXED_ENTRIES);
        self.paths.ensure_root()?;
        write_json_atomic(&self.paths.state_file(), &copy)
    }
}

/// 写入节流器（X-3）。只记时间戳，不做 I/O 决策以外的判断。
#[derive(Clone, Debug)]
pub struct StateThrottle {
    throttle_ms: u64,
    last_write_ms: Option<u64>,
}

impl StateThrottle {
    /// 指定节流窗口。
    pub fn new(throttle_ms: u64) -> Self {
        Self {
            throttle_ms,
            last_write_ms: None,
        }
    }

    /// 是否到了可以写的时刻。
    pub fn is_due(&self, now_ms: u64) -> bool {
        match self.last_write_ms {
            None => true,
            Some(last) => now_ms.saturating_sub(last) >= self.throttle_ms,
        }
    }

    /// 到期才写；返回是否真的写了。写失败不更新节流窗口，下次还会重试。
    pub fn flush_if_due(
        &mut self,
        store: &StateStore,
        state: &StateFile,
        now_ms: u64,
    ) -> std::io::Result<bool> {
        if !self.is_due(now_ms) {
            return Ok(false);
        }
        store.save(state)?;
        self.last_write_ms = Some(now_ms);
        Ok(true)
    }

    /// 强制 flush：退出、隐藏到托盘、状态退化时使用。
    pub fn flush_now(
        &mut self,
        store: &StateStore,
        state: &StateFile,
        now_ms: u64,
    ) -> std::io::Result<()> {
        store.save(state)?;
        self.last_write_ms = Some(now_ms);
        Ok(())
    }
}

impl Default for StateThrottle {
    fn default() -> Self {
        Self::new(STATE_THROTTLE_MS)
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

    fn store() -> (StateStore, PathBuf) {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("voltnest-state-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        (StateStore::new(DataPaths::with_root(&dir)), dir)
    }

    fn entry(id: &str, seen: u64) -> DeviceStateEntry {
        DeviceStateEntry {
            stable_id: id.to_string(),
            transport: "hid".into(),
            display_name: "AJ159 NL".into(),
            model_name_raw: "AJ159 NL".into(),
            last_known_battery: Some(90),
            last_known_at_ms: Some(seen),
            last_seen_at_ms: Some(seen),
            ..DeviceStateEntry::default()
        }
    }

    #[test]
    fn delete_state_file_and_everything_still_works() {
        // X-5：写成自动化测试，不靠人工记得。
        let (s, dir) = store();
        let mut state = StateFile::default();
        state.upsert(entry("hid:7768:6426", T));
        s.save(&state).unwrap();
        assert!(dir.join("state.json").exists());

        fs::remove_file(dir.join("state.json")).unwrap();
        let loaded = s.load(T);
        assert!(loaded.devices.is_empty(), "删掉 state.json 只应失去回填");
        // 删掉之后还能继续写。
        s.save(&loaded).unwrap();
        assert!(dir.join("state.json").exists());
    }

    #[test]
    fn round_trip_keeps_battery_and_notifications() {
        let (s, _dir) = store();
        let mut state = StateFile::default();
        let mut e = entry("hid:7768:6426", T);
        e.notifications.push(NotificationStateEntry {
            threshold: 20,
            armed: false,
            last_notified_at_ms: Some(T - 1_000),
            last_result: Some("sent".into()),
        });
        state.upsert(e);
        s.save(&state).unwrap();
        let loaded = s.load(T);
        let got = loaded.get("hid:7768:6426").unwrap();
        assert_eq!(got.last_known_battery, Some(90));
        assert_eq!(got.notifications.len(), 1);
        assert!(!got.notifications[0].armed);
    }

    #[test]
    fn writes_schema_version_and_preserves_unknown_fields() {
        let (s, dir) = store();
        fs::write(
            dir.join("state.json"),
            r#"{"schemaVersion":1,"futureTop":true,"devices":[{"stableId":"hid:7768:6426","displayName":"AJ159 NL","futureDevice":9}]}"#,
        )
        .unwrap();
        let state = s.load(T);
        assert_eq!(state.extra.get("futureTop"), Some(&Value::Bool(true)));
        assert_eq!(
            state.devices[0].extra.get("futureDevice"),
            Some(&Value::from(9))
        );
        s.save(&state).unwrap();
        let text = fs::read_to_string(dir.join("state.json")).unwrap();
        assert!(text.contains("futureTop"));
        assert!(text.contains("futureDevice"));
        assert!(text.contains("\"schemaVersion\": 1"));
    }

    #[test]
    fn corrupt_state_is_discarded_without_blocking() {
        let (s, dir) = store();
        fs::write(dir.join("state.json"), b"\0\0\0").unwrap();
        let loaded = s.load(T);
        assert!(loaded.devices.is_empty());
        assert!(!dir.join("state.json").exists());
        assert!(dir.join(format!("state.corrupt-{T}.json")).exists());
    }

    #[test]
    fn prunes_to_limit_keeping_most_recent() {
        // X-7 / Y-5：地址轮换产生的 orphan 靠 32 条上限淘汰。
        let mut state = StateFile::default();
        for i in 0..40u64 {
            state.upsert(entry(&format!("ble:00:00:00:00:00:{i:02x}"), T + i));
        }
        state.prune(MAX_INDEXED_ENTRIES);
        assert_eq!(state.devices.len(), 32);
        assert!(
            state.get("ble:00:00:00:00:00:27").is_some(),
            "最近的必须保留"
        );
        assert!(
            state.get("ble:00:00:00:00:00:00").is_none(),
            "最旧的必须淘汰"
        );
        // 输出稳定有序，避免文件内容抖动。
        let mut sorted = state.devices.clone();
        sorted.sort_by(|a, b| a.stable_id.cmp(&b.stable_id));
        assert_eq!(sorted, state.devices);
    }

    #[test]
    fn upsert_replaces_same_id() {
        let mut state = StateFile::default();
        state.upsert(entry("hid:7768:6426", T));
        let mut newer = entry("hid:7768:6426", T + 5);
        newer.last_known_battery = Some(80);
        state.upsert(newer);
        assert_eq!(state.devices.len(), 1);
        assert_eq!(state.devices[0].last_known_battery, Some(80));
    }

    #[test]
    fn throttle_allows_first_write_then_waits_five_seconds() {
        let (store, dir) = store();
        let mut throttle = StateThrottle::default();
        let state = StateFile::default();

        assert!(
            throttle.flush_if_due(&store, &state, T).unwrap(),
            "首次必须写"
        );
        assert!(!throttle.flush_if_due(&store, &state, T + 4_999).unwrap());
        assert!(
            !throttle
                .flush_if_due(&store, &state, T + 5_000 - 1)
                .unwrap()
        );
        assert!(
            throttle.flush_if_due(&store, &state, T + 5_000).unwrap(),
            "恰好 5s 放行"
        );
        assert!(dir.join("state.json").exists());
    }

    #[test]
    fn throttle_force_flush_ignores_window() {
        let (store, dir) = store();
        let mut throttle = StateThrottle::default();
        let state = StateFile::default();
        throttle.flush_if_due(&store, &state, T).unwrap();
        fs::remove_file(dir.join("state.json")).unwrap();
        throttle.flush_now(&store, &state, T + 1).unwrap();
        assert!(
            dir.join("state.json").exists(),
            "强制 flush 必须无视节流窗口"
        );
    }

    #[test]
    fn throttle_survives_clock_backward() {
        let (store, _dir) = store();
        let mut throttle = StateThrottle::default();
        let state = StateFile::default();
        throttle.flush_if_due(&store, &state, T).unwrap();
        // 时间后跳：saturating_sub 得 0，不得因此 panic 或疯狂写盘。
        assert!(!throttle.is_due(T - 1_000_000));
    }
}
