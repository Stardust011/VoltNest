//! 状态机测试：五种状态、完整转换表、三个阈值边界与时钟异常。

mod common;

use common::*;
use voltnest_core::{DeviceState, EngineConfig, Transition, UNREADABLE_KINDS_MAX};
use voltnest_provider::Percent;
use voltnest_provider::{
    ErrorCode, ProviderEvent, RemovalReason, Sensitivity, StableId, Transport, ValidReading,
};

#[test]
fn arrived_creates_unknown_without_battery() {
    let mut e = engine();
    let t = e.apply_event(&hid_arrived(), Some(Transport::Hid), T0);
    assert_eq!(t, Transition::Created);
    let rec = e.record("hid:7768:6426").expect("记录已建");
    assert_eq!(rec.state, DeviceState::Unknown);
    assert!(rec.battery.is_none(), "「没读到」不得渲染成 0%");
    assert!(rec.present);
    assert_eq!(rec.display_name, "AJ159 NL");
    assert_eq!(rec.model_key.as_deref(), Some("aj159 nl"));
}

#[test]
fn first_reading_goes_straight_to_online() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let t = e.apply_event(&hid_reading(90), Some(Transport::Hid), T0 + 1_000);
    assert_eq!(
        t,
        Transition::StateChanged {
            from: DeviceState::Unknown,
            to: DeviceState::Online
        }
    );
    let rec = e.record("hid:7768:6426").unwrap();
    assert_eq!(rec.state, DeviceState::Online);
    assert_eq!(rec.battery.map(Percent::get), Some(90));
    assert_eq!(rec.last_valid_at_ms, Some(T0 + 1_000));
}

#[test]
fn stale_boundary_is_strict_greater_than() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0);

    assert!(!e.tick(T0 + 45_000 - 1), "-1ms 不得退化");
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Online
    );

    assert!(!e.tick(T0 + 45_000), "恰好等于阈值不得退化（使用 >）");
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Online
    );

    assert!(e.tick(T0 + 45_000 + 1), "+1ms 必须退化");
    assert_eq!(e.record("hid:7768:6426").unwrap().state, DeviceState::Stale);
}

#[test]
fn unavailable_and_offline_boundaries() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0);

    assert!(e.tick(T0 + 60_000));
    assert_eq!(e.record("hid:7768:6426").unwrap().state, DeviceState::Stale);
    assert!(e.tick(T0 + 60_000 + 1));
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Unavailable
    );

    assert!(!e.tick(T0 + 180_000));
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Unavailable
    );
    assert!(e.tick(T0 + 180_000 + 1));
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Offline
    );
}

#[test]
fn tick_only_reports_true_on_real_change() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0);

    assert!(!e.tick(T0 + 1_000));
    assert!(e.tick(T0 + 46_000), "Online → Stale 必须报告");
    assert!(!e.tick(T0 + 47_000), "仍为 Stale，不得重复报告");
    assert!(e.tick(T0 + 61_000), "Stale → Unavailable 必须报告");
    assert!(!e.tick(T0 + 62_000));
    assert!(e.tick(T0 + 181_000), "Unavailable → Offline 必须报告");
}

#[test]
fn reading_recovers_to_online_from_stale_and_unavailable() {
    for age in [46_000u64, 61_000] {
        let mut e = engine();
        feed(&mut e, &hid_arrived(), Transport::Hid, T0);
        feed(&mut e, &hid_reading(90), Transport::Hid, T0);
        e.tick(T0 + age);
        let before = e.record("hid:7768:6426").unwrap().state;
        assert!(matches!(
            before,
            DeviceState::Stale | DeviceState::Unavailable
        ));

        let t = e.apply_event(&hid_reading(88), Some(Transport::Hid), T0 + age + 1);
        assert_eq!(
            t,
            Transition::StateChanged {
                from: before,
                to: DeviceState::Online
            }
        );
        assert_eq!(
            e.record("hid:7768:6426").unwrap().state,
            DeviceState::Online
        );
    }
}

#[test]
fn re_arrival_clears_offline_back_to_unknown() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    e.tick(T0 + 200_000);
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Offline
    );

    let t = e.apply_event(&hid_arrived(), Some(Transport::Hid), T0 + 201_000);
    assert_eq!(
        t,
        Transition::StateChanged {
            from: DeviceState::Offline,
            to: DeviceState::Unknown
        }
    );
    let rec = e.record("hid:7768:6426").unwrap();
    assert!(rec.battery.is_none());
    assert!(rec.last_valid_at_ms.is_none());
    assert!(!e.tick(T0 + 202_000), "Unknown 无读数，不得因时间退化");
}

#[test]
fn unreadable_on_unknown_stays_unknown_but_records_error() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let event = ProviderEvent::Unreadable {
        stable_id: hid_id(),
        code: ErrorCode::MalformedFrame,
        detail: Some("cmd_mismatch(want=0x03 got=0x23) · 6B: 06 FA 23 00 5A 01".into()),
    };
    let t = e.apply_event(&event, Some(Transport::Hid), T0 + 1);
    assert_eq!(t, Transition::None, "从未有读数 → 保持 Unknown");
    let rec = e.record("hid:7768:6426").unwrap();
    assert_eq!(rec.state, DeviceState::Unknown);
    assert_eq!(rec.last_error, Some(ErrorCode::MalformedFrame));
}

#[test]
fn unreadable_after_reading_is_unavailable_and_keeps_last_valid() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    let event = ProviderEvent::Unreadable {
        stable_id: hid_id(),
        code: ErrorCode::PercentOutOfRange,
        detail: None,
    };
    let t = e.apply_event(&event, Some(Transport::Hid), T0 + 5_000);
    assert_eq!(
        t,
        Transition::StateChanged {
            from: DeviceState::Online,
            to: DeviceState::Unavailable
        }
    );
    let rec = e.record("hid:7768:6426").unwrap();
    assert_eq!(rec.last_valid_at_ms, Some(T0), "坏帧不得推进 lastValidAt");
}

#[test]
fn unreadable_does_not_override_offline() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    let lost = ProviderEvent::LinkLost {
        stable_id: hid_id(),
        code: ErrorCode::DeviceGone,
    };
    e.apply_event(&lost, Some(Transport::Hid), T0 + 1);
    let bad = ProviderEvent::Unreadable {
        stable_id: hid_id(),
        code: ErrorCode::EmptyPayload,
        detail: None,
    };
    e.apply_event(&bad, Some(Transport::Hid), T0 + 2);
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Offline
    );
}

#[test]
fn link_lost_is_immediate_offline_without_percent() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    let lost = ProviderEvent::LinkLost {
        stable_id: hid_id(),
        code: ErrorCode::DeviceGone,
    };
    let t = e.apply_event(&lost, Some(Transport::Hid), T0 + 120);
    assert_eq!(
        t,
        Transition::StateChanged {
            from: DeviceState::Online,
            to: DeviceState::Offline
        }
    );
    let rec = e.record("hid:7768:6426").unwrap();
    assert!(rec.present, "LinkLost 不销毁托盘图标");
    assert!(rec.battery.is_none(), "离线期间不得继续展示电流量");
    assert_eq!(rec.last_known_battery.map(Percent::get), Some(90));
}

#[test]
fn removed_drops_presence_and_tray_membership() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    let snap = e.snapshot(T0 + 1);
    assert_eq!(
        snap.tray.visible_device_ids,
        vec!["hid:7768:6426".to_string()]
    );

    let removed = ProviderEvent::Removed {
        stable_id: hid_id(),
        reason: RemovalReason::Unplugged,
    };
    e.apply_event(&removed, Some(Transport::Hid), T0 + 2_000);
    let snap = e.snapshot(T0 + 2_000);
    assert!(
        snap.tray.visible_device_ids.is_empty(),
        "物理消失才销毁图标"
    );
    assert_eq!(snap.devices[0].state, DeviceState::Offline);
}

#[test]
fn transport_down_only_affects_its_own_transport() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    feed(&mut e, &ble_reading(80), Transport::Ble, T0);

    let down = ProviderEvent::TransportDown {
        code: ErrorCode::Internal,
        detail: "provider thread panicked".into(),
    };
    e.apply_event(&down, Some(Transport::Hid), T0 + 1);
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Offline
    );
    assert_eq!(
        e.record("ble:01:00:00:23:04:35").unwrap().state,
        DeviceState::Online
    );
}

#[test]
fn binding_conflict_does_not_create_second_record() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let conflict = ProviderEvent::BindingConflict {
        stable_id: hid_id(),
        detail: "second receiver with same vid/pid".into(),
    };
    let t = e.apply_event(&conflict, Some(Transport::Hid), T0 + 1);
    assert_eq!(t, Transition::BindingConflictRecorded);
    assert_eq!(e.records().len(), 1);
    assert!(e.record("hid:7768:6426").unwrap().conflict.is_some());
    assert!(
        e.diagnostics()
            .recent()
            .iter()
            .any(|d| d.code == "binding_conflict")
    );
}

#[test]
fn duplicate_arrival_does_not_duplicate_records() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_arrived(), Transport::Hid, T0 + 5_000);
    assert_eq!(e.records().len(), 1, "5s 重新枚举不得建第二条");
}

#[test]
fn clock_backward_does_not_underflow_and_clears_cooldown() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(15), Transport::Hid, T0);
    assert_eq!(e.take_notifications().len(), 1, "首次低电量必须提醒");

    e.tick(T0 + 1_000);
    assert!(!e.tick(T0 - 3_600_000), "时间后跳不得触发退化");
    let rec = e.record("hid:7768:6426").unwrap();
    assert_eq!(rec.state, DeviceState::Online);
    assert!(
        e.diagnostics()
            .recent()
            .iter()
            .any(|d| d.code == "clock_backward")
    );
}

#[test]
fn backfilled_reading_is_dropped() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0);

    let stale = ValidReading::new(hid_id(), Percent::new(50).unwrap(), Some(false));
    let t = e.apply_event(
        &ProviderEvent::Reading(stale),
        Some(Transport::Hid),
        T0 - 5_000,
    );
    assert_eq!(t, Transition::Backfilled);
    let rec = e.record("hid:7768:6426").unwrap();
    assert_eq!(rec.battery.map(Percent::get), Some(90));
    assert_eq!(rec.last_valid_at_ms, Some(T0));
}

#[test]
fn clock_forward_jump_degrades_in_one_batch() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(90), Transport::Hid, T0);
    e.tick(T0 + 1_000);
    assert!(e.tick(T0 + 700_000), "11 分钟后一跳到底");
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Offline
    );
    assert!(
        e.diagnostics()
            .recent()
            .iter()
            .any(|d| d.code == "clock_forward_jump")
    );
}

#[test]
fn sensitivity_scales_the_whole_chain() {
    let mut standard = engine();
    let mut loose = engine_with(EngineConfig {
        sensitivity: Sensitivity::Loose,
        ..EngineConfig::default()
    });
    for e in [&mut standard, &mut loose] {
        feed(e, &hid_arrived(), Transport::Hid, T0);
        feed(e, &hid_reading(90), Transport::Hid, T0);
    }
    assert!(standard.tick(T0 + 50_000));
    assert_eq!(
        standard.record("hid:7768:6426").unwrap().state,
        DeviceState::Stale
    );
    assert!(!loose.tick(T0 + 50_000));
    assert_eq!(
        loose.record("hid:7768:6426").unwrap().state,
        DeviceState::Online
    );

    assert!(loose.tick(T0 + 90_001));
    assert_eq!(
        loose.record("hid:7768:6426").unwrap().state,
        DeviceState::Stale
    );
    assert!(loose.tick(T0 + 120_001));
    assert_eq!(
        loose.record("hid:7768:6426").unwrap().state,
        DeviceState::Unavailable
    );
}

#[test]
fn ble_uses_its_own_thresholds() {
    let mut e = engine();
    feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    feed(&mut e, &ble_reading(80), Transport::Ble, T0);
    assert!(!e.tick(T0 + 50_000));
    assert_eq!(
        e.record("ble:01:00:00:23:04:35").unwrap().state,
        DeviceState::Online
    );
    assert!(e.tick(T0 + 90_001));
    assert_eq!(
        e.record("ble:01:00:00:23:04:35").unwrap().state,
        DeviceState::Stale
    );
}

#[test]
fn ble_charging_is_always_unknown() {
    let mut e = engine();
    feed(&mut e, &ble_arrived(), Transport::Ble, T0);
    let event = ProviderEvent::Reading(ValidReading::new(
        ble_id(),
        Percent::new(80).unwrap(),
        Some(true),
    ));
    e.apply_event(&event, Some(Transport::Ble), T0 + 1);
    assert_eq!(e.record("ble:01:00:00:23:04:35").unwrap().charging, None);
}

#[test]
fn reading_before_arrival_is_not_lost() {
    let mut e = engine();
    let t = e.apply_event(&hid_reading(70), Some(Transport::Hid), T0);
    assert_eq!(
        t,
        Transition::StateChanged {
            from: DeviceState::Unknown,
            to: DeviceState::Online
        }
    );
    assert_eq!(e.records().len(), 1);
    e.apply_event(&hid_arrived(), Some(Transport::Hid), T0 + 10);
    assert_eq!(e.records().len(), 1);
    assert_eq!(e.record("hid:7768:6426").unwrap().display_name, "AJ159 NL");
}

#[test]
fn seed_backfills_unknown_with_last_known_only() {
    let mut e = engine();
    e.seed(
        &[voltnest_core::SeedEntry {
            stable_id: "hid:7768:6426".into(),
            transport: Transport::Hid,
            display_name: "AJ159 NL".into(),
            model_name_raw: "AJ159 NL".into(),
            last_known_battery: Some(90),
            last_known_at_ms: Some(T0 - 600_000),
            first_seen_at_ms: Some(T0 - 86_400_000),
            last_seen_at_ms: Some(T0 - 600_000),
            path: Some("\\\\?\\hid#vid_7768".into()),
            notifications: vec![],
        }],
        T0,
    );
    let rec = e.record("hid:7768:6426").unwrap();
    assert_eq!(rec.state, DeviceState::Unknown);
    assert!(rec.battery.is_none(), "回填电量不得进入状态机");
    assert_eq!(rec.last_known_battery.map(Percent::get), Some(90));
    assert!(!rec.present, "未枚举到之前不得渲染假设备卡（D8）");
    assert_eq!(rec.model_key.as_deref(), Some("aj159 nl"));
    assert!(e.snapshot(T0).tray.visible_device_ids.is_empty());

    e.apply_event(&hid_reading(88), Some(Transport::Hid), T0 + 1);
    let snap = e.snapshot(T0 + 1);
    let dto = &snap.devices[0];
    assert_eq!(dto.state, DeviceState::Online);
    assert_eq!(dto.battery, Some(88));
}

#[test]
fn seed_ignores_malformed_ids() {
    let mut e = engine();
    e.seed(
        &[voltnest_core::SeedEntry {
            stable_id: "hid:7768".into(),
            transport: Transport::Hid,
            display_name: "bad".into(),
            model_name_raw: "bad".into(),
            last_known_battery: None,
            last_known_at_ms: None,
            first_seen_at_ms: None,
            last_seen_at_ms: None,
            path: None,
            notifications: vec![],
        }],
        T0,
    );
    assert!(e.records().is_empty());
    assert!(
        e.diagnostics()
            .recent()
            .iter()
            .any(|d| d.code == "seed_id_invalid")
    );
}

#[test]
fn tray_order_is_pruned_to_limit_keeping_recent() {
    let mut e = engine();
    let mut order: Vec<String> = (0..35).map(|i| format!("hid:{i:04x}:0001")).collect();
    let fresh_id = StableId::hid(0x0034, 0x0001);
    let fresh = fresh_id.as_str().to_string();
    let fact = ProviderEvent::Arrived(voltnest_provider::DeviceFact::new(
        fresh_id,
        Transport::Hid,
        "hid-vbas",
        "Fresh",
        "Fresh",
    ));
    e.apply_event(&fact, Some(Transport::Hid), T0);
    order.insert(0, fresh.clone());
    e.set_tray_order(order);

    let kept = e.tray_order();
    assert_eq!(kept.len(), 32, "超过 32 条必须淘汰");
    assert_eq!(kept[0], fresh, "最近出现的条目必须保留且位置不变");
    assert_eq!(e.records().len(), 1);
}

// ---------- 诊断：坏帧数据 ----------

/// 坏帧被拒时诊断行必须带成因与原始字节，而非只有一个错误码（否则答不了「为什么坏」）。
fn unreadable_frame(detail: &str) -> ProviderEvent {
    ProviderEvent::Unreadable {
        stable_id: hid_id(),
        code: ErrorCode::MalformedFrame,
        detail: Some(detail.into()),
    }
}

#[test]
fn malformed_frame_detail_reaches_diagnostics() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let detail = "cmd_mismatch(want=0x03 got=0x23) · 6B: 06 FA 23 00 5A 01";
    feed(&mut e, &unreadable_frame(detail), Transport::Hid, T0 + 1);

    let events = e.diagnostics().recent();
    let event = events
        .iter()
        .find(|d| d.code == "reading_unreadable")
        .expect("坏帧必须留一条诊断");
    assert_eq!(event.level.as_str(), "warn");
    assert_eq!(event.device_id.as_deref(), Some("hid:7768:6426"));
    assert_eq!(event.detail, format!("malformed_frame · {detail}"));
}

#[test]
fn unreadable_without_detail_stays_bare_code() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let event = ProviderEvent::Unreadable {
        stable_id: hid_id(),
        code: ErrorCode::AccessDenied,
        detail: None,
    };
    feed(&mut e, &event, Transport::Hid, T0 + 1);
    let events = e.diagnostics().recent();
    let diag = events
        .iter()
        .find(|d| d.code == "reading_unreadable")
        .expect("失败必须留一条诊断");
    assert_eq!(diag.detail, "access_denied");
}

#[test]
fn identical_malformed_frames_are_coalesced() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let same = "cmd_mismatch(want=0x03 got=0x23) · 6B: 06 FA 23 00 5A 01";
    for i in 0..5 {
        feed(&mut e, &unreadable_frame(same), Transport::Hid, T0 + i);
    }
    assert_eq!(unreadable_count(&e), 1, "完全相同的坏帧只留一条");

    let other = "cmd_mismatch(want=0x03 got=0x24) · 6B: 06 FA 24 00 5A 01";
    feed(&mut e, &unreadable_frame(other), Transport::Hid, T0 + 6);
    assert_eq!(unreadable_count(&e), 2);

    feed(&mut e, &hid_reading(90), Transport::Hid, T0 + 1_000);
    feed(&mut e, &unreadable_frame(same), Transport::Hid, T0 + 1_001);
    assert_eq!(unreadable_count(&e), 2, "有效读数不得让同一条坏帧重新留痕");
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Unavailable
    );
}

#[test]
fn alternating_malformed_frames_each_get_one_line() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let a = "cmd_mismatch(want=0x03 got=0x23) · 6B: 06 FA 23 00 5A 01";
    let b = "cmd_mismatch(want=0x03 got=0x24) · 6B: 06 FA 24 00 5A 01";
    for i in 0..20 {
        let text = if i % 2 == 0 { a } else { b };
        feed(&mut e, &unreadable_frame(text), Transport::Hid, T0 + i);
    }
    assert_eq!(unreadable_count(&e), 2, "每种坏帧各留一条，不随次数增长");
}

#[test]
fn distinct_malformed_kinds_are_capped() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    for i in 0..(UNREADABLE_KINDS_MAX + 5) {
        let text = format!("cmd_mismatch(want=0x03 got=0x{i:02X}) · 6B: 06 FA 00 00 5A 01");
        feed(
            &mut e,
            &unreadable_frame(&text),
            Transport::Hid,
            T0 + i as u64,
        );
    }
    assert_eq!(unreadable_count(&e), UNREADABLE_KINDS_MAX);
}

/// 设备重新出现是新的「一次出现」：种类集合清空，坏帧重新留痕。
#[test]
fn kinds_reset_when_device_reappears() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let same = "cmd_mismatch(want=0x03 got=0x23) · 6B: 06 FA 23 00 5A 01";
    feed(&mut e, &unreadable_frame(same), Transport::Hid, T0);
    assert_eq!(unreadable_count(&e), 1);

    feed(
        &mut e,
        &ProviderEvent::Removed {
            stable_id: hid_id(),
            reason: RemovalReason::EnumerationGone,
        },
        Transport::Hid,
        T0 + 1_000,
    );
    feed(&mut e, &hid_arrived(), Transport::Hid, T0 + 2_000);
    feed(&mut e, &unreadable_frame(same), Transport::Hid, T0 + 2_001);
    assert_eq!(unreadable_count(&e), 2);
}

fn unreadable_count(e: &voltnest_core::Engine) -> usize {
    e.diagnostics()
        .recent()
        .iter()
        .filter(|d| d.code == "reading_unreadable")
        .count()
}
