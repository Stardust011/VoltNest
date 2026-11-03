import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  CaptureEventDto,
  CaptureStartRequest,
  CaptureStartResult,
  DiagnosticsDto,
  HidTargetDto,
  ManualDeviceDto,
  OnboardingChoice,
  ReportDto,
  ReportForm,
  SettingsDto,
  Snapshot,
} from "./types";

// 快照事件名（W：`state://snapshot`）。
export const SNAPSHOT_EVENT = "state://snapshot";

// 诊断变更事件名（宿主侧 `host::DIAGNOSTICS_EVENT`）。
export const DIAGNOSTICS_EVENT = "state://diagnostics";

// 抓包进度事件名（宿主侧 `probe::CAPTURE_EVENT`）。
export const CAPTURE_EVENT = "probe://capture";

export function getSnapshot(): Promise<Snapshot> {
  return invoke<Snapshot>("get_snapshot");
}

export function getSettings(): Promise<SettingsDto> {
  return invoke<SettingsDto>("get_settings");
}

export function updateSettings(settings: SettingsDto): Promise<SettingsDto> {
  return invoke<SettingsDto>("update_settings", { settings });
}

export function getDiagnostics(): Promise<DiagnosticsDto> {
  return invoke<DiagnosticsDto>("get_diagnostics");
}

export function restartTransport(transport: "hid" | "ble"): Promise<void> {
  return invoke<void>("restart_transport", { transport });
}

export function testNotification(): Promise<void> {
  return invoke<void>("test_notification");
}

export function setAutostart(enabled: boolean): Promise<boolean> {
  return invoke<boolean>("set_autostart", { enabled });
}

export function completeOnboarding(choice: OnboardingChoice): Promise<SettingsDto> {
  return invoke<SettingsDto>("complete_onboarding", { choice });
}

/** 收进托盘：宿主隐藏窗口并强制落盘，进程继续跑。 */
export function hideToTray(): Promise<void> {
  return invoke<void>("hide_to_tray");
}

/** 退出程序：宿主 `app.exit(0)`，由 Exit 事件统一停采集线程、落盘。 */
export function quitApp(): Promise<void> {
  return invoke<void>("quit_app");
}

export function subscribeSnapshot(handler: (snap: Snapshot) => void) {
  return listen<Snapshot>(SNAPSHOT_EVENT, (event) => handler(event.payload));
}

export function subscribeDiagnostics(handler: () => void) {
  return listen(DIAGNOSTICS_EVENT, () => handler());
}

// ---- 生成上报数据 ----

/** 枚举设备（按 VID:PID 聚合，含型号表里没有的设备）。 */
export function listHidTargets(): Promise<HidTargetDto[]> {
  return invoke<HidTargetDto[]>("list_hid_targets");
}

/** 开始抓包。返回会话号，数据经 `probe://capture` 分批推送。 */
export function startHidCapture(request: CaptureStartRequest): Promise<CaptureStartResult> {
  return invoke<CaptureStartResult>("start_hid_capture", { request });
}

/** 请求停止（已录到的数据保留）。 */
export function stopHidCapture(sessionId: number): Promise<void> {
  return invoke<void>("stop_hid_capture", { sessionId });
}

/** 丢弃会话并释放帧内存（关闭面板时调用）。 */
export function discardHidCapture(sessionId: number): Promise<void> {
  return invoke<void>("discard_hid_capture", { sessionId });
}

/** 生成 issue 报告：返回正文并落盘一份完整文件。 */
export function generateIssueReport(form: ReportForm): Promise<ReportDto> {
  return invoke<ReportDto>("generate_issue_report", { form });
}

/** 保存为一条上报记录（报告同时落盘）。 */
export function saveManualDevice(form: ReportForm): Promise<ManualDeviceDto> {
  return invoke<ManualDeviceDto>("save_manual_device", { form });
}

export function listManualDevices(): Promise<ManualDeviceDto[]> {
  return invoke<ManualDeviceDto[]>("list_manual_devices");
}

export function removeManualDevice(id: string): Promise<void> {
  return invoke<void>("remove_manual_device", { id });
}

/** 读取某条手动设备的报告正文（报告文件是唯一数据源）。 */
export function getManualReport(id: string): Promise<string> {
  return invoke<string>("get_manual_report", { id });
}

export function subscribeCapture(handler: (event: CaptureEventDto) => void) {
  return listen<CaptureEventDto>(CAPTURE_EVENT, (event) => handler(event.payload));
}
