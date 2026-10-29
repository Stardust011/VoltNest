import { describe, expect, it } from "vitest";
import { formatBytes, formatDurationMs, parseHexFrames, parseLine, shortHex } from "../lib/hex";

describe("parseHexFrames", () => {
  it("忽略空行，按行拆分", () => {
    const parsed = parseHexFrames("06 FB 00\n\n  \n0A 01");
    expect(parsed.error).toBeNull();
    expect(parsed.frames).toEqual(["06 FB 00", "0A 01"]);
  });

  it("规范化分隔符与 0x 前缀，输出统一大写", () => {
    expect(parseHexFrames("0x06,0xfb-00").frames).toEqual(["06 FB 00"]);
    expect(parseHexFrames("06:fb_00").frames).toEqual(["06 FB 00"]);
    expect(parseHexFrames("06fb00").frames).toEqual(["06 FB 00"]);
  });

  it("报错时指出行号，且不返回半份结果", () => {
    const parsed = parseHexFrames("06 FB\n06 Z");
    expect(parsed.frames).toEqual([]);
    expect(parsed.error).toContain("第 2 行");
    expect(parsed.error).toContain("Z");
  });

  it("空输入不算错误（纯监听）", () => {
    expect(parseHexFrames("   \n\n")).toEqual({ frames: [], error: null });
  });
});

describe("parseLine", () => {
  it("拒绝半个字节而不是猜成 0x05", () => {
    expect(parseLine("0x5")).toBe("十六进制位数为奇数，无法凑成整字节");
    expect(parseLine("5")).toBe("十六进制位数为奇数，无法凑成整字节");
  });

  it("拒绝空报文与非十六进制字符", () => {
    expect(parseLine("  ")).toBe("报文为空");
    expect(parseLine("06 号")).toContain("非十六进制字符");
  });

  it("拒绝超过上限的报文", () => {
    const result = parseLine("00 ".repeat(1025));
    expect(result).toContain("报文过长");
  });

  it("与 formatBytes 互逆", () => {
    const bytes = [0x00, 0x06, 0xfb, 0xff];
    expect(parseLine(formatBytes(bytes))).toEqual(bytes);
  });
});

describe("展示辅助", () => {
  it("formatBytes 补零并大写", () => {
    expect(formatBytes([0, 6, 251])).toBe("00 06 FB");
    expect(formatBytes([])).toBe("");
  });

  it("shortHex 超长才截断", () => {
    expect(shortHex("06 FB")).toBe("06 FB");
    expect(shortHex("00 01 02 03 04", 3)).toBe("00 01 02 …");
  });

  it("formatDurationMs 在分钟处切换写法", () => {
    expect(formatDurationMs(20_000)).toBe("20 秒");
    expect(formatDurationMs(60_000)).toBe("1 分");
    expect(formatDurationMs(65_000)).toBe("1 分 05 秒");
    expect(formatDurationMs(-1)).toBe("0 秒");
  });
});
