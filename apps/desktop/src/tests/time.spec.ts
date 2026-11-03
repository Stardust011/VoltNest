import { describe, expect, it } from "vitest";
import { anchorOffset, ageSince, humanizeAgo, serverNow } from "../lib/time";

describe("humanizeAgo", () => {
  it("把不足一分钟的间隔显示为「刚刚」", () => {
    expect(humanizeAgo(0)).toBe("刚刚");
    expect(humanizeAgo(59_999)).toBe("刚刚");
  });

  it("在分/时/天的边界上分档", () => {
    expect(humanizeAgo(60_000)).toBe("1 分钟前");
    expect(humanizeAgo(3_599_999)).toBe("59 分钟前");
    expect(humanizeAgo(3_600_000)).toBe("1 小时前");
    expect(humanizeAgo(86_399_999)).toBe("23 小时前");
    expect(humanizeAgo(86_400_000)).toBe("1 天前");
  });

  it("负值与非法值退化为「刚刚」而不是抛出", () => {
    expect(humanizeAgo(-1)).toBe("刚刚");
    expect(humanizeAgo(Number.NaN)).toBe("刚刚");
    expect(humanizeAgo(Number.POSITIVE_INFINITY)).toBe("刚刚");
  });
});

describe("anchorOffset / serverNow", () => {
  it("以快照为锚点，之后只用单调计时器推算", () => {
    const offset = anchorOffset(1_792_631_234_567, 1_000);
    expect(serverNow(offset, 1_000)).toBe(1_792_631_234_567);
    expect(serverNow(offset, 4_000)).toBe(1_792_631_237_567);
  });

  it("本地时钟跳变不影响推算结果", () => {
    const offset = anchorOffset(500_000, 2_000);
    expect(serverNow(offset, 3_000) - serverNow(offset, 2_000)).toBe(1_000);
  });

  it("重新对齐后立刻回到宿主时间轴", () => {
    const stale = anchorOffset(1_000_000, 0);
    expect(serverNow(stale, 1_000)).toBe(1_001_000);
    const realigned = anchorOffset(1_000_500, 1_000);
    expect(serverNow(realigned, 1_000)).toBe(1_000_500);
    expect(serverNow(realigned, 2_000)).toBe(1_001_500);
  });
});

describe("ageSince", () => {
  it("无读数时返回 null", () => {
    expect(ageSince(null, 1_000)).toBeNull();
  });

  it("时钟后跳时夹到 0，不返回负数", () => {
    expect(ageSince(2_000, 1_000)).toBe(0);
  });

  it("正常情况返回差值", () => {
    expect(ageSince(1_000, 61_000)).toBe(60_000);
  });
});
