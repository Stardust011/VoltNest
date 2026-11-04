//! 用户层呈现：改名、删除、手动合并、分离与重名消歧。

mod common;

use common::*;
use voltnest_core::{DeviceState, Engine, ManagedDevices, ManagedMerge};
use voltnest_provider::{ProviderEvent, Transport};

fn ble_arrived_alpha() -> ProviderEvent {
    ProviderEvent::Arrived(voltnest_provider::testing::ble_fact("Alpha"))
}

/// 两台不同型号的设备各自在线：不会被自动分组抢走。
fn two_devices() -> Engine {
    let mut e = engine();
    feed(&mut e, &ble_arrived_alpha(), Transport::Ble, T0);
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &ble_reading(55), Transport::Ble, T0 + 10);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0 + 20);
    e
}

fn managed(
    aliases: &[(&str, &str)],
    removed: &[&str],
    merges: Vec<ManagedMerge>,
) -> ManagedDevices {
    ManagedDevices {
        aliases: aliases
            .iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect(),
        removed: removed.iter().map(|id| id.to_string()).collect(),
        merges,
        separated: Vec::new(),
    }
}

#[test]
fn removed_device_leaves_devices_cards_and_tray() {
    let mut e = two_devices();
    let before = e.snapshot(T0 + 30);
    assert_eq!(before.devices.len(), 2);
    assert_eq!(before.tray.present, 2);

    e.set_managed(managed(&[], &[hid_id().as_str()], vec![]));
    let after = e.snapshot(T0 + 31);
    assert_eq!(after.devices.len(), 1, "删除的设备不再出现在列表里");
    assert_eq!(after.devices[0].stable_id, ble_id().as_str());
    assert_eq!(after.tray.present, 1, "托盘与列表同一口径");
    assert_eq!(after.tray.visible_device_ids.len(), 1);
}

#[test]
fn alias_renames_and_reorders_devices() {
    let mut e = two_devices();
    e.set_managed(managed(&[("hid:7768:6426", "Aaa 鼠标")], &[], vec![]));
    let snap = e.snapshot(T0 + 30);
    assert_eq!(
        snap.devices[0].stable_id,
        hid_id().as_str(),
        "改名参与排序（tray.order 为空时按名称升序）"
    );
    assert_eq!(snap.devices[0].display_name, "Aaa 鼠标");
}

#[test]
fn user_merge_makes_one_card_and_one_tray_unit() {
    let mut e = two_devices();
    e.set_managed(managed(
        &[],
        &[],
        vec![ManagedMerge {
            primary: hid_id().as_str().to_string(),
            members: vec![hid_id().as_str().to_string(), ble_id().as_str().to_string()],
            name: "桌面一套".to_string(),
        }],
    ));
    let snap = e.snapshot(T0 + 30);
    assert_eq!(snap.devices.len(), 2, "记录本身不合并");
    assert_eq!(snap.cards.len(), 1);
    let card = &snap.cards[0];
    assert!(card.user_merged);
    assert_eq!(card.card_id, format!("user:{}", hid_id().as_str()));
    assert_eq!(card.display_name, "桌面一套");
    assert_eq!(card.primary_device_id, hid_id().as_str());
    assert_eq!(card.battery, Some(90));
    assert!(card.inactive_member_text.contains("在线"), "{card:?}");
    assert_eq!(snap.tray.present, 1, "合并后托盘只算一台");
    assert_eq!(snap.tray.visible_device_ids, vec![hid_id().as_str()]);
}

#[test]
fn merge_primary_falls_back_to_active_member() {
    let mut e = two_devices();
    // 主设备掉线：卡片改用另一台的读数，而不是显示一台离线设备的 0。
    feed(
        &mut e,
        &ProviderEvent::LinkLost {
            stable_id: hid_id(),
            code: voltnest_provider::ErrorCode::DeviceGone,
        },
        Transport::Hid,
        T0 + 40,
    );
    e.set_managed(managed(
        &[],
        &[],
        vec![ManagedMerge {
            primary: hid_id().as_str().to_string(),
            members: vec![hid_id().as_str().to_string(), ble_id().as_str().to_string()],
            name: "桌面一套".to_string(),
        }],
    ));
    let snap = e.snapshot(T0 + 41);
    assert_eq!(snap.cards[0].primary_device_id, ble_id().as_str());
    assert_eq!(snap.cards[0].battery, Some(55));
}

#[test]
fn separating_merge_restores_two_rows() {
    let mut e = two_devices();
    e.set_managed(managed(
        &[],
        &[],
        vec![ManagedMerge {
            primary: hid_id().as_str().to_string(),
            members: vec![hid_id().as_str().to_string(), ble_id().as_str().to_string()],
            name: "桌面一套".to_string(),
        }],
    ));
    assert_eq!(e.snapshot(T0 + 30).cards.len(), 1);

    e.set_managed(managed(&[], &[], vec![]));
    let snap = e.snapshot(T0 + 31);
    assert!(snap.cards.is_empty(), "分离后不再有合并卡");
    assert_eq!(snap.tray.present, 2);
}

#[test]
fn same_name_devices_get_transport_suffix() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    let snap = e.snapshot(T0 + 5);
    assert_eq!(snap.devices.len(), 2);
    let names: Vec<&str> = snap
        .devices
        .iter()
        .map(|d| d.display_name.as_str())
        .collect();
    assert!(names.contains(&"AJ159 NL · 2.4G"), "{names:?}");
    assert!(names.contains(&"AJ159 NL · 蓝牙"), "{names:?}");
}

#[test]
fn alias_removes_the_duplicate_name() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    e.set_managed(managed(&[("hid:7768:6426", "独有的名字")], &[], vec![]));
    let snap = e.snapshot(T0 + 5);
    assert!(
        snap.devices.iter().any(|d| d.display_name == "独有的名字"),
        "改名后不再重名，不应追加链路后缀"
    );
    assert!(
        snap.devices.iter().any(|d| d.display_name == "AJ159 NL"),
        "另一台保留原名"
    );
}

#[test]
fn incomplete_merge_group_is_ignored() {
    let mut e = two_devices();
    e.set_managed(managed(
        &[],
        &[],
        vec![ManagedMerge {
            primary: hid_id().as_str().to_string(),
            members: vec![hid_id().as_str().to_string()],
            name: "只剩一台".to_string(),
        }],
    ));
    let snap = e.snapshot(T0 + 30);
    assert!(snap.cards.is_empty(), "成员不足两台时不出卡");
    assert_eq!(snap.devices.len(), 2);
}

#[test]
fn register_creates_placeholder_without_tray_icon() {
    let mut e = engine();
    let id = hid_id();
    assert!(e.register(&id, "手动添加", T0));
    assert!(!e.register(&id, "重复添加", T0 + 1), "重复登记必须被拒绝");

    let snap = e.snapshot(T0 + 2);
    assert_eq!(snap.devices.len(), 1);
    assert_eq!(snap.devices[0].display_name, "手动添加");
    assert_eq!(snap.devices[0].state, DeviceState::Unknown);
    assert!(
        snap.tray.visible_device_ids.is_empty(),
        "还没采到读数的手动设备不占托盘图标"
    );
}

#[test]
fn same_name_same_transport_gets_sequence_suffix() {
    let mut e = engine();
    let a = voltnest_provider::StableId::ble([0x01, 0, 0, 0, 0, 0x01]);
    let b = voltnest_provider::StableId::ble([0x01, 0, 0, 0, 0, 0x02]);
    for id in [&a, &b] {
        let fact = voltnest_provider::testing::ble_fact("同名耳机");
        let mut fact = fact;
        fact.stable_id = id.clone();
        feed(&mut e, &ProviderEvent::Arrived(fact), Transport::Ble, T0);
    }
    let snap = e.snapshot(T0 + 3);
    assert_eq!(snap.devices.len(), 2);
    let names: Vec<&str> = snap
        .devices
        .iter()
        .map(|d| d.display_name.as_str())
        .collect();
    assert!(names.contains(&"同名耳机 · 蓝牙"), "{names:?}");
    assert!(names.contains(&"同名耳机 · 蓝牙 (2)"), "{names:?}");
}

/// HID 活跃、BLE 只登记：构成一张自动合并卡。
fn auto_merged() -> Engine {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0 + 10);
    e
}

#[test]
fn separating_auto_merge_restores_two_rows() {
    let mut e = auto_merged();
    let before = e.snapshot(T0 + 20);
    assert_eq!(before.cards.len(), 1, "同型号一活跃一非活跃应自动合并");
    let card_id = before.cards[0].card_id.clone();
    assert!(!before.cards[0].user_merged);

    e.set_managed(ManagedDevices {
        separated: vec![card_id],
        ..managed(&[], &[], vec![])
    });

    let after = e.snapshot(T0 + 21);
    assert!(after.cards.is_empty(), "分离后不再自动并卡");
    assert_eq!(after.devices.len(), 2, "两台设备各自成行");
    assert_eq!(after.tray.present, 2, "托盘按呈现单元计数");
}

#[test]
fn auto_merge_anchor_is_the_hid_side_member() {
    let mut e = auto_merged();
    let snap = e.snapshot(T0 + 20);
    let card = &snap.cards[0];
    assert_eq!(
        card.anchor_id,
        hid_id().as_str(),
        "自动合并的锚点恒取 HID 那一侧，不随活跃度翻转"
    );
    assert!(
        card.member_device_ids.contains(&card.anchor_id),
        "锚点必须是本卡成员"
    );
}

#[test]
fn user_merge_anchor_ignores_which_member_is_active() {
    // HID 只登记、没有读数（不活跃），BLE 有读数（活跃）。
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    feed(&mut e, &ble_reading(55), Transport::Ble, T0 + 10);
    e.set_managed(managed(
        &[],
        &[],
        vec![ManagedMerge {
            primary: hid_id().as_str().to_string(),
            members: vec![hid_id().as_str().to_string(), ble_id().as_str().to_string()],
            name: "合并组".into(),
        }],
    ));

    let snap = e.snapshot(T0 + 20);
    let card = &snap.cards[0];
    assert!(card.user_merged);
    assert_eq!(
        card.primary_device_id,
        ble_id().as_str(),
        "主成员让位给活跃成员"
    );
    assert_eq!(
        card.anchor_id,
        hid_id().as_str(),
        "锚点是配置主设备，不跟着活跃度翻，否则悬浮窗配置会丢"
    );
}
