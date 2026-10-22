//! 低电量通知策略：**跌破触发 + 迟滞 + 冷却**（AB-1..AB-5）。
//!
//! 关键点：通知由事件触发，不是周期性提醒。8 小时不充电只应产生 2 次通知
//! （20% 与 10% 各一次）；持续可见性由托盘环颜色承担（AB-3）。

use std::collections::{BTreeMap, HashMap};

/// 默认冷却：30 分钟。
pub const DEFAULT_COOLDOWN_MS: u64 = 30 * 60_000;
/// 默认迟滞：回升到「阈值 + 5%」才重新武装。
pub const DEFAULT_REARM_MARGIN: u8 = 5;

/// 通知配置。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NotificationConfig {
    /// 阈值列表（百分比），内部按降序处理，保证同一帧只报最高的那一档。
    pub thresholds: Vec<u8>,
    /// 同一 `(deviceId, threshold)` 的冷却。
    pub cooldown_ms: u64,
    /// 迟滞余量。
    pub rearm_margin: u8,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            thresholds: vec![20, 10],
            cooldown_ms: DEFAULT_COOLDOWN_MS,
            rearm_margin: DEFAULT_REARM_MARGIN,
        }
    }
}

impl NotificationConfig {
    /// 规范化：去重、降序、丢弃越界阈值（`> 100`）。
    pub fn normalized(mut self) -> Self {
        self.thresholds.retain(|t| *t <= 100);
        self.thresholds.sort_unstable_by(|a, b| b.cmp(a));
        self.thresholds.dedup();
        if self.thresholds.is_empty() {
            self.thresholds = vec![20, 10];
        }
        self
    }
}

/// 一次待发送的低电量通知。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LowBatteryAlert {
    /// 设备标识。
    pub stable_id: String,
    /// 设备名（宿主命名，前端不推导）。
    pub display_name: String,
    /// 被跌破的阈值。
    pub threshold: u8,
    /// 当前电量。
    pub battery: u8,
    /// 事件时间（UTC 毫秒）。
    pub at_ms: u64,
}

/// 单个 `(device, threshold)` 的持久化状态（AB-5 诊断页要展示这三项）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NotificationEntry {
    /// 设备标识。
    pub stable_id: String,
    /// 阈值。
    pub threshold: u8,
    /// 是否已武装（`true` = 可以触发下一次跌破）。
    pub armed: bool,
    /// 上次通知时间。
    pub last_notified_at_ms: Option<u64>,
    /// 上次通知结果（`sent` / `suppressed_charging` / `suppressed_cooldown`）。
    pub last_result: Option<String>,
}

#[derive(Clone, Debug)]
struct EntryState {
    armed: bool,
    last_notified_at_ms: Option<u64>,
    last_result: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct DeviceState {
    /// 上一次**参与跌破判定**的电量。
    ///
    /// 充电期间不更新：否则「插着电充到 5% 才拔线」会因为没有「跌破」过程而永远不提醒。
    last_battery: Option<u8>,
    entries: BTreeMap<u8, EntryState>,
}

/// 通知簿：纯逻辑，不含任何 I/O。宿主拿到 [`LowBatteryAlert`] 后自行发送 Toast。
#[derive(Clone, Debug)]
pub struct NotificationBook {
    config: NotificationConfig,
    devices: HashMap<String, DeviceState>,
}

impl NotificationBook {
    /// 构造。
    pub fn new(config: NotificationConfig) -> Self {
        Self {
            config: config.normalized(),
            devices: HashMap::new(),
        }
    }

    /// 当前配置。
    pub fn config(&self) -> &NotificationConfig {
        &self.config
    }

    /// 更新配置（设置页改动）。
    pub fn set_config(&mut self, config: NotificationConfig) {
        self.config = config.normalized();
    }

    /// 用 `state.json` 回填：重启后不得重复通知（AB-4）。
    pub fn seed(
        &mut self,
        stable_id: &str,
        last_battery: Option<u8>,
        entries: &[NotificationEntry],
    ) {
        let state = self.devices.entry(stable_id.to_string()).or_default();
        if let Some(b) = last_battery {
            state.last_battery = Some(b);
        }
        for e in entries.iter().filter(|e| e.stable_id == stable_id) {
            state.entries.insert(
                e.threshold,
                EntryState {
                    armed: e.armed,
                    last_notified_at_ms: e.last_notified_at_ms,
                    last_result: e.last_result.clone(),
                },
            );
        }
    }

    /// 观察一次有效读数，返回需要发送的通知（最多一条）。
    ///
    /// `charging == Some(true)` 时不发送（AB 抑制条件）。BLE 的 `charging` 恒为 `None`，
    /// 因此不会被误判为「未充电」。
    pub fn observe(
        &mut self,
        stable_id: &str,
        display_name: &str,
        battery: u8,
        charging: Option<bool>,
        now_ms: u64,
    ) -> Option<LowBatteryAlert> {
        let state = self.devices.entry(stable_id.to_string()).or_default();
        let prev = state.last_battery;
        let mut alert: Option<LowBatteryAlert> = None;

        for threshold in self.config.thresholds.clone() {
            let entry = state
                .entries
                .entry(threshold)
                .or_insert_with(|| EntryState {
                    armed: true,
                    last_notified_at_ms: None,
                    last_result: None,
                });

            // 迟滞：只有回升到「阈值 + 5%」才重新武装（AB-2）。
            if battery >= threshold.saturating_add(self.config.rearm_margin) {
                entry.armed = true;
            }

            // 跌破判定：上次 >= 阈值且本次 < 阈值；首次获得读数且已低于阈值也算跌破。
            let fell_below = match prev {
                Some(p) => p >= threshold && battery < threshold,
                None => battery < threshold,
            };
            if !fell_below || !entry.armed || alert.is_some() {
                continue;
            }

            if charging == Some(true) {
                entry.last_result = Some("suppressed_charging".to_string());
                continue;
            }

            // 冷却只挡「快速往返」，不承担持续提醒职责（AB-3）。
            let cooldown_ok = entry
                .last_notified_at_ms
                .is_none_or(|last| now_ms.saturating_sub(last) >= self.config.cooldown_ms);
            if !cooldown_ok {
                entry.last_result = Some("suppressed_cooldown".to_string());
                continue;
            }

            entry.armed = false;
            entry.last_notified_at_ms = Some(now_ms);
            entry.last_result = Some("sent".to_string());
            alert = Some(LowBatteryAlert {
                stable_id: stable_id.to_string(),
                display_name: display_name.to_string(),
                threshold,
                battery,
                at_ms: now_ms,
            });
        }

        // 充电期间冻结判定基线，拔线后才能观察到「跌破」。
        if charging != Some(true) {
            state.last_battery = Some(battery);
        }

        alert
    }

    /// 覆盖某个 `(device, threshold)` 的最后结果（AB-5）。
    ///
    /// `observe()` 只能记录「策略判定为应当发送」，它不知道宿主最终有没有送出去：
    /// Toast 可能被系统通知设置关掉、模式可能是「关闭」。宿主投递完再回填真实结果，
    /// 这样诊断页显示的是事实而不是意图。
    pub fn mark_result(&mut self, stable_id: &str, threshold: u8, result: &str) {
        if let Some(device) = self.devices.get_mut(stable_id)
            && let Some(entry) = device.entries.get_mut(&threshold)
        {
            entry.last_result = Some(result.to_string());
        }
    }

    /// 时钟后跳（系统时间被调早）：清除全部冷却，保留 `armed`（Z-9）。
    pub fn clear_cooldowns(&mut self) {
        for device in self.devices.values_mut() {
            for entry in device.entries.values_mut() {
                entry.last_notified_at_ms = None;
                entry.last_result = Some("cooldown_cleared_clock_jump".to_string());
            }
        }
    }

    /// 导出全部 `(device, threshold)` 状态用于持久化与诊断页（AB-5）。
    pub fn entries(&self) -> Vec<NotificationEntry> {
        let mut out = Vec::new();
        let mut ids: Vec<&String> = self.devices.keys().collect();
        ids.sort();
        for id in ids {
            let device = &self.devices[id];
            for (threshold, entry) in &device.entries {
                out.push(NotificationEntry {
                    stable_id: id.clone(),
                    threshold: *threshold,
                    armed: entry.armed,
                    last_notified_at_ms: entry.last_notified_at_ms,
                    last_result: entry.last_result.clone(),
                });
            }
        }
        out
    }

    /// 该设备上次参与判定的电量（持久化 `last_battery`）。
    pub fn last_battery(&self, stable_id: &str) -> Option<u8> {
        self.devices.get(stable_id).and_then(|d| d.last_battery)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> NotificationBook {
        NotificationBook::new(NotificationConfig::default())
    }

    const T0: u64 = 1_800_000_000_000;

    #[test]
    fn fires_on_first_reading_below_threshold() {
        let mut b = book();
        let a = b
            .observe("hid:1:2", "Dev", 15, Some(false), T0)
            .expect("首装即低电量必须提醒");
        assert_eq!(a.threshold, 20);
        assert_eq!(a.battery, 15);
    }

    #[test]
    fn does_not_fire_on_normal_reading() {
        assert!(
            book()
                .observe("hid:1:2", "Dev", 90, Some(false), T0)
                .is_none()
        );
    }

    #[test]
    fn fires_only_on_crossing_not_on_every_reading() {
        let mut b = book();
        assert!(b.observe("d", "Dev", 25, Some(false), T0).is_none());
        assert!(b.observe("d", "Dev", 19, Some(false), T0 + 1).is_some());
        // 持续在阈值下不得重复提醒（AB-1）。
        assert!(b.observe("d", "Dev", 18, Some(false), T0 + 2).is_none());
        assert!(b.observe("d", "Dev", 17, Some(false), T0 + 3).is_none());
    }

    #[test]
    fn hysteresis_requires_threshold_plus_five() {
        // 冷却关掉，单独验证迟滞（AB-2）。
        let mut b = NotificationBook::new(NotificationConfig {
            thresholds: vec![20, 10],
            cooldown_ms: 0,
            rearm_margin: 5,
        });
        assert!(b.observe("d", "Dev", 25, Some(false), T0).is_none());
        assert!(b.observe("d", "Dev", 19, Some(false), T0 + 1).is_some());
        // 19% ↔ 21% 的采样抖动不得连续触发（未回到 25% 就没重新武装）。
        assert!(b.observe("d", "Dev", 21, Some(false), T0 + 2).is_none());
        assert!(b.observe("d", "Dev", 19, Some(false), T0 + 3).is_none());
        // 回升到 25%（= 阈值 + 5%）才重新武装。
        assert!(b.observe("d", "Dev", 25, Some(false), T0 + 4).is_none());
        assert!(b.observe("d", "Dev", 19, Some(false), T0 + 5).is_some());
    }

    #[test]
    fn cooldown_blocks_fast_round_trip() {
        let mut b = book();
        assert!(b.observe("d", "Dev", 25, Some(false), T0).is_none());
        assert!(b.observe("d", "Dev", 19, Some(false), T0 + 1).is_some());
        // 充电 3 分钟又拔掉，5 分钟内再次跌破 → 冷却拦截。
        assert!(
            b.observe("d", "Dev", 25, Some(false), T0 + 60_000)
                .is_none()
        );
        assert!(
            b.observe("d", "Dev", 19, Some(false), T0 + 120_000)
                .is_none()
        );
        // 30 分钟后放行。
        assert!(
            b.observe("d", "Dev", 25, Some(false), T0 + 1_800_000)
                .is_none()
        );
        assert!(
            b.observe("d", "Dev", 19, Some(false), T0 + 1_800_001)
                .is_some()
        );
    }

    #[test]
    fn cooldown_boundary_is_inclusive() {
        let mut b = book();
        assert!(b.observe("d", "Dev", 25, Some(false), T0).is_none());
        assert!(b.observe("d", "Dev", 19, Some(false), T0 + 1).is_some());
        assert!(
            b.observe("d", "Dev", 25, Some(false), T0 + 1 + DEFAULT_COOLDOWN_MS)
                .is_none()
        );
        // 恰好 30 分钟（-1ms 仍被挡，恰好放行）。
        let mut b2 = book();
        assert!(b2.observe("d", "Dev", 25, Some(false), T0).is_none());
        assert!(b2.observe("d", "Dev", 19, Some(false), T0).is_some());
        assert!(
            b2.observe("d", "Dev", 25, Some(false), T0 + DEFAULT_COOLDOWN_MS - 1)
                .is_none()
        );
        assert!(
            b2.observe("d", "Dev", 19, Some(false), T0 + DEFAULT_COOLDOWN_MS - 1)
                .is_none()
        );
    }

    #[test]
    fn charging_suppresses_and_unplug_still_alerts() {
        let mut b = book();
        assert!(
            b.observe("d", "Dev", 15, Some(true), T0).is_none(),
            "充电时不得发送"
        );
        // 拔线后仍低于阈值：基线被冻结在插入前，因此能观察到跌破。
        let a = b
            .observe("d", "Dev", 15, Some(false), T0 + 10)
            .expect("拔线后应提醒");
        assert_eq!(a.threshold, 20);
    }

    #[test]
    fn charging_holds_baseline_so_later_unplug_fires() {
        let mut b = book();
        assert!(b.observe("d", "Dev", 50, Some(false), T0).is_none());
        assert!(
            b.observe("d", "Dev", 5, Some(true), T0 + 1).is_none(),
            "充电中不提醒"
        );
        assert!(
            b.observe("d", "Dev", 5, Some(false), T0 + 2).is_some(),
            "拔线即跌破"
        );
    }

    #[test]
    fn both_thresholds_fire_one_at_a_time() {
        let mut b = book();
        assert!(
            b.observe("d", "Dev", 15, Some(false), T0).is_some(),
            "20% 档"
        );
        assert!(
            b.observe("d", "Dev", 9, Some(false), T0 + 1).is_some(),
            "10% 档"
        );
        assert!(b.observe("d", "Dev", 8, Some(false), T0 + 2).is_none());
    }

    #[test]
    fn seed_prevents_duplicate_on_restart() {
        let mut b = book();
        let alert = b.observe("d", "Dev", 25, Some(false), T0).is_none();
        assert!(alert);
        assert!(b.observe("d", "Dev", 19, Some(false), T0 + 1).is_some());
        let entries = b.entries();
        let last = b.last_battery("d");

        let mut restored = book();
        restored.seed("d", last, &entries);
        // 重启后同一读数不得再提醒（AB-4）。
        assert!(
            restored
                .observe("d", "Dev", 19, Some(false), T0 + 2)
                .is_none()
        );
    }

    #[test]
    fn clock_backward_clears_cooldown() {
        let mut b = book();
        assert!(b.observe("d", "Dev", 25, Some(false), T0).is_none());
        assert!(b.observe("d", "Dev", 19, Some(false), T0).is_some());
        b.clear_cooldowns();
        // 冷却已清除，重新跌破即可发送（Z-9）。
        assert!(b.observe("d", "Dev", 25, Some(false), T0 - 10).is_none());
        assert!(b.observe("d", "Dev", 19, Some(false), T0 - 9).is_some());
    }

    #[test]
    fn config_normalization_dedups_and_sorts() {
        let c = NotificationConfig {
            thresholds: vec![10, 20, 10, 200],
            cooldown_ms: 1000,
            rearm_margin: 5,
        }
        .normalized();
        assert_eq!(c.thresholds, vec![20, 10]);

        let c = NotificationConfig {
            thresholds: vec![],
            cooldown_ms: 1,
            rearm_margin: 5,
        }
        .normalized();
        assert_eq!(
            c.thresholds,
            vec![20, 10],
            "空阈值退回默认，不得静默关闭通知"
        );
    }

    #[test]
    fn entries_are_sorted_and_carry_diagnostics() {
        let mut b = book();
        assert!(b.observe("b", "B", 15, Some(false), T0).is_some());
        assert!(b.observe("a", "A", 15, Some(false), T0).is_some());
        let entries = b.entries();
        assert_eq!(
            entries[0].stable_id, "a",
            "按 stableId 排序，便于诊断页展示"
        );
        // 只有被跌破的那一档写了结果；未跌破的 10% 档保持未武装过的空结果。
        assert!(
            entries
                .iter()
                .any(|e| e.last_result.as_deref() == Some("sent"))
        );
        assert!(
            !entries
                .iter()
                .any(|e| e.armed && e.last_notified_at_ms.is_some()),
            "已通知的条目必须解除武装"
        );
    }

    #[test]
    fn host_can_overwrite_delivery_result() {
        let mut b = book();
        assert!(b.observe("a", "A", 15, Some(false), T0).is_some());
        assert_eq!(
            b.entries()[0].last_result.as_deref(),
            Some("sent"),
            "策略层先记下「应当发送」"
        );
        b.mark_result("a", 20, "suppressed_mode_off");
        assert_eq!(
            b.entries()[0].last_result.as_deref(),
            Some("suppressed_mode_off")
        );
        // 未知设备 / 未知阈值是空操作，不能 panic。
        b.mark_result("nope", 20, "failed");
        b.mark_result("a", 7, "failed");
        assert_eq!(
            b.entries()[0].last_result.as_deref(),
            Some("suppressed_mode_off")
        );
    }
}
