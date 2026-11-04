export type DeviceState = "unknown" | "online" | "stale" | "unavailable" | "offline";

export interface DeviceDto {
  stableId: string;
  transport: "hid" | "ble";
  provider: string;
  displayName: string;
  modelKey: string | null;
  deviceType: string;
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
  /** 呈现单元的稳定锚点（恒为某个成员的 stableId）；悬浮窗、托盘菜单都以它为键。 */
  anchorId?: string;
  /** 卡片标题：用户合并取合并名，自动合并取主成员名称。 */
  displayName: string;
  /** 由用户手动合并（决定是否提供「分离合并」）。 */
  userMerged: boolean;
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
  tray: {
    visibleDeviceIds: string[];
    present: number;
    live: number;
    overflow: number;
  };
}

/** 悬浮窗单元：一个呈现单元（一台设备或一个合并组）对应一个可独立显隐的窗口。 */
export interface FloatingUnitDto {
  anchor: string;
  title: string;
  visible: boolean;
  offline: boolean;
  side: number;
  transparency: number;
}

export interface FloatingStateDto {
  rev: number;
  units: FloatingUnitDto[];
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

/** 「添加设备」候选：与自动识别共用同一套筛选。 */
export interface DeviceCandidateDto {
  stableId: string;
  displayName: string;
  transport: "hid" | "ble";
  /** `0xVVVV:0xPPPP`；蓝牙为 null。 */
  vidPid: string | null;
  /** 已在设备列表里。 */
  inList: boolean;
  /** 曾被删除，可恢复。 */
  removed: boolean;
}

// ---- 生成上报数据（HID 抓包）----

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
