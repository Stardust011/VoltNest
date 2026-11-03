//! 设备类型识别在宿主侧的落地：快照输出、诊断留痕，以及「不影响电量采集」。
//!
//! 这些用例只驱动 `voltnest-core`（纯逻辑），不碰任何设备 I/O。

mod common;

use common::*;
use voltnest_core::{DeviceState, DiagLevel, Transition};
use voltnest_provider::testing::{ble_fact, hid_fact};
use voltnest_provider::{
    ClassificationMethod, DeviceClassification, DeviceFact, DeviceType, ProviderEvent, Transport,
};

fn classified(fact: DeviceFact, ty: DeviceType, method: ClassificationMethod) -> DeviceFact {
    fact.with_classification(DeviceClassification::new(
        ty,
        method,
        vec!["测试证据".to_string()],
    ))
}

#[test]
fn classification_reaches_snapshot() {
    let mut e = engine();
    let fact = classified(
        hid_fact("AJ159 NL"),
        DeviceType::Mouse,
        ClassificationMethod::VerifiedDeviceMapping,
    );
    e.apply_event(&ProviderEvent::Arrived(fact), Some(Transport::Hid), T0);

    let snap = e.snapshot(T0);
    assert_eq!(snap.devices.len(), 1);
    assert_eq!(snap.devices[0].device_type, "mouse");
    assert_eq!(
        snap.devices[0].classification_method,
        "verified_device_mapping"
    );
}

#[test]
fn unknown_classification_still_collects_battery() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    assert_eq!(
        e.record("hid:7768:6426")
            .unwrap()
            .classification
            .device_type,
        DeviceType::Unknown
    );

    let t = e.apply_event(&hid_reading(77), Some(Transport::Hid), T0 + 1_000);
    assert_eq!(
        t,
        Transition::StateChanged {
            from: DeviceState::Unknown,
            to: DeviceState::Online
        }
    );
    let snap = e.snapshot(T0 + 2_000);
    assert_eq!(snap.devices[0].battery, Some(77));
    assert_eq!(snap.devices[0].device_type, "unknown");
    assert_eq!(snap.devices[0].state, DeviceState::Online);
}

#[test]
fn reclassification_is_recorded_once_per_change() {
    let mut e = engine();
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let count = |e: &voltnest_core::Engine| {
        e.diagnostics()
            .recent()
            .into_iter()
            .filter(|d| d.code == "device_classified")
            .count()
    };
    assert_eq!(count(&e), 1, "首次到达必须留一条识别记录（含未判定原因）");

    feed(&mut e, &hid_arrived(), Transport::Hid, T0 + 1_000);
    assert_eq!(count(&e), 1, "结论没变不得重复刷");

    let fact = classified(
        hid_fact("AJ159 NL"),
        DeviceType::Mouse,
        ClassificationMethod::VerifiedDeviceMapping,
    );
    feed(
        &mut e,
        &ProviderEvent::Arrived(fact),
        Transport::Hid,
        T0 + 2_000,
    );
    let diag: Vec<_> = e
        .diagnostics()
        .recent()
        .into_iter()
        .filter(|d| d.code == "device_classified")
        .collect();
    assert_eq!(diag.len(), 2);
    assert!(
        diag[0].detail.contains("unknown -> mouse"),
        "变更记录要能看出前后值：{}",
        diag[0].detail
    );
    assert_eq!(
        e.record("hid:7768:6426").unwrap().classification.method,
        ClassificationMethod::VerifiedDeviceMapping
    );
}

#[test]
fn reading_before_arrival_leaves_type_unknown_then_filled() {
    let mut e = engine();
    e.apply_event(&hid_reading(50), Some(Transport::Hid), T0);
    assert_eq!(e.records().len(), 1);
    assert_eq!(
        e.records()[0].classification.device_type,
        DeviceType::Unknown
    );

    let fact = classified(
        hid_fact("AJ159 NL"),
        DeviceType::Mouse,
        ClassificationMethod::HidUsage,
    );
    e.apply_event(
        &ProviderEvent::Arrived(fact),
        Some(Transport::Hid),
        T0 + 500,
    );
    assert_eq!(
        e.record("hid:7768:6426")
            .unwrap()
            .classification
            .device_type,
        DeviceType::Mouse
    );
    assert_eq!(
        e.record("hid:7768:6426").unwrap().state,
        DeviceState::Online
    );
    assert_eq!(
        e.record("hid:7768:6426").unwrap().battery.map(|b| b.get()),
        Some(50)
    );
}

#[test]
fn conflict_evidence_is_surfaced_without_a_type() {
    let mut e = engine();
    let mut conflict = DeviceClassification::unknown();
    conflict.evidence.push(
        "HID Usage 0x0001/0x0006 = Keyboard；HID Usage 0x0001/0x0002 = Mouse；证据冲突：keyboard vs mouse；按冲突规则返回 unknown，不静默择一"
            .to_string(),
    );
    let fact = hid_fact("AJ159 NL").with_classification(conflict);
    e.apply_event(&ProviderEvent::Arrived(fact), Some(Transport::Hid), T0);

    let snap = e.snapshot(T0);
    assert_eq!(snap.devices[0].device_type, "unknown");
    assert_eq!(snap.devices[0].classification_method, "unknown");
    let recent = e.diagnostics().recent();
    let conclusion = recent
        .iter()
        .find(|d| d.code == "device_classified")
        .expect("识别记录必须存在");
    assert_eq!(conclusion.detail, "unknown");
    let diag = recent
        .iter()
        .find(|d| d.code == "device_classification_evidence")
        .expect("冲突原因必须留痕");
    assert_eq!(diag.level, DiagLevel::Debug);
    assert!(
        diag.detail.contains("证据冲突"),
        "诊断必须保留冲突原因：{}",
        diag.detail
    );
}

#[test]
fn evidence_goes_to_debug_and_conclusion_stays_short() {
    let mut e = engine();
    let mut c = DeviceClassification::new(
        DeviceType::Mouse,
        ClassificationMethod::VerifiedDeviceMapping,
        vec![
            "HID Usage 0x0001/0x0006 = Keyboard".to_string(),
            "HID Usage 0x0001/0x0002 = Mouse".to_string(),
            "复合集合：keyboard + mouse".to_string(),
            "可核验映射 → mouse（config.xml 1.0.7.6 · ajazz deviceType=mouse）".to_string(),
        ],
    );
    c.skipped.push("HID Usage 0x000c/0x0001 未支持".to_string());
    c.skipped.push("HID Usage 0xffb2/0x0001 未支持".to_string());
    let fact = hid_fact("AJ159 NL").with_classification(c);
    e.apply_event(&ProviderEvent::Arrived(fact), Some(Transport::Hid), T0);

    let recent = e.diagnostics().recent();
    let info = recent
        .iter()
        .find(|d| d.code == "device_classified")
        .expect("结论行必须存在");
    assert_eq!(info.level, DiagLevel::Info);
    assert_eq!(info.detail, "mouse via verified_device_mapping");
    assert_eq!(info.device_id.as_deref(), Some("hid:7768:6426"));

    let debug = recent
        .iter()
        .find(|d| d.code == "device_classification_evidence")
        .expect("证据必须留痕（debug）");
    assert_eq!(debug.level, DiagLevel::Debug);
    assert_eq!(debug.device_id.as_deref(), Some("hid:7768:6426"));
    for want in [
        "HID Usage 0x0001/0x0002 = Mouse",
        "复合集合：keyboard + mouse",
        "ajazz deviceType=mouse",
        "HID Usage 0x000c/0x0001 未支持",
    ] {
        assert!(
            debug.detail.contains(want),
            "debug 证据缺「{want}」：{}",
            debug.detail
        );
    }
}

#[test]
fn ble_device_type_travels_the_same_path() {
    let mut e = engine();
    let fact = classified(
        ble_fact("AJ159 NL"),
        DeviceType::Mouse,
        ClassificationMethod::BleAppearance,
    );
    e.apply_event(&ProviderEvent::Arrived(fact), Some(Transport::Ble), T0);
    let snap = e.snapshot(T0);
    assert_eq!(snap.devices[0].transport, "ble");
    assert_eq!(snap.devices[0].device_type, "mouse");
    assert_eq!(snap.devices[0].classification_method, "ble_appearance");
}
