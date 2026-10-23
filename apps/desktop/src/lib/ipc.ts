// Tauri 命令封装：前端只经此与宿主通信，不直接调 HID/BLE 或读写配置文件。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DiagnosticsDto, OnboardingChoice, SettingsDto, Snapshot } from "./types";

// 快照事件名（W：`state://snapshot`）。
export const SNAPSHOT_EVENT = "state://snapshot";

// 诊断变更事件名（宿主侧 `host::DIAGNOSTICS_EVENT`）。
export const DIAGNOSTICS_EVENT = "state://diagnostics";

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

export function subscribeSnapshot(handler: (snap: Snapshot) => void) {
  return listen<Snapshot>(SNAPSHOT_EVENT, (event) => handler(event.payload));
}

// 订阅「诊断已变」信号。事件无载荷——数据仍以 `get_diagnostics` 为准，宿主只通知「该重拉」，避免把 transports/环形缓冲塞进事件通道。
export function subscribeDiagnostics(handler: () => void) {
  return listen(DIAGNOSTICS_EVENT, () => handler());
}
