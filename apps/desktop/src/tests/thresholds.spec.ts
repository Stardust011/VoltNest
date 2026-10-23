import { describe, expect, it } from "vitest";
import {
  applyThresholdEdit,
  clampThreshold,
  parseThresholdInput,
  validateThresholdPair,
  validateThresholdValue,
  THRESHOLD_MAX,
  THRESHOLD_MIN,
  ORDER_MESSAGE,
  RANGE_MESSAGE,
  type Thresholds,
} from "../lib/thresholds";

/** 两档都不报错。 */
function isValidPair(raws: [string, string]): boolean {
  const [first, second] = validateThresholdPair(raws);
  return first === null && second === null;
}

/** 正式值必须始终满足业务约束：5～95 且第一档比第二档至少高 1%。 */
function expectInvariant(pair: Thresholds) {
  for (const value of pair) {
    expect(Number.isInteger(value)).toBe(true);
    expect(value).toBeGreaterThanOrEqual(THRESHOLD_MIN);
    expect(value).toBeLessThanOrEqual(THRESHOLD_MAX);
  }
  expect(pair[0] - pair[1]).toBeGreaterThanOrEqual(1);
}

describe("parseThresholdInput", () => {
  it("接受整数与前后空白", () => {
    expect(parseThresholdInput("20")).toBe(20);
    expect(parseThresholdInput(" 20 ")).toBe(20);
    expect(parseThresholdInput("+7")).toBe(7);
    expect(parseThresholdInput("-3")).toBe(-3);
  });

  it("空串、小数、非数字一律视为未完成输入", () => {
    for (const raw of ["", "   ", "12.5", "1e2", "abc", "5%", "０"]) {
      expect(parseThresholdInput(raw)).toBeNull();
    }
  });
});

describe("clampThreshold", () => {
  it("夹进 0～95", () => {
    expect(clampThreshold(-1)).toBe(THRESHOLD_MIN);
    expect(clampThreshold(96)).toBe(THRESHOLD_MAX);
    expect(clampThreshold(0)).toBe(0);
    expect(clampThreshold(20)).toBe(20);
  });
});

describe("单档校验", () => {
  it("初始合法值不报错", () => {
    expect(validateThresholdValue("20")).toBeNull();
    expect(validateThresholdValue("10")).toBeNull();
  });

  it("空字符串立即非法", () => {
    expect(validateThresholdValue("")).toBe(RANGE_MESSAGE);
  });

  it("非数字与小数非法", () => {
    expect(validateThresholdValue("abc")).toBe(RANGE_MESSAGE);
    expect(validateThresholdValue("12.5")).toBe(RANGE_MESSAGE);
  });

  it("越界数值非法（-1 / 96）", () => {
    expect(validateThresholdValue("-1")).toBe(RANGE_MESSAGE);
    expect(validateThresholdValue("96")).toBe(RANGE_MESSAGE);
    expect(validateThresholdValue("-40")).toBe(RANGE_MESSAGE);
  });

  it("边界值 0 / 95 单档合法", () => {
    expect(validateThresholdValue("0")).toBeNull();
    expect(validateThresholdValue("95")).toBeNull();
  });
});

describe("两档联动校验", () => {
  it("合法组合无错误", () => {
    expect(isValidPair(["20", "10"])).toBe(true);
    expect(isValidPair(["6", "5"])).toBe(true);
    expect(isValidPair(["1", "0"])).toBe(true);
    expect(isValidPair(["95", "94"])).toBe(true);
    expect(isValidPair(["95", "0"])).toBe(true);
  });

  it("两档相等时两档一起报顺序错误", () => {
    expect(validateThresholdPair(["20", "20"])).toEqual([ORDER_MESSAGE, ORDER_MESSAGE]);
  });

  it("间距不足 1%（第一档不高于第二档）报顺序错误", () => {
    expect(validateThresholdPair(["10", "20"])).toEqual([ORDER_MESSAGE, ORDER_MESSAGE]);
    expect(validateThresholdPair(["0", "95"])).toEqual([ORDER_MESSAGE, ORDER_MESSAGE]);
  });

  it("单档本身非法时只报该档，不掺顺序错误", () => {
    expect(validateThresholdPair(["", "10"])).toEqual([RANGE_MESSAGE, null]);
    expect(validateThresholdPair(["20", ""])).toEqual([null, RANGE_MESSAGE]);
    expect(validateThresholdPair(["96", "95"])).toEqual([RANGE_MESSAGE, null]);
  });

  it("非法修正为合法后红色状态消失", () => {
    const invalid: [string, string] = ["20", "20"];
    expect(isValidPair(invalid)).toBe(false);
    const fixed = applyThresholdEdit([20, 20], 1, "19");
    expect(fixed).not.toBeNull();
    expect(isValidPair([String(fixed?.[0]), String(fixed?.[1])])).toBe(true);
  });
});

describe("applyThresholdEdit", () => {
  it("常规编辑直接生效", () => {
    expect(applyThresholdEdit([20, 10], 0, "30")).toEqual([30, 10]);
    expect(applyThresholdEdit([20, 10], 1, "8")).toEqual([20, 8]);
  });

  it("夹紧越界输入，边界值落到 0 / 95", () => {
    expect(applyThresholdEdit([20, 10], 0, "120")).toEqual([95, 10]);
    expect(applyThresholdEdit([20, 10], 1, "-1")).toEqual([20, 0]);
    expect(applyThresholdEdit([20, 10], 1, "0")).toEqual([20, 0]);
  });

  it("非整数输入返回 null，不产生正式值", () => {
    expect(applyThresholdEdit([20, 10], 0, "")).toBeNull();
    expect(applyThresholdEdit([20, 10], 0, "12.5")).toBeNull();
    expect(applyThresholdEdit([20, 10], 0, "abc")).toBeNull();
    expect(applyThresholdEdit([20, 10], 0, 12.5)).toBeNull();
  });

  it("保持原有修正顺序：编辑哪一档，哪一档让位", () => {
    // 第一档被抬到第二档之上。
    expect(applyThresholdEdit([20, 10], 0, "8")).toEqual([11, 10]);
    // 第二档被压到第一档之下。
    expect(applyThresholdEdit([20, 10], 1, "30")).toEqual([20, 19]);
  });

  it("修正撞到上界时移动另一档，不产出 96", () => {
    // 旧实现：next[0] = 95 + 1 = 96。
    const next = applyThresholdEdit([90, 95], 0, "90");
    expect(next).toEqual([95, 94]);
    expectInvariant(next as Thresholds);
    expect(applyThresholdEdit([10, 95], 0, "95")).toEqual([95, 94]);
  });

  it("修正撞到下界时移动另一档，不产出负数", () => {
    // 旧实现：next[1] = 0 - 1 越界。
    const next = applyThresholdEdit([0, 1], 1, "0");
    expect(next).toEqual([1, 0]);
    expectInvariant(next as Thresholds);
    // 常规下界修正：第二档让位到第一档之下。
    expect(applyThresholdEdit([5, 10], 1, "6")).toEqual([5, 4]);
  });

  it("不修改传入的正式值", () => {
    const current: Thresholds = [20, 10];
    applyThresholdEdit(current, 0, "60");
    expect(current).toEqual([20, 10]);
  });

  it("任何合法输入都产出合法两档", () => {
    // 穷举所有起始组合 × 代表性输入（含越界与边界），收集违例后一次断言，避免上百万次 expect。
    const values = [-1, 0, 4, 5, 6, 20, 50, 94, 95, 96, 100, 1000];
    const violations: string[] = [];
    for (let first = THRESHOLD_MIN; first <= THRESHOLD_MAX; first += 1) {
      for (let second = THRESHOLD_MIN; second <= THRESHOLD_MAX; second += 1) {
        for (const index of [0, 1] as const) {
          for (const value of values) {
            const next = applyThresholdEdit([first, second], index, value);
            const bad =
              next === null ||
              next.some((v) => !Number.isInteger(v) || v < THRESHOLD_MIN || v > THRESHOLD_MAX) ||
              next[0] - next[1] < 1;
            if (bad) violations.push(`${first},${second} #${index} <= ${value} → ${String(next)}`);
          }
        }
      }
    }
    expect(violations).toEqual([]);
  });
});
