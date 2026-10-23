//! Tauri 命令：前端唯一的写入入口。纪律：先改内存、再落盘、最后 Wake 一次快照；不直接碰 `voltnest_hid`/`voltnest_ble`，采集线程只由监管模块管理。

use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use voltnest_core::{DiagLevel, Snapshot};
use voltnest_provider::{Transport, clock};
use voltnest_storage::Settings;

use crate::dto::{
    DiagEventDto, DiagnosticsDto, NotificationStateDto, OnboardingChoice, SettingsDto,
    TransportStatusDto,
};
use crate::host::{self, HostEvent, Shared, lock};
use crate::supervisor::SupervisorCmd;
use crate::{notification, tray};

/// 当前全量快照。
#[tauri::command]
pub fn get_snapshot(shared: State<'_, Arc<Shared>>) -> Snapshot {
    host::snapshot(&shared)
}

/// 当前设置。
#[tauri::command]
pub fn get_settings(shared: State<'_, Arc<Shared>>) -> SettingsDto {
    SettingsDto::from_settings(&lock(&shared.settings))
}

/// 覆盖式更新设置。
#[tauri::command]
pub fn update_settings(
    settings: SettingsDto,
    shared: State<'_, Arc<Shared>>,
) -> Result<SettingsDto, String> {
    let next = write_settings(&shared, |current| settings.apply_to(current))?;
    notify_wake(&shared);
    // 改阈值重建通知触发节点（旧档位丢弃、新档位建出），诊断页须重拉，否则仍显示改前列表，像「新阈值没生效」。
    notify_diagnostics(&shared);
    Ok(SettingsDto::from_settings(&next))
}

/// 诊断页数据。
#[tauri::command]
pub fn get_diagnostics(shared: State<'_, Arc<Shared>>) -> DiagnosticsDto {
    let (events, notifications) = {
        let engine = lock(&shared.engine);
        let events: Vec<DiagEventDto> = engine
            .diagnostics()
            .recent()
            .into_iter()
            .map(DiagEventDto::from)
            .collect();
        let notifications: Vec<NotificationStateDto> = engine
            .notification_entries()
            .into_iter()
            .map(NotificationStateDto::from)
            .collect();
        (events, notifications)
    };

    let depth = shared.queue_depth();
    let transports = lock(&shared.transports)
        .iter()
        .map(|t| TransportStatusDto {
            transport: t.transport.as_str(),
            family: t.family,
            status: t.status.as_str(),
            restarts: t.restarts,
            last_error: t.last_error.clone(),
            queued_events: depth,
        })
        .collect();

    DiagnosticsDto {
        events,
        transports,
        notifications,
        autostart_registered: voltnest_platform::autostart::is_enabled(),
        aumid_registered: voltnest_platform::aumid::is_registered(),
        data_root: shared.paths.root().display().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// 手动重启某个采集线程组（诊断页按钮）。
#[tauri::command]
pub fn restart_transport(transport: String, shared: State<'_, Arc<Shared>>) -> Result<(), String> {
    let Some(kind) = Transport::from_prefix(&transport) else {
        return Err(format!("未知的 transport：{transport}"));
    };
    let sender = lock(&shared.supervisors).get(&kind).cloned();
    let Some(sender) = sender else {
        return Err("采集线程组尚未启动".to_string());
    };
    sender
        .send(SupervisorCmd::Restart)
        .map_err(|_| "监管线程已退出，请重启应用".to_string())?;
    lock(&shared.engine).push_diag(
        clock::now_utc_ms(),
        DiagLevel::Info,
        "transport_restart_requested",
        None,
        format!("用户手动重启 {} 采集", kind.display_label()),
    );
    notify_wake(&shared);
    Ok(())
}

/// 发送一条测试通知。
#[tauri::command]
pub fn test_notification(app: AppHandle) -> Result<(), String> {
    notification::test(&app)
}

/// 开/关开机自启。返回**注册表实际状态**而非请求值：写 `HKCU\...\Run` 可能被组策略/安全软件拦掉，前端用返回值回拨开关，避免「界面显示已开启、实际不自启」。
#[tauri::command]
pub fn set_autostart(enabled: bool, shared: State<'_, Arc<Shared>>) -> Result<bool, String> {
    let actual = apply_autostart(enabled)?;
    write_settings(&shared, |current| current.autostart = actual)?;
    Ok(actual)
}

/// 完成首次引导。
#[tauri::command]
pub fn complete_onboarding(
    choice: OnboardingChoice,
    shared: State<'_, Arc<Shared>>,
) -> Result<SettingsDto, String> {
    let actual = apply_autostart(choice.autostart).unwrap_or_else(|err| {
        log::warn!("引导期间写启动项失败: {err}");
        false
    });
    let next = write_settings(&shared, |current| {
        current.onboarding_completed = true;
        current.autostart = actual;
        current.start_minimized = choice.start_minimized;
    })?;
    notify_wake(&shared);
    Ok(SettingsDto::from_settings(&next))
}

/// 打开主面板（托盘菜单之外的入口，例如引导完成后的跳转）。
#[tauri::command]
pub fn show_panel(app: AppHandle) {
    tray::show_main(&app);
}

/// 收进托盘：隐藏主窗口，进程与采集线程照旧。前端不自己调 `hide()` —— 那样会绕开落盘，最后几秒的电量变化可能丢。
#[tauri::command]
pub fn hide_to_tray(app: AppHandle, shared: State<'_, Arc<Shared>>) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Err("主窗口不存在".to_string());
    };
    window.hide().map_err(|e| format!("隐藏窗口失败：{e}"))?;
    host::persist(&shared, clock::now_utc_ms(), true);
    Ok(())
}

/// 退出程序：交给 `app.exit(0)`，由 `RunEvent::Exit` 统一停监管线程并强制落盘。前端不能走 `close()` —— 宿主的 `prevent_close` 把它截成「收托盘」。
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    log::info!("用户从标题栏退出");
    app.exit(0);
}

fn notify_wake(shared: &Shared) {
    let _ = shared.tx.send(HostEvent::Wake);
}

/// 提示前端「诊断数据已变」。
fn notify_diagnostics(shared: &Shared) {
    let _ = shared.tx.send(HostEvent::Diagnostics);
}

fn apply_autostart(enabled: bool) -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法定位程序路径：{e}"))?;
    voltnest_platform::autostart::set_enabled(&exe, enabled).map_err(|e| e.to_string())?;
    Ok(voltnest_platform::autostart::is_enabled())
}

/// 读-改-写设置：内存副本 → 同步状态机 → 落盘，任一步失败都不留半成品。
fn write_settings(shared: &Shared, mutate: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
    let mut next = lock(&shared.settings).clone();
    mutate(&mut next);
    // 先落盘再换内存：写盘失败时界面保持旧值，不出现「看起来改了、重启又变回去」。
    shared
        .settings_store
        .save(&next)
        .map_err(|e| format!("保存设置失败：{e}"))?;
    *lock(&shared.settings) = next.clone();
    shared.apply_settings(&next);
    Ok(next)
}
