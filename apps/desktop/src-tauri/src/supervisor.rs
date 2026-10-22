//! Provider 线程组监管：panic 隔离与退避重启（RT-1 / RT-2 / §3.3）。
//!
//! 每个 transport 一个监管线程，它自己**不做 I/O**，只做三件事：
//!
//! 1. 起真正的采集线程，用 `catch_unwind` 包住 `Provider::run`；
//! 2. 把采集线程上报的事件原样转发给宿主事件循环；
//! 3. 采集线程异常结束时上报 `TransportDown`，按 5s / 15s / 60s 退避重启，
//!    连续 5 次失败后标记「不可用」，只等用户手动重启。
//!
//! 关键点：**panic 不得跨 transport 传播**。HID 的解析器崩溃时蓝牙必须照常工作，
//! 所以崩溃隔离发生在采集线程边界，而不是在宿主里 catch。

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
    /// `hid-nearlink-ajazz`。
    pub fn hid() -> Self {
        Self {
            transport: Transport::Hid,
            family: voltnest_provider::FAMILY_HID_NEARLINK_AJAZZ,
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
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let name = format!("voltnest-{}-supervisor", spec.transport.as_str());
    {
        let mut statuses = lock(&shared.transports);
        upsert(&mut statuses, &spec, TransportState::Running, 0, 0, None);
    }
    std::thread::Builder::new()
        .name(name)
        .spawn(move || supervise(shared, spec, cmd_rx))
        .expect("创建 Provider 监管线程失败");
    cmd_tx
}

fn supervise(shared: Arc<Shared>, spec: Spec, cmd_rx: Receiver<SupervisorCmd>) {
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

        if failures >= MAX_CONSECUTIVE_FAILURES {
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

        let delay = BACKOFF_MS[(failures.saturating_sub(1) as usize).min(BACKOFF_MS.len() - 1)];
        match wait_cmd(&cmd_rx, delay, &shared) {
            Wait::Command(SupervisorCmd::Restart) => failures = 0,
            Wait::Command(SupervisorCmd::Stop) | Wait::Exit => return,
            Wait::Elapsed => {}
        }
    }
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
    let mut statuses = lock(&shared.transports);
    upsert(&mut statuses, spec, status, failures, restarts, last_error);
}
