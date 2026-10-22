/**
 * Tauri 命令封装。
 *
 * 前端只通过这里与宿主通信：不直接调用 HID/BLE，不读写 settings.json / state.json。
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  DiagnosticsDto,
  OnboardingChoice,
  SettingsDto,
  Snapshot,
} from "./types";

/** 快照事件名（W：`state://snapshot`）。 */
export const SNAPSHOT_EVENT = "state://snapshot";

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
