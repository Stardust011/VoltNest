//! 前端 DTO：一次全量快照。
//!
//! 不含 UI 状态；命名、分组、排序全由 Rust 决定。时间统一 UTC 毫秒，`battery == null` 表示无有效读数。
//! `rev` 仅在进程内单调递增，重启归零，不持久化。

use crate::state::DeviceState;
use serde::{Deserialize, Serialize};

/// 一次全量快照。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// 进程内单调递增版本号。
    pub rev: u64,
    /// 生成快照时的宿主 UTC 毫秒。
    pub server_now_ms: u64,
    /// 设备记录（已按 `settings.tray.order` 排序）。
    pub devices: Vec<DeviceDto>,
    /// 呈现分组卡片：仅满足分组谓词的两条记录才出现。未分组设备由前端从 `devices` 中取未被引用的成员。
    pub cards: Vec<CardDto>,
    /// 托盘可见集合。
    pub tray: TraySnapshotDto,
}

/// 单台设备。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    /// `hid:7768:6426` / `ble:01:00:00:23:04:35`。
    pub stable_id: String,
    /// `hid` / `ble`。
    pub transport: String,
    /// 协议家族 ID，值取自 `DeviceRecord::family_id`；字段名固定为 `provider`（wire 契约）。
    pub provider: String,
    /// 型号名（Provider 表查得 / GAP 原文）。
    pub display_name: String,
    /// 归一化型号键，参与呈现分组。`null` = 未命中型号表。
    pub model_key: Option<String>,
    /// 设备类型（应用层枚举小写，如 `mouse` / `keyboard`）；`unknown` 表示证据不足。
    pub device_type: String,
    /// 得出 `deviceType` 的方法（`hid_usage` / `ble_appearance` / `verified_device_mapping` / `unknown`），仅用于溯源。
    pub classification_method: String,
    /// 当前电量。`null` = 当前无有效读数。
    pub battery: Option<u8>,
    /// 上次已知电量，仅副行展示，不参与状态与通知。
    pub last_known_battery: Option<u8>,
    /// 上次已知电量的时间。
    pub last_known_at_ms: Option<u64>,
    /// 充电状态。`null` = 未知（BLE 恒为 `null`，界面显示 Unknown）。
    pub is_charging: Option<bool>,
    /// 五态。
    pub state: DeviceState,
    /// 最后一次有效读数时间；退化判定的唯一依据。
    pub last_valid_at_ms: Option<u64>,
    /// 最近一次结构化错误码。
    pub last_error: Option<String>,
}

/// 合并卡（仅在三个条件同时满足时产生）。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardDto {
    /// 卡片 ID（= `modelKey`）。
    pub card_id: String,
    /// 活跃成员（提供主电量）。
    pub primary_device_id: String,
    /// 全部成员。
    pub member_device_ids: Vec<String>,
    /// 主电量。
    pub battery: Option<u8>,
    /// 主成员充电状态。
    pub is_charging: Option<bool>,
    /// 主成员状态。
    pub state: DeviceState,
    /// 非活跃成员的说明行，例如「2.4G 链路 · 未活动（12 分钟前）」。
    pub inactive_member_text: String,
}

/// 托盘可见集合与顺序。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraySnapshotDto {
    /// 应存在的托盘图标（按 `settings.tray.order`，上限 4）。
    pub visible_device_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_serializes_camel_case_and_null_battery() {
        let snap = Snapshot {
            rev: 42,
            server_now_ms: 1_792_631_234_567,
            devices: vec![DeviceDto {
                stable_id: "hid:7768:6426".into(),
                transport: "hid".into(),
                provider: "hid-vbas".into(),
                display_name: "AJ159 NL".into(),
                model_key: Some("aj159 nl".into()),
                device_type: "mouse".into(),
                classification_method: "verified_device_mapping".into(),
                battery: None,
                last_known_battery: Some(90),
                last_known_at_ms: Some(1_792_631_200_000),
                is_charging: Some(true),
                state: DeviceState::Unknown,
                last_valid_at_ms: None,
                last_error: Some("malformed_frame".into()),
            }],
            cards: vec![],
            tray: TraySnapshotDto {
                visible_device_ids: vec![],
            },
        };
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"serverNowMs\":1792631234567"));
        assert!(json.contains("\"stableId\":\"hid:7768:6426\""));
        assert!(
            json.contains("\"battery\":null"),
            "无读数必须是 null，不能是 0：{json}"
        );
        assert!(json.contains("\"lastKnownBattery\":90"));
        assert!(json.contains("\"isCharging\":true"));
        assert!(json.contains("\"deviceType\":\"mouse\""));
        assert!(json.contains("\"classificationMethod\":\"verified_device_mapping\""));
        assert!(json.contains("\"state\":\"unknown\""));
        assert!(json.contains("\"inactiveMemberText\"") || json.contains("\"cards\":[]"));
    }

    #[test]
    fn round_trip_is_lossless() {
        let snap = Snapshot {
            rev: 1,
            server_now_ms: 10,
            devices: vec![],
            cards: vec![CardDto {
                card_id: "aj159 nl".into(),
                primary_device_id: "ble:01:00:00:23:04:35".into(),
                member_device_ids: vec!["ble:01:00:00:23:04:35".into(), "hid:7768:6426".into()],
                battery: None,
                is_charging: None,
                state: DeviceState::Stale,
                inactive_member_text: "2.4G 链路 · 未活动（12 分钟前）".into(),
            }],
            tray: TraySnapshotDto {
                visible_device_ids: vec!["hid:7768:6426".into()],
            },
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, snap);
    }
}
