//! 低电量通知投递：策略判定在 `NotificationBook`，这里只负责送出并把真实结果回填状态机（诊断页据此区分「系统吞了」与「我们没发」）。不做自动降级。
//!
//! Windows 不用 `tauri_plugin_notification`（其对 AUMID 的处理会让开发态回落 PowerShell 图标），改用 `tauri-winrt-notification` 固定 AUMID 与图标。

use std::sync::Arc;

use tauri::AppHandle;
#[cfg(not(windows))]
use tauri_plugin_notification::NotificationExt;

use voltnest_core::{DiagLevel, LowBatteryNotification, Snapshot};
use voltnest_provider::clock;
use voltnest_storage::NotificationMode;

use crate::host::{HostState, lock};
use crate::tray;

/// 投递一批低电量通知。
pub fn dispatch_notifications(
    app: &AppHandle,
    shared: &Arc<HostState>,
    snap: &Snapshot,
    notifications: Vec<LowBatteryNotification>,
    _now: u64,
) {
    if notifications.is_empty() {
        return;
    }
    let mode = lock(&shared.settings).notifications.mode;

    for notification in notifications {
        let result = match mode {
            NotificationMode::Off => "suppressed_mode_off",
            NotificationMode::System => match send_toast(app, &notification) {
                Ok(()) => "sent",
                Err(err) => {
                    log::warn!("Toast 发送失败: {err}");
                    push_diag(
                        shared,
                        DiagLevel::Warn,
                        "notification_failed",
                        Some(notification.stable_id.clone()),
                        format!("系统通知发送失败：{err}"),
                    );
                    "failed"
                }
            },
            NotificationMode::TrayBubble => {
                let text = format!(
                    "{} 电量 {}%（阈值 {}%）",
                    notification.display_name, notification.battery, notification.threshold
                );
                let outcome = tray::bubble(app, snap, &notification.stable_id, &text);
                match outcome {
                    tray::BubbleOutcome::Sent => {}
                    tray::BubbleOutcome::NoIcon => log::warn!(
                        "托盘里没有可用图标，低电量提示未送出（{}）",
                        notification.stable_id
                    ),
                    tray::BubbleOutcome::UnsupportedPlatform => push_diag(
                        shared,
                        DiagLevel::Warn,
                        "notification_unsupported_platform",
                        Some(notification.stable_id.clone()),
                        "托盘气泡模式在当前平台不可用（Linux 托盘后端没有 tooltip 实现）：请改用系统通知",
                    ),
                }
                tray_bubble_result(outcome)
            }
        };
        lock(&shared.engine).mark_notification_result(
            &notification.stable_id,
            notification.threshold,
            result,
        );
    }
}

/// 托盘气泡三态 → 写进 `last_result` 的值（`UnsupportedPlatform` 不可并入 `failed`）。
fn tray_bubble_result(outcome: tray::BubbleOutcome) -> &'static str {
    match outcome {
        tray::BubbleOutcome::Sent => "tray_tooltip",
        tray::BubbleOutcome::NoIcon => "failed",
        tray::BubbleOutcome::UnsupportedPlatform => "unsupported_platform",
    }
}

/// 发送一条 Windows Toast：标题是设备+现象，正文是读数与阈值。
#[cfg(windows)]
fn send_toast(_app: &AppHandle, notification: &LowBatteryNotification) -> Result<(), String> {
    toast(
        &format!("{} 电量偏低", notification.display_name),
        &format!(
            "剩余 {}%，已跌破 {}% 提醒线。",
            notification.battery, notification.threshold
        ),
    )
}

/// 非 Windows：仍走 Tauri 官方插件。
#[cfg(not(windows))]
fn send_toast(app: &AppHandle, notification: &LowBatteryNotification) -> Result<(), String> {
    app.notification()
        .builder()
        .title(format!("{} 电量偏低", notification.display_name))
        .body(format!(
            "剩余 {}%，已跌破 {}% 提醒线。",
            notification.battery, notification.threshold
        ))
        .show()
        .map_err(|e| e.to_string())
}

/// Windows Toast：显式 AUMID + 显式图标。
#[cfg(windows)]
fn toast(title: &str, body: &str) -> Result<(), String> {
    use tauri_winrt_notification::{Duration, IconCrop, Toast};

    let mut toast = Toast::new(voltnest_platform::APP_USER_MODEL_ID)
        .title(title)
        .text1(body)
        .duration(Duration::Short);

    if let Some(icon) = crate::toast_icon::path() {
        toast = toast.icon(icon, IconCrop::Square, "VoltNest");
    }

    toast.show().map_err(|err| err.to_string())
}

/// 测试通知：不写任何通知状态，只验证系统通道是否通。
#[cfg(windows)]
pub fn send_test_notification(_app: &AppHandle) -> Result<(), String> {
    toast("VoltNest 测试通知", "看到这条说明系统通知通道正常。")
}

/// 测试通知：不写任何通知状态，只验证系统通道是否通。
#[cfg(not(windows))]
pub fn send_test_notification(app: &AppHandle) -> Result<(), String> {
    app.notification()
        .builder()
        .title("VoltNest 测试通知")
        .body("看到这条说明系统通知通道正常。")
        .show()
        .map_err(|e| e.to_string())
}

fn push_diag(
    shared: &Arc<HostState>,
    level: DiagLevel,
    code: &'static str,
    device_id: Option<String>,
    detail: impl Into<String>,
) {
    lock(&shared.engine).push_diag(clock::now_utc_ms(), level, code, device_id, detail);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三态必须映射成三个互不相同的值。
    #[test]
    fn tray_bubble_outcomes_map_to_distinct_results() {
        assert_eq!(
            tray_bubble_result(tray::BubbleOutcome::Sent),
            "tray_tooltip"
        );
        assert_eq!(tray_bubble_result(tray::BubbleOutcome::NoIcon), "failed");
        assert_eq!(
            tray_bubble_result(tray::BubbleOutcome::UnsupportedPlatform),
            "unsupported_platform",
            "平台不支持必须是独立取值，不能并进 failed"
        );

        let values = [
            tray_bubble_result(tray::BubbleOutcome::Sent),
            tray_bubble_result(tray::BubbleOutcome::NoIcon),
            tray_bubble_result(tray::BubbleOutcome::UnsupportedPlatform),
        ];
        for (i, a) in values.iter().enumerate() {
            for b in &values[i + 1..] {
                assert_ne!(a, b, "三态不能共用一个结果字符串：{values:?}");
            }
        }
    }
}
