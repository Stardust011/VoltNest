//! 状态机与设备注册表。
//!
//! 1. 显式时间：所有接口接收 `now_ms`，本模块不读时钟。
//! 2. 退化只由 `lastValidAt` 与阈值链驱动，且只在状态真变化时对外可见。
//! 3. 坏帧不改变状态：`Unreadable` 只写诊断与 `lastError`。

use crate::diagnostics::{DiagLevel, Diagnostics};
use crate::humanize::humanize_ago;
use crate::managed::ManagedDevices;
use crate::notify::{
    LowBatteryNotification, NotificationBook, NotificationConfig, NotificationEntry,
    NotificationNode,
};
use crate::record::{DeviceRecord, UNREADABLE_KINDS_MAX};
use crate::snapshot::{CardDto, DeviceDto, Snapshot, TraySnapshotDto};
use crate::state::DeviceState;
use std::collections::{BTreeMap, BTreeSet};
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

/// 托盘计数单位：卡片合并后的一个设备（未分组时就是记录本身）。
struct TrayUnit {
    /// 任一成员物理存在。
    present: bool,
    /// 任一成员非 `Offline`。
    live: bool,
    /// 代表该单元上托盘的记录 id，优先取卡片主成员。
    icon_id: Option<String>,
}

/// 设备状态机 + 注册表 + 快照生成器。宿主只持有一个实例。
#[derive(Debug)]
pub struct Engine {
    profiles: Vec<&'static ProviderProfile>,
    sensitivity: Sensitivity,
    notifications: NotificationBook,
    records: Vec<DeviceRecord>,
    pending_notifications: Vec<LowBatteryNotification>,
    tray_order: Vec<String>,
    hidden: Vec<String>,
    managed: ManagedDevices,
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
            managed: ManagedDevices::default(),
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

    /// 设置用户层的呈现改动（别名 / 删除 / 合并）。
    pub fn set_managed(&mut self, managed: ManagedDevices) {
        self.managed = managed;
    }

    /// 当前用户层配置。
    pub fn managed(&self) -> &ManagedDevices {
        &self.managed
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

    /// 回填一次通知的真实投递结果，覆盖 `last_result`。
    pub fn mark_notification_result(&mut self, stable_id: &str, threshold: u8, result: &str) {
        self.notifications.mark_result(stable_id, threshold, result);
    }

    // ---------- 事件 ----------

    /// 应用一个 Provider 事件。
    ///
    /// `transport` 只在 [`ProviderEvent::TransportDown`] 生效（该变体不带 transport），其它变体忽略。
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

    /// 把一次设备类型识别写进诊断：`info` 写结论，`debug` 写逐条证据。
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

    /// 1s tick：比对时间戳，仅在状态真变化时返回 `true`。时钟后跳会记诊断并清除通知冷却。
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
    /// 手动登记一台设备（「添加设备」用）：记录已存在时返回 `false`。
    ///
    /// 新建的记录 `present = false`，等 Provider 的 `Arrived` 补齐型号与在场状态；托盘不因此多个图标。
    pub fn register(&mut self, stable_id: &StableId, display_name: &str, now_ms: u64) -> bool {
        if self.index_of(stable_id).is_some() {
            return false;
        }
        let transport = stable_id.transport().unwrap_or(Transport::Hid);
        let family = self.family_for_transport(transport);
        let mut rec = DeviceRecord::new(
            stable_id.clone(),
            transport,
            family,
            display_name,
            "",
            now_ms,
        );
        rec.present = false;
        self.records.push(rec);
        self.diagnostics.push(
            now_ms,
            DiagLevel::Info,
            "device_added",
            Some(stable_id.as_str().to_string()),
            display_name.to_string(),
        );
        true
    }

    /// 生成一次全量快照，`rev` 自增。
    pub fn snapshot(&mut self, now_ms: u64) -> Snapshot {
        self.rev += 1;
        let order = self.sorted_indices();
        let names = self.display_names(&order);
        let devices: Vec<DeviceDto> = order.iter().map(|&i| self.device_dto(i, &names)).collect();
        let cards = self.build_cards(&order, now_ms, &names);
        let tray = self.tray_snapshot(&order, &cards);
        Snapshot {
            rev: self.rev,
            server_now_ms: now_ms,
            devices,
            cards,
            tray,
        }
    }

    /// 托盘视图：图标与计数都按呈现单元（卡片合并同型号的两条链路）计，避免一条设备算两次。
    fn tray_snapshot(&self, order: &[usize], cards: &[CardDto]) -> TraySnapshotDto {
        let mut owner: BTreeMap<&str, &str> = BTreeMap::new();
        for card in cards {
            for id in &card.member_device_ids {
                owner.insert(id.as_str(), card.primary_device_id.as_str());
            }
        }

        let mut units: Vec<TrayUnit> = Vec::new();
        let mut index: BTreeMap<&str, usize> = BTreeMap::new();
        for &i in order {
            let rec = &self.records[i];
            let unit_id = owner
                .get(rec.stable_id.as_str())
                .copied()
                .unwrap_or(rec.stable_id.as_str());
            let at = *index.entry(unit_id).or_insert_with(|| {
                units.push(TrayUnit {
                    present: false,
                    live: false,
                    icon_id: None,
                });
                units.len() - 1
            });
            let unit = &mut units[at];
            unit.present |= rec.present;
            unit.live |= rec.present && rec.state != DeviceState::Offline;

            let eligible = rec.has_tray_icon(self.is_hidden(rec.stable_id.as_str()));
            let is_primary = unit_id == rec.stable_id.as_str();
            if eligible && (unit.icon_id.is_none() || is_primary) {
                unit.icon_id = Some(rec.stable_id.as_str().to_string());
            }
        }

        let present = units.iter().filter(|u| u.present).count();
        let live = units.iter().filter(|u| u.live).count();
        let candidates: Vec<&TrayUnit> = units.iter().filter(|u| u.icon_id.is_some()).collect();
        let visible_device_ids = candidates
            .iter()
            .take(self.tray_capacity)
            .filter_map(|u| u.icon_id.clone())
            .collect();

        TraySnapshotDto {
            visible_device_ids,
            present,
            live,
            overflow: candidates.len().saturating_sub(self.tray_capacity),
        }
    }

    fn device_dto(&self, idx: usize, names: &BTreeMap<usize, String>) -> DeviceDto {
        let r = &self.records[idx];
        DeviceDto {
            stable_id: r.stable_id.as_str().to_string(),
            transport: r.transport.as_str().to_string(),
            provider: r.family_id.to_string(),
            display_name: names
                .get(&idx)
                .cloned()
                .unwrap_or_else(|| self.base_name(idx)),
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

    /// 呈现分组：先出用户合并卡，再对未被用户认领的记录做自动分组。
    ///
    /// 自动分组三个条件同时满足：① 两条 `modelKey` 非空且严格相等；② 不都活跃；③ 只涉及 HID 与 BLE。
    fn build_cards(
        &self,
        order: &[usize],
        now_ms: u64,
        names: &BTreeMap<usize, String>,
    ) -> Vec<CardDto> {
        let mut cards = self.user_cards(order, now_ms);
        let claimed: BTreeSet<&str> = self
            .managed
            .merges
            .iter()
            .flat_map(|group| group.members.iter().map(String::as_str))
            .collect();

        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for &idx in order {
            if claimed.contains(self.records[idx].stable_id.as_str()) {
                continue;
            }
            if let Some(key) = self.records[idx].model_key.as_deref()
                && !key.is_empty()
            {
                groups.entry(key.to_string()).or_default().push(idx);
            }
        }
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
                (true, false) => (a, b),
                (false, true) => (b, a),
                (false, false) => {
                    if pick_inactive_primary(ra, rb).0.stable_id == ra.stable_id {
                        (a, b)
                    } else {
                        (b, a)
                    }
                }
                (true, true) => continue,
            };
            let age_text = match self.records[inactive].age_ms(now_ms) {
                Some(age) => format!(
                    "{} 链路 · 未活动（{}）",
                    self.records[inactive].transport.display_label(),
                    humanize_ago(age)
                ),
                None => format!(
                    "{} 链路 · 未活动",
                    self.records[inactive].transport.display_label()
                ),
            };
            cards.push(CardDto {
                card_id: key,
                display_name: names
                    .get(&active)
                    .cloned()
                    .unwrap_or_else(|| self.base_name(active)),
                user_merged: false,
                primary_device_id: self.records[active].stable_id.as_str().to_string(),
                member_device_ids: vec![
                    self.records[active].stable_id.as_str().to_string(),
                    self.records[inactive].stable_id.as_str().to_string(),
                ],
                battery: self.records[active].battery.map(Percent::get),
                is_charging: self.records[active].charging,
                state: self.records[active].state,
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

    /// 用户合并卡：成员全部可见时才出卡，成员不足两台就当没合并过。
    fn user_cards(&self, order: &[usize], now_ms: u64) -> Vec<CardDto> {
        let mut cards = Vec::new();
        for group in &self.managed.merges {
            let members: Vec<usize> = group
                .members
                .iter()
                .filter_map(|id| {
                    order
                        .iter()
                        .copied()
                        .find(|&i| self.records[i].stable_id.as_str() == id)
                })
                .collect();
            if members.len() < 2 {
                continue;
            }
            let primary = self.pick_primary(&members, &group.primary);
            let others: Vec<usize> = members.iter().copied().filter(|&i| i != primary).collect();
            let name = match group.name.trim() {
                "" => self.base_name(primary),
                trimmed => trimmed.to_string(),
            };
            cards.push(CardDto {
                card_id: format!("user:{}", group.primary),
                display_name: name,
                user_merged: true,
                primary_device_id: self.records[primary].stable_id.as_str().to_string(),
                member_device_ids: std::iter::once(primary)
                    .chain(others.iter().copied())
                    .map(|i| self.records[i].stable_id.as_str().to_string())
                    .collect(),
                battery: self.records[primary].battery.map(Percent::get),
                is_charging: self.records[primary].charging,
                state: self.records[primary].state,
                inactive_member_text: others
                    .iter()
                    .map(|&i| self.member_note(i, now_ms))
                    .collect::<Vec<_>>()
                    .join("、"),
            });
        }
        cards
    }

    /// 主成员：用户指定的主设备活跃就用它；否则让位给第一个活跃成员；都不活跃则按指定。
    fn pick_primary(&self, members: &[usize], configured: &str) -> usize {
        let configured_idx = members
            .iter()
            .copied()
            .find(|&i| self.records[i].stable_id.as_str() == configured)
            .unwrap_or(members[0]);
        if self.records[configured_idx].state.is_active() {
            return configured_idx;
        }
        members
            .iter()
            .copied()
            .find(|&i| self.records[i].state.is_active())
            .unwrap_or(configured_idx)
    }

    /// 合并卡里非主成员的说明行。
    fn member_note(&self, idx: usize, now_ms: u64) -> String {
        let rec = &self.records[idx];
        let label = rec.transport.display_label();
        if rec.state.is_active() {
            return format!("{label} 链路 · 在线");
        }
        match rec.age_ms(now_ms) {
            Some(age) => format!("{label} 链路 · 未活动（{}）", humanize_ago(age)),
            None => format!("{label} 链路 · 未活动"),
        }
    }

    /// 呈现名：自定义名称优先，否则用 Provider 上报的名称。
    fn base_name(&self, idx: usize) -> String {
        let rec = &self.records[idx];
        self.managed
            .alias(rec.stable_id.as_str())
            .unwrap_or(&rec.display_name)
            .to_string()
    }

    /// 重名消歧：同名设备追加链路标签，同链路再重名时补序号，保证两张卡片不会一模一样。
    fn display_names(&self, order: &[usize]) -> BTreeMap<usize, String> {
        let bases: Vec<(usize, String)> = order.iter().map(|&i| (i, self.base_name(i))).collect();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for (_, name) in &bases {
            *counts.entry(name.to_lowercase()).or_default() += 1;
        }
        let mut used: BTreeMap<String, usize> = BTreeMap::new();
        bases
            .into_iter()
            .map(|(idx, name)| {
                let duplicated = counts
                    .get(&name.to_lowercase())
                    .copied()
                    .unwrap_or(1)
                    .gt(&1);
                let base = if duplicated {
                    format!("{name} · {}", self.records[idx].transport.display_label())
                } else {
                    name
                };
                let seen = used.entry(base.clone()).or_default();
                *seen += 1;
                let label = if *seen > 1 {
                    format!("{base} ({seen})")
                } else {
                    base
                };
                (idx, label)
            })
            .collect()
    }

    /// 诊断页的通知节点：同一台设备的多条链路按呈现分组合成一个节点。
    pub fn notification_nodes(&self, now_ms: u64) -> Vec<NotificationNode> {
        let mut owner: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
        for card in self.build_cards(&self.sorted_indices(), now_ms, &BTreeMap::new()) {
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

    /// 读数可能早于 `Arrived` 到达，按 transport 反推家族建记录，随后 `Arrived` 补齐名称。
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

    /// 面板 / 卡片 / 托盘的统一排序：`tray.order` → 名称 → id。已删除的设备不出现在任何呈现里。
    fn sorted_indices(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.records.len())
            .filter(|&i| !self.managed.is_removed(self.records[i].stable_id.as_str()))
            .collect();
        idx.sort_by(|&a, &b| {
            let ra = &self.records[a];
            let rb = &self.records[b];
            self.order_index(ra.stable_id.as_str())
                .cmp(&self.order_index(rb.stable_id.as_str()))
                .then_with(|| {
                    self.base_name(a)
                        .to_lowercase()
                        .cmp(&self.base_name(b).to_lowercase())
                })
                .then_with(|| ra.stable_id.cmp(&rb.stable_id))
        });
        idx
    }

    /// `tray.order` 超上限时淘汰「最久未出现」的条目。
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
/// 错误码放最前，细节缺省时退化为只有错误码。
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
