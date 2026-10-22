//! 测试用 Provider（`pub mod testing`，**不用** `cfg(test)`）。
//!
//! Phase 2 的全部状态机测试都通过这些实现驱动，因此它们必须随 crate 一起发布，
//! 而不是只存在于本 crate 的测试二进制里。

use crate::Provider;
use crate::event::{DeviceFact, ProviderEvent, ValidReading};
use crate::ids::{Percent, StableId};
use crate::profile::{BLE_BAS, HID_VBAS, ProviderProfile};
use crate::transport::Transport;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

/// 不带时间戳的 HID 设备事实（`hid:7768:6426` → `AJ159 NL`）。
pub fn hid_fact(display_name: &str) -> DeviceFact {
    DeviceFact::new(
        StableId::hid(0x7768, 0x6426),
        Transport::Hid,
        crate::FAMILY_HID_VBAS,
        display_name,
        display_name,
    )
}

/// 不带时间戳的 BLE 设备事实。
pub fn ble_fact(display_name: &str) -> DeviceFact {
    DeviceFact::new(
        StableId::ble([0x01, 0x00, 0x00, 0x23, 0x04, 0x35]),
        Transport::Ble,
        crate::FAMILY_BLE_BAS,
        display_name,
        display_name,
    )
}

/// 有效读数；`pct > 100` 时返回 `None`（与 `Percent::new` 同语义）。
pub fn reading(stable_id: StableId, pct: u8, charging: Option<bool>) -> Option<ValidReading> {
    Percent::new(pct).map(|b| ValidReading::new(stable_id, b, charging))
}

/// 按脚本顺序发送事件，不起线程。
#[derive(Debug)]
pub struct ScriptedProvider {
    profile: &'static ProviderProfile,
    script: Vec<ProviderEvent>,
}

impl ScriptedProvider {
    /// 用显式 Profile 构造（测试自定义能力声明时使用）。
    pub fn with_profile(profile: &'static ProviderProfile, script: Vec<ProviderEvent>) -> Self {
        Self { profile, script }
    }

    /// HID 家族脚本。
    pub fn hid(script: Vec<ProviderEvent>) -> Self {
        Self {
            profile: &HID_VBAS,
            script,
        }
    }

    /// BLE 家族脚本。
    pub fn ble(script: Vec<ProviderEvent>) -> Self {
        Self {
            profile: &BLE_BAS,
            script,
        }
    }

    /// 脚本长度，便于宿主测试断言。
    pub fn len(&self) -> usize {
        self.script.len()
    }

    /// 脚本是否为空。
    pub fn is_empty(&self) -> bool {
        self.script.is_empty()
    }
}

impl Provider for ScriptedProvider {
    fn profile(&self) -> &'static ProviderProfile {
        self.profile
    }

    fn run(self: Box<Self>, tx: Sender<ProviderEvent>, stop: Arc<AtomicBool>) {
        for event in self.script {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if tx.send(event).is_err() {
                return;
            }
        }
    }
}

/// 立即 panic，用于验证 RT-2 的 `catch_unwind` 隔离。
pub struct PanicProvider {
    profile: &'static ProviderProfile,
}

impl PanicProvider {
    /// 默认使用 HID 家族声明。
    pub fn new() -> Self {
        Self { profile: &HID_VBAS }
    }

    /// 指定 Profile（验证「只影响当前 transport」）。
    pub fn with_profile(profile: &'static ProviderProfile) -> Self {
        Self { profile }
    }
}

impl Default for PanicProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for PanicProvider {
    fn profile(&self) -> &'static ProviderProfile {
        self.profile
    }

    fn run(self: Box<Self>, _tx: Sender<ProviderEvent>, _stop: Arc<AtomicBool>) {
        panic!("scripted provider panic（测试用）");
    }
}

/// 循环到 `stop` 置位才退出，用于验证 RT-3/RT-4 的停止与 join 上限。
pub struct LoopProvider {
    profile: &'static ProviderProfile,
    ticks: Arc<AtomicUsize>,
}

impl LoopProvider {
    /// 构造，并返回可观测的 tick 计数器。
    pub fn new() -> (Self, Arc<AtomicUsize>) {
        let ticks = Arc::new(AtomicUsize::new(0));
        (
            Self {
                profile: &HID_VBAS,
                ticks: Arc::clone(&ticks),
            },
            ticks,
        )
    }
}

impl Default for LoopProvider {
    fn default() -> Self {
        Self::new().0
    }
}

impl Provider for LoopProvider {
    fn profile(&self) -> &'static ProviderProfile {
        self.profile
    }

    fn run(self: Box<Self>, _tx: Sender<ProviderEvent>, stop: Arc<AtomicBool>) {
        while !stop.load(Ordering::Relaxed) {
            self.ticks.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_provider_emits_in_order() {
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let script = vec![
            ProviderEvent::Arrived(hid_fact("AJ159 NL")),
            ProviderEvent::Reading(
                reading(StableId::hid(0x7768, 0x6426), 90, Some(false)).unwrap(),
            ),
        ];
        Box::new(ScriptedProvider::hid(script)).run(tx, stop);
        let got: Vec<_> = rx.try_iter().map(|e| e.kind()).collect();
        assert_eq!(got, ["arrived", "reading"]);
    }

    #[test]
    fn scripted_provider_stops_when_flag_set() {
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(true));
        let script = vec![ProviderEvent::Arrived(hid_fact("AJ159 NL"))];
        Box::new(ScriptedProvider::hid(script)).run(tx, stop);
        assert_eq!(rx.try_iter().count(), 0, "stop 置位后不得再发事件");
    }

    #[test]
    fn factories_use_expected_ids() {
        assert_eq!(hid_fact("AJ159 NL").stable_id.as_str(), "hid:7768:6426");
        assert_eq!(
            ble_fact("AJ159 NL").stable_id.as_str(),
            "ble:01:00:00:23:04:35"
        );
        assert_eq!(hid_fact("AJ159 NL").provider, "hid-vbas");
        assert_eq!(ble_fact("AJ159 NL").provider, "ble-bas");
    }

    #[test]
    fn reading_factory_rejects_out_of_range() {
        assert!(reading(StableId::hid(1, 2), 101, None).is_none());
        assert!(reading(StableId::hid(1, 2), 100, None).is_some());
    }

    #[test]
    fn loop_provider_stops() {
        let (p, ticks) = LoopProvider::new();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        let (tx, _rx) = std::sync::mpsc::channel();
        let h = std::thread::spawn(move || Box::new(p).run(tx, stop2));
        while ticks.load(Ordering::Relaxed) == 0 {
            std::thread::yield_now();
        }
        stop.store(true, Ordering::Relaxed);
        h.join().unwrap();
        assert!(ticks.load(Ordering::Relaxed) >= 1);
    }

    #[test]
    fn panic_provider_panics() {
        let p = PanicProvider::new();
        let (tx, _rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let h = std::thread::spawn(move || Box::new(p).run(tx, stop));
        assert!(h.join().is_err(), "PanicProvider 必须以 panic 结束");
    }
}
