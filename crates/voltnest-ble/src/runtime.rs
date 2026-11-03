//! BLE Provider 运行时：1 个协调线程 + 2 个 I/O 执行器，per-device 状态只归协调线程所有。
//!
//! 只跟随不拉起：仅枚举系统已连接的设备，只读 `0x2A19` 与 GAP `0x2A00`，不写特征、不触发配对；
//! 断链由 `CentralEvent::DeviceDisconnected` 实时上报，60s 巡检只做兜底与新设备发现。
//! Windows（WinRT）与 Linux（bluez）在枚举来源、`connect()` 幂等性、事件覆盖范围与失败清理上语义不同，
//! 差异逐条写在对应实现的 `cfg` 分支注释里；新增平台需逐条核对 btleplug 各后端语义。

use crate::bas;
use crate::policy::{Admission, BlePolicy, PolicyBook};
use crate::{
    ENUM_FAILURES_BEFORE_DOWN, ENUM_FAILURES_MIN_WINDOW_MS, EVENT_RESUBSCRIBE_DELAY_MS,
    INIT_TIMEOUT_MS, IO_WORKERS, LINK_CONFIRM_TIMEOUT_MS, LINK_LOSS_DEBOUNCE_MS,
    PRESENCE_MISSES_BEFORE_REMOVED,
};
use btleplug::api::{
    Central, CentralEvent, CentralState, CharPropFlags, Manager as _, Peripheral as _,
    RetrievePeripheralsOptions,
};
use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
use futures::StreamExt;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::time::{error::Elapsed, timeout};
use uuid::Uuid;
use voltnest_provider::{
    BLE_BAS, BleEvidence, Cadence, DIAG_BYTES, DeviceFact, ErrorCode, FAMILY_BLE_BAS, Percent,
    Provider, ProviderEvent, ProviderProfile, RemovalReason, StableId, Transport, ValidReading,
    classify_ble, hex_bytes,
};

/// 后端不可用时写进 `detail` 的平台提示，供诊断页直接告诉用户该查什么。
#[cfg(target_os = "linux")]
const PLATFORM_HINT: &str = "（Linux/bluez：需 bluetoothd 运行且系统 dbus 可用；当前用户需在 bluetooth 组或有 polkit 授权；无适配器时请检查 rfkill / WSL2 蓝牙直通）";

/// 后端不可用时写进 `detail` 的平台提示，供诊断页直接告诉用户该查什么。
#[cfg(target_os = "windows")]
const PLATFORM_HINT: &str = "（Windows：请检查蓝牙服务是否运行、适配器是否启用）";

/// 失败清理时是否可以断开链路（Windows：列表内设备均由本会话 `connect()`，允许断开）。
#[cfg(target_os = "windows")]
const DISCONNECT_SHARED_LINKS: bool = true;

/// 失败清理时是否可以断开链路（bluez：链路系统级共享，只断开自己建立的）。
#[cfg(target_os = "linux")]
const DISCONNECT_SHARED_LINKS: bool = false;

/// Provider 启动/运行期的结构化失败：`run` 把它原样转成 `ProviderEvent::TransportDown`。
///
/// 不 panic、不静默返回；宿主只按 `code` 分类，`detail` 进诊断页与日志。
#[derive(Debug)]
struct TransportFailure {
    code: ErrorCode,
    detail: String,
}

impl TransportFailure {
    fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    /// btleplug 错误 → 结构化失败：权限被拒映射为 `AccessDenied`，其余归 `Internal`，
    /// 可读原因留在 `detail` 里。
    fn from_btleplug(context: &str, e: &btleplug::Error) -> Self {
        let code = match e {
            btleplug::Error::PermissionDenied => ErrorCode::AccessDenied,
            _ => ErrorCode::Internal,
        };
        Self::new(code, format!("{context}: {e}"))
    }
}

/// `ble-bas` 的 Provider 实现。
#[derive(Debug, Clone, Copy)]
pub struct BleProvider {
    profile: &'static ProviderProfile,
}

impl BleProvider {
    /// 使用固定的 `ble-bas` Profile。
    pub fn new() -> Self {
        Self { profile: &BLE_BAS }
    }

    fn poll_interval_ms(&self) -> u64 {
        match self.profile.cadence {
            Cadence::NotifyWithPoll { poll } => poll.default_ms,
            Cadence::ListenOnly { .. } | Cadence::Polled { .. } => 60_000,
        }
    }

    /// 出现检测（枚举）周期；未声明时退化为复用读数轮询周期。
    fn presence_interval_ms(&self) -> u64 {
        self.profile
            .presence_poll_ms
            .unwrap_or_else(|| self.poll_interval_ms())
    }
}

impl Default for BleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for BleProvider {
    fn profile(&self) -> &'static ProviderProfile {
        self.profile
    }

    fn run(self: Box<Self>, tx: Sender<ProviderEvent>, stop: Arc<AtomicBool>) {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(IO_WORKERS)
            .thread_name("ble-io")
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(ProviderEvent::TransportDown {
                    code: ErrorCode::Internal,
                    detail: format!("tokio runtime 创建失败: {e}"),
                });
                return;
            }
        };
        let poll_ms = self.poll_interval_ms();
        let presence_ms = self.presence_interval_ms();
        if let Err(failure) = rt.block_on(coordinator(tx.clone(), stop, poll_ms, presence_ms)) {
            log::warn!("ble-bas 不可用（{}）：{}", failure.code, failure.detail);
            let _ = tx.send(ProviderEvent::TransportDown {
                code: failure.code,
                detail: failure.detail,
            });
        }
    }
}

/// 会话任务 → 协调线程的消息。
#[derive(Debug)]
enum BleSessionEvent {
    Ready {
        addr: [u8; 6],
        name: Option<String>,
        battery: Result<Percent, ErrorCode>,
        /// 类型识别证据（服务发现结果 + 实际读到的 Appearance）。
        evidence: BleEvidence,
    },
    Reading {
        addr: [u8; 6],
        battery: Percent,
    },
    Failed {
        addr: [u8; 6],
        code: ErrorCode,
        /// 诊断细节（被拒的通知帧字节）；无可用数据时为 `None`。
        detail: Option<String>,
    },
    /// 事件桥：链路已断（未去抖、未确认）。
    LinkDown {
        id: PeripheralId,
    },
    /// 事件桥：链路已恢复，用于在同一去抖窗口内抵消 `LinkDown`。
    LinkUp {
        id: PeripheralId,
    },
    /// 事件桥：适配器掉电/被移除，所有会话立即失效。
    AdapterOff,
}

struct SessionHandle {
    task: tokio::task::JoinHandle<()>,
    /// 去抖窗口到期后用来复核「是否真的还断着」。
    peripheral: Peripheral,
}

/// 协调线程退出时（含 `?` 提前返回）停掉事件桥，避免一直持有事件流。
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// 事件桥：全进程唯一的事件流消费者，只做「取 `PeripheralId` → 转发」。
///
/// drain 循环里不能有阻塞或耗时工作（事件流容量有限、lag 会被静默丢弃）；
/// 事件流结束后自动重订阅，避免实时检测永久退化成巡检。
fn spawn_event_bridge(
    adapter: Adapter,
    tx: tokio_mpsc::UnboundedSender<BleSessionEvent>,
    stop: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while !stop.load(Ordering::Relaxed) {
            let mut events =
                match timeout(Duration::from_millis(INIT_TIMEOUT_MS), adapter.events()).await {
                    Ok(Ok(events)) => events,
                    Ok(Err(e)) => {
                        log::warn!("ble-bas 事件流订阅失败（实时断链不可用，退回巡检）：{e}");
                        tokio::time::sleep(Duration::from_millis(EVENT_RESUBSCRIBE_DELAY_MS)).await;
                        continue;
                    }
                    Err(_) => {
                        log::warn!("ble-bas 事件流订阅超时（实时断链不可用，退回巡检）");
                        tokio::time::sleep(Duration::from_millis(EVENT_RESUBSCRIBE_DELAY_MS)).await;
                        continue;
                    }
                };

            while let Some(event) = events.next().await {
                let msg = match event {
                    CentralEvent::DeviceDisconnected(id) => BleSessionEvent::LinkDown { id },
                    CentralEvent::DeviceConnected(id) => BleSessionEvent::LinkUp { id },
                    CentralEvent::StateUpdate(CentralState::PoweredOff) => {
                        BleSessionEvent::AdapterOff
                    }
                    _ => continue,
                };
                if tx.send(msg).is_err() {
                    return;
                }
            }

            if !stop.load(Ordering::Relaxed) {
                log::warn!("ble-bas 事件流结束，{EVENT_RESUBSCRIBE_DELAY_MS}ms 后重订阅");
                tokio::time::sleep(Duration::from_millis(EVENT_RESUBSCRIBE_DELAY_MS)).await;
            }
        }
    })
}

/// 事件里的 `PeripheralId` → 本会话跟踪的地址。
///
/// 不能靠地址转换或 `adapter.peripheral(&id)` 反查，只能线性扫描会话表比对 `Peripheral::id()`
/// （≤ `MAX_TRACKED` 台，足够）。
fn addr_of(sessions: &HashMap<[u8; 6], SessionHandle>, id: &PeripheralId) -> Option<[u8; 6]> {
    sessions
        .iter()
        .find(|(_, handle)| handle.peripheral.id() == *id)
        .map(|(addr, _)| *addr)
}

/// 去抖到期后的判定：只有明确读到仍然连接才放弃上报；读失败/超时按断链处理。
fn keep_link(reconnected: Option<bool>) -> bool {
    matches!(reconnected, Some(true))
}

/// 协调线程：枚举、策略裁决、事件映射，独占全部 per-device 状态。
///
/// `poll_ms` 只决定读数节奏，`presence_ms` 决定枚举节奏，两者必须分开。
async fn coordinator(
    tx: Sender<ProviderEvent>,
    stop: Arc<AtomicBool>,
    poll_ms: u64,
    presence_ms: u64,
) -> Result<(), TransportFailure> {
    let manager = timeout(Duration::from_millis(INIT_TIMEOUT_MS), Manager::new())
        .await
        .map_err(|_| {
            TransportFailure::new(
                ErrorCode::Internal,
                format!("btleplug Manager 初始化超时（{INIT_TIMEOUT_MS}ms）{PLATFORM_HINT}"),
            )
        })?
        .map_err(|e| {
            TransportFailure::from_btleplug(
                &format!("btleplug Manager 初始化失败{PLATFORM_HINT}"),
                &e,
            )
        })?;
    let adapters = timeout(Duration::from_millis(INIT_TIMEOUT_MS), manager.adapters())
        .await
        .map_err(|_| {
            TransportFailure::new(
                ErrorCode::Internal,
                format!("枚举适配器超时（{INIT_TIMEOUT_MS}ms）{PLATFORM_HINT}"),
            )
        })?
        .map_err(|e| {
            TransportFailure::from_btleplug(&format!("枚举适配器失败{PLATFORM_HINT}"), &e)
        })?;
    let adapter = adapters.into_iter().next().ok_or_else(|| {
        TransportFailure::new(
            ErrorCode::Internal,
            format!("系统没有可用的 BLE 适配器{PLATFORM_HINT}"),
        )
    })?;
    log::info!("ble-bas 启动：poll={poll_ms}ms presence={presence_ms}ms");

    let policy_cfg = BlePolicy::default();
    let mut book = PolicyBook::new(policy_cfg);
    let semaphore = Arc::new(Semaphore::new(policy_cfg.max_concurrent));
    let (msg_tx, mut msg_rx) = tokio_mpsc::unbounded_channel::<BleSessionEvent>();

    let _events = AbortOnDrop(spawn_event_bridge(
        adapter.clone(),
        msg_tx.clone(),
        Arc::clone(&stop),
    ));
    let mut pending: HashMap<[u8; 6], Instant> = HashMap::new();

    let mut sessions: HashMap<[u8; 6], SessionHandle> = HashMap::new();
    let mut announced: HashSet<[u8; 6]> = HashSet::new();
    let mut known: HashSet<[u8; 6]> = HashSet::new();
    let mut misses: HashMap<[u8; 6], u32> = HashMap::new();
    let start = Instant::now();
    let mut last_presence: Option<Instant> = None;
    let mut rescan_pending = false;
    let mut enum_failures: u32 = 0;
    let mut enum_failing_since: Option<Instant> = None;

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let now = start.elapsed().as_millis() as u64;

        while let Ok(msg) = msg_rx.try_recv() {
            match msg {
                BleSessionEvent::Ready {
                    addr,
                    name,
                    battery,
                    evidence,
                } => {
                    book.on_success(addr);
                    let stable_id = StableId::ble(addr);
                    let display = name
                        .clone()
                        .unwrap_or_else(|| stable_id.as_str().to_string());
                    if announced.insert(addr) {
                        let classification = classify_ble(&evidence, None);
                        let fact = DeviceFact::new(
                            stable_id.clone(),
                            Transport::Ble,
                            FAMILY_BLE_BAS,
                            display.clone(),
                            name.unwrap_or_default(),
                        )
                        .with_path(stable_id.as_str().to_string())
                        .with_classification(classification);
                        let _ = tx.send(ProviderEvent::Arrived(fact));
                    }
                    match battery {
                        Ok(percent) => {
                            let _ = tx.send(ProviderEvent::Reading(ValidReading::new(
                                stable_id, percent, None,
                            )));
                        }
                        Err(code) => {
                            let _ = tx.send(ProviderEvent::Unreadable {
                                stable_id,
                                code,
                                detail: None,
                            });
                        }
                    }
                }
                BleSessionEvent::Reading { addr, battery } => {
                    let _ = tx.send(ProviderEvent::Reading(ValidReading::new(
                        StableId::ble(addr),
                        battery,
                        None,
                    )));
                }
                BleSessionEvent::Failed { addr, code, detail } => {
                    if let Some(handle) = sessions.remove(&addr) {
                        handle.task.abort();
                    }
                    announced.remove(&addr);
                    let stable_id = StableId::ble(addr);
                    match code {
                        ErrorCode::ServiceMissing => {
                            book.on_service_missing(addr, now);
                            let _ = tx.send(ProviderEvent::Unreadable {
                                stable_id,
                                code,
                                detail,
                            });
                        }
                        ErrorCode::DeviceGone => {
                            book.on_link_lost(addr);
                            let _ = tx.send(ProviderEvent::LinkLost { stable_id, code });
                        }
                        other => {
                            book.on_failure(addr, now, other);
                            let _ = tx.send(ProviderEvent::Unreadable {
                                stable_id,
                                code: other,
                                detail,
                            });
                        }
                    }
                }
                BleSessionEvent::LinkDown { id } => {
                    let Some(addr) = addr_of(&sessions, &id) else {
                        continue;
                    };
                    pending.entry(addr).or_insert_with(Instant::now);
                    log::debug!(
                        "ble-bas {addr:?} 断链事件到达，进入 {LINK_LOSS_DEBOUNCE_MS}ms 去抖"
                    );
                }
                BleSessionEvent::LinkUp { id } => match addr_of(&sessions, &id) {
                    Some(addr) => {
                        if pending.remove(&addr).is_some() {
                            log::info!("ble-bas {addr:?} 在去抖窗口内重连，吞掉本次断链");
                        }
                    }
                    None => rescan_pending = true,
                },
                BleSessionEvent::AdapterOff => {
                    log::warn!("ble-bas 适配器掉电：{} 个会话立即断链", sessions.len());
                    pending.clear();
                    for addr in sessions.keys().copied().collect::<Vec<_>>() {
                        if let Some(handle) = sessions.remove(&addr) {
                            handle.task.abort();
                        }
                        announced.remove(&addr);
                        book.on_link_lost(addr);
                        let _ = tx.send(ProviderEvent::LinkLost {
                            stable_id: StableId::ble(addr),
                            code: ErrorCode::DeviceGone,
                        });
                    }
                }
            }
        }

        let due: Vec<[u8; 6]> = pending
            .iter()
            .filter(|(_, at)| at.elapsed() >= Duration::from_millis(LINK_LOSS_DEBOUNCE_MS))
            .map(|(addr, _)| *addr)
            .collect();
        for addr in due {
            pending.remove(&addr);
            let Some(peripheral) = sessions.get(&addr).map(|h| h.peripheral.clone()) else {
                continue;
            };
            let reconnected = timeout(
                Duration::from_millis(LINK_CONFIRM_TIMEOUT_MS),
                peripheral.is_connected(),
            )
            .await
            .ok()
            .and_then(Result::ok);
            if keep_link(reconnected) {
                log::info!("ble-bas {:?} 去抖到期时已重连，取消断链上报", addr);
                continue;
            }
            if let Some(handle) = sessions.remove(&addr) {
                handle.task.abort();
            }
            announced.remove(&addr);
            book.on_link_lost(addr);
            log::info!("ble-bas {:?} 链路断开（事件驱动）", addr);
            let _ = tx.send(ProviderEvent::LinkLost {
                stable_id: StableId::ble(addr),
                code: ErrorCode::DeviceGone,
            });
        }

        let scan_due = rescan_pending
            || last_presence.is_none_or(|t| t.elapsed() >= Duration::from_millis(presence_ms));
        if scan_due {
            rescan_pending = false;
            last_presence = Some(Instant::now());
            match scan_and_spawn(
                &adapter,
                &mut book,
                &mut sessions,
                &mut announced,
                &mut known,
                &msg_tx,
                &semaphore,
                &tx,
                now,
                poll_ms,
                &mut misses,
            )
            .await
            {
                Ok(()) => {
                    if enum_failures > 0 {
                        log::info!("ble-bas 巡检恢复：此前连续失败 {enum_failures} 轮");
                        enum_failures = 0;
                        enum_failing_since = None;
                    }
                }
                Err(e) => {
                    if enum_failing_since.is_none() {
                        enum_failing_since = Some(Instant::now());
                    }
                    enum_failures = enum_failures.saturating_add(1);
                    let failure = TransportFailure::from_btleplug("巡检失败", &e);
                    log::warn!(
                        "ble-bas 巡检失败（第 {enum_failures}/{ENUM_FAILURES_BEFORE_DOWN} 轮）：{}",
                        failure.detail
                    );
                    let window_elapsed = enum_failing_since.is_some_and(|t| {
                        t.elapsed() >= Duration::from_millis(ENUM_FAILURES_MIN_WINDOW_MS)
                    });
                    if enum_failures >= ENUM_FAILURES_BEFORE_DOWN && window_elapsed {
                        return Err(failure);
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    for (_, handle) in sessions.drain() {
        handle.task.abort();
    }
    Ok(())
}

/// 候选设备枚举：`retrieve_peripherals` 的「已连接 + 含 `0x180F`」选择器（WinRT 的已连接设备选择器）。
#[cfg(target_os = "windows")]
async fn candidate_peripherals(adapter: &Adapter) -> Result<Vec<Peripheral>, btleplug::Error> {
    adapter
        .retrieve_peripherals(RetrievePeripheralsOptions {
            identifiers: None,
            services: Some(vec![uuid16(bas::SERVICE_BATTERY)]),
        })
        .await
}

/// 候选设备枚举：取全量已知设备 → 按 `Device1.Connected` 过滤 → 按 `UUIDs` 排除明确非电池设备。
///
/// bluez 的 `Device1.UUIDs` 在服务解析前为空，当选择器会永久排除电池设备，故不用服务选择器。
#[cfg(target_os = "linux")]
async fn candidate_peripherals(adapter: &Adapter) -> Result<Vec<Peripheral>, btleplug::Error> {
    let all = adapter
        .retrieve_peripherals(RetrievePeripheralsOptions::default())
        .await?;
    let mut connected = Vec::new();
    for p in all {
        if matches!(p.is_connected().await, Ok(true)) && worth_probing(&p).await {
            connected.push(p);
        }
    }
    Ok(connected)
}

/// 已连接的设备是否值得建会话：`UUIDs` 已解析且不含 `0x180F` 则跳过，为空（尚未解析）则放行。
///
/// 只在 Linux 编译；Windows 侧同样的筛选由 `retrieve_peripherals` 的选择器完成。
#[cfg(target_os = "linux")]
async fn worth_probing(p: &Peripheral) -> bool {
    match p.properties().await {
        Ok(Some(props)) => worth_probing_uuids(&props.services),
        _ => true,
    }
}

/// `worth_probing` 的判定本体：`UUIDs` 为空视为「尚未解析」，放行。
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn worth_probing_uuids(uuids: &[Uuid]) -> bool {
    uuids.is_empty() || uuids.contains(&uuid16(bas::SERVICE_BATTERY))
}

#[allow(clippy::too_many_arguments)]
async fn scan_and_spawn(
    adapter: &Adapter,
    book: &mut PolicyBook,
    sessions: &mut HashMap<[u8; 6], SessionHandle>,
    announced: &mut HashSet<[u8; 6]>,
    known: &mut HashSet<[u8; 6]>,
    msg_tx: &tokio_mpsc::UnboundedSender<BleSessionEvent>,
    semaphore: &Arc<Semaphore>,
    tx: &Sender<ProviderEvent>,
    now_ms: u64,
    poll_ms: u64,
    misses: &mut HashMap<[u8; 6], u32>,
) -> Result<(), btleplug::Error> {
    let peripherals = candidate_peripherals(adapter).await?;
    log::debug!("ble-bas 巡检：候选设备 {} 台", peripherals.len());
    let mut seen: HashSet<[u8; 6]> = HashSet::new();

    for p in peripherals {
        let addr: [u8; 6] = p.address().into();
        seen.insert(addr);
        known.insert(addr);

        if sessions.contains_key(&addr) {
            continue;
        }
        if !matches!(book.admission(addr, now_ms), Admission::Admit) {
            log::debug!("ble-bas 跳过 {:?}：策略未放行（负缓存/退避/超限）", addr);
            continue;
        }
        if semaphore.available_permits() == 0 {
            continue;
        }

        book.track(addr);
        misses.remove(&addr);
        let task = tokio::spawn(session_task(
            p.clone(),
            addr,
            msg_tx.clone(),
            Arc::clone(semaphore),
            *book.policy(),
            poll_ms,
        ));
        sessions.insert(
            addr,
            SessionHandle {
                task,
                peripheral: p,
            },
        );
    }

    let vanished: Vec<[u8; 6]> = known
        .iter()
        .filter(|addr| !seen.contains(*addr))
        .copied()
        .collect();
    for addr in vanished {
        let count = misses.entry(addr).or_default();
        *count = count.saturating_add(1);
        if *count < PRESENCE_MISSES_BEFORE_REMOVED {
            log::debug!("ble-bas {addr:?} 本轮枚举不到（第 {count} 轮），暂不判消失");
            continue;
        }
        misses.remove(&addr);
        known.remove(&addr);
        sessions.remove(&addr);
        announced.remove(&addr);
        book.forget(addr);
        let _ = tx.send(ProviderEvent::Removed {
            stable_id: StableId::ble(addr),
            reason: RemovalReason::EnumerationGone,
        });
    }
    Ok(())
}

/// 失败清理：按平台决定是否断开链路，见 `DISCONNECT_SHARED_LINKS`。
async fn cleanup_link(peripheral: &Peripheral, already_connected: bool) {
    if !DISCONNECT_SHARED_LINKS && already_connected {
        return;
    }
    let _ = peripheral.disconnect().await;
}

/// 单设备会话：建链 → 发现服务 → 读名称 → 订阅通知 → 保底轮询。
///
/// 它是无状态执行器：只回传结果，不写任何共享状态。
async fn session_task(
    peripheral: Peripheral,
    addr: [u8; 6],
    msg_tx: tokio_mpsc::UnboundedSender<BleSessionEvent>,
    semaphore: Arc<Semaphore>,
    policy: BlePolicy,
    poll_ms: u64,
) {
    let fail = |code: ErrorCode| {
        let _ = msg_tx.send(BleSessionEvent::Failed {
            addr,
            code,
            detail: None,
        });
    };

    let permit = match semaphore.acquire_owned().await {
        Ok(p) => p,
        Err(_) => return,
    };

    #[cfg(target_os = "windows")]
    let already_connected = false;
    #[cfg(target_os = "linux")]
    let already_connected = matches!(peripheral.is_connected().await, Ok(true));

    let connect: Result<Result<(), btleplug::Error>, Elapsed> = if already_connected {
        Ok(Ok(()))
    } else {
        timeout(
            Duration::from_millis(policy.connect_timeout_ms),
            peripheral.connect(),
        )
        .await
    };
    #[cfg(target_os = "windows")]
    let connect_ok = matches!(connect, Ok(Ok(())));
    #[cfg(target_os = "linux")]
    let connect_ok =
        matches!(connect, Ok(Ok(()))) || matches!(peripheral.is_connected().await, Ok(true));
    if !connect_ok {
        log::warn!(
            "ble-bas 建链失败 {:?}：{}ms 内未连通",
            addr,
            policy.connect_timeout_ms
        );
        fail(ErrorCode::IoTimeout);
        return;
    }

    let discover = timeout(
        Duration::from_millis(policy.discover_timeout_ms),
        peripheral.discover_services(),
    )
    .await;
    if !matches!(discover, Ok(Ok(()))) {
        log::warn!("ble-bas 服务发现失败 {:?}", addr);
        cleanup_link(&peripheral, already_connected).await;
        fail(ErrorCode::IoTimeout);
        return;
    }

    let services = peripheral.services();
    #[cfg(target_os = "linux")]
    if services.is_empty() {
        log::info!(
            "ble-bas {:?} 服务列表为空（bluez 尚未完成服务解析），按退避重试、不进负缓存",
            addr
        );
        cleanup_link(&peripheral, already_connected).await;
        let _ = msg_tx.send(BleSessionEvent::Failed {
            addr,
            code: ErrorCode::IoTimeout,
            detail: Some("服务列表为空：bluez 尚未完成服务解析，不进负缓存".to_string()),
        });
        return;
    }
    if !services
        .iter()
        .any(|s| s.uuid == uuid16(bas::SERVICE_BATTERY))
    {
        log::info!("ble-bas {:?} 无 0x180F，进入 30 分钟负缓存", addr);
        cleanup_link(&peripheral, already_connected).await;
        fail(ErrorCode::ServiceMissing);
        return;
    }

    let chars: Vec<_> = peripheral.characteristics().into_iter().collect();
    let Some(battery_char) = chars
        .iter()
        .find(|c| c.uuid == uuid16(bas::CHAR_BATTERY_LEVEL))
        .cloned()
    else {
        log::warn!("ble-bas {:?} 有 0x180F 但缺 0x2A19", addr);
        cleanup_link(&peripheral, already_connected).await;
        fail(ErrorCode::ServiceMissing);
        return;
    };
    if !battery_char.properties.contains(CharPropFlags::READ) {
        log::warn!("ble-bas {:?} 的 0x2A19 不可读（无 READ 属性）", addr);
        cleanup_link(&peripheral, already_connected).await;
        fail(ErrorCode::MalformedFrame);
        return;
    }

    let name_char = chars
        .iter()
        .find(|c| c.uuid == uuid16(bas::CHAR_GAP_DEVICE_NAME));
    let appearance_char = chars
        .iter()
        .find(|c| c.uuid == uuid16(bas::CHAR_GAP_APPEARANCE));
    let (name_raw, appearance_raw) = tokio::join!(
        read_optional(&peripheral, name_char, policy.read_timeout_ms),
        read_optional(&peripheral, appearance_char, policy.read_timeout_ms),
    );
    let name = name_raw.as_deref().and_then(bas::parse_device_name);
    let mut evidence = BleEvidence {
        services: Vec::new(),
        appearance: appearance_raw.as_deref().and_then(bas::parse_appearance),
    };
    for service in &services {
        if let Some(short) = sig_u16(&service.uuid) {
            evidence.push_service(short);
        }
    }

    let subscribed = timeout(
        Duration::from_millis(policy.read_timeout_ms),
        peripheral.subscribe(&battery_char),
    )
    .await
    .is_ok_and(|r| r.is_ok());

    let first = read_battery(&peripheral, &battery_char, policy.read_timeout_ms).await;
    log::info!(
        "ble-bas {:?} 会话就绪：name={:?} subscribe={} 首读={:?}",
        addr,
        name,
        subscribed,
        first
    );
    let _ = msg_tx.send(BleSessionEvent::Ready {
        addr,
        name,
        battery: first,
        evidence,
    });
    drop(permit);

    let poll = Duration::from_millis(poll_ms);
    if subscribed && let Ok(mut stream) = peripheral.notifications().await {
        loop {
            tokio::select! {
                item = stream.next() => match item {
                    Some(data) if data.uuid == uuid16(bas::CHAR_BATTERY_LEVEL) => {
                        match bas::parse_battery_level(&data.value) {
                            Ok(percent) => {
                                if msg_tx.send(BleSessionEvent::Reading { addr, battery: percent }).is_err() {
                                    return;
                                }
                            }
                            Err(code) => {
                                let _ = msg_tx.send(BleSessionEvent::Failed {
                                    addr,
                                    code,
                                    detail: Some(format!(
                                        "notify {}B: {}",
                                        data.value.len(),
                                        hex_bytes(&data.value, DIAG_BYTES)
                                    )),
                                });
                                return;
                            }
                        }
                    }
                    Some(_) => {}
                    None => {
                        let _ = msg_tx.send(BleSessionEvent::Failed {
                            addr,
                            code: ErrorCode::DeviceGone,
                            detail: None,
                        });
                        return;
                    }
                },
                _ = tokio::time::sleep(poll) => {
                    if !poll_once(&peripheral, &battery_char, &msg_tx, addr, policy.read_timeout_ms).await {
                        return;
                    }
                }
            }
        }
    }

    loop {
        tokio::time::sleep(poll).await;
        if !poll_once(
            &peripheral,
            &battery_char,
            &msg_tx,
            addr,
            policy.read_timeout_ms,
        )
        .await
        {
            return;
        }
    }
}

/// 保底轮询一次；返回 `false` 表示会话应结束。
async fn poll_once(
    peripheral: &Peripheral,
    ch: &btleplug::api::Characteristic,
    msg_tx: &tokio_mpsc::UnboundedSender<BleSessionEvent>,
    addr: [u8; 6],
    read_timeout_ms: u64,
) -> bool {
    if let Ok(false) = peripheral.is_connected().await {
        let _ = msg_tx.send(BleSessionEvent::Failed {
            addr,
            code: ErrorCode::DeviceGone,
            detail: None,
        });
        return false;
    }
    match read_battery(peripheral, ch, read_timeout_ms).await {
        Ok(percent) => msg_tx
            .send(BleSessionEvent::Reading {
                addr,
                battery: percent,
            })
            .is_ok(),
        Err(code) => {
            let _ = msg_tx.send(BleSessionEvent::Failed {
                addr,
                code,
                detail: None,
            });
            false
        }
    }
}

/// 读取 `0x2A19`（走非缓存读取路径，返回值不是 GATT 缓存）。
async fn read_battery(
    peripheral: &Peripheral,
    ch: &btleplug::api::Characteristic,
    read_timeout_ms: u64,
) -> Result<Percent, ErrorCode> {
    match timeout(Duration::from_millis(read_timeout_ms), peripheral.read(ch)).await {
        Ok(Ok(bytes)) => bas::parse_battery_level(&bytes),
        Ok(Err(_)) => Err(ErrorCode::IoTimeout),
        Err(_) => Err(ErrorCode::IoTimeout),
    }
}

/// `0x180F` / `0x2A19` 等 16 位 UUID 的完整形式。
fn uuid16(short: u16) -> Uuid {
    Uuid::from_u128(u128::from(short) << 96 | 0x0000_1000_8000_0080_5f9b_34fb)
}

/// 完整 UUID → SIG 16 位短 UUID；非 SIG 分配（厂商 128 位）返回 `None`。
fn sig_u16(uuid: &Uuid) -> Option<u16> {
    const SIG_BASE_LOW: u128 = 0x0000_1000_8000_0080_5f9b_34fb;
    const LOW_96_MASK: u128 = 0x0000_0000_ffff_ffff_ffff_ffff_ffff_ffff;
    let raw = uuid.as_u128();
    let short = raw >> 96;
    (short <= 0xffff && (raw & LOW_96_MASK) == SIG_BASE_LOW).then_some(short as u16)
}

/// 读取一个可选特征：特征缺失、无 READ 属性、超时或读失败一律 `None`。
///
/// 「读不到」与「读到空值」必须区分开，返回 `None` 后由调用方走「未取得」分支，绝不补默认值。
async fn read_optional(
    peripheral: &Peripheral,
    ch: Option<&btleplug::api::Characteristic>,
    read_timeout_ms: u64,
) -> Option<Vec<u8>> {
    let ch = ch?;
    if !ch.properties.contains(CharPropFlags::READ) {
        return None;
    }
    match timeout(Duration::from_millis(read_timeout_ms), peripheral.read(ch)).await {
        Ok(Ok(bytes)) => Some(bytes),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 权限被拒映射为 `AccessDenied`，其它错误归 `Internal`。
    #[test]
    fn permission_denied_maps_to_access_denied() {
        let failure =
            TransportFailure::from_btleplug("枚举适配器失败", &btleplug::Error::PermissionDenied);
        assert_eq!(failure.code, ErrorCode::AccessDenied);
        assert!(
            failure.detail.contains("枚举适配器失败"),
            "{}",
            failure.detail
        );

        let failure =
            TransportFailure::from_btleplug("巡检失败", &btleplug::Error::NoAdapterAvailable);
        assert_eq!(failure.code, ErrorCode::Internal);
        assert!(
            failure.detail.contains("No Bluetooth adapter"),
            "{}",
            failure.detail
        );
    }

    /// 去抖到期后只有「明确仍连接」才取消上报。
    #[test]
    fn keep_link_only_trusts_explicit_true() {
        assert!(keep_link(Some(true)));
        assert!(!keep_link(Some(false)));
        assert!(!keep_link(None), "查询失败按断链处理");
    }

    /// 候选筛选：`UUIDs` 为空视为尚未解析必须放行，已解析且不含 `0x180F` 才排除。
    #[test]
    fn worth_probing_uses_battery_service_when_uuids_known() {
        assert!(worth_probing_uuids(&[]), "UUIDs 未解析时不得排除设备");
        assert!(worth_probing_uuids(&[uuid16(bas::SERVICE_BATTERY)]));
        assert!(worth_probing_uuids(&[
            uuid16(bas::SERVICE_GENERIC_ACCESS),
            uuid16(bas::SERVICE_BATTERY),
        ]));
        assert!(!worth_probing_uuids(&[uuid16(0x110E), uuid16(0x110B)]));
    }

    #[test]
    fn uuid16_matches_bluetooth_base() {
        assert_eq!(
            uuid16(0x180F).to_string(),
            "0000180f-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            uuid16(0x2A19).to_string(),
            "00002a19-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            uuid16(0x2A00).to_string(),
            "00002a00-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            uuid16(0x2A01).to_string(),
            "00002a01-0000-1000-8000-00805f9b34fb"
        );
    }

    #[test]
    fn sig_u16_round_trips_standard_uuids_only() {
        for short in [0x180F, 0x1812, 0x180A, 0x2A01] {
            let full = uuid16(short);
            assert_eq!(sig_u16(&full), Some(short), "{short:#06x}");
        }
        let vendor = Uuid::parse_str("6e400001-b5a3-f393-e0a9-e50e24dcca9e").unwrap();
        assert_eq!(sig_u16(&vendor), None);
        let foreign = Uuid::parse_str("1234180f-0000-1000-8000-00805f9b34fb").unwrap();
        assert_eq!(sig_u16(&foreign), None);
    }
}
