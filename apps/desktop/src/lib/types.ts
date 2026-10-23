/**
 * 与 Rust 侧 `voltnest_core::Snapshot` 一一对应的 DTO 类型。
 *
 * 这些类型是**契约**：字段名必须与 serde camelCase 输出一致，
 * 前端不得在这里添加派生业务字段（W-1 / W-2）。
 */

export type DeviceState = "unknown" | "online" | "stale" | "unavailable" | "offline";

export interface DeviceDto {
  stableId: string;
  transport: "hid" | "ble";
  provider: string;
  displayName: string;
  modelKey: string | null;
  /**
   * 设备类型（Rust 侧枚举的小写形式）。`"unknown"` = 证据不足。
   *
   * 这是**已判定的结论**，前端只负责翻译成文案，不得据型号名/链路再推断一次（W-2）。
   */
  deviceType: string;
  /** 得出 `deviceType` 的方法，仅用于诊断溯源。 */
  classificationMethod: string;
  battery: number | null;
  lastKnownBattery: number | null;
  lastKnownAtMs: number | null;
  isCharging: boolean | null;
  state: DeviceState;
  lastValidAtMs: number | null;
  lastPushAtMs: number | null;
  lastError: string | null;
}

export interface CardDto {
  cardId: string;
  primaryDeviceId: string;
  memberDeviceIds: string[];
  battery: number | null;
  isCharging: boolean | null;
  state: DeviceState;
  inactiveMemberText: string;
}

export interface Snapshot {
  rev: number;
  serverNowMs: number;
  devices: DeviceDto[];
  cards: CardDto[];
  tray: { visibleDeviceIds: string[] };
}

export type NotificationMode = "system" | "trayBubble" | "off";
export type Sensitivity = "sensitive" | "standard" | "loose";

export interface SettingsDto {
  schemaVersion: number;
  sensitivity: Sensitivity;
  notifications: { mode: NotificationMode; lowThresholds: [number, number] };
  autostart: boolean;
  startMinimized: boolean;
  onboardingCompleted: boolean;
  tray: { order: string[]; hidden: string[] };
}

export interface DiagEventDto {
  atMs: number;
  level: "info" | "warn" | "error";
  code: string;
  deviceId: string | null;
  detail: string;
}

export interface TransportStatusDto {
  transport: "hid" | "ble";
  family: string;
  status: "running" | "down" | "unavailable";
  restarts: number;
  lastError: string | null;
  queuedEvents: number;
}

export interface NotificationStateDto {
  stableId: string;
  threshold: number;
  armed: boolean;
  lastNotifiedAtMs: number | null;
  lastResult: string | null;
}

export interface DiagnosticsDto {
  events: DiagEventDto[];
  transports: TransportStatusDto[];
  notifications: NotificationStateDto[];
  autostartRegistered: boolean;
  aumidRegistered: boolean;
  dataRoot: string;
  version: string;
}

export interface OnboardingChoice {
  autostart: boolean;
  startMinimized: boolean;
}
