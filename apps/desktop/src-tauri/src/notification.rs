//! 低电量通知投递。策略判定在 `NotificationBook` 里完成；这里只把「应当发送」变成「真的送出」并把真实结果回填状态机 —— 诊断页区分「系统吞了」与「我们没发」靠的就是这层返回值。不做自动降级：`system` 发不出就记 `failed`，不偷偷改托盘气泡。
//!
//! # 为什么 Windows 不用 `tauri_plugin_notification`
//! 发送方传入的 AUMID 决定 Toast 的应用名与图标；该插件在 Windows 上仅「看起来已安装」才设 AUMID，把 `target\debug|release` 下的进程一律当开发运行、直接不设，于是 `notify-rust` 回落 `Toast::POWERSHELL_APP_ID`：开发时通知顶上是 PowerShell 蓝图标与「Windows PowerShell」标题，与我们的 `HKCU\...\VoltNest.Desktop` 注册无关。这里改用 `tauri-winrt-notification`，AUMID 恒定、图标显式给，开发与安装版看到同一个「VoltNest」。

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
                let outcome = tray::bubble(app, &alert.stable_id, &text);
                match outcome {
                    // 真写进 tooltip 了，无需额外动作。
                    tray::BubbleOutcome::Sent => {}
                    // 我们真没发（没有落脚图标）：日志交代清楚，诊断页记 `failed`。
                    tray::BubbleOutcome::NoIcon => log::warn!(
                        "托盘里没有可用图标，低电量提示未送出（{}）",
                        alert.stable_id
                    ),
                    // 平台不支持：既不能记成 `failed`（那会和「没图标」混为一谈），
                    // 也不能打上面那句归因错误的日志。
                    tray::BubbleOutcome::UnsupportedPlatform => push_diag(
                        shared,
                        DiagLevel::Warn,
                        "notification_unsupported_platform",
                        Some(alert.stable_id.clone()),
                        "托盘气泡模式在当前平台不可用（Linux 托盘后端没有 tooltip 实现）：请改用系统通知",
                    ),
                }
                tray_bubble_result(outcome)
            }
        };
        lock(&shared.engine).mark_notification_result(&alert.stable_id, alert.threshold, result);
    }
}

/// 托盘气泡三态 → 写进 `last_result` 的值（诊断页原样展示，前端不做映射）。
///
/// 抽成纯函数是为了能直接测：`UnsupportedPlatform` 必须与 `NoIcon` 分成两个值 ——
/// 让它们共用 `failed`，用户就会把「这个平台的托盘后端没有 tooltip」读成「托盘里没图标」，
/// 正是本模块开头那条「诊断页要能分辨『系统吞了』与『我们没发』」要禁止的归因混淆。
fn tray_bubble_result(outcome: tray::BubbleOutcome) -> &'static str {
    match outcome {
        tray::BubbleOutcome::Sent => "tray_tooltip",
        tray::BubbleOutcome::NoIcon => "failed",
        tray::BubbleOutcome::UnsupportedPlatform => "unsupported_platform",
    }
}

/// 发送一条 Windows Toast。标题是设备+现象、正文是读数与阈值；Windows 会把 AUMID 解析的「VoltNest」渲染在标题上方，标题本身不重复应用名。
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

    // `appLogoOverride` 直接指定文件，不依赖 AUMID 的 IconUri 能否解析：数据目录被挪走/图标被清理也只是少张图，不影响发送。
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 三态必须映射成三个互不相同的值：诊断页只原样展示这个字符串，
    /// 一旦两档共用一个值，用户就分不清「平台不支持」与「托盘里没图标」。
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
