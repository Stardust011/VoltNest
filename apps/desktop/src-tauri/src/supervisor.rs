//! Provider 线程组监管：panic 隔离与退避重启。每个 transport 一个监管线程，只做三件事：用 `catch_unwind` 起采集线程、把事件转发宿主、异常退出时补报 `TransportDown` 并按 5s/15s/60s 退避重启，连续 5 次失败转「不可用」。
//!
//! panic 隔离在采集线程边界，不跨 transport 传播。

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use voltnest_provider::{ErrorCode, Provider, ProviderEvent, Transport};

use crate::dto::{TransportHealth, TransportStatus};
use crate::host::{HostEvent, HostState, lock};

/// 退避序列，最后一次之后一直用它。
pub const BACKOFF_MS: [u64; 3] = [5_000, 15_000, 60_000];

/// 连续失败上限：超过后标记不可用，等用户手动重启。
pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;

/// 监管指令。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SupervisorCmd {
    /// 立即重启采集线程（重置失败计数）。
    Restart,
    /// 停止并退出监管线程。
    Stop,
}

/// 一个 transport 的采集参数。
pub struct TransportSpec {
    /// 传输层。
    pub transport: Transport,
    /// 协议家族 ID（诊断展示）。
    pub family: &'static str,
    /// 构造采集器。函数指针而非闭包：每次重启都要重新构造。
    pub make: fn() -> Box<dyn Provider>,
}

impl TransportSpec {
    /// `hid-vbas`。
    pub fn hid() -> Self {
        Self {
            transport: Transport::Hid,
            family: voltnest_provider::FAMILY_HID_VBAS,
            make: || Box::new(voltnest_hid::runtime::HidProvider::new()),
        }
    }

    /// `ble-bas`。
    pub fn ble() -> Self {
        Self {
            transport: Transport::Ble,
            family: voltnest_provider::FAMILY_BLE_BAS,
            make: || Box::new(voltnest_ble::runtime::BleProvider::new()),
        }
    }
}

/// 一次运行的结果。
enum RunOutcome {
    /// 收到 `Stop` 指令正常停止。
    Stopped,
    /// 收到 `Restart` 指令，需要重置失败计数。
    Restarted,
    /// `run()` 自行返回（未收到停止指令）→ 按失败处理；携带本轮已转发的 `TransportDown`。
    Returned(Option<(ErrorCode, String)>),
    /// `run()` panic。
    Panicked(String),
}

/// 一次失败要对外表达的原因。
struct Failure {
    /// 进日志、`TransportStatus.last_error` 与 `TransportDown.detail` 的可读原因。
    detail: String,
    /// 结构化错误码。
    code: ErrorCode,
    /// 是否还需要上报 `TransportDown`（provider 本轮已自己报过就不重复）。
    report: bool,
}

/// 启动一个 transport 的监管线程，返回指令通道。
pub fn spawn_supervisor(shared: Arc<HostState>, spec: TransportSpec) -> Sender<SupervisorCmd> {
    spawn_supervisor_with_backoff(shared, spec, &BACKOFF_MS).0
}

/// 同上，但可指定退避表并拿到线程句柄（测试用毫秒级表，产品用 [`BACKOFF_MS`]）。
pub fn spawn_supervisor_with_backoff(
    shared: Arc<HostState>,
    spec: TransportSpec,
    backoff: &'static [u64],
) -> (Sender<SupervisorCmd>, std::thread::JoinHandle<()>) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let name = format!("voltnest-{}-supervisor", spec.transport.as_str());
    {
        let mut statuses = lock(&shared.transports);
        upsert(&mut statuses, &spec, TransportHealth::Running, 0, 0, None);
    }
    let handle = std::thread::Builder::new()
        .name(name)
        .spawn(move || supervise(shared, spec, cmd_rx, backoff))
        .expect("创建 Provider 监管线程失败");
    (cmd_tx, handle)
}

fn supervise(
    shared: Arc<HostState>,
    spec: TransportSpec,
    cmd_rx: Receiver<SupervisorCmd>,
    backoff: &'static [u64],
) {
    let mut failures = 0u32;
    let mut restarts = 0u32;

    'outer: loop {
        if shared.shutting_down.load(Ordering::Relaxed) {
            return;
        }
        set_status(
            &shared,
            &spec,
            TransportHealth::Running,
            failures,
            restarts,
            None,
        );

        let failure = match run_once(&shared, &spec, &cmd_rx) {
            RunOutcome::Stopped => return,
            RunOutcome::Restarted => {
                failures = 0;
                restarts = restarts.saturating_add(1);
                continue;
            }
            RunOutcome::Returned(Some((code, detail))) => Failure {
                detail,
                code,
                report: false,
            },
            RunOutcome::Returned(None) => Failure {
                detail: "采集线程自行退出".to_string(),
                code: ErrorCode::Internal,
                report: true,
            },
            RunOutcome::Panicked(msg) => Failure {
                detail: msg,
                code: ErrorCode::Internal,
                report: true,
            },
        };
        let Failure {
            detail,
            code,
            report,
        } = failure;

        failures = failures.saturating_add(1);
        restarts = restarts.saturating_add(1);
        log::error!(
            "{} 采集线程失败（连续 {failures} 次）：{detail}",
            spec.transport.as_str()
        );
        if report {
            push_down(&shared, &spec, code, &detail);
        }
        set_status(
            &shared,
            &spec,
            TransportHealth::Down,
            failures,
            restarts,
            Some(detail.clone()),
        );

        match backoff_for(failures, backoff) {
            None => {
                set_status(
                    &shared,
                    &spec,
                    TransportHealth::Unavailable,
                    failures,
                    restarts,
                    Some(detail.clone()),
                );
                {
                    let mut engine = lock(&shared.engine);
                    engine.push_diag(
                        voltnest_provider::clock::now_utc_ms(),
                        voltnest_core::DiagLevel::Error,
                        "transport_unavailable",
                        None,
                        format!(
                            "{} 连续 {failures} 次失败，已停止自动重启：{detail}",
                            spec.transport.display_label()
                        ),
                    );
                }
                loop {
                    if shared.shutting_down.load(Ordering::Relaxed) {
                        return;
                    }
                    match cmd_rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(SupervisorCmd::Restart) => {
                            failures = 0;
                            restarts = restarts.saturating_add(1);
                            continue 'outer;
                        }
                        Ok(SupervisorCmd::Stop)
                        | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            return;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
            }
            Some(delay) => match wait_cmd(&cmd_rx, delay, &shared) {
                Wait::Command(SupervisorCmd::Restart) => failures = 0,
                Wait::Command(SupervisorCmd::Stop) | Wait::Exit => return,
                Wait::Elapsed => {}
            },
        }
    }
}

/// 第 `failures` 次连续失败后的退避时长；达到上限返回 `None`（转人工重启）。
pub fn backoff_for(failures: u32, table: &[u64]) -> Option<u64> {
    if failures == 0 || failures >= MAX_CONSECUTIVE_FAILURES || table.is_empty() {
        return None;
    }
    let last = table.len() - 1;
    Some(table[(failures.saturating_sub(1) as usize).min(last)])
}

/// 退避等待的结果。
enum Wait {
    /// 收到指令。
    Command(SupervisorCmd),
    /// 延时走完，可以重启采集线程。
    Elapsed,
    /// 宿主正在退出。
    Exit,
}

fn wait_cmd(cmd_rx: &Receiver<SupervisorCmd>, delay_ms: u64, shared: &HostState) -> Wait {
    let mut left = delay_ms;
    while left > 0 {
        if shared.shutting_down.load(Ordering::Relaxed) {
            return Wait::Exit;
        }
        let step = left.min(200);
        match cmd_rx.recv_timeout(Duration::from_millis(step)) {
            Ok(cmd) => return Wait::Command(cmd),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => left -= step,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Wait::Exit,
        }
    }
    Wait::Elapsed
}

/// 起一次采集线程并把它跑完。
fn run_once(
    shared: &Arc<HostState>,
    spec: &TransportSpec,
    cmd_rx: &Receiver<SupervisorCmd>,
) -> RunOutcome {
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    let provider = (spec.make)();
    let stop_for_worker = Arc::clone(&stop);
    let worker = std::thread::Builder::new()
        .name(format!("voltnest-{}", spec.transport.as_str()))
        .spawn(move || {
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                provider.run(event_tx, stop_for_worker)
            }));
            let _ = done_tx.send(result.err().map(describe_panic));
        });

    let worker = match worker {
        Ok(handle) => handle,
        Err(err) => return RunOutcome::Panicked(format!("无法创建采集线程: {err}")),
    };

    let mut reported: Option<(ErrorCode, String)> = None;

    let outcome = loop {
        match done_rx.try_recv() {
            Ok(None) => break RunOutcome::Returned(None),
            Ok(Some(msg)) => break RunOutcome::Panicked(msg),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                break RunOutcome::Panicked("采集线程异常终止".into());
            }
        }
        match cmd_rx.try_recv() {
            Ok(SupervisorCmd::Stop) => break RunOutcome::Stopped,
            Ok(SupervisorCmd::Restart) => break RunOutcome::Restarted,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
        }
        if shared.shutting_down.load(Ordering::Relaxed) {
            break RunOutcome::Stopped;
        }
        match event_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => forward_tracked(shared, spec.transport, &mut reported, event),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };

    stop.store(true, Ordering::Relaxed);
    let _ = worker.join();
    while let Ok(event) = event_rx.try_recv() {
        forward_tracked(shared, spec.transport, &mut reported, event);
    }
    match outcome {
        RunOutcome::Returned(_) => RunOutcome::Returned(reported),
        other => other,
    }
}

/// 转发事件，并记下其中的 `TransportDown`（本轮已上报的失败原因）。
fn forward_tracked(
    shared: &Arc<HostState>,
    transport: Transport,
    reported: &mut Option<(ErrorCode, String)>,
    event: ProviderEvent,
) {
    if let ProviderEvent::TransportDown { code, detail } = &event {
        *reported = Some((*code, detail.clone()));
    }
    forward(shared, transport, event);
}

/// 把事件塞进宿主队列。队列无界，但入队计数用于积压告警。
fn forward(shared: &Arc<HostState>, transport: Transport, event: ProviderEvent) {
    shared.mark_enqueued();
    if shared
        .tx
        .send(HostEvent::Provider { transport, event })
        .is_err()
    {
        shared.mark_dequeued();
    }
}

/// 上报 `TransportDown`：该 transport 的全部设备转 `Offline`，但不动别的 transport。
fn push_down(shared: &Arc<HostState>, spec: &TransportSpec, code: ErrorCode, detail: &str) {
    forward(
        shared,
        spec.transport,
        ProviderEvent::TransportDown {
            code,
            detail: detail.to_string(),
        },
    );
}

fn describe_panic(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        format!("采集线程 panic: {msg}")
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        format!("采集线程 panic: {msg}")
    } else {
        "采集线程 panic（不可读载荷）".to_string()
    }
}

fn upsert(
    statuses: &mut Vec<TransportStatus>,
    spec: &TransportSpec,
    status: TransportHealth,
    failures: u32,
    restarts: u32,
    last_error: Option<String>,
) {
    let next = TransportStatus {
        transport: spec.transport,
        family: spec.family,
        status,
        failures,
        restarts,
        last_error,
    };
    match statuses.iter_mut().find(|s| s.transport == spec.transport) {
        Some(slot) => *slot = next,
        None => statuses.push(next),
    }
}

fn set_status(
    shared: &HostState,
    spec: &TransportSpec,
    status: TransportHealth,
    failures: u32,
    restarts: u32,
    last_error: Option<String>,
) {
    {
        let mut statuses = lock(&shared.transports);
        upsert(&mut statuses, spec, status, failures, restarts, last_error);
    }
    let _ = shared.tx.send(HostEvent::Diagnostics);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use voltnest_provider::Transport;
    use voltnest_provider::testing::PanicProvider;
    use voltnest_storage::{DataPaths, Settings, SettingsStore, StateStore};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn temp_shared() -> (Arc<HostState>, Receiver<HostEvent>) {
        let root = std::env::temp_dir().join(format!(
            "voltnest-supervisor-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let paths = DataPaths::with_root(&root);
        let (tx, rx) = std::sync::mpsc::channel();
        let shared = HostState::new(
            paths.clone(),
            Settings::default(),
            SettingsStore::new(paths.clone()),
            StateStore::new(paths),
            tx,
        );
        (shared, rx)
    }

    fn hid_spec(make: fn() -> Box<dyn Provider>) -> TransportSpec {
        TransportSpec {
            transport: Transport::Hid,
            family: voltnest_provider::FAMILY_HID_VBAS,
            make,
        }
    }

    #[test]
    fn backoff_follows_five_fifteen_sixty_then_gives_up() {
        assert_eq!(backoff_for(1, &BACKOFF_MS), Some(5_000));
        assert_eq!(backoff_for(2, &BACKOFF_MS), Some(15_000));
        assert_eq!(backoff_for(3, &BACKOFF_MS), Some(60_000));
        assert_eq!(
            backoff_for(4, &BACKOFF_MS),
            Some(60_000),
            "最后一级一直用它"
        );
        assert_eq!(
            backoff_for(5, &BACKOFF_MS),
            None,
            "第 5 次失败后不再自动重启"
        );
        assert_eq!(backoff_for(0, &BACKOFF_MS), None);
        assert_eq!(backoff_for(1, &[]), None);
    }

    #[test]
    fn status_change_notifies_diagnostics_channel() {
        let (shared, rx) = temp_shared();
        let (cmd, handle) = spawn_supervisor_with_backoff(
            Arc::clone(&shared),
            hid_spec(|| Box::new(PanicProvider::new())),
            &[1, 1, 1, 1],
        );

        let mut notified = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !notified && std::time::Instant::now() < deadline {
            if let Ok(HostEvent::Diagnostics) = rx.recv_timeout(Duration::from_millis(200)) {
                notified = true;
            }
        }
        assert!(notified, "线程组状态变化必须通知诊断页");

        let _ = cmd.send(SupervisorCmd::Stop);
        handle.join().expect("监管线程应能正常退出");
    }

    #[test]
    fn panic_becomes_transport_down_then_unavailable() {
        let (shared, rx) = temp_shared();
        let (cmd, handle) = spawn_supervisor_with_backoff(
            Arc::clone(&shared),
            hid_spec(|| Box::new(PanicProvider::new())),
            &[1, 1, 1, 1],
        );

        let mut down = 0;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while down == 0 && std::time::Instant::now() < deadline {
            if let Ok(HostEvent::Provider { event, .. }) =
                rx.recv_timeout(Duration::from_millis(200))
                && matches!(event, ProviderEvent::TransportDown { .. })
            {
                down += 1;
            }
        }
        assert!(down >= 1, "panic 必须上报 TransportDown");

        let mut unavailable = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !unavailable && std::time::Instant::now() < deadline {
            unavailable = lock(&shared.transports)
                .iter()
                .any(|s| s.status == TransportHealth::Unavailable);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(unavailable, "连续失败达到上限后必须转人工重启");

        cmd.send(SupervisorCmd::Stop).expect("监管线程仍在");
        handle.join().expect("监管线程应能正常退出");
    }

    /// 「先上报 `TransportDown` 再 `run()` 返回」的采集器（Linux BLE 权限被拒即此路径）。
    struct DownThenExit;

    /// provider 给出的可读原因：监管线程不得把它换成「采集线程自行退出」。
    const DOWN_DETAIL: &str =
        "初始化失败: PermissionDenied（Linux/bluez：当前用户需在 bluetooth 组或有 polkit 授权）";

    impl Provider for DownThenExit {
        fn profile(&self) -> &'static voltnest_provider::ProviderProfile {
            &voltnest_provider::BLE_BAS
        }

        fn run(self: Box<Self>, tx: Sender<ProviderEvent>, _stop: Arc<AtomicBool>) {
            let _ = tx.send(ProviderEvent::TransportDown {
                code: ErrorCode::AccessDenied,
                detail: DOWN_DETAIL.to_string(),
            });
        }
    }

    fn ble_spec(make: fn() -> Box<dyn Provider>) -> TransportSpec {
        TransportSpec {
            transport: Transport::Ble,
            family: voltnest_provider::FAMILY_BLE_BAS,
            make,
        }
    }

    #[test]
    fn provider_failure_reason_survives_the_supervisor() {
        let (shared, rx) = temp_shared();
        let (cmd, handle) = spawn_supervisor_with_backoff(
            Arc::clone(&shared),
            ble_spec(|| Box::new(DownThenExit)),
            &[1, 1, 1, 1],
        );

        let mut seen: Vec<(ErrorCode, String)> = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while seen.len() < 2 && std::time::Instant::now() < deadline {
            if let Ok(HostEvent::Provider {
                event: ProviderEvent::TransportDown { code, detail },
                ..
            }) = rx.recv_timeout(Duration::from_millis(200))
            {
                seen.push((code, detail));
            }
        }

        let _ = cmd.send(SupervisorCmd::Stop);
        handle.join().expect("监管线程应能正常退出");

        assert!(
            seen.len() >= 2,
            "provider 自行退出仍要按失败退避重启，不能静默停掉：{seen:?}"
        );
        for (code, detail) in &seen {
            assert_eq!(
                *code,
                ErrorCode::AccessDenied,
                "provider 的结构化错误码不能被 Internal 覆盖：{seen:?}"
            );
            assert_eq!(
                detail, DOWN_DETAIL,
                "provider 给出的失败原因不能被「采集线程自行退出」覆盖：{seen:?}"
            );
        }
    }
}
