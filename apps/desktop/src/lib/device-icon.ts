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

export function deviceTypeIcon(deviceType: string, transport: DeviceDto["transport"]): Component {
  return BY_TYPE[deviceType] ?? BY_TRANSPORT[transport] ?? Link;
}
