// 展示层纯函数：只把快照字段翻译成 CSS/文案，不做业务推断（W-2 / W-3）。

import type { DeviceDto, DeviceState } from "./types";

export interface Tone {
  /** 主文本色（Tailwind 类）。 */
  text: string;
  /** 圆点 / 边框色。 */
  dot: string;
  /** 电量环颜色。 */
  ring: string;
  /** 状态标签背景。 */
  chip: string;
  label: string;
}

const TONES: Record<DeviceState, Tone> = {
  online: {
    text: "text-emerald-700 dark:text-emerald-300",
    dot: "bg-emerald-500",
    ring: "#10b981",
    chip: "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
    label: "在线",
  },
  stale: {
    text: "text-amber-700 dark:text-amber-300",
    dot: "bg-amber-500",
    ring: "#f59e0b",
    chip: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
    label: "数据过期",
  },
  unavailable: {
    text: "text-orange-700 dark:text-orange-300",
    dot: "bg-orange-500",
    ring: "#f97316",
    chip: "bg-orange-500/10 text-orange-700 dark:text-orange-300",
    label: "读取失败",
  },
  offline: {
    text: "text-neutral-500 dark:text-neutral-400",
    dot: "bg-neutral-400",
    ring: "#a3a3a3",
    chip: "bg-neutral-500/10 text-neutral-600 dark:text-neutral-400",
    label: "离线",
  },
  unknown: {
    text: "text-sky-700 dark:text-sky-300",
    dot: "bg-sky-500",
    ring: "#38bdf8",
    chip: "bg-sky-500/10 text-sky-700 dark:text-sky-300",
    label: "等待读数",
  },
};

export function toneFor(state: DeviceState): Tone {
  return TONES[state] ?? TONES.unknown;
}

// 充电绿：与托盘 `tray.rs::EMERALD` 同色值，两侧必须一致（§12）。充电不分档，闪电已表「在充」，弧长仍表进度。
const CHARGING_GREEN = "#10b981";

// 电量环颜色与托盘对齐（§12）：<10% 红、<20% 橙、充电绿；离线/过期沿用状态色。
export function batteryColor(battery: number | null, charging: boolean, state: DeviceState): string {
  if (state !== "online" && state !== "stale") return toneFor(state).ring;
  if (charging) return CHARGING_GREEN;
  if (battery === null) return toneFor(state).ring;
  if (battery < 10) return "#ef4444";
  if (battery < 20) return "#f97316";
  return toneFor(state).ring;
}

// null 时返回 0（只画底环，不画比例）。
export function ringProgress(battery: number | null): number {
  if (battery === null) return 0;
  return Math.min(100, Math.max(0, battery));
}

export function stateLabel(state: DeviceState): string {
  return toneFor(state).label;
}

// 副行文案，全部由快照字段决定。
export function secondaryLine(device: DeviceDto): string {
  if (device.battery === null) {
    // 缓存值已在主位显示，副行只补时间语义。
    if (device.lastKnownBattery !== null) return "上次读数";
    return device.state === "unknown" ? "尚未收到读数" : "无可用读数";
  }
  if (device.isCharging === true) return "充电中";
  if (device.isCharging === null) return "充电状态未知";
  return "使用电池";
}

// 主读数：状态为 `unknown`（启动回填/离线重现）时用 `lastKnownBattery` 撑首屏，避免「— + 空进度条」。
// unavailable/offline 是「确定读不到」，不借缓存假装有实时读数。
export function displayedBattery(device: DeviceDto): number | null {
  if (device.battery !== null) return device.battery;
  return device.state === "unknown" ? device.lastKnownBattery : null;
}

export function transportLabel(transport: "hid" | "ble"): string {
  return transport === "hid" ? "2.4G" : "蓝牙";
}

// 诊断列表默认折叠 `debug`：级别只控「默认是否展示」，不控「是否记录」——逐条证据仍留环形缓冲，展开可核对，过滤不改数据（W-2）。
export function visibleDiagEvents<T extends { level: string }>(
  events: readonly T[],
  showDebug: boolean,
): T[] {
  return showDebug ? [...events] : events.filter((e) => e.level !== "debug");
}

// 设备类型 → 中文短标签。`unknown`/未知串返回 `null`：识别不出就不显示，前端不猜类型（W-2）。
export function deviceTypeLabel(deviceType: string): string | null {
  switch (deviceType) {
    case "keyboard":
      return "键盘";
    case "mouse":
      return "鼠标";
    case "gamepad":
      return "手柄";
    case "headset":
      return "头戴耳机";
    case "headphones":
      return "耳机";
    case "speaker":
      return "音箱";
    case "controller":
      return "控制器";
    case "other":
      return "其他";
    default:
      return null;
  }
}

export function batteryText(device: DeviceDto): string {
  return device.battery === null ? "—" : `${device.battery}%`;
}
