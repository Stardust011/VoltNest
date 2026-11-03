//! 用户对呈现层的改动：自定义名称、删除列表、合并组。
//!
//! 只影响快照的呈现，不改设备记录与状态机；由宿主从 `settings.json` 灌入。

use std::collections::BTreeMap;

/// 一个用户合并组。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ManagedMerge {
    /// 主设备 `stableId`（卡片 ID 由它派生）。
    pub primary: String,
    /// 全部成员（含主设备）。
    pub members: Vec<String>,
    /// 卡片名称。
    pub name: String,
}

/// 设备管理的呈现配置。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ManagedDevices {
    /// `stableId` → 自定义名称。
    pub aliases: BTreeMap<String, String>,
    /// 用户删除的设备。
    pub removed: Vec<String>,
    /// 用户合并组。
    pub merges: Vec<ManagedMerge>,
}

impl ManagedDevices {
    /// 自定义名称（空白视为未设置）。
    pub fn alias(&self, stable_id: &str) -> Option<&str> {
        self.aliases
            .get(stable_id)
            .map(String::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }

    /// 是否被用户删除。
    pub fn is_removed(&self, stable_id: &str) -> bool {
        self.removed.iter().any(|id| id == stable_id)
    }

    /// 某设备所在的合并组。
    pub fn group_of(&self, stable_id: &str) -> Option<&ManagedMerge> {
        self.merges
            .iter()
            .find(|group| group.members.iter().any(|id| id == stable_id))
    }
}
