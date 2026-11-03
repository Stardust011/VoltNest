//! 宿主事件循环：唯一持有 `Engine` 的地方。
//!
//! 职责边界：唯一时钟调用点（每批只取一次 `now_utc_ms()` 向下透传）；1s tick 驱动退化且仅真变化才发布；250ms 合并窗口把多次变化合成一次快照（≤4Hz）；所有快照在同一把锁里生成保证 `rev` 单调。Provider 线程只往 `tx` 塞事件、永不碰 `Engine`；`state.json` 只在发布路径写，状态机里不做 I/O。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use tauri::{AppHandle, Emitter};
use voltnest_core::{
    DiagLevel, Engine, EngineConfig, NotificationConfig, NotificationEntry, SeedEntry, Snapshot,
};
use voltnest_provider::{ProviderEvent, Transport, clock};
use voltnest_storage::{
    DataPaths, DeviceStateEntry, NotificationStateEntry, SCHEMA_VERSION, STATE_THROTTLE_MS,
    Settings, SettingsStore, StateFile, StateStore, StateThrottle,
};

use crate::dto::TransportStatus;
use crate::supervisor::SupervisorCmd;
use crate::{notification, tray};

/// 快照事件名（与前端 `SNAPSHOT_EVENT` 一字不差）。
pub const SNAPSHOT_EVENT: &str = "state://snapshot";

/// 诊断数据变更事件名（与前端 `DIAGNOSTICS_EVENT` 一字不差）。
pub const DIAGNOSTICS_EVENT: &str = "state://diagnostics";

/// 合并窗口：250ms 内的变化发一次快照。
pub const SNAPSHOT_WINDOW_MS: u64 = 250;

/// 状态退化 tick。
pub const TICK_MS: u64 = 1_000;

/// 队列深度告警阈值：超过 64 条报警。
pub const QUEUE_ALERT_DEPTH: usize = 64;

/// 宿主收到的三类唤醒。
pub enum HostEvent {
    /// Provider 线程组上报的原始事件。
    Provider {
        /// 发出事件的 transport。
        transport: Transport,
        /// 原始事件。
        event: ProviderEvent,
    },
    /// 强制发布一次快照（设置变更、手动重启等用户动作）。
    Wake,
    /// 采集线程组状态变化（重启次数/状态/最近错误），诊断页据此重拉。
    /// 只发信号不带载荷：诊断数据唯一来源是 `get_diagnostics`，事件通道不复制缓冲。
    Diagnostics,
}

/// 跨线程共享的宿主状态。
///
/// 曾名 `Shared`（不含任何信息），现按职责命名；同名变量 `shared` 保持不变。
pub struct HostState {
    /// 设备状态唯一所有者。
    pub engine: Mutex<Engine>,
    /// 当前设置（内存副本，落盘由 `settings_store` 负责）。
    pub settings: Mutex<Settings>,
    /// `settings.json` 读写。
    pub settings_store: SettingsStore,
    /// `state.json` 读写。
    pub state_store: StateStore,
    /// `state.json` 写入节流。
    pub state_throttle: Mutex<StateThrottle>,
    /// 采集线程组状态（诊断页展示）。
    pub transports: Mutex<Vec<TransportStatus>>,
    /// 每个 transport 的监管通道，供「重启采集」使用。
    pub supervisors: Mutex<HashMap<Transport, Sender<SupervisorCmd>>>,
    /// 当前存在的设备托盘图标（用于删除过期图标）。
    pub tray_ids: Mutex<Vec<String>>,
    /// 数据根目录与版本号。
    pub paths: DataPaths,
    /// 事件发送端，Provider 监管线程与命令共用。
    pub tx: Sender<HostEvent>,
    /// 在途事件数（> [`QUEUE_ALERT_DEPTH`] 时报警）。
    pub queued: AtomicUsize,
    /// 退出中：Provider 线程组据此停止。
    pub shutting_down: AtomicBool,
}

/// 取锁并忽略「中毒」：Provider 侧 panic 不该让宿主跟着瘫痪。
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl HostState {
    /// 组装共享状态并回填 `state.json`。
    pub fn new(
        paths: DataPaths,
        settings: Settings,
        settings_store: SettingsStore,
        state_store: StateStore,
        tx: Sender<HostEvent>,
    ) -> Arc<Self> {
        let engine = Engine::new(
            &[&voltnest_provider::HID_VBAS, &voltnest_provider::BLE_BAS],
            engine_config(&settings),
        )
        .expect("D5 自检失败：Provider 注册表非法，这是编译期就该发现的问题");

        let shared = Arc::new(Self {
            engine: Mutex::new(engine),
            settings: Mutex::new(settings),
            settings_store,
            state_store,
            state_throttle: Mutex::new(StateThrottle::new(STATE_THROTTLE_MS)),
            transports: Mutex::new(Vec::new()),
            supervisors: Mutex::new(HashMap::new()),
            tray_ids: Mutex::new(Vec::new()),
            paths,
            tx,
            queued: AtomicUsize::new(0),
            shutting_down: AtomicBool::new(false),
        });

        let stored = shared.state_store.load(clock::now_utc_ms());
        {
            let mut engine = lock(&shared.engine);
            let entries = seed_entries(&stored);
            if !entries.is_empty() {
                engine.seed(&entries, clock::now_utc_ms());
            }
        }
        shared
    }

    /// 把设置同步进状态机（档位、通知阈值、托盘顺序与隐藏列表）。
    pub fn apply_settings(&self, settings: &Settings) {
        let mut engine = lock(&self.engine);
        engine.set_sensitivity(settings.sensitivity());
        engine.set_notification_config(notification_config(settings));
        engine.set_tray_order(settings.tray.order.clone());
        engine.set_hidden(settings.tray.hidden.clone());
    }

    /// 当前在途事件数。
    pub fn queue_depth(&self) -> usize {
        self.queued.load(Ordering::Relaxed)
    }

    /// 记录一次事件入队。
    pub fn mark_enqueued(&self) {
        self.queued.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录一次事件出队。
    pub fn mark_dequeued(&self) {
        self.queued.fetch_sub(1, Ordering::Relaxed);
    }
}

/// 由 `state.json` 生成回填条目。无法识别 transport 的条目直接丢弃（只影响回填）。
fn seed_entries(state: &StateFile) -> Vec<SeedEntry> {
    state
        .devices
        .iter()
        .filter_map(|d| {
            let transport = Transport::from_prefix(&d.transport)?;
            Some(SeedEntry {
                stable_id: d.stable_id.clone(),
                transport,
                display_name: d.display_name.clone(),
                model_name_raw: d.model_name_raw.clone(),
                last_known_battery: d.last_known_battery,
                last_known_at_ms: d.last_known_at_ms,
                first_seen_at_ms: d.first_seen_at_ms,
                last_seen_at_ms: d.last_seen_at_ms,
                path: d.last_path.clone(),
                notifications: d
                    .notifications
                    .iter()
                    .map(|n| NotificationEntry {
                        stable_id: d.stable_id.clone(),
                        threshold: n.threshold,
                        armed: n.armed,
                        last_notified_at_ms: n.last_notified_at_ms,
                        last_result: n.last_result.clone(),
                    })
                    .collect(),
            })
        })
        .collect()
}

/// 当前快照（命令 `get_snapshot` 用）。
pub fn snapshot(shared: &HostState) -> Snapshot {
    let mut engine = lock(&shared.engine);
    engine.snapshot(clock::now_utc_ms())
}

/// 发布一次快照：事件 → 托盘 → 通知 → 落盘。
pub fn publish_snapshot(app: &AppHandle, shared: &Arc<HostState>, now: u64) {
    let (snap, notifications) = {
        let mut engine = lock(&shared.engine);
        let snap = engine.snapshot(now);
        let notifications = engine.take_notifications();
        (snap, notifications)
    };

    if let Err(err) = app.emit(SNAPSHOT_EVENT, &snap) {
        log::warn!("快照事件发送失败: {err}");
    }
    tray::sync_tray_icons(app, shared, &snap);
    notification::dispatch_notifications(app, shared, &snap, notifications, now);
    persist_state(shared, now, false);
}

/// 通知前端诊断数据已变。`restarts`/`status` 不属于设备、不在快照里，
/// 缺这条信号诊断页就只能切页时重拉，后台退避/手动重启的计数变化不会自动上界面。
fn emit_diagnostics(app: &AppHandle) {
    if let Err(err) = app.emit(DIAGNOSTICS_EVENT, ()) {
        log::warn!("诊断事件发送失败: {err}");
    }
}

/// 把 `Engine` 当前状态整理成可落盘的 `state.json`。
pub fn build_state_file(shared: &HostState) -> StateFile {
    let engine = lock(&shared.engine);
    let notifications = engine.notification_entries();
    let mut file = StateFile {
        schema_version: SCHEMA_VERSION,
        devices: Vec::new(),
        extra: Default::default(),
    };
    for record in engine.records() {
        let id = record.stable_id.as_str();
        file.upsert(DeviceStateEntry {
            stable_id: id.to_string(),
            transport: record.transport.as_str().to_string(),
            display_name: record.display_name.clone(),
            model_name_raw: record.model_name_raw.clone(),
            last_known_battery: record.last_known_battery.map(|p| p.get()),
            last_known_at_ms: record.last_known_at_ms,
            last_path: record.path.clone(),
            first_seen_at_ms: Some(record.first_seen_at_ms),
            last_seen_at_ms: Some(record.last_seen_at_ms),
            notifications: notifications
                .iter()
                .filter(|n| n.stable_id == id)
                .map(|n| NotificationStateEntry {
                    threshold: n.threshold,
                    armed: n.armed,
                    last_notified_at_ms: n.last_notified_at_ms,
                    last_result: n.last_result.clone(),
                })
                .collect(),
            extra: Default::default(),
        });
    }
    file.prune(voltnest_storage::MAX_INDEXED_ENTRIES);
    file
}

/// 落盘：默认走 5s 节流，`force` 用于退出 / 隐藏 / 状态退化。
pub fn persist_state(shared: &HostState, now: u64, force: bool) {
    let file = build_state_file(shared);
    let mut throttle = lock(&shared.state_throttle);
    let result = if force {
        throttle.flush_now(&shared.state_store, &file, now)
    } else {
        throttle
            .flush_if_due(&shared.state_store, &file, now)
            .map(|_| ())
    };
    if let Err(err) = result {
        log::warn!("state.json 写入失败: {err}");
    }
}

/// 由设置构造引擎配置。「阈值」一词两义：状态退化阈值来自 Provider/档位，用户的 `lowThresholds` 只影响通知、不参与退化。
pub fn engine_config(settings: &Settings) -> EngineConfig {
    EngineConfig {
        sensitivity: settings.sensitivity(),
        notifications: notification_config(settings),
        ..EngineConfig::default()
    }
}

/// 由设置构造通知策略。
pub fn notification_config(settings: &Settings) -> NotificationConfig {
    NotificationConfig {
        low_battery_levels: settings.notifications.low_thresholds.to_vec(),
        ..NotificationConfig::default()
    }
    .normalized()
}

/// 应用一个 Provider 事件，返回是否需要发布快照。
fn apply_event(shared: &HostState, transport: Transport, event: &ProviderEvent, now: u64) -> bool {
    let mut engine = lock(&shared.engine);
    let visible = engine
        .apply_event(event, Some(transport), now)
        .should_publish();
    let depth = shared.queue_depth();
    if depth > QUEUE_ALERT_DEPTH && depth.is_multiple_of(QUEUE_ALERT_DEPTH) {
        engine.push_diag(
            now,
            DiagLevel::Warn,
            "event_queue_backlog",
            None,
            format!("未消费事件 {depth} 条"),
        );
    }
    visible
}

/// 启动宿主事件循环线程。
pub fn spawn_host_loop(
    app: AppHandle,
    shared: Arc<HostState>,
    rx: Receiver<HostEvent>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("voltnest-host".into())
        .spawn(move || {
            let mut last_tick = clock::now_utc_ms();
            let mut deadline: Option<u64> = None;
            let mut dirty = false;

            loop {
                let now = clock::now_utc_ms();
                let tick_at = last_tick.saturating_add(TICK_MS);
                let due = deadline.map_or(tick_at, |d| d.min(tick_at));
                let wait = due.saturating_sub(now).clamp(1, 200);

                match rx.recv_timeout(std::time::Duration::from_millis(wait)) {
                    Ok(HostEvent::Provider { transport, event }) => {
                        shared.mark_dequeued();
                        dirty |= apply_event(&shared, transport, &event, clock::now_utc_ms());
                    }
                    Ok(HostEvent::Wake) => dirty = true,
                    Ok(HostEvent::Diagnostics) => emit_diagnostics(&app),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }

                while let Ok(next) = rx.try_recv() {
                    match next {
                        HostEvent::Provider { transport, event } => {
                            shared.mark_dequeued();
                            dirty |= apply_event(&shared, transport, &event, clock::now_utc_ms());
                        }
                        HostEvent::Wake => dirty = true,
                        HostEvent::Diagnostics => emit_diagnostics(&app),
                    }
                }

                let now = clock::now_utc_ms();
                if now >= tick_at {
                    last_tick = now;
                    let mut engine = lock(&shared.engine);
                    dirty |= engine.tick(now);
                }

                if dirty && deadline.is_none() {
                    deadline = Some(now.saturating_add(SNAPSHOT_WINDOW_MS));
                }
                if let Some(at) = deadline
                    && now >= at
                {
                    publish_snapshot(&app, &shared, now);
                    deadline = None;
                    dirty = false;
                }
            }

            log::info!("宿主事件循环退出");
        })
        .expect("创建宿主事件循环失败")
}
