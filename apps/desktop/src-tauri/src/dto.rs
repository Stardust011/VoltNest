//! 宿主 → 前端的 DTO。快照直接复用 `Snapshot`（本身就是传输格式）；设置与诊断另建 DTO 是因为：磁盘类型带 `extra`（未知字段保留）属于磁盘格式，不该进 IPC，诊断页还要拼装运行时状态（线程组/自启/AUMID），而这些不在任何 crate 里。

use serde::{Deserialize, Serialize};
use voltnest_core::{DiagEvent, NotificationEntry};
use voltnest_provider::Transport;
use voltnest_storage::{NotificationMode, Settings, TraySettings};

/// 设置 DTO（camelCase，与 `apps/desktop/src/lib/types.ts` 一一对应）。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    /// schema 版本，前端只回传不解释。
    pub schema_version: u32,
    /// `sensitive` / `standard` / `loose`。
    pub sensitivity: String,
    /// 通知相关。
    pub notifications: NotificationDto,
    /// 开机自启（以注册表实际状态为准，见 `set_autostart`）。
    pub autostart: bool,
    /// 启动时最小化到托盘。
    pub start_minimized: bool,
    /// 首次引导是否完成。
    pub onboarding_completed: bool,
    /// 托盘顺序与隐藏列表。
    pub tray: TrayDto,
}

/// 通知设置。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationDto {
    /// `system` / `trayBubble` / `off`。
    pub mode: NotificationMode,
    /// `[偏低阈值, 极低阈值]`。
    pub low_thresholds: [u8; 2],
}

/// 托盘设置。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayDto {
    /// 用户指定的设备顺序。
    pub order: Vec<String>,
    /// 用户隐藏的设备。
    pub hidden: Vec<String>,
}

impl SettingsDto {
    /// 由磁盘设置构造。
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            schema_version: s.schema_version,
            sensitivity: s.sensitivity.clone(),
            notifications: NotificationDto {
                mode: s.notifications.mode,
                low_thresholds: s.notifications.low_thresholds,
            },
            autostart: s.autostart,
            start_minimized: s.start_minimized,
            onboarding_completed: s.onboarding_completed,
            tray: TrayDto {
                order: s.tray.order.clone(),
                hidden: s.tray.hidden.clone(),
            },
        }
    }

    /// 写回磁盘设置。**只覆盖已知字段**：`schemaVersion`/`extra`/未来新增字段沿用磁盘值，否则「读-改-写保留未知字段」会在 IPC 层被悄悄破坏。
    pub fn apply_to(&self, target: &mut Settings) {
        if let Some(sensitivity) = voltnest_provider::Sensitivity::parse(&self.sensitivity) {
            target.set_sensitivity(sensitivity);
        }
        target.notifications.mode = self.notifications.mode;
        target.notifications.low_thresholds = clamp_thresholds(self.notifications.low_thresholds);
        target.autostart = self.autostart;
        target.start_minimized = self.start_minimized;
        target.onboarding_completed = self.onboarding_completed;
        target.tray = TraySettings {
            order: self.tray.order.clone(),
            hidden: self.tray.hidden.clone(),
            extra: std::mem::take(&mut target.tray.extra),
        };
    }
}

/// 阈值必须是一对 `0..=95` 且严格递减的整数。IPC 是不可信边界：越界值若落盘，`NotificationConfig::normalized()` 下次启动会静默改值，用户会看到「设置自己变了」。下界取 `0` 而非 `5`，须与设置页输入框的 `THRESHOLD_MIN`（`lib/thresholds.ts`）一致，否则用户填的 0 会在保存瞬间被改成 5。
fn clamp_thresholds(pair: [u8; 2]) -> [u8; 2] {
    let a = pair[0].clamp(0, 95);
    let b = pair[1].clamp(0, 95);
    let (high, low) = if a == b {
        (a, a.saturating_sub(1))
    } else {
        (a.max(b), a.min(b))
    };
    if high > low {
        [high, low]
    } else {
        // 只可能是 a == b == 0：抬高高阈值而不是压低低阈值，避免出现负数。
        [1, 0]
    }
}

/// 诊断 DTO。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsDto {
    /// 诊断事件，最近的在前。
    pub events: Vec<DiagEventDto>,
    /// 采集线程组状态。
    pub transports: Vec<TransportStatusDto>,
    /// 通知 `(device, threshold)` 状态。
    pub notifications: Vec<NotificationStateDto>,
    /// 自启项是否已写入。
    pub autostart_registered: bool,
    /// AUMID 是否已注册。
    pub aumid_registered: bool,
    /// 数据根目录（`%APPDATA%\VoltNest`）。
    pub data_root: String,
    /// 应用版本。
    pub version: String,
}

/// 一条诊断事件。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagEventDto {
    /// UTC 毫秒。
    pub at_ms: u64,
    /// `info` / `warn` / `error`。
    pub level: &'static str,
    /// 稳定代码。
    pub code: String,
    /// 相关设备。
    pub device_id: Option<String>,
    /// 细节。
    pub detail: String,
}

impl From<DiagEvent> for DiagEventDto {
    fn from(e: DiagEvent) -> Self {
        Self {
            at_ms: e.at_ms,
            level: e.level.as_str(),
            code: e.code.to_string(),
            device_id: e.device_id,
            detail: e.detail,
        }
    }
}

/// 采集线程组状态。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportStatusDto {
    /// `hid` / `ble`。
    pub transport: &'static str,
    /// 协议家族 ID。
    pub family: &'static str,
    /// `running` / `down` / `unavailable`。
    pub status: &'static str,
    /// 累计重启次数。
    pub restarts: u32,
    /// 最后一次失败原因。
    pub last_error: Option<String>,
    /// 未消费事件数（> 64 时需要报警）。
    pub queued_events: usize,
}

/// 通知状态。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationStateDto {
    /// 设备标识。
    pub stable_id: String,
    /// 阈值。
    pub threshold: u8,
    /// 是否处于可触发状态。
    pub armed: bool,
    /// 上次发送时间。
    pub last_notified_at_ms: Option<u64>,
    /// 最后结果（`sent` / `suppressed_cooldown` / `failed` …）。
    pub last_result: Option<String>,
}

impl From<NotificationEntry> for NotificationStateDto {
    fn from(e: NotificationEntry) -> Self {
        Self {
            stable_id: e.stable_id,
            threshold: e.threshold,
            armed: e.armed,
            last_notified_at_ms: e.last_notified_at_ms,
            last_result: e.last_result,
        }
    }
}

/// 首次引导的选择（前端一次性提交，宿主负责落盘与注册）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingChoice {
    /// 是否开启开机自启（默认关闭，必须显式选择）。
    pub autostart: bool,
    /// 是否启动时最小化到托盘。
    pub start_minimized: bool,
}

/// 采集线程组运行时状态（宿主内部维护，供 DTO 转换）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TransportStatus {
    /// 传输层。
    pub transport: Transport,
    /// 家族 ID。
    pub family: &'static str,
    /// 当前状态。
    pub status: TransportState,
    /// 连续失败次数。
    pub failures: u32,
    /// 累计重启次数。
    pub restarts: u32,
    /// 最后失败原因。
    pub last_error: Option<String>,
}

/// 线程组状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransportState {
    /// 运行中。
    Running,
    /// 正在退避重启。
    Down,
    /// 连续失败超过上限，等待手动重启。
    Unavailable,
}

impl TransportState {
    /// 稳定字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Down => "down",
            Self::Unavailable => "unavailable",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voltnest_storage::Settings;

    #[test]
    fn thresholds_are_kept_strictly_descending() {
        assert_eq!(clamp_thresholds([20, 10]), [20, 10]);
        // 逆序：以数值大小重排，高阈值在前。
        assert_eq!(clamp_thresholds([10, 20]), [20, 10]);
        // 相等：拆开一位，避免同一档重复触发。
        assert_eq!(clamp_thresholds([15, 15]), [15, 14]);
        // 下界 0 合法：设置页允许填 0%。
        assert_eq!(clamp_thresholds([0, 0]), [1, 0]);
        assert_eq!(clamp_thresholds([1, 0]), [1, 0]);
        // 越界：夹到 0..=95 后重排。
        assert_eq!(clamp_thresholds([200, 5]), [95, 5]);
        assert_eq!(clamp_thresholds([5, 250]), [95, 5]);
        // 上界修正不能把值推出区间。
        assert_eq!(clamp_thresholds([95, 95]), [95, 94]);
    }

    #[test]
    fn apply_keeps_unknown_fields() {
        let mut disk = Settings::default();
        disk.extra
            .insert("futureFlag".into(), serde_json::json!(true));
        disk.tray
            .extra
            .insert("futureTray".into(), serde_json::json!("keep"));

        let mut dto = SettingsDto::from_settings(&disk);
        dto.sensitivity = "loose".into();
        dto.onboarding_completed = true;
        dto.apply_to(&mut disk);

        assert_eq!(disk.sensitivity().as_str(), "loose");
        assert!(disk.onboarding_completed);
        assert_eq!(
            disk.extra.get("futureFlag"),
            Some(&serde_json::json!(true)),
            "顶层未知字段必须原样保留"
        );
        assert_eq!(
            disk.tray.extra.get("futureTray"),
            Some(&serde_json::json!("keep")),
            "嵌套未知字段同样要保留"
        );
    }

    #[test]
    fn apply_rejects_out_of_range_thresholds() {
        let mut disk = Settings::default();
        let mut dto = SettingsDto::from_settings(&disk);
        dto.notifications.low_thresholds = [0, 250];
        dto.apply_to(&mut disk);
        assert_eq!(disk.notifications.low_thresholds, [95, 0]);
    }
}
