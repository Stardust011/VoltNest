//! 快照、排序、呈现分组与 rev。

mod common;

use common::*;
use voltnest_core::{DeviceState, Engine, EngineConfig};
use voltnest_provider::testing::{ble_fact, hid_fact};
use voltnest_provider::{ProviderEvent, Sensitivity, StableId, Transport};

/// 让 HID 侧变「非活跃」：读数后等过 offline 阈值，再由 tick 推到 Offline。
fn make_hid_inactive(e: &mut Engine, now: u64) {
    common::feed(e, &hid_arrived(), Transport::Hid, now);
    common::feed(e, &hid_reading(90), Transport::Hid, now);
    e.tick(now + 180_001);
}

#[test]
fn rev_increases_monotonically_within_process() {
    let mut e = engine();
    let a = e.snapshot(T0);
    let b = e.snapshot(T0 + 1);
    let c = e.snapshot(T0 + 2);
    assert_eq!((a.rev, b.rev, c.rev), (1, 2, 3));
}

#[test]
fn snapshot_carries_server_now_and_null_battery() {
    let mut e = engine();
    common::feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let snap = e.snapshot(T0 + 5);
    assert_eq!(snap.server_now_ms, T0 + 5);
    assert_eq!(snap.devices.len(), 1);
    assert!(snap.devices[0].battery.is_none());
    assert_eq!(snap.devices[0].state, DeviceState::Unknown);
    assert_eq!(snap.devices[0].last_known_battery, None);
}

#[test]
fn devices_follow_tray_order_then_name() {
    let mut e = engine();
    // 两台不同 VID 的 HID 设备。
    let a = StableId::hid(0x1111, 0x0001);
    let b = StableId::hid(0x2222, 0x0001);
    let fact_a = ProviderEvent::Arrived(voltnest_provider::DeviceFact::new(
        a.clone(),
        Transport::Hid,
        "hid-vbas",
        "Zeta",
        "Zeta",
    ));
    let fact_b = ProviderEvent::Arrived(voltnest_provider::DeviceFact::new(
        b.clone(),
        Transport::Hid,
        "hid-vbas",
        "Alpha",
        "Alpha",
    ));
    common::feed(&mut e, &fact_a, Transport::Hid, T0);
    common::feed(&mut e, &fact_b, Transport::Hid, T0);
    // 未设置 order 时按名称排序。
    let snap = e.snapshot(T0);
    assert_eq!(snap.devices[0].display_name, "Alpha");
    assert_eq!(snap.devices[1].display_name, "Zeta");

    // 设置 order 后按 order 排序（排序统一由 Rust 决定）。
    e.set_tray_order(vec![a.as_str().to_string()]);
    let snap = e.snapshot(T0);
    assert_eq!(snap.devices[0].stable_id, a.as_str());
    assert_eq!(snap.devices[1].stable_id, b.as_str());
}

#[test]
fn tray_respects_capacity_presence_and_hidden() {
    let mut e = engine_with(EngineConfig {
        tray_capacity: 2,
        ..EngineConfig::default()
    });
    let mut ids = Vec::new();
    for i in 0..3u16 {
        let id = StableId::hid(0x3000 + i, 1);
        let fact = ProviderEvent::Arrived(voltnest_provider::DeviceFact::new(
            id.clone(),
            Transport::Hid,
            "hid-vbas",
            format!("Dev{i}"),
            format!("Dev{i}"),
        ));
        common::feed(&mut e, &fact, Transport::Hid, T0);
        ids.push(id);
    }
    e.set_tray_order(ids.iter().map(|i| i.as_str().to_string()).collect());

    let snap = e.snapshot(T0);
    assert_eq!(
        snap.tray.visible_device_ids.len(),
        2,
        "上限 4 由配置给出，此处设为 2"
    );
    assert_eq!(snap.tray.visible_device_ids[0], ids[0].as_str());

    // 隐藏是用户显式动作。
    e.set_hidden(vec![ids[0].as_str().to_string()]);
    let snap = e.snapshot(T0);
    assert_eq!(snap.tray.visible_device_ids[0], ids[1].as_str());

    // 物理消失后不再有图标。
    let removed = ProviderEvent::Removed {
        stable_id: ids[1].clone(),
        reason: voltnest_provider::RemovalReason::Unplugged,
    };
    e.apply_event(&removed, Some(Transport::Hid), T0 + 1);
    let snap = e.snapshot(T0 + 1);
    assert!(
        !snap
            .tray
            .visible_device_ids
            .contains(&ids[1].as_str().to_string())
    );
}

#[test]
fn groups_hid_active_with_ble_inactive() {
    // 场景：切到 2.4G，蓝牙链路变成「未活动」。
    let mut e = engine();
    // 先让 BLE 有读数再衰败。
    common::feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    common::feed(&mut e, &ble_reading(80), Transport::Ble, T0);
    e.tick(T0 + 400_000); // BLE offline 360s
    assert_eq!(
        e.record("ble:01:00:00:23:04:35").unwrap().state,
        DeviceState::Offline
    );

    common::feed(&mut e, &hid_arrived(), Transport::Hid, T0 + 400_000);
    common::feed(&mut e, &hid_reading(90), Transport::Hid, T0 + 400_000);

    let snap = e.snapshot(T0 + 400_000);
    assert_eq!(
        snap.cards.len(),
        1,
        "同 modelKey + 一活跃一非活跃 + 双 transport → 合并"
    );
    let card = &snap.cards[0];
    assert_eq!(card.card_id, "aj159 nl");
    assert_eq!(card.primary_device_id, "hid:7768:6426");
    assert_eq!(card.battery, Some(90));
    assert_eq!(card.member_device_ids.len(), 2);
    assert!(
        card.inactive_member_text
            .starts_with("蓝牙 链路 · 未活动（"),
        "{}",
        card.inactive_member_text
    );
}

#[test]
fn groups_ble_active_with_hid_inactive() {
    let mut e = engine();
    common::feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    common::feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    e.tick(T0 + 200_000);
    common::feed(&mut e, &ble_arrived(), Transport::Ble, T0 + 200_000);
    common::feed(&mut e, &ble_reading(70), Transport::Ble, T0 + 200_000);

    let snap = e.snapshot(T0 + 200_000);
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].primary_device_id, "ble:01:00:00:23:04:35");
    assert_eq!(snap.cards[0].battery, Some(70));
    assert!(
        snap.cards[0]
            .inactive_member_text
            .starts_with("2.4G 链路 · 未活动（")
    );
}

#[test]
fn no_group_when_both_active() {
    let mut e = engine();
    common::feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    common::feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    common::feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    common::feed(&mut e, &ble_reading(70), Transport::Ble, T0);
    assert!(e.snapshot(T0 + 1).cards.is_empty(), "两条都活跃 → 不合并");
}

#[test]
fn no_group_when_both_inactive() {
    let mut e = engine();
    common::feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    common::feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    common::feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    common::feed(&mut e, &ble_reading(80), Transport::Ble, T0);
    e.tick(T0 + 400_000);
    assert!(
        e.snapshot(T0 + 400_000).cards.is_empty(),
        "两条都不活跃 → 不合并"
    );
}

#[test]
fn no_group_when_model_key_differs_or_missing() {
    let mut e = engine();
    // 名称不同（只差一位也不行，禁止前缀匹配）。
    let hid = ProviderEvent::Arrived(hid_fact("AJ159 NL P"));
    let ble = ProviderEvent::Arrived(ble_fact("AJ159 NL"));
    common::feed(&mut e, &hid, Transport::Hid, T0);
    common::feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    common::feed(&mut e, &ble, Transport::Ble, T0);
    common::feed(&mut e, &ble_reading(70), Transport::Ble, T0);
    assert!(
        e.snapshot(T0 + 1).cards.is_empty(),
        "modelKey 不相等 → 不合并"
    );

    // 名称为空 → modelKey 为 None → 谓词自然失败。
    let mut e2 = engine();
    let empty_hid = ProviderEvent::Arrived(hid_fact(""));
    let empty_ble = ProviderEvent::Arrived(ble_fact(""));
    common::feed(&mut e2, &empty_hid, Transport::Hid, T0);
    common::feed(&mut e2, &hid_reading(90), Transport::Hid, T0);
    common::feed(&mut e2, &empty_ble, Transport::Ble, T0);
    common::feed(&mut e2, &ble_reading(70), Transport::Ble, T0);
    assert!(e2.snapshot(T0 + 1).cards.is_empty());
}

#[test]
fn no_group_with_three_members_or_same_transport() {
    // 三条同 modelKey（两只同型号 + 一只另一链路）→ 不合并。
    let mut e = engine();
    let third = StableId::hid(0x9999, 0x0001);
    let third_fact = ProviderEvent::Arrived(voltnest_provider::DeviceFact::new(
        third,
        Transport::Hid,
        "hid-vbas",
        "AJ159 NL",
        "AJ159 NL",
    ));
    common::feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    common::feed(&mut e, &third_fact, Transport::Hid, T0);
    common::feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    common::feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    common::feed(&mut e, &ble_reading(70), Transport::Ble, T0);
    e.tick(T0 + 400_000);
    assert!(
        e.snapshot(T0 + 400_000).cards.is_empty(),
        "三条同 modelKey → 退回并列"
    );
}

#[test]
fn card_follows_primary_position_in_order() {
    let mut e = engine();
    common::feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    common::feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    common::feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    common::feed(&mut e, &ble_reading(70), Transport::Ble, T0);
    e.tick(T0 + 400_000); // 所有都过期
    // 重新激活 BLE，让它成为主成员，并置于 order 首位。
    common::feed(&mut e, &ble_reading(70), Transport::Ble, T0 + 400_001);
    e.set_tray_order(vec!["ble:01:00:00:23:04:35".to_string()]);
    let snap = e.snapshot(T0 + 400_001);
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.devices[0].stable_id, "ble:01:00:00:23:04:35");
    assert_eq!(snap.cards[0].primary_device_id, "ble:01:00:00:23:04:35");
}

#[test]
fn inactive_text_without_any_reading_has_no_duration() {
    // 从未收到读数的非活跃成员：不得出现「18446744073709551 分钟前」这种下溢文案。
    let mut e = engine();
    common::feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    // HID 无读数（Unknown，非活跃），BLE 有读数（活跃）。
    common::feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    common::feed(&mut e, &ble_reading(70), Transport::Ble, T0);
    let snap = e.snapshot(T0 + 1);
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].inactive_member_text, "2.4G 链路 · 未活动");
}

#[test]
fn stale_counts_as_active_for_grouping() {
    // Stale 仍有一条可展示的当前读数，因此算活跃。
    let mut e = engine();
    common::feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    common::feed(&mut e, &ble_reading(80), Transport::Ble, T0);
    e.tick(T0 + 400_000);
    common::feed(&mut e, &hid_arrived(), Transport::Hid, T0 + 400_000);
    common::feed(&mut e, &hid_reading(90), Transport::Hid, T0 + 400_000);
    e.tick(T0 + 445_001); // HID 距读数 45.001s → Stale；BLE 仍 Offline
    let snap = e.snapshot(T0 + 445_001);
    assert_eq!(e.record("hid:7768:6426").unwrap().state, DeviceState::Stale);
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].primary_device_id, "hid:7768:6426");
}

#[test]
fn sensitivity_does_not_change_grouping() {
    let mut e = engine_with(EngineConfig {
        sensitivity: Sensitivity::Loose,
        ..EngineConfig::default()
    });
    common::feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    common::feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    common::feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    common::feed(&mut e, &ble_reading(70), Transport::Ble, T0);
    e.tick(T0 + 400_000);
    assert!(e.snapshot(T0 + 400_000).cards.is_empty());
}

#[test]
fn make_hid_inactive_helper_is_consistent() {
    let mut e = engine();
    make_hid_inactive(&mut e, T0);
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Offline
    );
}
