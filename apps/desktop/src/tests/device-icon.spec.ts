import { describe, expect, it } from "vitest";
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
import { deviceTypeIcon } from "../lib/device-icon";

describe("deviceTypeIcon", () => {
  it("已知类型给出对应图标", () => {
    expect(deviceTypeIcon("keyboard", "hid")).toBe(Keyboard);
    expect(deviceTypeIcon("mouse", "hid")).toBe(Mouse);
    expect(deviceTypeIcon("gamepad", "hid")).toBe(Gamepad2);
    expect(deviceTypeIcon("controller", "hid")).toBe(Joystick);
    expect(deviceTypeIcon("headset", "ble")).toBe(Headset);
    expect(deviceTypeIcon("headphones", "ble")).toBe(Headphones);
    expect(deviceTypeIcon("speaker", "ble")).toBe(Speaker);
  });

  it("类型不可知时按链路兜底：2.4G → Link、蓝牙 → BluetoothConnected", () => {
    for (const value of ["unknown", "other", "", "some_future_type"]) {
      expect(deviceTypeIcon(value, "hid")).toBe(Link);
      expect(deviceTypeIcon(value, "ble")).toBe(BluetoothConnected);
    }
  });

  it("类型已知时不受链路影响", () => {
    expect(deviceTypeIcon("mouse", "ble")).toBe(Mouse);
  });
});
