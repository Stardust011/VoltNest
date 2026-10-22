//! 测试共享装置：一个只装了两个 v1 Provider 的空引擎，以及常用事件构造器。

#![allow(dead_code)]

use voltnest_core::{Engine, EngineConfig};
use voltnest_provider::Percent;
use voltnest_provider::testing::{ble_fact, hid_fact};
use voltnest_provider::{BLE_BAS, HID_VBAS, ProviderEvent, StableId, Transport, ValidReading};

/// 固定的「现在」：2027-01-15 08:00:00 UTC 左右，纯数字避免依赖真实时间。
pub const T0: u64 = 1_800_000_000_000;

pub fn engine() -> Engine {
    Engine::new(&[&HID_VBAS, &BLE_BAS], EngineConfig::default())
        .expect("v1 的 Profile 必须通过 PC-1..PC-7")
}

pub fn engine_with(config: EngineConfig) -> Engine {
    Engine::new(&[&HID_VBAS, &BLE_BAS], config).expect("profile 合法")
}

pub fn hid_id() -> StableId {
    StableId::hid(0x7768, 0x6426)
}

pub fn ble_id() -> StableId {
    StableId::ble([0x01, 0x00, 0x00, 0x23, 0x04, 0x35])
}

pub fn hid_arrived() -> ProviderEvent {
    ProviderEvent::Arrived(hid_fact("AJ159 NL"))
}

pub fn ble_arrived() -> ProviderEvent {
    ProviderEvent::Arrived(ble_fact("AJ159 NL"))
}

pub fn hid_reading(pct: u8) -> ProviderEvent {
    ProviderEvent::Reading(ValidReading::new(
        hid_id(),
        Percent::new(pct).expect("电量合法"),
        Some(false),
    ))
}

pub fn hid_reading_charging(pct: u8) -> ProviderEvent {
    ProviderEvent::Reading(ValidReading::new(
        hid_id(),
        Percent::new(pct).expect("电量合法"),
        Some(true),
    ))
}

pub fn ble_reading(pct: u8) -> ProviderEvent {
    ProviderEvent::Reading(ValidReading::new(
        ble_id(),
        Percent::new(pct).expect("电量合法"),
        None,
    ))
}

/// 便捷：把事件按来源 transport 应用进引擎。
pub fn feed(engine: &mut Engine, event: &ProviderEvent, transport: Transport, now_ms: u64) {
    engine.apply_event(event, Some(transport), now_ms);
}
