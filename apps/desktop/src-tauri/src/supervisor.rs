//! Provider 线程组监管：panic 隔离与退避重启（RT-1/RT-2/§3.3）。每个 transport 一个监管线程，自身不做 I/O，只做三件事：用 `catch_unwind` 包住 `Provider::run` 起采集线程；把上报事件原样转发宿主；采集线程异常结束时上报 `TransportDown` 并按 5s/15s/60s 退避重启，连续 5 次失败标记「不可用」等手动重启。**panic 不得跨 transport 传播**：HID 解析器崩溃时蓝牙必须照常，故隔离在采集线程边界而非宿主里 catch。

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use voltnest_provider::{ErrorCode, Provider, ProviderEvent, Transport};

use crate::dto::{TransportState, TransportStatus};
use crate::host::{HostEvent, Shared, lock};

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

/// 一个 transport 的采集规格。
pub struct Spec {
    /// 传输层。
    pub transport: Transport,
    /// 协议家族 ID（诊断展示）。
    pub family: &'static str,
    /// 构造采集器。用函数指针而非闭包：采集器必须可以在每次重启时重新构造。
    pub make: fn() -> Box<dyn Provider>,
}

impl Spec {
    /// `hid-vbas`。
    pub fn hid() -> Self {
        Self {
            transport: Transport::Hid,
            family: voltnest_provider::FAMILY_HID_VBAS,
            make: || Box::new(voltnest_hid::provider::HidProvider::new()),
        }
    }

    /// `ble-bas`。
    pub fn ble() -> Self {
        Self {
            transport: Transport::Ble,
            family: voltnest_provider::FAMILY_BLE_BAS,
            make: || Box::new(voltnest_ble::provider::BleProvider::new()),
        }
    }
}

/// 一次运行的结果。
enum RunOutcome {
    /// 收到 `Stop` 指令正常停止。
    Stopped,
    /// 收到 `Restart` 指令，需要重置失败计数。
    Restarted,
    /// `run()` 自行返回（未收到停止指令）→ 按失败处理。
    Returned,
    /// `run()` panic。
    Panicked(String),
}

/// 启动一个 transport 的监管线程，返回用于发送指令的通道。
pub fn start(shared: Arc<Shared>, spec: Spec) -> Sender<SupervisorCmd> {
    start_with(shared, spec, &BACKOFF_MS).0
}

/// 同上，但可指定退避表并拿到线程句柄（测试用毫秒级表，产品用 [`BACKOFF_MS`]）。
pub fn start_with(
    shared: Arc<Shared>,
    spec: Spec,
    backoff: &'static [u64],
) -> (Sender<SupervisorCmd>, std::thread::JoinHandle<()>) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let name = format!("voltnest-{}-supervisor", spec.transport.as_str());
    {
        let mut statuses = lock(&shared.transports);
        upsert(&mut statuses, &spec, TransportState::Running, 0, 0, None);
    }
    let handle = std::thread::Builder::new()
        .name(name)
        .spawn(move || supervise(shared, spec, cmd_rx, backoff))
        .expect("创建 Provider 监管线程失败");
    (cmd_tx, handle)
}

fn supervise(
    shared: Arc<Shared>,
    spec: Spec,
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
            TransportState::Running,
            failures,
            restarts,
            None,
        );

        let detail = match run_once(&shared, &spec, &cmd_rx) {
            RunOutcome::Stopped => return,
            RunOutcome::Restarted => {
                failures = 0;
                restarts = restarts.saturating_add(1);
                continue;
            }
            RunOutcome::Returned => "采集线程自行退出".to_string(),
            RunOutcome::Panicked(msg) => msg,
        };

        failures = failures.saturating_add(1);
        restarts = restarts.saturating_add(1);
        log::error!(
            "{} 采集线程失败（连续 {failures} 次）：{detail}",
            spec.transport.as_str()
        );
        push_down(&shared, &spec, &detail);
        set_status(
            &shared,
            &spec,
            TransportState::Down,
            failures,
            restarts,
            Some(detail.clone()),
        );

        match backoff_for(failures, backoff) {
            None => {
                set_status(
                    &shared,
                    &spec,
                    TransportState::Unavailable,
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
                // 只等手动重启：期间每 500ms 检查一次退出标志。
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

/// 第 `failures` 次连续失败后的退避时长；达到上限返回 `None`（转人工重启）。抽成纯函数以便用毫秒级退避表测完整序列，不必真等 60 秒。
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

fn wait_cmd(cmd_rx: &Receiver<SupervisorCmd>, delay_ms: u64, shared: &Shared) -> Wait {
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
fn run_once(shared: &Arc<Shared>, spec: &Spec, cmd_rx: &Receiver<SupervisorCmd>) -> RunOutcome {
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    let provider = (spec.make)();
    let stop_for_worker = Arc::clone(&stop);
    let worker = std::thread::Builder::new()
        .name(format!("voltnest-{}", spec.transport.as_str()))
        .spawn(move || {
            // RT-2：panic 必须在采集线程边界被吞掉，转成一条结构化失败原因。
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                provider.run(event_tx, stop_for_worker)
            }));
            let _ = done_tx.send(result.err().map(describe_panic));
        });

    let worker = match worker {
        Ok(handle) => handle,
        Err(err) => return RunOutcome::Panicked(format!("无法创建采集线程: {err}")),
    };

    let outcome = loop {
        // 1. 采集线程是否已经结束
        match done_rx.try_recv() {
            Ok(None) => break RunOutcome::Returned,
            Ok(Some(msg)) => break RunOutcome::Panicked(msg),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                break RunOutcome::Panicked("采集线程异常终止".into());
            }
        }
        // 2. 宿主指令
        match cmd_rx.try_recv() {
            Ok(SupervisorCmd::Stop) => break RunOutcome::Stopped,
            Ok(SupervisorCmd::Restart) => break RunOutcome::Restarted,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
        }
        if shared.shutting_down.load(Ordering::Relaxed) {
            break RunOutcome::Stopped;
        }
        // 3. 转发事件
        match event_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => forward(shared, spec.transport, event),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            // 采集线程已放下发送端：给它一点时间把 done 消息送出来。
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };

    stop.store(true, Ordering::Relaxed);
    let _ = worker.join();
    // 收尾：把可能残留的事件转发出去（含最后一次读数）。
    while let Ok(event) = event_rx.try_recv() {
        forward(shared, spec.transport, event);
    }
    outcome
}

/// 把事件塞进宿主队列。队列无界，但入队计数用于积压告警。
fn forward(shared: &Arc<Shared>, transport: Transport, event: ProviderEvent) {
    shared.note_queued();
    if shared
        .tx
        .send(HostEvent::Provider { transport, event })
        .is_err()
    {
        shared.note_dequeued();
    }
}

/// 上报 `TransportDown`：该 transport 的 L1 全部转 `Offline`，但不动别的 transport。
fn push_down(shared: &Arc<Shared>, spec: &Spec, detail: &str) {
    forward(
        shared,
        spec.transport,
        ProviderEvent::TransportDown {
            code: ErrorCode::Internal,
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
    spec: &Spec,
    status: TransportState,
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
    shared: &Shared,
    spec: &Spec,
    status: TransportState,
    failures: u32,
    restarts: u32,
    last_error: Option<String>,
) {
    {
        let mut statuses = lock(&shared.transports);
        upsert(&mut statuses, spec, status, failures, restarts, last_error);
    }
    // 重启次数只在这里推进，而 transports 不在快照里：不主动通知，诊断页就永远是旧的。
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

    fn temp_shared() -> (Arc<Shared>, Receiver<HostEvent>) {
        let root = std::env::temp_dir().join(format!(
            "voltnest-supervisor-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let paths = DataPaths::with_root(&root);
        let (tx, rx) = std::sync::mpsc::channel();
        let shared = Shared::new(
            paths.clone(),
            Settings::default(),
            SettingsStore::new(paths.clone()),
            StateStore::new(paths),
            tx,
        );
        (shared, rx)
    }

    fn hid_spec(make: fn() -> Box<dyn Provider>) -> Spec {
        Spec {
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
        let (cmd, handle) = start_with(
            Arc::clone(&shared),
            hid_spec(|| Box::new(PanicProvider::new())),
            &[1, 1, 1, 1],
        );

        // 线程组每次改状态（含重启次数 +1）都必须推一条信号，
        // 否则诊断页只能等切页时重新拉取，界面上永远是旧数值。
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
        let (cmd, handle) = start_with(
            Arc::clone(&shared),
            hid_spec(|| Box::new(PanicProvider::new())),
            &[1, 1, 1, 1],
        );

        // 1. panic 必须转成 TransportDown，而不是静默消失。
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

        // 2. 连续失败达到上限后转「不可用」，停止自动重启。
        let mut unavailable = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !unavailable && std::time::Instant::now() < deadline {
            unavailable = lock(&shared.transports)
                .iter()
                .any(|s| s.status == TransportState::Unavailable);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(unavailable, "连续失败达到上限后必须转人工重启");

        // 3. Stop 能让监管线程干净退出（不遗留采集线程）。
        cmd.send(SupervisorCmd::Stop).expect("监管线程仍在");
        handle.join().expect("监管线程应能正常退出");
    }
}
