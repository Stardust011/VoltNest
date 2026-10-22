//! 低电量通知投递（§12「通知」）。
//!
//! 策略判定在 `voltnest_core::NotificationBook` 里完成，这里只负责把
//! 「应当发送」变成「真的送出去了」，并把真实结果回填给状态机 ——
//! 诊断页要区分「系统吞了」和「我们没发」，靠的就是这一层返回值。
//!
//! 不做自动降级：`system` 发不出就记 `failed`，不会偷偷改成托盘气泡。

use std::sync::Arc;

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use voltnest_core::{DiagLevel, LowBatteryAlert, Snapshot};
use voltnest_provider::clock;
use voltnest_storage::NotificationMode;

use crate::host::{Shared, lock};
use crate::tray::BASE_TRAY_ID;

/// 托盘气泡模式的提示时长。
const BUBBLE_MS: u64 = 6_000;

/// 投递一批低电量通知。
pub fn dispatch(
    app: &AppHandle,
    shared: &Arc<Shared>,
    _snap: &Snapshot,
    alerts: Vec<LowBatteryAlert>,
    _now: u64,
) {
    if alerts.is_empty() {
        return;
    }
    let mode = lock(&shared.settings).notifications.mode;

    for alert in alerts {
        let result = match mode {
            NotificationMode::Off => "suppressed_mode_off",
            NotificationMode::System => match send_toast(app, &alert) {
                Ok(()) => "sent",
                Err(err) => {
                    log::warn!("Toast 发送失败: {err}");
                    push_diag(
                        shared,
                        DiagLevel::Warn,
                        "notification_failed",
                        Some(alert.stable_id.clone()),
                        format!("系统通知发送失败：{err}"),
                    );
                    "failed"
                }
            },
            NotificationMode::TrayBubble => {
                bubble(app, &alert);
                "tray_tooltip"
            }
        };
        lock(&shared.engine).mark_notification_result(&alert.stable_id, alert.threshold, result);
    }
}

/// 发送一条 Windows Toast。
fn send_toast(app: &AppHandle, alert: &LowBatteryAlert) -> Result<(), String> {
    app.notification()
        .builder()
        .title(format!("{} 电量偏低", alert.display_name))
        .body(format!(
            "剩余 {}%，已跌破 {}% 提醒线。",
            alert.battery, alert.threshold
        ))
        .show()
        .map_err(|e| e.to_string())
}

/// 托盘气泡模式：Tauri 2 没有暴露 `Shell_NotifyIcon` 的气泡接口，
/// 这里退化为「基础托盘图标 tooltip 提示 6 秒」，并在 README 的已知限制里写明。
fn bubble(app: &AppHandle, alert: &LowBatteryAlert) {
    let Some(tray) = app.tray_by_id(&tauri::tray::TrayIconId::new(BASE_TRAY_ID)) else {
        return;
    };
    let text = format!(
        "{} 电量 {}%（阈值 {}%）",
        alert.display_name, alert.battery, alert.threshold
    );
    let _ = tray.set_tooltip(Some(&text));

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(BUBBLE_MS));
        if let Some(tray) = app.tray_by_id(&tauri::tray::TrayIconId::new(BASE_TRAY_ID)) {
            let _ = tray.set_tooltip(Some("VoltNest"));
        }
    });
}

/// 测试通知：不写任何通知状态，只验证系统通道是否通。
pub fn test(app: &AppHandle) -> Result<(), String> {
    app.notification()
        .builder()
        .title("VoltNest 测试通知")
        .body("看到这条说明系统通知通道正常。")
        .show()
        .map_err(|e| e.to_string())
}

fn push_diag(
    shared: &Arc<Shared>,
    level: DiagLevel,
    code: &'static str,
    device_id: Option<String>,
    detail: impl Into<String>,
) {
    lock(&shared.engine).push_diag(clock::now_utc_ms(), level, code, device_id, detail);
}
