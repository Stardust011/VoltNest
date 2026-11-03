//! 主面板窗口的生命周期：显示、隐藏，以及隐藏满 [`RETIRE_AFTER`] 后销毁窗口（连带退出 WebView2 渲染进程），下次打开时重建。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::host::lock;

/// 主窗口 label，与 `tauri.conf.json` 一致。
pub const LABEL: &str = "main";
/// 窗口隐藏多久之后回收。
pub const RETIRE_AFTER: Duration = Duration::from_secs(60);
/// 重建窗口的初始尺寸，与 `tauri.conf.json` 一致。
const DEFAULT_SIZE: (f64, f64) = (420.0, 640.0);
/// 重建窗口的最小尺寸，与 `tauri.conf.json` 一致。
const MIN_SIZE: (f64, f64) = (360.0, 480.0);

/// 窗口开始隐藏的时刻；`None` 表示可见或已回收。
static HIDDEN_SINCE: Mutex<Option<Instant>> = Mutex::new(None);
/// 回收前记下的位置与尺寸（物理像素），重建时还原。
static LAST_BOUNDS: Mutex<Option<(i32, i32, u32, u32)>> = Mutex::new(None);

/// 显示并聚焦主面板；窗口已回收则重建。
pub fn show(app: &AppHandle) {
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || show_now(&handle)) {
        log::error!("调度主面板显示失败：{err}");
    }
}

/// 隐藏主面板并开始计时回收。
pub fn hide(app: &AppHandle) {
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || hide_now(&handle)) {
        log::error!("调度主面板隐藏失败：{err}");
    }
}

/// 主线程：取消计时，重建（若已回收）并显示窗口。
fn show_now(app: &AppHandle) {
    *lock(&HIDDEN_SINCE) = None;
    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        None => match build(app) {
            Ok(window) => window,
            Err(err) => {
                log::error!("重建主窗口失败：{err}");
                return;
            }
        },
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// 主线程：隐藏窗口并挂上回收计时。
fn hide_now(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL)
        && let Err(err) = window.hide()
    {
        log::warn!("隐藏主窗口失败：{err}");
        return;
    }
    *lock(&HIDDEN_SINCE) = Some(Instant::now());

    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(RETIRE_AFTER);
        let inner = handle.clone();
        if let Err(err) = handle.run_on_main_thread(move || retire_now(&inner)) {
            log::warn!("调度窗口回收失败：{err}");
        }
    });
}

/// 主线程：窗口仍不可见且隐藏已满时长时销毁窗口。
fn retire_now(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        *lock(&HIDDEN_SINCE) = None;
        return;
    };
    let hidden_for = (*lock(&HIDDEN_SINCE)).map(|since| since.elapsed());
    let visible = window.is_visible().unwrap_or(false);
    if !due_for_retire(hidden_for, visible) {
        return;
    }

    remember_bounds(&window);
    *lock(&HIDDEN_SINCE) = None;
    match window.destroy() {
        Ok(()) => log::info!(
            "主面板隐藏 {}s，已销毁窗口以回收 WebView2 渲染进程",
            RETIRE_AFTER.as_secs()
        ),
        Err(err) => log::error!("销毁主窗口失败：{err}"),
    }
}

/// 回收判定：隐藏时长达到门槛且此刻确实不可见。
fn due_for_retire(hidden_for: Option<Duration>, visible: bool) -> bool {
    !visible && hidden_for.is_some_and(|elapsed| elapsed >= RETIRE_AFTER)
}

/// 记下当前窗口的位置与尺寸，供重建时还原。
fn remember_bounds(window: &WebviewWindow) {
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };
    *lock(&LAST_BOUNDS) = Some((position.x, position.y, size.width, size.height));
}

/// 按配置重建主窗口，并还原上次的位置与尺寸。
fn build(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::default())
        .title("VoltNest")
        .inner_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .resizable(true)
        .maximizable(false)
        .decorations(false)
        .shadow(true)
        .skip_taskbar(false)
        .visible(false)
        .devtools(false)
        .build()?;

    if let Some((x, y, width, height)) = *lock(&LAST_BOUNDS) {
        let _ = window.set_position(PhysicalPosition::new(x, y));
        let _ = window.set_size(PhysicalSize::new(width, height));
    }
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retires_only_after_the_threshold() {
        assert!(!due_for_retire(
            Some(RETIRE_AFTER - Duration::from_secs(1)),
            false
        ));
        assert!(due_for_retire(Some(RETIRE_AFTER), false));
        assert!(due_for_retire(Some(RETIRE_AFTER * 2), false));
    }

    #[test]
    fn keeps_the_window_when_it_is_back_in_front() {
        assert!(
            !due_for_retire(Some(RETIRE_AFTER * 2), true),
            "用户已经重新打开面板，不能把窗口收走"
        );
        assert!(!due_for_retire(None, false), "没有计时就不回收");
    }
}
