//! 状态机测试：五种状态、完整转换表、三个阈值边界、时钟异常（K / Z 系列）。

mod common;

use common::*;
use voltnest_core::{DeviceState, EngineConfig, Transition};
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
    // Z-3：任何有效读数直接 Online，不经过中间态。
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
    // HID 标准档 staleAfter = 45s。
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

    // 60s → Unavailable（60_000 恰好仍为 Stale）。
    assert!(e.tick(T0 + 60_000));
    assert_eq!(e.record("hid:7768:6426").unwrap().state, DeviceState::Stale);
    assert!(e.tick(T0 + 60_000 + 1));
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Unavailable
    );

    // 180s → Offline。
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
    // Z-4 / W-8：状态真变化才 emit。
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
    // Offline → Unknown（重新出现）；旧读数必须清掉，否则下一次 tick 立刻又变灰。
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
    // E：Unknown 与 Unavailable 必须可区分。
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let event = ProviderEvent::Unreadable {
        stable_id: hid_id(),
        code: ErrorCode::MalformedFrame,
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
    // 链路已断时迟到的解析错误不得把状态抬回 Unavailable。
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
    };
    e.apply_event(&bad, Some(Transport::Hid), T0 + 2);
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Offline
    );
}

#[test]
fn link_lost_is_immediate_offline_without_percent() {
    // P-1 / Z-5：链路消失立即 Offline，不等下一个周期。
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
    assert!(rec.present, "LinkLost 不销毁托盘图标（AA-2）");
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
        "AA-2：物理消失才销毁图标"
    );
    assert_eq!(snap.devices[0].state, DeviceState::Offline);
}

#[test]
fn transport_down_only_affects_its_own_transport() {
    // RT-1：一处挂死只影响当前 transport。
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
    // Y-3：保留先发现的一条，第二台只告警。
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
    // Z-8 / Z-9：u64 下溢不得变成天文数字，冷却必须清除。
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    feed(&mut e, &hid_reading(15), Transport::Hid, T0);
    assert_eq!(e.take_alerts().len(), 1, "首次低电量必须提醒");

    e.tick(T0 + 1_000);
    // 系统时间被调早 1 小时。
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
    // Z-10：时钟后跳产生的回填值必须丢弃，不得改写 lastValidAt / battery。
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
    // 前跳允许批量退化。
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
    // 标准档 45s 已过期，宽松档 90s 未过期。
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

    // 宽松档 120s 仍应停在 Stale（unavailable=120s，使用 >）。
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
    // Windows BLE 阈值 90/120/360：50s 时仍 Online（若误用 HID 阈值会变 Stale）。
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
    // PC-5：BLE 不得显示「未充电」。
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
    // 读线程可能先于枚举线程给出第一条读数。
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
    // 随后的 Arrived 补齐名称，不新建记录。
    e.apply_event(&hid_arrived(), Some(Transport::Hid), T0 + 10);
    assert_eq!(e.records().len(), 1);
    assert_eq!(e.record("hid:7768:6426").unwrap().display_name, "AJ159 NL");
}

#[test]
fn seed_backfills_unknown_with_last_known_only() {
    // §11 / D8：回填后主电量「—」，副行上次 X%。
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
    // 回填记录不该出现在面板排序的托盘集合里。
    assert!(e.snapshot(T0).tray.visible_device_ids.is_empty());

    // 第一条有效读数到达后切换为 online 并清除副行。
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
    // X-7：超过 32 条淘汰最久未出现项（无记录的按「从未出现」处理）。
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
