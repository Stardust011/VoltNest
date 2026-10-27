//! BLE Provider 运行时：1 协调线程 + 2 I/O 执行器。
//!
//! 线程数依据：单线程巡检最坏 8×(4s 建链+3s 发现+2s 读取)=72s，已超 60s 保底周期；2 执行器最坏 4×9s=36s，留余量。`MAX_CONCURRENT == IO_WORKERS == 2`，无需额外信号量。
//!
//! 行为约束：
//! - **只跟随不拉起**：仅枚举「Windows 已连接」的设备建会话；对已配对未连接设备调 `connect()` 实测 7.97s 才失败。
//! - **断链事件优先、巡检兜底**：链路断开由 `CentralEvent::DeviceDisconnected` 实时上报，
//!   60s 巡检只负责「漏事件时最终一致」与「发现新设备」，见下文《实时断链检测》。
//! - **枚举必须走 `retrieve_peripherals`**：`Adapter::peripherals()` 只返回本次会话**扫描到的广播设备**
//!   （btleplug WinRT 后端的 manager 缓存，由 `start_scan` 填充），已连接设备通常不再广播 → 恒为空。
//!   实测同一台鼠标：`peripherals()` = 0 台，`retrieve_peripherals(services=[0x180F])` = 1 台。
//!   （以上是 Windows/WinRT 的结论；bluez 的枚举来源完全不同，见下方复核表。）
//! - **只读**：只 READ + 订阅 `0x2A19`，不写特征、不触发配对。
//! - **负缓存**：无 `0x180F` 的设备 30 分钟内不再摸。
//! - **名称只取 GAP `0x2A00`**：不用 Windows 缓存的 `local_name`，也不用 bluez 的 `Device1.Alias`
//!   （两者都是用户可改的系统级名字）。
//!
//! # 实时断链检测
//!
//! 检测原理：链路一断，系统链路层（supervision timeout 到期或收到 HCI Disconnect Complete）就会
//! 把「已连接」状态翻成 false，btleplug 把这个变化翻成 `CentralEvent::DeviceDisconnected`，
//! 经 `adapter.events()` 送到这里 → 去抖 → `ProviderEvent::LinkLost`。60s 巡检保留为兜底，
//! 于是「实时性」的定义是「链路一断就收到通知」，而不是「亚秒级」——后者受限于链路层，任何库都给不了。
//!
//! 事件桥只在**协调线程之外**的一个 task 里 drain，原因：事件流容量有限且无重放
//! （Windows/Android 是 `broadcast(16)`，lag 的事件被静默丢弃），drain 侧不能有任何阻塞工作。
//!
//! 各后端语义差异（决定谁能收到事件，见 `spawn_event_bridge` 的注释）：
//!
//! | 后端 | 事件来源 | 覆盖范围 |
//! | --- | --- | --- |
//! | Windows / WinRT | `BluetoothLEDevice.ConnectionStatusChanged`，在 `connect()` 内注册 | **仅有本进程 `connect()` 过的设备** |
//! | Linux / bluez | bluer 的 `Device1.Connected` 属性变更 | 系统级，含别的进程连的设备 |
//!
//! 因此：① 任何平台都必须对本会话跟踪的设备调过 `connect()`，否则 Windows 收不到事件；
//! ② bluez 的事件可能指向本进程从未跟踪的地址，必须按会话表过滤；
//! ③ 事件只做「变化通知」，设备清单仍然只能来自 `retrieve_peripherals`（bluez 会回放已连接设备，Windows 不会，
//!    二者不可互换）。
//!
//! 重连策略：不主动拉起。断链后清掉会话与策略计数（`PolicyBook::on_link_lost`，不计失败、不进退避），
//! 等系统自己把设备重连回来，由下一轮枚举重新建会话；系统重连发生在去抖窗口内时，直接吞掉这次断链，不对外发事件。
//!
//! # 设备类型识别（与电量采集解耦）
//! 证据只来自本会话已做的事：服务发现 + 一次 GAP `0x2A01` 只读，故不额外建链、不订阅、不写特征。
//! - Appearance 与名称并发读（`tokio::join!`），共用单次读超时；失败/缺失当「未取得」，不影响 `Ready`。
//! - 判定是纯函数 `classify_ble`，在协调线程做，会话只回传原始证据。
//! - 广播的 `manufacturer_data` / `local_name` 不参与判定：前者无协议文档，后者是用户可改的系统缓存名。
//!
//! # Windows 语义在 bluez 后端下的复核（逐条对应实现处的 cfg 注释）
//!
//! 依据 btleplug 0.13.5 源码（src/bluez/*、src/api/mod.rs）与 BlueZ Device1 API 推定；
//! 当前环境（WSL2，无适配器、bluetoothd 未运行）**无法真机实测**，故只把源码可直接证实的差异
//! 写成平台分支，其余平台共用同一套逻辑。
//!
//! | Windows 既有语义 | bluez 下是否成立 | 本文件的做法 |
//! | --- | --- | --- |
//! | 枚举必须走 retrieve_peripherals；peripherals() 只回放本次扫描的广播 | **不成立**：bluez 的 peripherals() 直接查 bluetoothd 的 ObjectManager，返回全部已知设备（含未连接） | 仍用 retrieve_peripherals，但 bluez 侧不带服务选择器，改由 is_connected() 过滤 → 「只跟随已连接」 |
//! | 选择器带 services=[0x180F] 即「已连接且有电池服务」 | **只部分成立**：bluez 的 Device1.UUIDs 只在 bluetoothd 解析过服务后才含 0x180F，首次枚举可能为空 → 电池设备会被永久排除 | 不把服务当选择器；UUIDs 非空且不含 0x180F 的直接跳过（不打扰用户已连的手机/耳机），为空则留给会话内服务发现判定 |
//! | 名称只取 GAP 0x2A00，不用 local_name（Windows 注册表缓存名） | **成立且更必要**：bluez 的 Device1.Alias 同样是用户可改的系统级名字 | 不分平台，一律读 0x2A00，读不到留空 |
//! | 读取一律 Uncached，缓存值不进状态机 | **结论成立、机制不同**：bluez 的 read() → Characteristic1.ReadValue，由 bluetoothd 现场执行 ATT 读，不存在 WinRT 那层 GATT 缓存 | 无需额外参数；CachedValueRejected 在 bluez 侧不会被触发 |
//! | manager 缓存由 start_scan 填充 | **不成立**：bluez 没有这层缓存，本模块也从不 start_scan | 无影响 |
//! | 已连接设备重复 connect() 是幂等 no-op | **不成立**：bluez 对已连接设备再 Connect 返回 org.bluez.Error.AlreadyConnected | 先 is_connected()，已连接则跳过 connect()；失败清理只断开自己建立的链路 |
//! | 发现失败 = 无服务，直接负缓存 30 分钟 | **不成立**：bluez 的 discover_services() 读的是已解析的服务树，未解析时为空集，与「没有 0x180F」是两件事 | 服务列表为空时不进负缓存，按 IoTimeout 退避重试 |
//! | `DeviceDisconnected` 只覆盖本进程连的设备 | **不成立**：bluez 是 `Device1.Connected` 属性变更，系统级，含别的进程连的设备 | 事件入协调线程前按会话表过滤，未跟踪地址直接丢弃 |

use crate::bas;
use crate::policy::{Admission, BlePolicy, PolicyBook};
use crate::{
    ENUM_FAILURES_BEFORE_DOWN, EVENT_RESUBSCRIBE_DELAY_MS, INIT_TIMEOUT_MS, IO_WORKERS,
    LINK_CONFIRM_TIMEOUT_MS, LINK_LOSS_DEBOUNCE_MS,
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

/// 后端不可用时写进 `detail` 的平台提示：诊断页要能直接告诉用户该查什么。
#[cfg(target_os = "linux")]
const PLATFORM_HINT: &str = "（Linux/bluez：需 bluetoothd 运行且系统 dbus 可用；当前用户需在 bluetooth 组或有 polkit 授权；无适配器时请检查 rfkill / WSL2 蓝牙直通）";

/// 后端不可用时写进 `detail` 的平台提示：诊断页要能直接告诉用户该查什么。
#[cfg(target_os = "windows")]
const PLATFORM_HINT: &str = "（Windows：请检查蓝牙服务是否运行、适配器是否启用）";

/// 失败清理时是否可以断开链路。
///
/// Windows：列表里的设备一律由本会话 `connect()`（对已连接设备是 no-op），沿用既有的「失败即断开」行为；
/// bluez：链路是系统级共享的，把用户正在用的耳机/手机踢下线是破坏性副作用，故只断开自己建立的链路。
#[cfg(target_os = "windows")]
const DISCONNECT_SHARED_LINKS: bool = true;

/// 失败清理时是否可以断开链路，见同名的 Windows 定义。
#[cfg(target_os = "linux")]
const DISCONNECT_SHARED_LINKS: bool = false;

/// Provider 启动/运行期的结构化失败：`run` 把它原样转成 `ProviderEvent::TransportDown`。
///
/// 不 panic、不静默返回：宿主只按错误码分类，`detail` 进诊断页与日志。
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

    /// btleplug 错误 → 结构化失败。权限被拒（polkit / 不在 bluetooth 组）单独区分，
    /// 其余归 `Internal`——宿主只做分类，可读原因留在 `detail` 里。
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
            // BLE 家族能力校验保证只能是 NotifyWithPoll；其余变体给一个安全的兜底值。
            Cadence::ListenOnly { .. } | Cadence::Polled { .. } => 60_000,
        }
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
        if let Err(failure) = rt.block_on(coordinator(tx.clone(), stop, poll_ms)) {
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
    /// 去抖窗口到期后用来复核「是否真的还断着」；`is_connected()` 在 `connect()` 之后才可信。
    peripheral: Peripheral,
}

/// 协调线程退出时（含 `?` 提前返回）必须停掉事件桥，否则会一直持有事件流。
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// 事件桥：全进程唯一的事件流消费者。
///
/// 只做「取 `PeripheralId` → 转发」，不要在 drain 循环里做任何耗时工作：
/// Windows/Android 的事件是 `broadcast(16)`，消费者慢了会 lag，而 lag 错误被 btleplug 静默丢弃。
///
/// 事件流结束（后端重启、dbus 断流）后自动重订阅：实时检测不能因为一次断流就永久退化成巡检。
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
                    // 协调线程已经收场，事件桥没有存在意义。
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
/// 不能直接用 `PeripheralId` 转地址：WinRT 的 `address()` 是 `pub(crate)`（外部拿不到），
/// bluez 的 `PeripheralId` 里根本没有 MAC（只有 `DeviceId`）。也不能调 `adapter.peripheral(&id)`：
/// WinRT 后端在 emit `DeviceDisconnected` 之前就把它从 manager 表里删掉了，反查必然失败。
/// 故只能拿会话表的 `Peripheral::id()` 反查（≤ `MAX_TRACKED` 台，线性扫描足够）。
fn addr_of(sessions: &HashMap<[u8; 6], SessionHandle>, id: &PeripheralId) -> Option<[u8; 6]> {
    sessions
        .iter()
        .find(|(_, handle)| handle.peripheral.id() == *id)
        .map(|(addr, _)| *addr)
}

/// 去抖到期后的判定：只有**明确读到仍然连接**才放弃上报。
///
/// 读失败/超时按断链处理——`DeviceDisconnected` 是比一次查询更强的证据（bluez 侧查询可能刚好撞上 dbus 抖动）。
fn keep_link(reconnected: Option<bool>) -> bool {
    matches!(reconnected, Some(true))
}

/// 协调线程：枚举、策略裁决、事件映射。独占全部 per-device 状态。
async fn coordinator(
    tx: Sender<ProviderEvent>,
    stop: Arc<AtomicBool>,
    poll_ms: u64,
) -> Result<(), TransportFailure> {
    // 初始化带总超时：dbus 上的方法调用可能停在「按需拉起 bluetoothd」上，采集线程不能被它吊死。
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
    log::info!("ble-bas 启动：poll={poll_ms}ms");

    let policy_cfg = BlePolicy::default();
    let mut book = PolicyBook::new(policy_cfg);
    let semaphore = Arc::new(Semaphore::new(policy_cfg.max_concurrent));
    let (msg_tx, mut msg_rx) = tokio_mpsc::unbounded_channel::<BleSessionEvent>();

    // 事件桥必须在任何 connect() 之前启动：事件流无重放，订阅晚了会漏掉建立期间的断链。
    let _events = AbortOnDrop(spawn_event_bridge(
        adapter.clone(),
        msg_tx.clone(),
        Arc::clone(&stop),
    ));
    // 去抖中的断链：地址 → 首次收到断链事件的时刻。
    let mut pending: HashMap<[u8; 6], Instant> = HashMap::new();

    let mut sessions: HashMap<[u8; 6], SessionHandle> = HashMap::new();
    let mut announced: HashSet<[u8; 6]> = HashSet::new();
    let mut known: HashSet<[u8; 6]> = HashSet::new();
    let start = Instant::now();
    let mut last_scan: Option<Instant> = None;
    // 连续巡检失败轮数：单轮失败只跳过本轮，连续失败才判定后端整体不可用。
    let mut enum_failures: u32 = 0;

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let now = start.elapsed().as_millis() as u64;

        // 1) 会话消息 → ProviderEvent
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
                        // 暂无有出处的 BLE 侧设备映射，因此只交给标准证据判定。
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
                                stable_id, percent,
                                // BLE 充电状态协议层不可得，恒为 Unknown。
                                None,
                            )));
                        }
                        Err(code) => {
                            // 首读失败没有可复核的帧（超时 / 无服务），细节留空。
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
                            // 负缓存，否则每 60s 被摸一次，永不停止。
                            book.on_service_missing(addr, now);
                            let _ = tx.send(ProviderEvent::Unreadable {
                                stable_id,
                                code,
                                detail,
                            });
                        }
                        ErrorCode::DeviceGone => {
                            // 链路不存在 → Offline，不是 Unavailable。断链不是我方失败，不进退避。
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
                    // 未跟踪的地址一律丢弃：bluez 的事件是系统级的，别的进程连的设备也会进来。
                    let Some(addr) = addr_of(&sessions, &id) else {
                        continue;
                    };
                    // 连续事件只保留最早时刻，避免把窗口不断往后推。
                    pending.entry(addr).or_insert_with(Instant::now);
                    log::debug!(
                        "ble-bas {addr:?} 断链事件到达，进入 {LINK_LOSS_DEBOUNCE_MS}ms 去抖"
                    );
                }
                BleSessionEvent::LinkUp { id } => {
                    if let Some(addr) = addr_of(&sessions, &id)
                        && pending.remove(&addr).is_some()
                    {
                        log::info!("ble-bas {addr:?} 在去抖窗口内重连，吞掉本次断链");
                    }
                }
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

        // 1.5) 去抖到期 → 复核 → 上报。复核只信「明确仍连接」，读失败/超时都按断链处理（事件是更强证据）。
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

        // 2) 周期性重新枚举（保底轮询同时承担「出现检测」，60–300s 由 Provider 声明）
        let scan_due = last_scan.is_none_or(|t| t.elapsed() >= Duration::from_millis(poll_ms));
        if scan_due {
            last_scan = Some(Instant::now());
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
            )
            .await
            {
                Ok(()) => {
                    if enum_failures > 0 {
                        log::info!("ble-bas 巡检恢复：此前连续失败 {enum_failures} 轮");
                        enum_failures = 0;
                    }
                }
                Err(e) => {
                    // 单轮失败很常见（设备刚好消失、dbus 抖一下）：不能因此把「枚举不到」
                    // 当成「设备都没了」去发 Removed——那会让所有图标闪断。跳过本轮，
                    // 连续失败才判定后端整体不可用，交给 supervisor 退避重启。
                    enum_failures = enum_failures.saturating_add(1);
                    let failure = TransportFailure::from_btleplug("巡检失败", &e);
                    log::warn!(
                        "ble-bas 巡检失败（第 {enum_failures}/{ENUM_FAILURES_BEFORE_DOWN} 轮）：{}",
                        failure.detail
                    );
                    if enum_failures >= ENUM_FAILURES_BEFORE_DOWN {
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

/// 候选设备枚举。
///
/// Windows：必须用 `retrieve_peripherals` 的「已连接 + 含 `0x180F`」选择器（WinRT 的已连接设备选择器）。
#[cfg(target_os = "windows")]
async fn candidate_peripherals(adapter: &Adapter) -> Result<Vec<Peripheral>, btleplug::Error> {
    // 必须用 `retrieve_peripherals`（WinRT「已连接设备」选择器）而非 `peripherals()`：
    // 后者只回放本次会话扫描到的广播，已连接的 BLE 设备通常不再广播，列表恒为空。
    // 服务过滤放在选择器里，Windows 侧只返回带 Battery Service 的已连接设备。
    adapter
        .retrieve_peripherals(RetrievePeripheralsOptions {
            identifiers: None,
            services: Some(vec![uuid16(bas::SERVICE_BATTERY)]),
        })
        .await
}

/// 候选设备枚举。
///
/// bluez：`Device1.Connected` 才是「已在链上」的权威判据，而 `Device1.UUIDs` 只在 bluetoothd
/// 解析过服务后才含 `0x180F`，把它当选择器会让电池设备在首次枚举时被永久排除（详见模块文档复核表）。
/// 故这里取全量已知设备 → 本地按「已连接」过滤 → 再按 `UUIDs` 排除明确不是电池设备的设备。
#[cfg(target_os = "linux")]
async fn candidate_peripherals(adapter: &Adapter) -> Result<Vec<Peripheral>, btleplug::Error> {
    let all = adapter
        .retrieve_peripherals(RetrievePeripheralsOptions::default())
        .await?;
    let mut connected = Vec::new();
    for p in all {
        // 查询失败的设备（刚消失）当作未连接，不让单台设备打断整轮枚举。
        if matches!(p.is_connected().await, Ok(true)) && worth_probing(&p).await {
            connected.push(p);
        }
    }
    Ok(connected)
}

/// 已连接的设备是否值得建会话。
///
/// bluez 的 `UUIDs` 已解析且不含 `0x180F` → 不是电池设备，直接跳过：不去读它的特征，
/// 更不会在失败清理时打扰用户正在用的耳机/手机。`UUIDs` 为空 = 尚未解析，交给会话内服务发现判定。
/// Windows 侧同样的筛选由 `retrieve_peripherals` 的选择器完成，故本函数只在 Linux 编译。
#[cfg(target_os = "linux")]
async fn worth_probing(p: &Peripheral) -> bool {
    match p.properties().await {
        Ok(Some(props)) => worth_probing_uuids(&props.services),
        // 拿不到属性就不据此排除：宁可多发现一次，也不误杀电池设备。
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
        // 列表即「当前已连接」：WinRT 侧由选择器保证，bluez 侧由 is_connected() 保证，
        // 故不对列表外设备调 connect()（实测失败路径 7.97s）；列表内残留的失效设备由 4s 连接超时兜底。
        if !matches!(book.admission(addr, now_ms), Admission::Admit) {
            log::debug!("ble-bas 跳过 {:?}：策略未放行（负缓存/退避/超限）", addr);
            continue;
        }
        // 全局同时建链 ≤ 2，没有空闲许可就等下一个周期（不排队）。
        if semaphore.available_permits() == 0 {
            continue;
        }

        book.track(addr);
        let task = tokio::spawn(session_task(
            p.clone(),
            addr,
            msg_tx.clone(),
            Arc::clone(semaphore),
            *book.policy(),
            poll_ms,
        ));
        // 会话句柄保留 `Peripheral`：事件桥按 `Peripheral::id()` 反查地址，去抖到期还要用它复核连接态。
        sessions.insert(
            addr,
            SessionHandle {
                task,
                peripheral: p,
            },
        );
    }

    // 消失的设备：retrieve 不再返回 → Removed（唯一的图标销毁路径）。
    let vanished: Vec<[u8; 6]> = known
        .iter()
        .filter(|addr| !seen.contains(*addr))
        .copied()
        .collect();
    for addr in vanished {
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
        // 链路是别的进程/用户建立的，断开它等于把用户设备踢下线。
        return;
    }
    let _ = peripheral.disconnect().await;
}

/// 单设备会话：建链 → 发现服务 → 读名称 → 订阅通知 → 保底轮询。
///
/// 它是**无状态执行器**：只回传结果，不写任何共享状态（D6）。
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

    // 建链阶段持有并发许可；订阅成功后立即释放，让通知在更多设备上流动。
    let permit = match semaphore.acquire_owned().await {
        Ok(p) => p,
        Err(_) => return,
    };

    // 设备是否早就连着了：只有 bluez 侧需要区分（它决定 connect() 能否调用、失败清理能否断开）。
    // Windows：列表本身即「已连接」，connect() 对已连接设备是幂等 no-op，行为保持不变。
    #[cfg(target_os = "windows")]
    let already_connected = false;
    #[cfg(target_os = "linux")]
    let already_connected = matches!(peripheral.is_connected().await, Ok(true));

    let connect: Result<Result<(), btleplug::Error>, Elapsed> = if already_connected {
        // bluez：已连接的设备再 Connect 会返回 org.bluez.Error.AlreadyConnected，
        // 而「只跟随不拉起」的前提正是「设备本来就在链上」→ 必须跳过，否则每台设备都判建链失败。
        Ok(Ok(()))
    } else {
        // connect 必须自带超时（实测无链路的已配对设备要 7.97s 才返回）。
        timeout(
            Duration::from_millis(policy.connect_timeout_ms),
            peripheral.connect(),
        )
        .await
    };
    #[cfg(target_os = "windows")]
    let connect_ok = matches!(connect, Ok(Ok(())));
    #[cfg(target_os = "linux")]
    let connect_ok = matches!(connect, Ok(Ok(())))
        // 与上面的预检存在竞态：别的进程刚好把设备连上 → bluez 返回 AlreadyConnected。
        // 链路确实在就不是失败；链路不在时 is_connected() 为 false，仍然判失败。
        || matches!(peripheral.is_connected().await, Ok(true));
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
    // bluez：discover_services() 读的是 bluetoothd 已解析的服务树，设备刚连上还没解析时得到空集。
    // 空集 ≠「没有 0x180F」：判成 ServiceMissing 会把一台电池设备拉黑 30 分钟（Windows 侧维持既有分支）。
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
        // 无 Battery Service：进入负缓存，30 分钟内不再尝试。
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

    // 名称只取 GAP 0x2A00，读不到留空，绝不回退 Windows 缓存名。
    // Appearance 0x2A01 与名称并发读（tokio::join!）：都只读、可缺省，最坏仍是一次读超时，不翻倍「建会话→首读」时延。
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

    // 订阅失败不影响轮询路径：降级为「只轮询」，并把原因留给诊断。
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
                                // 通知帧被判不可读：字节带进诊断，否则只剩一个错误码。
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
                        // 通知流是 broadcast：链路断开时发送端仍活着，流不会结束。
                        // 这里只覆盖「本进程内发送端全部析构」，断链判定由事件桥与巡检负责。
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

    // 没有通知流（订阅失败或后端不支持）：退化为纯轮询。
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

/// 读取 `0x2A19`。btleplug 在 Windows 上走 `ReadValueAsync`（非缓存读取路径），
/// 因此返回值不是 GATT 缓存；缓存值只允许出现在诊断页。
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
///
/// 服务发现会同时返回标准服务与厂商自定义服务，证据里只收标准短 UUID，
/// 避免把私有 UUID 当成「标准服务」参与说明。
fn sig_u16(uuid: &Uuid) -> Option<u16> {
    const SIG_BASE_LOW: u128 = 0x0000_1000_8000_0080_5f9b_34fb;
    const LOW_96_MASK: u128 = 0x0000_0000_ffff_ffff_ffff_ffff_ffff_ffff;
    let raw = uuid.as_u128();
    let short = raw >> 96;
    (short <= 0xffff && (raw & LOW_96_MASK) == SIG_BASE_LOW).then_some(short as u16)
}

/// 读取一个可选特征：特征缺失、无 READ 属性、超时、读失败一律 `None`。
///
/// 「读不到」与「读到空值」必须区分开：这里返回 `None` 后，调用方走
/// 「未取得」分支（分类里的证据会写明原因），绝不补默认值。
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

    /// 权限被拒必须与「后端不可用」区分开：前者只能靠用户改权限解决。
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

    /// 去抖到期后只有「明确仍连接」才取消上报：读失败不能当成还连着。
    #[test]
    fn keep_link_only_trusts_explicit_true() {
        assert!(keep_link(Some(true)));
        assert!(!keep_link(Some(false)));
        assert!(!keep_link(None), "查询失败按断链处理");
    }

    /// bluez 的候选筛选：UUIDs 为空 = 尚未解析，必须放行；已解析且不含 0x180F 才排除。
    #[test]
    fn worth_probing_uses_battery_service_when_uuids_known() {
        assert!(worth_probing_uuids(&[]), "UUIDs 未解析时不得排除设备");
        assert!(worth_probing_uuids(&[uuid16(bas::SERVICE_BATTERY)]));
        assert!(worth_probing_uuids(&[
            uuid16(bas::SERVICE_GENERIC_ACCESS),
            uuid16(bas::SERVICE_BATTERY),
        ]));
        // 耳机/手机：已解析但确实没有电池服务 → 不摸。
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
        // 厂商自定义 128 位 UUID 不得被当成标准短 UUID。
        let vendor = Uuid::parse_str("6e400001-b5a3-f393-e0a9-e50e24dcca9e").unwrap();
        assert_eq!(sig_u16(&vendor), None);
        // 高 16 位非零（另一种 UUID 空间）也不得截断成短 UUID。
        let foreign = Uuid::parse_str("1234180f-0000-1000-8000-00805f9b34fb").unwrap();
        assert_eq!(sig_u16(&foreign), None);
    }
}
