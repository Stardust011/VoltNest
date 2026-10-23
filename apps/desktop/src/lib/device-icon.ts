// 设备类型 → 卡片图标。类型是 Rust 判定的结论，前端只挑图标、绝不二次推断（W-2）。
// 类型不可知（unknown/other/未知串）退化为链路图标：2.4G→Link、蓝牙→BluetoothConnected，只表「已连上」而非「已知设备」。

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

// 仅收录已确知的类型；other/unknown 走链路兜底。
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
