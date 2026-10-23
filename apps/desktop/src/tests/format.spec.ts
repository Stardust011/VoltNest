import { describe, expect, it } from "vitest";
import {
  batteryColor,
  batteryText,
  deviceTypeLabel,
  displayedBattery,
  ringProgress,
  secondaryLine,
  stateLabel,
  toneFor,
  transportLabel,
} from "../lib/format";
import type { DeviceDto, DeviceState } from "../lib/types";

function device(overrides: Partial<DeviceDto> = {}): DeviceDto {
  return {
    stableId: "hid:7768:6426",
    transport: "hid",
    provider: "hid-vbas",
    displayName: "AJ159 NL",
    modelKey: "aj159 nl",
    deviceType: "mouse",
    classificationMethod: "verified_device_mapping",
    battery: 90,
    lastKnownBattery: 90,
    lastKnownAtMs: 1_000,
    isCharging: false,
    state: "online",
    lastValidAtMs: 1_000,
    lastPushAtMs: 1_000,
    lastError: null,
    ...overrides,
  };
}

describe("deviceTypeLabel", () => {
  it("已知类型给出短标签", () => {
    expect(deviceTypeLabel("mouse")).toBe("鼠标");
    expect(deviceTypeLabel("keyboard")).toBe("键盘");
    expect(deviceTypeLabel("headset")).toBe("头戴耳机");
    expect(deviceTypeLabel("headphones")).toBe("耳机");
    expect(deviceTypeLabel("speaker")).toBe("音箱");
  });

  it("unknown 与未知字符串都返回 null（前端不得自己猜类型）", () => {
    for (const value of ["unknown", "", "MX-Master", "some_future_type"]) {
      expect(deviceTypeLabel(value)).toBeNull();
    }
  });
});

describe("toneFor / stateLabel", () => {
  it("五种状态各有独立文案", () => {
    const labels = (["online", "stale", "unavailable", "offline", "unknown"] as DeviceState[]).map(
      stateLabel,
    );
    expect(labels).toEqual(["在线", "数据过期", "读取失败", "离线", "等待读数"]);
    expect(new Set(labels).size).toBe(5);
  });

  it("未知状态回退到 unknown 而不是抛错", () => {
    // 宿主升级后新增状态时，旧前端必须仍能渲染。
    expect(toneFor("bogus" as DeviceState)).toEqual(toneFor("unknown"));
  });
});

describe("batteryColor", () => {
  it("充电优先于电量档位，且用托盘同款绿色", () => {
    expect(batteryColor(5, true, "online")).toBe("#10b981");
    expect(batteryColor(5, true, "online")).toBe(toneFor("online").ring);
  });

  it("低于 10% 红色、低于 20% 橙色、其余用主题色", () => {
    expect(batteryColor(9, false, "online")).toBe("#ef4444");
    expect(batteryColor(10, false, "online")).toBe("#f97316");
    expect(batteryColor(19, false, "online")).toBe("#f97316");
    expect(batteryColor(20, false, "online")).toBe(toneFor("online").ring);
  });

  it("非活跃状态用状态色，不看电量", () => {
    expect(batteryColor(5, false, "offline")).toBe(toneFor("offline").ring);
    expect(batteryColor(5, false, "unavailable")).toBe(toneFor("unavailable").ring);
    expect(batteryColor(null, false, "unknown")).toBe(toneFor("unknown").ring);
  });

  it("stale 仍按电量着色，过期不等于没电", () => {
    expect(batteryColor(9, false, "stale")).toBe("#ef4444");
  });
});

describe("ringProgress", () => {
  it("null 表示不画比例", () => {
    expect(ringProgress(null)).toBe(0);
  });

  it("夹在 0..100 之间", () => {
    expect(ringProgress(-5)).toBe(0);
    expect(ringProgress(42)).toBe(42);
    expect(ringProgress(140)).toBe(100);
  });
});

describe("secondaryLine", () => {
  it("有当前读数时按充电状态给文案", () => {
    expect(secondaryLine(device({ isCharging: true }))).toBe("充电中");
    expect(secondaryLine(device({ isCharging: false }))).toBe("使用电池");
    expect(secondaryLine(device({ isCharging: null }))).toBe("充电状态未知");
  });

  it("无当前读数但存在上次已知时只说「上次读数」，数值已在主位", () => {
    expect(secondaryLine(device({ battery: null, lastKnownBattery: 42 }))).toBe("上次读数");
  });

  it("从未有过读数时区分 unknown 与其它状态", () => {
    expect(secondaryLine(device({ battery: null, lastKnownBattery: null, state: "unknown" }))).toBe(
      "尚未收到读数",
    );
    expect(secondaryLine(device({ battery: null, lastKnownBattery: null, state: "offline" }))).toBe(
      "无可用读数",
    );
  });
});

describe("displayedBattery", () => {
  it("启动回填（unknown + 缓存值）时用缓存值填主读数", () => {
    expect(displayedBattery(device({ battery: null, state: "unknown", lastKnownBattery: 90 }))).toBe(
      90,
    );
  });

  it("实时读数优先于缓存", () => {
    expect(displayedBattery(device({ battery: 42, state: "online", lastKnownBattery: 90 }))).toBe(
      42,
    );
  });

  it("确定读不到的退化状态不借用缓存", () => {
    expect(displayedBattery(device({ battery: null, state: "offline", lastKnownBattery: 90 }))).toBe(
      null,
    );
    expect(
      displayedBattery(device({ battery: null, state: "unavailable", lastKnownBattery: 90 })),
    ).toBe(null);
  });

  it("既无读数也无缓存时返回 null", () => {
    expect(
      displayedBattery(device({ battery: null, state: "unknown", lastKnownBattery: null })),
    ).toBe(null);
  });
});

describe("transportLabel / batteryText", () => {
  it("链路名称与规格一致", () => {
    expect(transportLabel("hid")).toBe("2.4G");
    expect(transportLabel("ble")).toBe("蓝牙");
  });

  it("battery 为 null 时显示破折号，绝不用 0 代替", () => {
    expect(batteryText(device({ battery: null }))).toBe("—");
    expect(batteryText(device({ battery: 0 }))).toBe("0%");
  });
});
