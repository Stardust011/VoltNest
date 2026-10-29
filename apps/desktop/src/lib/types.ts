// 与 Rust 侧 `voltnest_core::Snapshot` 一一对应的 DTO 类型：字段名须匹配 serde camelCase，前端不得加派生业务字段。

export type DeviceState = "unknown" | "online" | "stale" | "unavailable" | "offline";

export interface DeviceDto {
  stableId: string;
  transport: "hid" | "ble";
  provider: string;
  displayName: string;
  modelKey: string | null;
  // 设备类型（Rust 枚举小写）。`"unknown"`=证据不足；已判定结论，前端只翻译、不再推断。
  deviceType: string;
  // 得出 `deviceType` 的方法，仅用于诊断溯源。
  classificationMethod: string;
  battery: number | null;
  lastKnownBattery: number | null;
  lastKnownAtMs: number | null;
  isCharging: boolean | null;
  state: DeviceState;
  lastValidAtMs: number | null;
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
  // `debug` 默认折叠：放逐条枚举细节而非结论。
  level: "debug" | "info" | "warn" | "error";
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

export interface NotificationNodeDto {
  // 节点覆盖的设备标识：合并卡的多条链路合成一个节点，主成员在前。
  deviceIds: string[];
  threshold: number;
  armed: boolean;
  lastNotifiedAtMs: number | null;
  lastResult: string | null;
}

export interface DiagnosticsDto {
  events: DiagEventDto[];
  transports: TransportStatusDto[];
  notifications: NotificationNodeDto[];
  autostartRegistered: boolean;
  aumidRegistered: boolean;
  dataRoot: string;
  version: string;
}

export interface OnboardingChoice {
  autostart: boolean;
  startMinimized: boolean;
}

// ---- 生成上报数据（HID 抓包）----
// 与 Rust 侧 `apps/desktop/src-tauri/src/dto.rs` 的 probe 段一一对应，字段名须匹配 camelCase。

/**
 * 一台设备（同一 VID:PID 的全部集合）。
 *
 * 交互的第一步是按 VID:PID 收敛：复合 HID 会把标准鼠标集合和厂商私有集合一起列出来，
 * 直接平铺会让用户在几十条 `0xFF00/0x0001` 里挑。
 */
export interface HidTargetDto {
  vid: number;
  pid: number;
  vidPid: string;
  displayName: string;
  manufacturer: string | null;
  /** 多个序列号用「、」连接；无则 null。 */
  serialSummary: string | null;
  /** 私有集合（UsagePage >= 0xFF00）数量。 */
  privateCollections: number;
  collections: HidCollectionDto[];
}

/** 一个 HID 集合（第二步的候选行）。`path` 是打开与去重的唯一标识。 */
export interface HidCollectionDto {
  path: string;
  vid: number;
  pid: number;
  /** `0xVVVV:0xPPPP`，宿主拼好，前端不再格式化。 */
  vidPid: string;
  displayName: string;
  product: string | null;
  manufacturer: string | null;
  serial: string | null;
  /** 后端不提供时为 -1。 */
  interfaceNumber: number;
  /** 该 path 上全部 Usage 的紧凑写法。 */
  usageSummary: string;
  /** 只声明了标准鼠标/键盘用法：多半不是厂商私有通道。 */
  standardOnly: boolean;
}

export interface CaptureStartRequest {
  path: string;
  durationMs: number;
  /** 十六进制文本，宿主负责解析；空数组 = 纯监听。 */
  writeFrames: string[];
  writeIntervalMs?: number;
}

export interface CaptureStartResult {
  sessionId: number;
  device: HidCollectionDto;
  durationMs: number;
  maxFrames: number;
}

export interface CaptureFrameDto {
  atMs: number;
  direction: "IN" | "OUT";
  len: number;
  /** 空格分隔的大写十六进制。 */
  hex: string;
}

export interface CaptureSummaryDto {
  frames: number;
  inputFrames: number;
  outputFrames: number;
  elapsedMs: number;
  truncated: boolean;
  writeFailures: number;
  writeError: string | null;
}

export type CaptureStatus = "running" | "done" | "stopped" | "failed";

export interface CaptureEventDto {
  sessionId: number;
  frames: CaptureFrameDto[];
  total: number;
  status: CaptureStatus;
  message: string | null;
  summary: CaptureSummaryDto | null;
}

/** 一次采样：会话号 + 当时驱动显示的电量。 */
export interface ReportSample {
  sessionId: number;
  /** null = 驱动没显示电量（不编造 0%）。 */
  batteryPercent: number | null;
}

/** 生成报告 / 保存手动设备共用的表单（采样按顺序编号）。 */
export interface ReportForm {
  samples: ReportSample[];
  name: string;
  steps: string;
  expectation: string;
  note: string;
}

export interface ReportDto {
  /** 只有正文：预览不落盘，文件在「保存到本地」时才生成。 */
  markdown: string;
}

export interface ManualDeviceDto {
  id: string;
  name: string;
  vidPid: string;
  usageSummary: string;
  product: string | null;
  serial: string | null;
  note: string;
  addedAtMs: number;
  lastCaptureAtMs: number | null;
  /** 全部采样的帧数之和。 */
  capturedFrames: number;
  /** 采样记录（含每次驱动显示的电量）。 */
  samples: ManualSampleDto[];
  reportPath: string | null;
}

export interface ManualSampleDto {
  /** null = 当时没填 / 驱动没显示。 */
  batteryPercent: number | null;
  frames: number;
  capturedAtMs: number;
}
