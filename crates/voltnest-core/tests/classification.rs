//! 设备类型识别在宿主侧的落地：快照契约、诊断留痕，以及「不影响电量采集」。
//!
//! 这些用例只驱动 `voltnest-core`（纯逻辑），不碰任何设备 I/O；真实设备的
//! 分类结果由 `docs/device-classification.md` 的实测记录负责。

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
    // 识别失败不得影响采集：这是「解耦」的可执行断言。
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
    // 首次：证据不足。
    feed(&mut e, &hid_arrived(), Transport::Hid, T0);
    let count = |e: &voltnest_core::Engine| {
        e.diagnostics()
            .recent()
            .into_iter()
            .filter(|d| d.code == "device_classified")
            .count()
    };
    assert_eq!(count(&e), 1, "首次到达必须留一条识别记录（含未判定原因）");

    // 第二次：同一结论，不该再刷诊断。
    feed(&mut e, &hid_arrived(), Transport::Hid, T0 + 1_000);
    assert_eq!(count(&e), 1, "结论没变不得重复刷");

    // 第三次：证据补齐 → 结论变化，留一条带新旧值的记录。
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
    // 读数先于 Arrived：记录被隐式建出，此时还没有任何类型证据。
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
    // 分类补齐不得清掉已有读数或回退状态。
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
    let diag = e
        .diagnostics()
        .recent()
        .into_iter()
        .find(|d| d.code == "device_classified")
        .expect("识别记录必须存在");
    assert!(
        diag.detail.contains("证据冲突"),
        "诊断必须保留冲突原因：{}",
        diag.detail
    );
}

#[test]
fn unsupported_usages_only_reach_debug_level() {
    // 只解析已支持的组合：未收录的 Usage 属于排查细节，走 debug；结论行必须干净。
    let mut e = engine();
    let mut c = DeviceClassification::new(
        DeviceType::Mouse,
        ClassificationMethod::VerifiedDeviceMapping,
        vec!["可核验映射 → mouse（来源：config.xml 1.0.7.6）".to_string()],
    );
    c.skipped
        .push("HID Usage 0x000c/0x0001 不在已支持组合内，不映射".to_string());
    c.skipped
        .push("HID Usage 0xffb2/0x0001 不在已支持组合内，不映射".to_string());
    let fact = hid_fact("AJ159 NL").with_classification(c);
    e.apply_event(&ProviderEvent::Arrived(fact), Some(Transport::Hid), T0);

    let recent = e.diagnostics().recent();
    let info = recent
        .iter()
        .find(|d| d.code == "device_classified")
        .expect("结论行必须存在");
    assert_eq!(info.level, DiagLevel::Info);
    assert!(
        !info.detail.contains("不在已支持组合内"),
        "结论行只留参与判定的证据：{}",
        info.detail
    );

    let debug = recent
        .iter()
        .find(|d| d.code == "device_classification_skipped")
        .expect("未参与判定的条目必须留 debug 记录");
    assert_eq!(debug.level, DiagLevel::Debug);
    assert_eq!(debug.device_id.as_deref(), Some("hid:7768:6426"));
    assert_eq!(
        debug.detail.matches("不在已支持组合内").count(),
        2,
        "逐条都要留痕：{}",
        debug.detail
    );
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
