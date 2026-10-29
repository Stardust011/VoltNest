//! 宿主 → 前端的 DTO。快照直接复用 `Snapshot`（本身就是传输格式）；设置与诊断另建 DTO 是因为：磁盘类型带 `extra`（未知字段保留）属于磁盘格式，不该进 IPC，诊断页还要拼装运行时状态（线程组/自启/AUMID），而这些不在任何 crate 里。

use serde::{Deserialize, Serialize};
use voltnest_core::{DiagEvent, NotificationNode};
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
    pub tray: TraySettingsDto,
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
pub struct TraySettingsDto {
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
            tray: TraySettingsDto {
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
        target.notifications.low_thresholds =
            clamp_low_battery_levels(self.notifications.low_thresholds);
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
fn clamp_low_battery_levels(pair: [u8; 2]) -> [u8; 2] {
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
    pub notifications: Vec<NotificationNodeDto>,
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
pub struct NotificationNodeDto {
    /// 节点覆盖的设备标识（合并卡的主成员在前）。
    pub device_ids: Vec<String>,
    /// 阈值。
    pub threshold: u8,
    /// 是否处于可触发状态。
    pub armed: bool,
    /// 上次发送时间。
    pub last_notified_at_ms: Option<u64>,
    /// 最后结果（`sent` / `suppressed_cooldown` / `failed` …）。
    pub last_result: Option<String>,
}

impl From<NotificationNode> for NotificationNodeDto {
    fn from(n: NotificationNode) -> Self {
        Self {
            device_ids: n.device_ids,
            threshold: n.threshold,
            armed: n.armed,
            last_notified_at_ms: n.last_notified_at_ms,
            last_result: n.last_result,
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
    pub status: TransportHealth,
    /// 连续失败次数。
    pub failures: u32,
    /// 累计重启次数。
    pub restarts: u32,
    /// 最后失败原因。
    pub last_error: Option<String>,
}

/// 线程组状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransportHealth {
    /// 运行中。
    Running,
    /// 正在退避重启。
    Down,
    /// 连续失败超过上限，等待手动重启。
    Unavailable,
}

impl TransportHealth {
    /// 稳定字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Down => "down",
            Self::Unavailable => "unavailable",
        }
    }
}

// ---- 生成上报数据（HID 抓包）----
//
// 这一组 DTO 的边界与前面一致：只做「结构搬运 + 展示字符串组装」。十六进制串在这里就拼好，
// 免得前端再实现一份格式化逻辑（两份实现迟早会漂移，而用户会拿界面上的字节去对 issue）。

/// 一台设备（同一 VID:PID 的全部集合）。
///
/// 交互先按 VID:PID 收敛，再让用户挑集合：复合 HID 会把标准鼠标集合与厂商私有集合
/// 一起列出来，直接铺平会让用户面对几十条 `0xFF00/0x0001` 无从下手。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HidTargetDto {
    /// 厂商 ID。
    pub vid: u16,
    /// 产品 ID。
    pub pid: u16,
    /// `0xVVVV:0xPPPP`。
    pub vid_pid: String,
    /// 列表显示名（产品名，缺失时为 VID:PID）。
    pub display_name: String,
    /// 厂商名。
    pub manufacturer: Option<String>,
    /// 序列号（多个用「、」连接，无则 `None`）。
    pub serial_summary: Option<String>,
    /// 私有集合（UsagePage `>= 0xFF00`）数量。
    pub private_collections: usize,
    /// 该设备的全部集合（顺序由宿主决定）。
    pub collections: Vec<HidCollectionDto>,
}

impl From<voltnest_hid::probe::TargetGroup> for HidTargetDto {
    fn from(group: voltnest_hid::probe::TargetGroup) -> Self {
        let vid_pid = group.vid_pid();
        let display_name = group.display_name();
        let serial_summary = group.serial_summary();
        let private_collections = group.private_collections();
        let collections = group
            .collections
            .into_iter()
            .map(HidCollectionDto::from)
            .collect();
        Self {
            vid: group.vid,
            pid: group.pid,
            vid_pid,
            display_name,
            manufacturer: group.manufacturer,
            serial_summary,
            private_collections,
            collections,
        }
    }
}

/// 一个 HID 集合（生成上报数据时的候选行）。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HidCollectionDto {
    /// 平台路径，回传给宿主用于打开。
    pub path: String,
    /// 厂商 ID。
    pub vid: u16,
    /// 产品 ID。
    pub pid: u16,
    /// `0xVVVV:0xPPPP`。
    pub vid_pid: String,
    /// 列表显示名（产品名，缺失时为 VID:PID）。
    pub display_name: String,
    /// 产品名原始值。
    pub product: Option<String>,
    /// 厂商名。
    pub manufacturer: Option<String>,
    /// 序列号。
    pub serial: Option<String>,
    /// 接口号（后端不提供时 `-1`）。
    pub interface_number: i32,
    /// 全部 Usage 的紧凑写法。
    pub usage_summary: String,
    /// 是否只有标准 Usage（鼠标/键盘），用于提示「这不是厂商私有通道」。
    pub standard_only: bool,
}

impl From<voltnest_hid::probe::CollectionInfo> for HidCollectionDto {
    fn from(info: voltnest_hid::probe::CollectionInfo) -> Self {
        // 展示串先算好再搬字段：`path`/`serial` 一旦被移动就没法再借用 `info`。
        let vid_pid = info.vid_pid();
        let display_name = info.display_name();
        let usage_summary = info.usage_summary();
        let standard_only = info.only_standard_usages();
        Self {
            path: info.path,
            vid: info.vid,
            pid: info.pid,
            vid_pid,
            display_name,
            product: info.product,
            manufacturer: info.manufacturer,
            serial: info.serial,
            interface_number: info.interface_number,
            usage_summary,
            standard_only,
        }
    }
}

/// 开始抓取的请求。`writeFrames` 为空 = 纯监听。
#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStartDto {
    /// 选中的集合路径。
    pub path: String,
    /// 录制时长（毫秒），宿主会夹到合法区间。
    pub duration_ms: u64,
    /// 录制期间周期性发送的查询报文（十六进制文本，每条一帧）。
    #[serde(default)]
    pub write_frames: Vec<String>,
    /// 查询报文发送间隔（毫秒）；缺省用宿主默认值。
    #[serde(default)]
    pub write_interval_ms: Option<u64>,
}

/// 开始抓取的回执：前端据此订阅进度并显示「正在录哪台」。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStartResultDto {
    /// 会话号（后续停止 / 报告 / 保存都用它）。
    pub session_id: u64,
    /// 实际打开的集合（宿主重新枚举过，不是前端传上来的那份）。
    pub device: HidCollectionDto,
    /// 实际生效的录制时长。
    pub duration_ms: u64,
    /// 实际生效的帧数上限。
    pub max_frames: usize,
}

/// 一帧原始报告。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureFrameDto {
    /// 相对录制开始的毫秒数。
    pub at_ms: u64,
    /// `IN` / `OUT`。
    pub direction: &'static str,
    /// 字节长度。
    pub len: usize,
    /// 空格分隔的大写十六进制。
    pub hex: String,
}

impl From<&voltnest_hid::probe::CapturedFrame> for CaptureFrameDto {
    fn from(frame: &voltnest_hid::probe::CapturedFrame) -> Self {
        Self {
            at_ms: frame.elapsed_ms,
            direction: frame.direction.as_str(),
            len: frame.data.len(),
            hex: frame.hex(),
        }
    }
}

/// 抓取统计。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSummaryDto {
    /// 总帧数。
    pub frames: usize,
    /// 输入帧数。
    pub input_frames: usize,
    /// 主动发出的帧数。
    pub output_frames: usize,
    /// 实际录制时长（毫秒）。
    pub elapsed_ms: u64,
    /// 是否因帧数上限提前结束。
    pub truncated: bool,
    /// 主动发送失败次数。
    pub write_failures: usize,
    /// 首次发送失败原因。
    pub write_error: Option<String>,
}

impl From<&voltnest_hid::probe::CaptureSummary> for CaptureSummaryDto {
    fn from(summary: &voltnest_hid::probe::CaptureSummary) -> Self {
        Self {
            frames: summary.frames,
            input_frames: summary.input_frames,
            output_frames: summary.output_frames,
            elapsed_ms: summary.elapsed_ms,
            truncated: summary.truncated,
            write_failures: summary.write_failures,
            write_error: summary.write_error.clone(),
        }
    }
}

/// 抓取进度事件（事件名 `probe://capture`）。
///
/// 批量下发而不是一帧一个事件：鼠标 1kHz 上报时逐帧发事件会把 IPC 打满，
/// 反而让界面卡住。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureEventDto {
    /// 会话号：前端用它丢弃上一个会话的迟到事件。
    pub session_id: u64,
    /// 本批新增的帧。
    pub frames: Vec<CaptureFrameDto>,
    /// 该会话累计帧数。
    pub total: usize,
    /// `running` / `done` / `stopped` / `failed`。
    pub status: &'static str,
    /// 失败原因（`failed` 时有值）。
    pub message: Option<String>,
    /// 结束时的统计（`done` / `stopped` 时有值）。
    pub summary: Option<CaptureSummaryDto>,
}

/// 一次采样：会话号 + 当时驱动显示的电量。
///
/// 电量与采样绑在一起（而不是设备级字段）：同一台设备在不同电量下各录一段，
/// 两段的字节差异才是定位电量字段的依据。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSampleDto {
    /// 抓包会话号。
    pub session_id: u64,
    /// 驱动 / 系统界面当时显示的电量（`None` = 驱动没显示）。
    #[serde(default)]
    pub battery_percent: Option<u8>,
}

/// 生成报告 / 保存手动设备的表单。
#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportFormDto {
    /// 采样（至少一条；顺序即报告里的样本编号）。
    pub samples: Vec<ReportSampleDto>,
    /// 设备名（用户可改，默认取 HID 产品名）。
    pub name: String,
    /// 操作步骤（用户填写，原样进报告）。
    #[serde(default)]
    pub steps: String,
    /// 期望行为。
    #[serde(default)]
    pub expectation: String,
    /// 补充说明。
    #[serde(default)]
    pub note: String,
}

/// 生成的报告：正文 + 落盘路径。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportDto {
    /// 完整 Markdown（用户可直接复制进 issue）。
    pub markdown: String,
    /// 已写入磁盘的文件路径。
    pub path: String,
}

/// 手动添加的设备（落盘记录的前端投影）。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualDeviceDto {
    /// 标识（`hid:xxxx:yyyy`）。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// `0xVVVV:0xPPPP`。
    pub vid_pid: String,
    /// 抓包时的 UsagePage / Usage。
    pub usage_summary: String,
    /// 产品名。
    pub product: Option<String>,
    /// 序列号。
    pub serial: Option<String>,
    /// 补充说明。
    pub note: String,
    /// 添加时间（UTC 毫秒）。
    pub added_at_ms: u64,
    /// 最近一次抓包时间。
    pub last_capture_at_ms: Option<u64>,
    /// 全部采样的帧数之和。
    pub captured_frames: usize,
    /// 采样记录（含每次驱动显示的电量）。
    pub samples: Vec<ManualSampleDto>,
    /// 报告文件路径（可能为 `None`：用户只保存了设备，没生成报告）。
    pub report_path: Option<String>,
}

/// 一次采样（列表里展示「80% → 75%」这种对照）。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualSampleDto {
    /// 驱动 / 系统显示的电量（`None` = 当时没填/没显示）。
    pub battery_percent: Option<u8>,
    /// 该次录到的帧数。
    pub frames: usize,
    /// 录制完成时间（UTC 毫秒）。
    pub captured_at_ms: u64,
}

impl From<&voltnest_storage::ManualSample> for ManualSampleDto {
    fn from(sample: &voltnest_storage::ManualSample) -> Self {
        Self {
            battery_percent: sample.battery_percent,
            frames: sample.frames,
            captured_at_ms: sample.captured_at_ms,
        }
    }
}

impl From<&voltnest_storage::ManualDevice> for ManualDeviceDto {
    fn from(device: &voltnest_storage::ManualDevice) -> Self {
        Self {
            id: device.id.clone(),
            name: device.name.clone(),
            vid_pid: format!("0x{:04X}:0x{:04X}", device.vid, device.pid),
            usage_summary: format!("0x{:04X}/0x{:04X}", device.usage_page, device.usage),
            product: device.product.clone(),
            serial: device.serial.clone(),
            note: device.note.clone(),
            added_at_ms: device.added_at_ms,
            last_capture_at_ms: device.last_capture_at_ms,
            captured_frames: device.captured_frames,
            samples: device.samples.iter().map(ManualSampleDto::from).collect(),
            report_path: device.report_path.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voltnest_storage::Settings;

    #[test]
    fn thresholds_are_kept_strictly_descending() {
        assert_eq!(clamp_low_battery_levels([20, 10]), [20, 10]);
        // 逆序：以数值大小重排，高阈值在前。
        assert_eq!(clamp_low_battery_levels([10, 20]), [20, 10]);
        // 相等：拆开一位，避免同一档重复触发。
        assert_eq!(clamp_low_battery_levels([15, 15]), [15, 14]);
        // 下界 0 合法：设置页允许填 0%。
        assert_eq!(clamp_low_battery_levels([0, 0]), [1, 0]);
        assert_eq!(clamp_low_battery_levels([1, 0]), [1, 0]);
        // 越界：夹到 0..=95 后重排。
        assert_eq!(clamp_low_battery_levels([200, 5]), [95, 5]);
        assert_eq!(clamp_low_battery_levels([5, 250]), [95, 5]);
        // 上界修正不能把值推出区间。
        assert_eq!(clamp_low_battery_levels([95, 95]), [95, 94]);
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
