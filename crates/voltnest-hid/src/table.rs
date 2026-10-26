//! 「VID:PID → 型号」快照表（品牌无关的加载器）。
//!
//! 每品牌一份 JSON 快照，由 [`crate::brands::BrandSpec::device_table_json`] 编译期嵌入（`include_str!`），安装目录无需带数据文件，也不会与二进制版本分叉。新增品牌只加数据文件 + 注册项，本文件不变。
//!
//! # 统一数据规范
//!
//! 全部品牌共用同一结构；`name` / `vendorDeviceId` 可缺省，其余字段（如 `internalModel`、`channels`、`ambiguous`）按原样保留在 [`Entry::extra`]：
//!
//! ```json
//! {
//!   "config_version": "1.0.0",
//!   "source": { "file": "config.xml", "software": "AJAZZ Driver（W1）", "software_version": "1.0.7.6" },
//!   "devices": {
//!     "7768:6426": { "name": "AJ159 NL", "vendorDeviceId": 102, "deviceType": "mouse", "mode": "2.4G" }
//!   }
//! }
//! ```
//!
//! - `devices` 的键是 `VID:PID`（大端十六进制、无 `0x` 前缀、冒号分隔），值是唯一的型号条目来源；不再有并行的 `by_vid_pid` 副本。
//! - `source` 可用 `{url, web_version}` 替代 `{software, software_version}`（网页版驱动抓取的映射表）。
//! - `vendorDeviceId` 是厂商驱动内的设备编号（如 AJAZZ 的 101..108），**不是**设备类型，禁止映射成 [`DeviceType`]。
//! - `deviceType` 是厂商声明的类型词（`mouse` / `keyboard` …），只有恰好等于规范类型词时才被采纳，见 [`Entry::declared_device_type`]。
//!
//! 匹配必须**精确**：`0x6425 = AJ159 NL P`、`0x6426 = AJ159 NL`，只差一位，任何前缀/区间匹配都会搞错型号。

use serde::Deserialize;
use std::collections::HashMap;
use voltnest_provider::DeviceType;

/// 当前数据规范版本（诊断展示用；解析时不强制相等）。
pub const CONFIG_VERSION: &str = "1.0.0";

/// 一个 VID:PID 对应的型号信息。
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// 型号名，例如 `AJ159 NL`。作为 `display_name` 与 `model_name_raw` 的来源。缺失表示需靠 ProductString 消歧的共用 PID。
    #[serde(default)]
    pub name: Option<String>,
    /// 厂商驱动内部的设备编号（AJAZZ 为 101..108）。**不是**设备类型。网页版驱动无此字段。
    #[serde(default)]
    pub vendor_device_id: Option<u16>,
    /// 厂商声明的设备类型词，例如 `mouse`。是否为规范类型词由 [`Entry::declared_device_type`] 判定。
    #[serde(default)]
    pub device_type: String,
    /// `USB` / `2.4G`。
    #[serde(default)]
    pub mode: String,
    /// 未建模的附加字段（`internalModel`、`channels`、`candidates` …），原样保留。
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Entry {
    /// 厂商驱动配置里的设备类型词 → [`DeviceType`]。`deviceType` 是厂商自定义字段而非标准：只有恰好等于规范类型词才映射，其余一律 `None`——把型号族名当类型猜正是规范禁止的「厂商自定义数据当标准」。另 `vendorDeviceId`（101..108）是驱动内部型号编号，非类型，禁止映射。
    pub fn declared_device_type(&self) -> Option<DeviceType> {
        match self.device_type.trim().to_ascii_lowercase().as_str() {
            "mouse" => Some(DeviceType::Mouse),
            "keyboard" => Some(DeviceType::Keyboard),
            "gamepad" => Some(DeviceType::Gamepad),
            "headset" => Some(DeviceType::Headset),
            "headphone" | "headphones" => Some(DeviceType::Headphones),
            "speaker" => Some(DeviceType::Speaker),
            "controller" => Some(DeviceType::Controller),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct RawMap {
    config_version: String,
    source: RawSource,
    devices: HashMap<String, Entry>,
}

#[derive(Default, Deserialize)]
struct RawSource {
    #[serde(default)]
    file: String,
    #[serde(default)]
    software: String,
    #[serde(default)]
    software_version: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    web_version: String,
}

impl RawSource {
    /// 统一成「来源文件 / 来源软件 / 版本」三段，桌面驱动与网页驱动两种写法都归一到这里。
    fn normalized(self) -> (String, String, String) {
        let file = if self.file.trim().is_empty() {
            self.url.clone()
        } else {
            self.file
        };
        let software = if self.software.trim().is_empty() {
            self.url
        } else {
            self.software
        };
        let version = if self.software_version.trim().is_empty() {
            self.web_version
        } else {
            self.software_version
        };
        (file, software, version)
    }
}

/// 一个品牌编译期嵌入的型号表。
#[derive(Debug)]
pub struct DeviceTable {
    map: HashMap<(u16, u16), Entry>,
    /// 数据规范版本（诊断页展示）。
    pub config_version: String,
    /// 数据来源文件（诊断页展示）。
    pub source_file: String,
    /// 数据来源软件（诊断页展示）。
    pub source_software: String,
    /// 官方驱动版本（诊断页展示）。
    pub source_version: String,
}

fn parse_hex_u16(s: &str) -> Option<u16> {
    let t = s.trim();
    let t = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    u16::from_str_radix(t, 16).ok()
}

impl DeviceTable {
    /// 解析 JSON。键格式：`"7768:6426"`。
    pub fn parse(json: &str) -> Result<Self, String> {
        let raw: RawMap =
            serde_json::from_str(json).map_err(|e| format!("型号表 JSON 解析失败: {e}"))?;
        if raw.config_version.trim().is_empty() {
            return Err("型号表缺少 config_version".to_string());
        }
        let mut map = HashMap::with_capacity(raw.devices.len());
        for (key, entry) in raw.devices {
            let (vid, pid) = key
                .split_once(':')
                .ok_or_else(|| format!("型号表键格式非法: {key}"))?;
            let vid = parse_hex_u16(vid).ok_or_else(|| format!("VID 非法: {key}"))?;
            let pid = parse_hex_u16(pid).ok_or_else(|| format!("PID 非法: {key}"))?;
            map.insert((vid, pid), entry);
        }
        if map.is_empty() {
            return Err("型号表为空".to_string());
        }
        let (source_file, source_software, source_version) = raw.source.normalized();
        Ok(Self {
            map,
            config_version: raw.config_version,
            source_file,
            source_software,
            source_version,
        })
    }

    /// 精确匹配。**禁止**前缀 / 区间匹配。
    pub fn lookup(&self, vid: u16, pid: u16) -> Option<&Entry> {
        self.map.get(&(vid, pid))
    }

    /// 型号名；查不到或该条目无名称时返回 `None`（宿主会回落到 ProductString 并让 `model_key = None`）。
    pub fn name(&self, vid: u16, pid: u16) -> Option<&str> {
        self.lookup(vid, pid)
            .and_then(|e| e.name.as_deref())
            .map(str::trim)
            .filter(|n| !n.is_empty())
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 是否为空（`len()` 的配套方法）。
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// 表内全部条目，按 VID/PID 排序。
    pub fn entries(&self) -> Vec<((u16, u16), &Entry)> {
        let mut v: Vec<_> = self.map.iter().map(|(k, e)| (*k, e)).collect();
        v.sort_by_key(|(k, _)| (k.0, k.1));
        v
    }

    /// 该 VID 是否属于本表覆盖的代际。
    pub fn known_vid(&self, vid: u16) -> bool {
        self.map.keys().any(|(v, _)| *v == vid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
        "config_version": "1.0.0",
        "source": { "file": "config.xml", "software": "Driver", "software_version": "9.9.9" },
        "devices": {
            "7768:6426": { "name": "M1", "vendorDeviceId": 1, "deviceType": "mouse", "mode": "2.4G" },
            "7768:6D26": { "name": "M1", "vendorDeviceId": 1, "deviceType": "mouse", "mode": "USB" }
        }
    }"#;

    #[test]
    fn parses_and_matches_exactly() {
        let t = DeviceTable::parse(JSON).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t.config_version, "1.0.0");
        assert_eq!(t.source_file, "config.xml");
        assert_eq!(t.source_software, "Driver");
        assert_eq!(t.source_version, "9.9.9");
        assert_eq!(t.name(0x7768, 0x6426), Some("M1"));
        assert_eq!(t.lookup(0x7768, 0x6426).unwrap().mode, "2.4G");
        assert_eq!(t.lookup(0x7768, 0x6D26).unwrap().mode, "USB");
        assert_eq!(t.name(0x7768, 0x6427), None, "近似 PID 必须 miss");
        assert!(t.known_vid(0x7768));
        assert!(!t.known_vid(0x249A));
    }

    #[test]
    fn entries_are_sorted() {
        let table = DeviceTable::parse(JSON).unwrap();
        let entries = table.entries();
        assert_eq!(entries[0].0, (0x7768, 0x6426));
        assert_eq!(entries[1].0, (0x7768, 0x6D26));
    }

    #[test]
    fn parse_rejects_bad_keys_and_empty_tables() {
        assert!(
            DeviceTable::parse(
                r#"{"config_version":"1.0.0","source":{"file":"x","software":"y","software_version":"1"},"devices":{}}"#
            )
            .is_err()
        );
        assert!(
            DeviceTable::parse(
                r#"{"config_version":"1.0.0","source":{"file":"x","software":"y","software_version":"1"},"devices":{"7768":{}}}"#
            )
            .is_err()
        );
        assert!(
            DeviceTable::parse(
                r#"{"config_version":"1.0.0","source":{"file":"x","software":"y","software_version":"1"},"devices":{"zz:6426":{"name":"A","vendorDeviceId":1,"deviceType":"mouse","mode":"2.4G"}}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn parse_accepts_any_config_version() {
        let json = JSON.replace("\"1.0.0\"", "\"v3.16.35\"");
        assert_eq!(
            DeviceTable::parse(&json).unwrap().config_version,
            "v3.16.35"
        );
    }

    /// 网页版驱动抓来的表：无 `vendorDeviceId`、无名称、带大量附加字段、`source` 用 `url` + `web_version`。
    #[test]
    fn parse_tolerates_web_driver_schema() {
        let web = r#"{
            "config_version": "v3.16.35",
            "source": { "file": "", "url": "https://hub.rapoo.cn/", "web_version": "3ShGzFuu" },
            "devices": {
                "3837:1015": { "name": null, "deviceType": "keyboard", "mode": "2.4G", "ambiguous": true,
                               "candidates": [{ "name": "G98 V3", "receiverProductName": "MCHOSE G98 V3 2.4G" }] },
                "24AE:1410": { "name": "VT0", "deviceType": "mouse", "mode": "2.4G", "channels": ["global"] }
            }
        }"#;
        let t = DeviceTable::parse(web).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t.source_file, "https://hub.rapoo.cn/");
        assert_eq!(t.source_version, "3ShGzFuu");
        assert_eq!(t.name(0x24AE, 0x1410), Some("VT0"));
        assert_eq!(t.name(0x3837, 0x1015), None, "无名称的条目不得给出空型号名");
        assert!(t.lookup(0x3837, 0x1015).unwrap().vendor_device_id.is_none());
        assert!(
            t.lookup(0x3837, 0x1015)
                .unwrap()
                .extra
                .contains_key("candidates")
        );
    }

    #[test]
    fn parse_rejects_missing_config_version() {
        let json = JSON.replace("\"config_version\": \"1.0.0\",", "");
        assert!(DeviceTable::parse(&json).is_err());
    }

    /// 旧结构（`source_file` + `software.version` + `by_vid_pid`）必须解析失败，而不是被静默接受。
    #[test]
    fn parse_rejects_legacy_schema() {
        let legacy = r#"{
            "source_file": "config.xml",
            "software": { "version": "1.0.7.6" },
            "by_vid_pid": {
                "7768:6426": { "name": "M1", "device_type": 1, "mode": "2.4G", "hid_interface": "VID_7768&PID_6426&MI_01", "device_attr": "mouse" }
            }
        }"#;
        assert!(DeviceTable::parse(legacy).is_err());
    }

    #[test]
    fn declared_device_type_accepts_canonical_words_only() {
        let mut e = Entry {
            name: Some("X".into()),
            vendor_device_id: Some(101),
            device_type: " Mouse ".into(),
            mode: "2.4G".into(),
            extra: HashMap::new(),
        };
        assert_eq!(e.declared_device_type(), Some(DeviceType::Mouse));

        // 实测的旧配置里 AJ179 / AJ139 的 `device_attr` 是型号族名而不是类型，这类值不得被猜成类型。
        for attr in [
            "aj179",
            "aj179v2",
            "aj139",
            "",
            "mx-master",
            "mouse+keyboard",
        ] {
            e.device_type = attr.to_string();
            assert_eq!(e.declared_device_type(), None, "attr={attr} 不得猜成类型");
        }
    }

    #[test]
    fn vendor_device_id_is_not_a_type() {
        // 101..108 是驱动内部型号编号；即便有人把它当类型，这里也不映射。
        let e = Entry {
            name: Some("AJ159 NL".into()),
            vendor_device_id: Some(102),
            device_type: "mouse".into(),
            mode: "2.4G".into(),
            extra: HashMap::new(),
        };
        assert_eq!(e.declared_device_type(), Some(DeviceType::Mouse));
        assert_eq!(e.vendor_device_id, Some(102));
    }
}
