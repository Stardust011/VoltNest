/**
 * 展示层纯函数：状态文案、颜色语义、托盘环几何。
 *
 * 这里只做「把 Rust 给的字段翻译成 CSS/文案」，不做任何业务推断：
 * 状态、分组、排序、副行文案全部来自快照（W-2 / W-3）。
 */

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

/**
 * 充电绿：与托盘 `tray.rs::EMERALD` 同一个色值，两侧并排看必须一致（规格 §12）。
 *
 * 充电时不按电量分档 —— 闪电已经把「在充电」说清楚了，弧长继续表达充到哪了。
 */
const CHARGING_GREEN = "#10b981";

/**
 * 电量环颜色：与托盘图标的规则对齐（规格 §12）。
 *
 * 低于 20% 橙色、低于 10% 红色、充电绿色；离线/过期沿用状态色。
 */
export function batteryColor(battery: number | null, charging: boolean, state: DeviceState): string {
  if (state !== "online" && state !== "stale") return toneFor(state).ring;
  if (charging) return CHARGING_GREEN;
  if (battery === null) return toneFor(state).ring;
  if (battery < 10) return "#ef4444";
  if (battery < 20) return "#f97316";
  return toneFor(state).ring;
}

/** 电量环的 stroke-dasharray：`null` 时返回 0（不画比例，只画底环）。 */
export function ringProgress(battery: number | null): number {
  if (battery === null) return 0;
  return Math.min(100, Math.max(0, battery));
}

export function stateLabel(state: DeviceState): string {
  return toneFor(state).label;
}

/** 副行文案：充电 / 上次已知 / 无读数，全部由快照字段决定。 */
export function secondaryLine(device: DeviceDto): string {
  if (device.battery === null) {
    // 缓存值已经在主读数位显示（见 displayedBattery），副行只补时间语义。
    if (device.lastKnownBattery !== null) return "上次读数";
    return device.state === "unknown" ? "尚未收到读数" : "无可用读数";
  }
  if (device.isCharging === true) return "充电中";
  if (device.isCharging === null) return "充电状态未知";
  return "使用电池";
}

/**
 * 卡片主读数：本次会话还没有读数、状态仍是 `unknown`（启动回填 / 离线后重新出现）时，
 * 用 `lastKnownBattery` 撑住首屏，避免出现「— + 空进度条」。
 *
 * `unavailable` / `offline` 不算 —— 那是「确定读不到」，不能假装还有实时读数。
 */
export function displayedBattery(device: DeviceDto): number | null {
  if (device.battery !== null) return device.battery;
  return device.state === "unknown" ? device.lastKnownBattery : null;
}

export function transportLabel(transport: "hid" | "ble"): string {
  return transport === "hid" ? "2.4G" : "蓝牙";
}

export function batteryText(device: DeviceDto): string {
  return device.battery === null ? "—" : `${device.battery}%`;
}
