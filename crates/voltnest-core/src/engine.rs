//! 状态机与设备注册表。
//!
//! 三条不可协商的约束：
//!
//! 1. 显式时间：所有接口接收 `now_ms`，本模块不读时钟。
//! 2. 退化只由 `lastValidAt` 与阈值链驱动，且只在状态真变化时对外可见。
//! 3. 坏帧不改变状态：`Unreadable` 只写诊断与 `lastError`，`Online/Stale/Unavailable` 之间的转移只依据时间。

use crate::diagnostics::{DiagLevel, Diagnostics};
use crate::humanize::humanize_ago;
use crate::notify::{
    LowBatteryNotification, NotificationBook, NotificationConfig, NotificationEntry,
    NotificationNode,
};
use crate::record::{DeviceRecord, UNREADABLE_KINDS_MAX};
use crate::snapshot::{CardDto, DeviceDto, Snapshot, TraySnapshotDto};
use crate::state::DeviceState;
use std::collections::BTreeMap;
use voltnest_provider::{
    ClassificationMethod, DegradationThresholds, DeviceFact, DeviceType, ErrorCode, Percent,
    ProfileError, ProviderEvent, ProviderProfile, RemovalReason, Sensitivity, StableId, Transport,
    ValidReading, validate_registry,
};

/// 引擎配置。阈值不在其中：它只能来自 Provider 与档位。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EngineConfig {
    /// 阈值档位。
    pub sensitivity: Sensitivity,
    /// 通知策略。
    pub notifications: NotificationConfig,
    /// 托盘图标上限。
    pub tray_capacity: usize,
    /// `tray.order` 条目上限：超过后淘汰最久未出现的条目。
    pub tray_order_limit: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            sensitivity: Sensitivity::Standard,
            notifications: NotificationConfig::default(),
            tray_capacity: 4,
            tray_order_limit: 32,
        }
    }
}

/// 一次事件或 tick 造成的记录变化。宿主据此判断是否需要发布快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transition {
    /// 无变化。
    None,
    /// 新建了设备记录（`Unknown`）。
    Created,
    /// 状态发生转移。
    StateChanged {
        /// 原状态。
        from: DeviceState,
        /// 新状态。
        to: DeviceState,
    },
    /// 收到有效读数，但状态本来就是 `Online`。
    Refreshed,
    /// 读数被丢弃：时间戳早于 `lastValidAt`（时钟后跳后的回填）。
    Backfilled,
    /// 记录了冲突告警，未新建第二条记录。
    BindingConflictRecorded,
}

impl Transition {
    /// 该变化是否值得发布一次快照（与 UI 可见性无关：`Created`/`Refreshed` 也要发）。
    ///
    /// 曾名 `is_visible`，读起来像「界面上是否可见」。
    pub fn should_publish(self) -> bool {
        !matches!(self, Self::None | Self::Backfilled)
    }
}

/// `state.json` 回填所需的字段。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeedEntry {
    /// 设备标识字符串。
    pub stable_id: String,
    /// 传输层。
    pub transport: Transport,
    /// 上次已知的显示名快照（仅诊断与展示，Provider 上报才是真值）。
    pub display_name: String,
    /// 上次已知的型号原文。
    pub model_name_raw: String,
    /// 上次已知电量。
    pub last_known_battery: Option<u8>,
    /// 上次已知电量时间。
    pub last_known_at_ms: Option<u64>,
    /// 首次发现时间。
    pub first_seen_at_ms: Option<u64>,
    /// 上次出现时间。
    pub last_seen_at_ms: Option<u64>,
    /// 诊断路径。
    pub path: Option<String>,
    /// 通知状态。
    pub notifications: Vec<NotificationEntry>,
}

/// 设备状态机 + 注册表 + 快照生成器。
///
/// 宿主只持有一个实例，在 Provider 事件、每秒 tick、设置变更时调用。
#[derive(Debug)]
pub struct Engine {
    profiles: Vec<&'static ProviderProfile>,
    sensitivity: Sensitivity,
    notifications: NotificationBook,
    records: Vec<DeviceRecord>,
    pending_notifications: Vec<LowBatteryNotification>,
    tray_order: Vec<String>,
    hidden: Vec<String>,
    tray_capacity: usize,
    tray_order_limit: usize,
    rev: u64,
    last_now_ms: Option<u64>,
    diagnostics: Diagnostics,
}

impl Engine {
    /// 构造并做 D5 的启动期自检。
    pub fn new(
        profiles: &[&'static ProviderProfile],
        config: EngineConfig,
    ) -> Result<Self, ProfileError> {
        validate_registry(profiles)?;
        Ok(Self {
            profiles: profiles.to_vec(),
            sensitivity: config.sensitivity,
            notifications: NotificationBook::new(config.notifications),
            records: Vec::new(),
            pending_notifications: Vec::new(),
            tray_order: Vec::new(),
            hidden: Vec::new(),
            tray_capacity: config.tray_capacity,
            tray_order_limit: config.tray_order_limit,
            rev: 0,
            last_now_ms: None,
            diagnostics: Diagnostics::new(),
        })
    }

    // ---------- 配置 ----------

    /// 当前档位。
    pub fn sensitivity(&self) -> Sensitivity {
        self.sensitivity
    }

    /// 切换档位。只影响阈值，不重建记录。
    pub fn set_sensitivity(&mut self, sensitivity: Sensitivity) {
        self.sensitivity = sensitivity;
    }

    /// 通知配置。
    pub fn notification_config(&self) -> &NotificationConfig {
        self.notifications.config()
    }

    /// 更新通知配置。
    pub fn set_notification_config(&mut self, config: NotificationConfig) {
        self.notifications.set_config(config);
    }

    /// 设置托盘顺序（`settings.tray.order`）。
    pub fn set_tray_order(&mut self, order: Vec<String>) {
        self.tray_order = order;
        self.prune_tray_order();
    }

    /// 当前托盘顺序（已按上限收敛）。
    pub fn tray_order(&self) -> &[String] {
        &self.tray_order
    }

    /// 设置托盘隐藏列表（`settings.tray.hidden`）。
    pub fn set_hidden(&mut self, hidden: Vec<String>) {
        self.hidden = hidden;
    }

    /// 某家族的阈值链（含档位缩放）。
    pub fn thresholds_for(&self, family_id: &str) -> DegradationThresholds {
        self.profile_for(family_id)
            .map(|p| p.thresholds_for(self.sensitivity))
            .unwrap_or(DegradationThresholds {
                stale_ms: 90_000,
                unavailable_ms: 120_000,
                offline_ms: 360_000,
            })
    }

    /// 记录只读视图。
    pub fn records(&self) -> &[DeviceRecord] {
        &self.records
    }

    /// 按 `stableId` 查记录。
    pub fn record(&self, stable_id: &str) -> Option<&DeviceRecord> {
        self.records
            .iter()
            .find(|r| r.stable_id.as_str() == stable_id)
    }

    /// 诊断。
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// 追加一条诊断（宿主用于记录 `PROFILE_CONFLICT` 等宿主级事件）。
    pub fn push_diag(
        &mut self,
        now_ms: u64,
        level: DiagLevel,
        code: &'static str,
        device_id: Option<String>,
        detail: impl Into<String>,
    ) {
        self.diagnostics
            .push(now_ms, level, code, device_id, detail);
    }

    /// 当前 rev。
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// 取走待发送的低电量通知。
    pub fn take_notifications(&mut self) -> Vec<LowBatteryNotification> {
        std::mem::take(&mut self.pending_notifications)
    }

    /// 导出通知状态用于持久化。
    pub fn notification_entries(&self) -> Vec<NotificationEntry> {
        self.notifications.entries()
    }

    /// 回填一次通知的真实投递结果。
    ///
    /// `take_notifications()` 给出「应当发送」，宿主真正投递（或被模式关闭、被系统拒绝）后
    /// 用本方法覆盖 `last_result`，诊断页才不会把「打算发」显示成「已发送」。
    pub fn mark_notification_result(&mut self, stable_id: &str, threshold: u8, result: &str) {
        self.notifications.mark_result(stable_id, threshold, result);
    }

    // ---------- 事件 ----------

    /// 应用一个 Provider 事件。
    ///
    /// `transport` 只在 [`ProviderEvent::TransportDown`] 生效：该变体不带 transport，而「只影响当前 transport」
    /// 是故障隔离的硬要求；其它变体忽略此参数。
    pub fn apply_event(
        &mut self,
        event: &ProviderEvent,
        transport: Option<Transport>,
        now_ms: u64,
    ) -> Transition {
        match event {
            ProviderEvent::Arrived(fact) => self.apply_arrived(fact, now_ms),
            ProviderEvent::Reading(reading) => self.apply_reading(reading, now_ms),
            ProviderEvent::Unreadable {
                stable_id,
                code,
                detail,
            } => self.apply_unreadable(stable_id, *code, detail.as_deref(), now_ms),
            ProviderEvent::LinkLost { stable_id, code } => {
                self.apply_link_lost(stable_id, *code, now_ms)
            }
            ProviderEvent::Removed { stable_id, reason } => {
                self.apply_removed(stable_id, *reason, now_ms)
            }
            ProviderEvent::TransportDown { code, detail } => {
                self.apply_transport_down(transport, *code, detail, now_ms)
            }
            ProviderEvent::BindingConflict { stable_id, detail } => {
                self.apply_binding_conflict(stable_id, detail, now_ms)
            }
        }
    }

    fn apply_arrived(&mut self, fact: &DeviceFact, now_ms: u64) -> Transition {
        if let Some(idx) = self.index_of(&fact.stable_id) {
            let (renamed, reclassified, from) = {
                let rec = &mut self.records[idx];
                let from = rec.state;
                let renamed = (rec.display_name != fact.display_name)
                    .then(|| (rec.display_name.clone(), fact.display_name.clone()));
                let reclassified = (rec.classification != fact.classification)
                    .then(|| (rec.classification.device_type, fact.classification.clone()));
                rec.classification.clone_from(&fact.classification);
                rec.model_name_raw.clone_from(&fact.model_name_raw);
                rec.model_key = crate::normalize::normalize(&fact.model_name_raw);
                if fact.path.is_some() {
                    rec.path.clone_from(&fact.path);
                }
                rec.present = true;
                rec.last_seen_at_ms = now_ms;
                if from == DeviceState::Offline {
                    rec.state = DeviceState::Unknown;
                    rec.last_valid_at_ms = None;
                    rec.battery = None;
                    rec.charging = None;
                    rec.last_error = None;
                    rec.unreadable_kinds.clear();
                }
                (renamed, reclassified, from)
            };
            if let Some((old, new)) = renamed {
                self.records[idx].display_name = new.clone();
                self.diagnostics.push(
                    now_ms,
                    DiagLevel::Info,
                    "display_name_updated",
                    Some(fact.stable_id.as_str().to_string()),
                    format!("{old} -> {new}"),
                );
            }
            if let Some((old, _)) = reclassified {
                self.push_classification(now_ms, fact, Some(old));
            }
            let to = self.records[idx].state;
            if from == to {
                Transition::Refreshed
            } else {
                Transition::StateChanged { from, to }
            }
        } else {
            let mut rec = DeviceRecord::new(
                fact.stable_id.clone(),
                fact.transport,
                fact.family_id,
                fact.display_name.clone(),
                fact.model_name_raw.clone(),
                now_ms,
            );
            rec.path = fact.path.clone();
            rec.classification.clone_from(&fact.classification);
            self.records.push(rec);
            self.diagnostics.push(
                now_ms,
                DiagLevel::Info,
                "device_arrived",
                Some(fact.stable_id.as_str().to_string()),
                format!("{} via {}", fact.display_name, fact.family_id),
            );
            self.push_classification(now_ms, fact, None);
            Transition::Created
        }
    }

    /// 把一次设备类型识别写进诊断，成功与失败都记。
    ///
    /// 两级分界是「结论需不需要它」：`info`（`device_classified`）只写结论，如 `mouse via verified_device_mapping`，
    /// 设备是谁由诊断行 `deviceId` 给出不在正文重复；`debug`（`device_classification_evidence`）写逐条证据、
    /// 冲突说明与未支持组合清单——一台复合 HID 能报 7 个集合，这些只该在排查时看。
    fn push_classification(&mut self, now_ms: u64, fact: &DeviceFact, from: Option<DeviceType>) {
        let c = &fact.classification;
        let mut detail = match from {
            Some(old) => format!("{old} -> {}", c.device_type),
            None => c.device_type.to_string(),
        };
        if c.method != ClassificationMethod::Unknown {
            detail.push_str(" via ");
            detail.push_str(c.method.as_str());
        }
        let device_id = Some(fact.stable_id.as_str().to_string());
        self.diagnostics.push(
            now_ms,
            DiagLevel::Info,
            "device_classified",
            device_id.clone(),
            detail,
        );

        let evidence = c.debug_text();
        if !evidence.is_empty() {
            self.diagnostics.push(
                now_ms,
                DiagLevel::Debug,
                "device_classification_evidence",
                device_id,
                evidence,
            );
        }
    }

    fn apply_reading(&mut self, reading: &ValidReading, now_ms: u64) -> Transition {
        let idx = self.ensure_record(&reading.stable_id, now_ms);
        let family = self.records[idx].family_id;
        let display_name = self.records[idx].display_name.clone();
        let stable_id = self.records[idx].stable_id.as_str().to_string();

        let charging = self
            .profile_for(family)
            .map(|p| p.sanitize_charging(reading.charging))
            .unwrap_or(reading.charging);

        if let Some(last) = self.records[idx].last_valid_at_ms
            && now_ms < last
        {
            self.diagnostics.push(
                now_ms,
                DiagLevel::Warn,
                "backfill_dropped",
                Some(stable_id),
                format!("reading at {now_ms} is older than lastValidAt {last}"),
            );
            return Transition::Backfilled;
        }

        let from = self.records[idx].state;
        {
            let rec = &mut self.records[idx];
            rec.battery = Some(reading.battery);
            rec.charging = charging;
            rec.last_valid_at_ms = Some(now_ms);
            rec.last_known_battery = Some(reading.battery);
            rec.last_known_at_ms = Some(now_ms);
            rec.last_error = None;
            rec.present = true;
            rec.last_seen_at_ms = now_ms;
            rec.state = DeviceState::Online;
        }

        if let Some(notification) = self.notifications.observe(
            &stable_id,
            &display_name,
            reading.battery.get(),
            charging,
            now_ms,
        ) {
            self.pending_notifications.push(notification);
        }

        if from == DeviceState::Online {
            Transition::Refreshed
        } else {
            Transition::StateChanged {
                from,
                to: DeviceState::Online,
            }
        }
    }

    fn apply_unreadable(
        &mut self,
        stable_id: &StableId,
        code: ErrorCode,
        detail: Option<&str>,
        now_ms: u64,
    ) -> Transition {
        let idx = self.ensure_record(stable_id, now_ms);
        let text = unreadable_detail(code, detail);
        let (from, to, log_line) = {
            let rec = &mut self.records[idx];
            rec.last_error = Some(code);
            let log_line = !rec.unreadable_kinds.iter().any(|k| k == &text)
                && rec.unreadable_kinds.len() < UNREADABLE_KINDS_MAX;
            if log_line {
                rec.unreadable_kinds.push(text.clone());
            }
            let from = rec.state;
            if rec.last_valid_at_ms.is_some() && from != DeviceState::Offline {
                rec.state = DeviceState::Unavailable;
            }
            (from, rec.state, log_line)
        };
        if log_line {
            self.diagnostics.push(
                now_ms,
                DiagLevel::Warn,
                "reading_unreadable",
                Some(stable_id.as_str().to_string()),
                text,
            );
        }
        if from == to {
            Transition::None
        } else {
            Transition::StateChanged { from, to }
        }
    }

    fn apply_link_lost(
        &mut self,
        stable_id: &StableId,
        code: ErrorCode,
        now_ms: u64,
    ) -> Transition {
        let idx = self.ensure_record(stable_id, now_ms);
        let from = {
            let rec = &mut self.records[idx];
            let from = rec.state;
            rec.state = DeviceState::Offline;
            rec.last_error = Some(code);
            rec.battery = None;
            rec.charging = None;
            from
        };
        self.diagnostics.push(
            now_ms,
            DiagLevel::Warn,
            "link_lost",
            Some(stable_id.as_str().to_string()),
            code.as_str(),
        );
        if from == DeviceState::Offline {
            Transition::None
        } else {
            Transition::StateChanged {
                from,
                to: DeviceState::Offline,
            }
        }
    }

    fn apply_removed(
        &mut self,
        stable_id: &StableId,
        reason: RemovalReason,
        now_ms: u64,
    ) -> Transition {
        let idx = self.ensure_record(stable_id, now_ms);
        let from = {
            let rec = &mut self.records[idx];
            let from = rec.state;
            rec.state = DeviceState::Offline;
            rec.present = false;
            rec.battery = None;
            rec.charging = None;
            from
        };
        self.diagnostics.push(
            now_ms,
            DiagLevel::Info,
            "device_removed",
            Some(stable_id.as_str().to_string()),
            reason.as_str(),
        );
        if from == DeviceState::Offline {
            Transition::None
        } else {
            Transition::StateChanged {
                from,
                to: DeviceState::Offline,
            }
        }
    }

    fn apply_transport_down(
        &mut self,
        transport: Option<Transport>,
        code: ErrorCode,
        detail: &str,
        now_ms: u64,
    ) -> Transition {
        let transports: Vec<Transport> = match transport {
            Some(t) => vec![t],
            None => vec![Transport::Hid, Transport::Ble],
        };
        self.diagnostics.push(
            now_ms,
            DiagLevel::Error,
            "transport_down",
            None,
            format!("{:?}: {detail}", transport),
        );
        let mut first: Option<(DeviceState, DeviceState)> = None;
        for rec in self.records.iter_mut() {
            if transports.contains(&rec.transport) {
                if rec.state != DeviceState::Offline {
                    first.get_or_insert((rec.state, DeviceState::Offline));
                    rec.state = DeviceState::Offline;
                    rec.battery = None;
                    rec.charging = None;
                }
                rec.last_error = Some(code);
            }
        }
        match first {
            Some((from, to)) => Transition::StateChanged { from, to },
            None => Transition::None,
        }
    }

    fn apply_binding_conflict(
        &mut self,
        stable_id: &StableId,
        detail: &str,
        now_ms: u64,
    ) -> Transition {
        if let Some(rec) = self.records.iter_mut().find(|r| r.stable_id == *stable_id) {
            rec.conflict = Some(detail.to_string());
        }
        self.diagnostics.push(
            now_ms,
            DiagLevel::Warn,
            "binding_conflict",
            Some(stable_id.as_str().to_string()),
            detail.to_string(),
        );
        Transition::BindingConflictRecorded
    }

    // ---------- 退化 ----------

    /// 1s tick：只比对时间戳，仅在状态真变化时返回 `true`。
    ///
    /// 系统时间后跳（`now < last_now`）记录诊断、清除通知冷却并丢弃回填值；前跳则允许设备批量退化。
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if let Some(last) = self.last_now_ms {
            if now_ms < last {
                self.diagnostics.push(
                    now_ms,
                    DiagLevel::Warn,
                    "clock_backward",
                    None,
                    format!("now {now_ms} < last {last}"),
                );
                self.notifications.clear_cooldowns();
            } else if now_ms.saturating_sub(last) > CLOCK_FORWARD_WARN_MS {
                self.diagnostics.push(
                    now_ms,
                    DiagLevel::Info,
                    "clock_forward_jump",
                    None,
                    format!("jumped {}s forward", (now_ms - last) / 1000),
                );
            }
        }
        self.last_now_ms = Some(now_ms);

        let mut changed = false;
        for idx in 0..self.records.len() {
            let family = self.records[idx].family_id;
            let thresholds = self.thresholds_for(family);
            let rec = &mut self.records[idx];
            let Some(last_valid) = rec.last_valid_at_ms else {
                continue;
            };
            if !rec.present {
                continue;
            }
            let age = now_ms.saturating_sub(last_valid);
            let target = if age > thresholds.offline_ms {
                DeviceState::Offline
            } else if age > thresholds.unavailable_ms {
                DeviceState::Unavailable
            } else if age > thresholds.stale_ms {
                DeviceState::Stale
            } else {
                rec.state
            };
            if rank(target) > rank(rec.state) {
                rec.state = target;
                changed = true;
            }
        }
        changed
    }

    // ---------- 回填 ----------

    /// 用 `state.json` 回填：设备一律 `Unknown`，`lastKnownBattery` 供 UI 首屏显示缓存值。
    pub fn seed(&mut self, entries: &[SeedEntry], now_ms: u64) {
        for entry in entries {
            let Some(stable_id) = StableId::parse(&entry.stable_id) else {
                self.diagnostics.push(
                    now_ms,
                    DiagLevel::Warn,
                    "seed_id_invalid",
                    None,
                    entry.stable_id.clone(),
                );
                continue;
            };
            if self.index_of(&stable_id).is_some() {
                continue;
            }
            let family = self.family_for_transport(entry.transport);
            let mut rec = DeviceRecord::new(
                stable_id.clone(),
                entry.transport,
                family,
                entry.display_name.clone(),
                entry.model_name_raw.clone(),
                entry.first_seen_at_ms.unwrap_or(now_ms),
            );
            rec.present = false;
            rec.last_seen_at_ms = entry.last_seen_at_ms.unwrap_or(now_ms);
            rec.path = entry.path.clone();
            rec.last_known_battery = entry.last_known_battery.and_then(Percent::new);
            rec.last_known_at_ms = entry.last_known_at_ms;
            self.records.push(rec);
            self.notifications.seed(
                stable_id.as_str(),
                entry.last_known_battery,
                &entry.notifications,
            );
        }
    }

    // ---------- 快照 ----------

    /// 生成一次全量快照，`rev` 自增。
    pub fn snapshot(&mut self, now_ms: u64) -> Snapshot {
        self.rev += 1;
        let order = self.sorted_indices();
        let devices: Vec<DeviceDto> = order.iter().map(|&i| self.device_dto(i)).collect();
        let cards = self.build_cards(&order, now_ms);
        let visible_device_ids = order
            .iter()
            .map(|&i| &self.records[i])
            .filter(|r| r.has_tray_icon(self.is_hidden(r.stable_id.as_str())))
            .take(self.tray_capacity)
            .map(|r| r.stable_id.as_str().to_string())
            .collect();
        Snapshot {
            rev: self.rev,
            server_now_ms: now_ms,
            devices,
            cards,
            tray: TraySnapshotDto { visible_device_ids },
        }
    }

    fn device_dto(&self, idx: usize) -> DeviceDto {
        let r = &self.records[idx];
        DeviceDto {
            stable_id: r.stable_id.as_str().to_string(),
            transport: r.transport.as_str().to_string(),
            provider: r.family_id.to_string(),
            display_name: r.display_name.clone(),
            model_key: r.model_key.clone(),
            device_type: r.classification.device_type.as_str().to_string(),
            classification_method: r.classification.method.as_str().to_string(),
            battery: r.battery.map(Percent::get),
            last_known_battery: r.last_known_battery.map(Percent::get),
            last_known_at_ms: r.last_known_at_ms,
            is_charging: r.charging,
            state: r.state,
            last_valid_at_ms: r.last_valid_at_ms,
            last_error: r.last_error.map(|c| c.as_str().to_string()),
        }
    }

    /// 呈现分组：三个条件必须同时满足：① 两条 `modelKey` 非空且严格相等；
    /// ② 该 `modelKey` 下两条不同 transport 的记录不都活跃（都活跃时并列，其余情况合并）；
    /// ③ 只涉及 HID 与 BLE 两个 transport。
    fn build_cards(&self, order: &[usize], now_ms: u64) -> Vec<CardDto> {
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for &idx in order {
            if let Some(key) = self.records[idx].model_key.as_deref()
                && !key.is_empty()
            {
                groups.entry(key.to_string()).or_default().push(idx);
            }
        }
        let mut cards = Vec::new();
        for (key, members) in groups {
            if members.len() != 2 {
                continue;
            }
            let (a, b) = (members[0], members[1]);
            let (ra, rb) = (&self.records[a], &self.records[b]);
            if ra.transport == rb.transport {
                continue;
            }
            let (active, inactive) = match (ra.state.is_active(), rb.state.is_active()) {
                (true, false) => (ra, rb),
                (false, true) => (rb, ra),
                (false, false) => pick_inactive_primary(ra, rb),
                (true, true) => continue,
            };
            let age_text = match inactive.age_ms(now_ms) {
                Some(age) => format!(
                    "{} 链路 · 未活动（{}）",
                    inactive.transport.display_label(),
                    humanize_ago(age)
                ),
                None => format!("{} 链路 · 未活动", inactive.transport.display_label()),
            };
            cards.push(CardDto {
                card_id: key,
                primary_device_id: active.stable_id.as_str().to_string(),
                member_device_ids: vec![
                    active.stable_id.as_str().to_string(),
                    inactive.stable_id.as_str().to_string(),
                ],
                battery: active.battery.map(Percent::get),
                is_charging: active.charging,
                state: active.state,
                inactive_member_text: age_text,
            });
        }
        let pos = |id: &str| {
            order
                .iter()
                .position(|&i| self.records[i].stable_id.as_str() == id)
        };
        cards.sort_by_key(|c| pos(&c.primary_device_id).unwrap_or(usize::MAX));
        cards
    }

    /// 诊断页的通知节点：同一台设备的多条链路按呈现分组合成一个节点。
    pub fn notification_nodes(&self, now_ms: u64) -> Vec<NotificationNode> {
        let mut owner: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
        for card in self.build_cards(&self.sorted_indices(), now_ms) {
            for id in &card.member_device_ids {
                owner.insert(
                    id.clone(),
                    (
                        card.primary_device_id.clone(),
                        card.member_device_ids.clone(),
                    ),
                );
            }
        }

        let mut nodes: Vec<NotificationNode> = Vec::new();
        let mut index: BTreeMap<(String, u8), usize> = BTreeMap::new();
        for entry in self.notifications.entries() {
            let (primary, members) = owner
                .get(&entry.stable_id)
                .cloned()
                .unwrap_or_else(|| (entry.stable_id.clone(), vec![entry.stable_id.clone()]));
            let key = (primary, entry.threshold);
            match index.get(&key) {
                Some(&i) => merge_node(&mut nodes[i], &entry),
                None => {
                    index.insert(key, nodes.len());
                    nodes.push(NotificationNode {
                        device_ids: members,
                        threshold: entry.threshold,
                        armed: entry.armed,
                        last_notified_at_ms: entry.last_notified_at_ms,
                        last_result: entry.last_result,
                    });
                }
            }
        }
        nodes.sort_by(|a, b| {
            a.device_ids
                .first()
                .cmp(&b.device_ids.first())
                .then_with(|| b.threshold.cmp(&a.threshold))
        });
        nodes
    }

    // ---------- 内部 ----------

    fn index_of(&self, stable_id: &StableId) -> Option<usize> {
        self.records.iter().position(|r| &r.stable_id == stable_id)
    }

    fn profile_for(&self, family_id: &str) -> Option<&'static ProviderProfile> {
        self.profiles
            .iter()
            .copied()
            .find(|p| p.family_id == family_id)
    }

    fn family_for_transport(&self, transport: Transport) -> &'static str {
        self.profiles
            .iter()
            .find(|p| p.transport == transport)
            .map(|p| p.family_id)
            .unwrap_or("unknown")
    }

    /// 读数可能早于 `Arrived` 到达（枚举与读取在不同线程）。按 transport 反推家族建记录，
    /// 避免丢弃第一条读数，随后 `Arrived` 补齐名称。
    fn ensure_record(&mut self, stable_id: &StableId, now_ms: u64) -> usize {
        if let Some(idx) = self.index_of(stable_id) {
            return idx;
        }
        let transport = stable_id.transport().unwrap_or(Transport::Hid);
        let family = self.family_for_transport(transport);
        let rec = DeviceRecord::new(
            stable_id.clone(),
            transport,
            family,
            stable_id.as_str(),
            "",
            now_ms,
        );
        self.records.push(rec);
        self.diagnostics.push(
            now_ms,
            DiagLevel::Warn,
            "record_implicit",
            Some(stable_id.as_str().to_string()),
            "读数先于 Arrived，已按 transport 反推家族",
        );
        self.records.len() - 1
    }

    fn is_hidden(&self, stable_id: &str) -> bool {
        self.hidden.iter().any(|h| h == stable_id)
    }

    fn order_index(&self, stable_id: &str) -> usize {
        self.tray_order
            .iter()
            .position(|s| s == stable_id)
            .unwrap_or(usize::MAX)
    }

    /// 面板 / 卡片 / 托盘的统一排序：`tray.order` → 名称 → id。
    fn sorted_indices(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.records.len()).collect();
        idx.sort_by(|&a, &b| {
            let ra = &self.records[a];
            let rb = &self.records[b];
            self.order_index(ra.stable_id.as_str())
                .cmp(&self.order_index(rb.stable_id.as_str()))
                .then_with(|| {
                    ra.display_name
                        .to_lowercase()
                        .cmp(&rb.display_name.to_lowercase())
                })
                .then_with(|| ra.stable_id.cmp(&rb.stable_id))
        });
        idx
    }

    /// `tray.order` 超上限时淘汰「最久未出现」的条目，只在保存设置时收敛，
    /// 不因临时拔掉接收器而丢失用户设置。
    fn prune_tray_order(&mut self) {
        if self.tray_order.len() <= self.tray_order_limit {
            return;
        }
        let mut decorated: Vec<(usize, u64, String)> = self
            .tray_order
            .iter()
            .enumerate()
            .map(|(pos, id)| {
                let seen = self
                    .records
                    .iter()
                    .find(|r| r.stable_id.as_str() == id)
                    .map(|r| r.last_seen_at_ms)
                    .unwrap_or(0);
                (pos, seen, id.clone())
            })
            .collect();
        decorated.sort_by_key(|(pos, seen, _)| (*seen, *pos));
        let remove = self.tray_order.len() - self.tray_order_limit;
        let drop_ids: Vec<String> = decorated
            .into_iter()
            .take(remove)
            .map(|(_, _, id)| id)
            .collect();
        self.tray_order.retain(|id| !drop_ids.contains(id));
    }
}

/// 时钟前跳诊断阈值：超过 10 分钟算一次跳变（含休眠唤醒）。
const CLOCK_FORWARD_WARN_MS: u64 = 10 * 60_000;

/// 两条都不活跃时的主成员：最后有效读数更新的优先，同为无读数或同一时刻则取 2.4G。
fn pick_inactive_primary<'a>(
    ra: &'a DeviceRecord,
    rb: &'a DeviceRecord,
) -> (&'a DeviceRecord, &'a DeviceRecord) {
    let rank = |r: &DeviceRecord| (r.last_valid_at_ms, r.transport == Transport::Hid);
    if rank(ra) >= rank(rb) {
        (ra, rb)
    } else {
        (rb, ra)
    }
}

/// 把同一节点的另一条链路状态并入：任一条已通知即视为已触发，结果取最近一次。
fn merge_node(node: &mut NotificationNode, entry: &NotificationEntry) {
    node.armed &= entry.armed;
    let newer = match (node.last_notified_at_ms, entry.last_notified_at_ms) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(a), Some(b)) => b > a,
    };
    if newer {
        node.last_notified_at_ms = entry.last_notified_at_ms;
        node.last_result = entry.last_result.clone();
    } else if node.last_result.is_none() {
        node.last_result = entry.last_result.clone();
    }
}

/// 诊断行文本：`malformed_frame · cmd_mismatch(want=0x03 got=0x23) · 6B: 06 FA …`。
///
/// 错误码放最前，按关键词筛选仍可命中；细节缺省时退化为只有错误码，与加字段前行为一致。
fn unreadable_detail(code: ErrorCode, detail: Option<&str>) -> String {
    match detail.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => format!("{code} · {d}"),
        None => code.as_str().to_string(),
    }
}

/// 状态退化深度，只允许沿 0→4 方向前进。
const fn rank(state: DeviceState) -> u8 {
    match state {
        DeviceState::Unknown => 0,
        DeviceState::Online => 1,
        DeviceState::Stale => 2,
        DeviceState::Unavailable => 3,
        DeviceState::Offline => 4,
    }
}
