//! VoltNest 悬浮窗：始终置顶、无边框、圆角正方形的毛玻璃卡片，可拖拽、可缩放。
//!
//! 两种运行方式：
//!
//! * **宿主托管**（`VoltNest.exe --floating-host`）：由主程序拉起，逐个渲染它下发的
//!   呈现单元，用户改动的窗口布局通过 NDJSON 回报给宿主持久化。一个事件循环托管 N 个窗口。
//! * **独立演示**（`cargo run -p voltnest-floating`）：内置一个假单元，用来单独调 UI。
//!
//! 窗口的置顶 / 无边框 / 透明 / 玻璃外观 / 圆角命中判定 / 右键菜单全部由
//! `ui/floating-window.slint` 声明，本 crate 只负责多窗口生命周期、拖拽缩放换算与协议。
//!
//! 关于「背景模糊」：Windows 11 的 DWM Acrylic（`DWMWA_SYSTEMBACKDROP_TYPE`）会铺满整个
//! 窗口矩形且不理会逐像素 alpha，圆角处会露出方块状的模糊底，与卡片的圆角外形冲突。
//! 因此玻璃质感全部在渲染层实现：半透明渐变 + 顶缘高光 + 1px 细边。

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod geometry;
pub mod proto;
mod session;
#[allow(missing_docs)]
mod ui;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use slint::{Timer, TimerMode};

pub use proto::{ChildMsg, HostMsg, PROTOCOL_VERSION, UnitWire};
use session::{ChildWriter, Session};

/// 会话模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionMode {
    /// 宿主托管：从 stdin 读 `sync` 消息。
    Hosted,
    /// 独立演示：内置一个假单元 + 心跳。
    Demo,
}

/// 会话启动参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionOptions {
    /// 运行模式。
    pub mode: SessionMode,
}

/// 会话错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    /// Slint 平台后端启动失败。
    Platform(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Platform(err) => write!(f, "悬浮窗事件循环启动失败：{err}"),
        }
    }
}

impl std::error::Error for SessionError {}

/// 从宿主读到的东西：消息本身，或者 stdin 关闭。
enum Incoming {
    Msg(HostMsg),
    Eof,
}

/// 唯一入口：建窗口、跑事件循环，直到 stdin 关闭或收到 `shutdown`。
///
/// 必须在主线程调用（Slint/winit 的要求）。
pub fn run_session(opts: SessionOptions) -> Result<(), SessionError> {
    select_backend()?;
    match opts.mode {
        SessionMode::Demo => run_demo(),
        SessionMode::Hosted => run_hosted(),
    }
}

/// 选后端，并把「悬浮窗不该出现的两处」关掉：任务栏与 Alt+Tab。
///
/// 走 `BackendSelector` 的窗口属性钩子而不是建好窗口再改 ex-style：`WS_EX_TOOLWINDOW` 事后
/// 补需要 hide/show 一圈才生效，窗口会闪一下，而钩子在 `CreateWindowEx` 之前就生效。
fn select_backend() -> Result<(), SessionError> {
    let selector = slint::BackendSelector::new().backend_name("winit".into());

    #[cfg(target_os = "windows")]
    let selector = selector.with_winit_window_attributes_hook(|attributes| {
        use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
        // `skip_taskbar` 在 Windows 上就是 `WS_EX_TOOLWINDOW`：没有任务栏按钮，也不进 Alt+Tab。
        attributes.with_skip_taskbar(true)
    });

    match selector.select() {
        Ok(()) => Ok(()),
        // 平台已经装好了（被别的程序嵌进来跑）：沿用现有后端即可。
        Err(slint::PlatformError::SetPlatformError(_)) => Ok(()),
        Err(err) => Err(SessionError::Platform(err.to_string())),
    }
}

fn run_demo() -> Result<(), SessionError> {
    let mut session = Session::new(ChildWriter::null());
    session.start_demo();
    slint::run_event_loop().map_err(|err| SessionError::Platform(err.to_string()))
}

fn run_hosted() -> Result<(), SessionError> {
    let writer = ChildWriter::stdout();
    writer.send(&ChildMsg::Ready {
        v: PROTOCOL_VERSION,
        pid: std::process::id(),
    });

    let inbox = spawn_stdin_reader(writer.clone());
    let session = Rc::new(RefCell::new(Session::new(writer)));

    // Slint 没有「后台线程醒来后跑一段代码」的通用钩子，这里用 30ms 的定时器排空队列。
    // 宿主最快 4Hz 下发，30ms 的轮询延迟完全够用，闲着时几乎不耗 CPU。
    let pump = Timer::default();
    {
        let session = session.clone();
        pump.start(TimerMode::Repeated, Duration::from_millis(30), move || {
            while let Ok(item) = inbox.try_recv() {
                match item {
                    Incoming::Msg(msg) => session.borrow_mut().apply(msg),
                    Incoming::Eof => {
                        let _ = slint::quit_event_loop();
                        return;
                    }
                }
            }
        });
    }

    let result =
        slint::run_event_loop_until_quit().map_err(|err| SessionError::Platform(err.to_string()));
    drop(pump);
    result
}

/// 读 stdin 的后台线程：一行一条 NDJSON，解析失败只记日志不退出。
fn spawn_stdin_reader(writer: ChildWriter) -> Receiver<Incoming> {
    let (tx, rx) = mpsc::channel::<Incoming>();
    std::thread::spawn(move || {
        use std::io::BufRead;

        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if line.trim().is_empty() {
                continue;
            }
            match proto::decode_host(&line) {
                Ok(msg) => {
                    let stop = matches!(msg, HostMsg::Shutdown { .. });
                    if tx.send(Incoming::Msg(msg)).is_err() || stop {
                        break;
                    }
                }
                Err(err) => {
                    writer.send(&ChildMsg::Log {
                        v: PROTOCOL_VERSION,
                        level: "warn".into(),
                        message: format!("{err}（已跳过该行）"),
                    });
                }
            }
        }
        // 父进程退出（管道关闭）时走到这里：让 UI 线程收尾退出。
        let _ = tx.send(Incoming::Eof);
    });
    rx
}
