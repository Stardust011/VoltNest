//! 「VID:PID → 型号」快照表（品牌无关的加载器）。
//!
//! 每个品牌一份 JSON 快照，由 [`crate::brands::BrandSpec::device_table_json`]
//! 编译期嵌入（`include_str!`）：安装目录不需要携带数据文件，也不会与二进制
//! 版本分叉（ADR-0010 D7）。新增品牌只新增一份数据文件 + 一个注册项，
//! 本文件无需改动（ADR-0011）。
//!
//! 匹配必须**精确**：`0x6425 = AJ159 NL P`、`0x6426 = AJ159 NL`，只差一位，
//! 任何前缀 / 区间匹配都会把型号搞错（规格 §2「型号匹配必须使用表驱动的 Profile」）。

use serde::Deserialize;
use std::collections::HashMap;

/// 一个 VID:PID 对应的型号信息。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Entry {
    /// 型号名，例如 `AJ159 NL`。作为 `display_name` 与 `model_name_raw` 的来源。
    pub name: String,
    /// 驱动内部的 device type。
    pub device_type: u16,
    /// `USB` / `2.4G`。
    pub mode: String,
    /// 例如 `VID_7768&PID_6426&MI_01`。
    pub hid_interface: String,
    /// `mouse` 等。
    pub device_attr: String,
}

impl Entry {
    /// 从 `hid_interface` 取接口号：`MI_01` → `1`。
    ///
    /// 集合选择顺序是「VID/PID 精确 → 表内 MI_xx → UsagePage」，
    /// 三者缺一不可（`docs/hid-rust-verification.md` §集合选择）。
    pub fn interface_index(&self) -> Option<u8> {
        let tail = self.hid_interface.rsplit("MI_").next()?;
        u8::from_str_radix(tail, 16).ok()
    }
}

#[derive(Deserialize)]
struct RawMap {
    source_file: String,
    software: RawSoftware,
    by_vid_pid: HashMap<String, Entry>,
}

#[derive(Deserialize)]
struct RawSoftware {
    version: String,
}

/// 一个品牌编译期嵌入的型号表。
#[derive(Debug)]
pub struct DeviceTable {
    map: HashMap<(u16, u16), Entry>,
    /// 数据来源文件名（诊断页展示）。
    pub source_file: String,
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
        let mut map = HashMap::with_capacity(raw.by_vid_pid.len());
        for (key, entry) in raw.by_vid_pid {
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
        Ok(Self {
            map,
            source_file: raw.source_file,
            source_version: raw.software.version,
        })
    }

    /// 精确匹配。**禁止**前缀 / 区间匹配。
    pub fn lookup(&self, vid: u16, pid: u16) -> Option<&Entry> {
        self.map.get(&(vid, pid))
    }

    /// 型号名；查不到返回 `None`（宿主会回落到 ProductString 并让 `model_key = None`）。
    pub fn name(&self, vid: u16, pid: u16) -> Option<&str> {
        self.lookup(vid, pid).map(|e| e.name.as_str())
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
        "source_file": "config.xml",
        "software": { "version": "9.9.9" },
        "by_vid_pid": {
            "7768:6426": { "name": "M1", "device_type": 1, "mode": "2.4G", "hid_interface": "VID_7768&PID_6426&MI_01", "device_attr": "mouse" },
            "7768:6D26": { "name": "M1", "device_type": 1, "mode": "USB",  "hid_interface": "VID_7768&PID_6D26&MI_00", "device_attr": "mouse" }
        }
    }"#;

    #[test]
    fn parses_and_matches_exactly() {
        let t = DeviceTable::parse(JSON).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t.source_file, "config.xml");
        assert_eq!(t.source_version, "9.9.9");
        assert_eq!(t.name(0x7768, 0x6426), Some("M1"));
        assert_eq!(t.lookup(0x7768, 0x6426).unwrap().interface_index(), Some(1));
        assert_eq!(t.lookup(0x7768, 0x6D26).unwrap().interface_index(), Some(0));
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
            DeviceTable::parse(r#"{"source_file":"x","software":{"version":"1"},"by_vid_pid":{}}"#)
                .is_err()
        );
        assert!(
            DeviceTable::parse(
                r#"{"source_file":"x","software":{"version":"1"},"by_vid_pid":{"7768":{}}}"#
            )
            .is_err()
        );
        assert!(
            DeviceTable::parse(
                r#"{"source_file":"x","software":{"version":"1"},"by_vid_pid":{"zz:6426":{"name":"A","device_type":1,"mode":"2.4G","hid_interface":"VID_7768&PID_6426&MI_01","device_attr":"mouse"}}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn interface_index_handles_absent_token() {
        let e = Entry {
            name: "X".into(),
            device_type: 1,
            mode: "USB".into(),
            hid_interface: "VID_7768&PID_6D26".into(),
            device_attr: "mouse".into(),
        };
        // rsplit 找不到 MI_ 时返回整串，hex 解析失败 → None。
        assert_eq!(e.interface_index(), None);
    }
}
