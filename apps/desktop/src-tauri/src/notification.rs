//! 低电量通知投递（§12「通知」）。
//!
//! 策略判定在 `voltnest_core::NotificationBook` 里完成，这里只负责把
//! 「应当发送」变成「真的送出去了」，并把真实结果回填给状态机 ——
//! 诊断页要区分「系统吞了」和「我们没发」，靠的就是这一层返回值。
//!
//! 不做自动降级：`system` 发不出就记 `failed`，不会偷偷改成托盘气泡。
//!
//! # 为什么 Windows 上不用 `tauri_plugin_notification`
//!
//! Toast 的应用名和图标由**发送方传入的 AUMID** 决定。插件在 Windows 上只在
//! 「看起来已安装」时才设置 AUMID —— 它把位于 `target\debug` / `target\release`
//! 的进程一律当成开发运行，直接不设（`tauri-plugin-notification` 的 `desktop.rs`），
//! 于是 `notify-rust` 回落到 `Toast::POWERSHELL_APP_ID`。结果就是：开发运行时
//! 通知顶上是 PowerShell 的蓝色图标和「Windows PowerShell」标题，跟我们的
//! `HKCU\Software\Classes\AppUserModelId\VoltNest.Desktop` 注册毫无关系。
//!
//! 这里改用同一套 WinRT Toast API（`tauri-winrt-notification`），AUMID 恒定、
//! 图标显式给，开发运行和安装版看到的是同一个「VoltNest」。

use std::sync::Arc;

use tauri::AppHandle;
#[cfg(not(windows))]
use tauri_plugin_notification::NotificationExt;

use voltnest_core::{DiagLevel, LowBatteryAlert, Snapshot};
use voltnest_provider::clock;
use voltnest_storage::NotificationMode;

use crate::host::{Shared, lock};
use crate::tray;

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
                let text = format!(
                    "{} 电量 {}%（阈值 {}%）",
                    alert.display_name, alert.battery, alert.threshold
                );
                if tray::bubble(app, &alert.stable_id, &text) {
                    "tray_tooltip"
                } else {
                    log::warn!(
                        "托盘里没有可用图标，低电量提示未送出（{}）",
                        alert.stable_id
                    );
                    "failed"
                }
            }
        };
        lock(&shared.engine).mark_notification_result(&alert.stable_id, alert.threshold, result);
    }
}

/// 发送一条 Windows Toast。
///
/// 标题是设备 + 现象，正文是读数与阈值：Windows 自己会把 AUMID 解析出的
/// 「VoltNest」渲染在标题上方，标题本身不该重复应用名。
#[cfg(windows)]
fn send_toast(_app: &AppHandle, alert: &LowBatteryAlert) -> Result<(), String> {
    toast(
        &format!("{} 电量偏低", alert.display_name),
        &format!(
            "剩余 {}%，已跌破 {}% 提醒线。",
            alert.battery, alert.threshold
        ),
    )
}

/// 非 Windows：仍走 Tauri 官方插件。
#[cfg(not(windows))]
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

/// Windows Toast：显式 AUMID + 显式图标。
#[cfg(windows)]
fn toast(title: &str, body: &str) -> Result<(), String> {
    use tauri_winrt_notification::{Duration, IconCrop, Toast};

    let mut toast = Toast::new(voltnest_platform::APP_USER_MODEL_ID)
        .title(title)
        .text1(body)
        .duration(Duration::Short);

    // `appLogoOverride`：头图直接指定文件，不依赖 AUMID 的 IconUri 能否解析，
    // 用户把数据目录挪走 / 图标被清理工具删掉时也只是少一张图，不影响发送。
    if let Some(icon) = crate::toast_icon::path() {
        toast = toast.icon(icon, IconCrop::Square, "VoltNest");
    }

    toast.show().map_err(|err| err.to_string())
}

/// 测试通知：不写任何通知状态，只验证系统通道是否通。
#[cfg(windows)]
pub fn test(_app: &AppHandle) -> Result<(), String> {
    toast("VoltNest 测试通知", "看到这条说明系统通知通道正常。")
}

/// 测试通知：不写任何通知状态，只验证系统通道是否通。
#[cfg(not(windows))]
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
