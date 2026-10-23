/**
 * 设备类型 → 设备卡片最左侧图标。
 *
 * 与 `format.ts::deviceTypeLabel` 同一套前提：类型是 Rust 判定的**结论**，
 * 前端只负责挑一个图标来呈现，绝不根据型号名/链路再推断一次（W-2）。
 *
 * 类型不可知（`unknown`、`other`、未来新增的未知字符串）时退化为**链路图标**：
 * 2.4G（HID）用 `Link`、蓝牙用 `BluetoothConnected`。这时图标表达的是
 * 「这是一条已连上的链路」，而不是在假装知道设备是什么。
 */

import {
  BluetoothConnected,
  Gamepad2,
  Headphones,
  Headset,
  Joystick,
  Keyboard,
  Link,
  Mouse,
  Speaker,
} from "@lucide/vue";
import type { Component } from "vue";
import type { DeviceDto } from "./types";

/** 只放「确实知道是什么」的类型；`other` / `unknown` 不在此列，走链路兜底。 */
const BY_TYPE: Readonly<Record<string, Component>> = {
  keyboard: Keyboard,
  mouse: Mouse,
  gamepad: Gamepad2,
  controller: Joystick,
  headset: Headset,
  headphones: Headphones,
  speaker: Speaker,
};

const BY_TRANSPORT: Readonly<Record<DeviceDto["transport"], Component>> = {
  hid: Link,
  ble: BluetoothConnected,
};

export function deviceTypeIcon(
  deviceType: string,
  transport: DeviceDto["transport"],
): Component {
  return BY_TYPE[deviceType] ?? BY_TRANSPORT[transport] ?? Link;
}
