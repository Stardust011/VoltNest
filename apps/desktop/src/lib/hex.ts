// 生成上报数据时的报文文本处理：把用户手填的十六进制变成「宿主能直接解析的规范文本」。
//
// 校验规则与 Rust 侧 `voltnest_hid::hex::parse` 逐条对齐（分隔符、0x 前缀、奇偶性、长度上限），
// 但这里只负责「即时反馈」——最终仍由宿主解析一次，界面永远不是可信边界。

/** 单条报文的字节上限（与 `voltnest_hid::hex::MAX_FRAME_BYTES` 一致）。 */
export const MAX_FRAME_BYTES = 1024;

/** 允许出现在字节之间的分隔符。 */
const SEPARATORS = /[\s,;:_-]/;

export interface ParsedHexFrames {
  /** 规范化后的报文（大写、空格分隔），可直接回传宿主。 */
  frames: string[];
  /** 第一处错误（含行号）；无错误为 `null`。 */
  error: string | null;
}

/**
 * 逐行解析用户输入：空行忽略，每行一条报文。
 *
 * 返回第一处错误而非「尽量解析」：发出去一条被截断的报文，比报错更难排查
 * （设备会安静地不理你）。
 */
export function parseHexFrames(text: string): ParsedHexFrames {
  const frames: string[] = [];
  const lines = text.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]!.trim();
    if (!line) continue;
    const bytes = parseLine(line);
    if (typeof bytes === "string") {
      return { frames: [], error: `第 ${index + 1} 行：${bytes}` };
    }
    frames.push(formatBytes(bytes));
  }
  return { frames, error: null };
}

/** 单行 → 字节数组；失败时返回错误文案。 */
export function parseLine(line: string): number[] | string {
  const chars = Array.from(line);
  const digits: number[] = [];
  for (let index = 0; index < chars.length; index += 1) {
    const char = chars[index]!;
    // `0x` / `0X` 前缀：跳过这两个字符，字节值本身照常收集。
    if (char === "0" && (chars[index + 1] === "x" || chars[index + 1] === "X")) {
      index += 1;
      continue;
    }
    if (SEPARATORS.test(char)) continue;
    if (!/[0-9a-fA-F]/.test(char)) return `含有非十六进制字符：${char}`;
    digits.push(Number.parseInt(char, 16));
  }
  if (digits.length === 0) return "报文为空";
  if (digits.length % 2 !== 0) return "十六进制位数为奇数，无法凑成整字节";
  const bytes: number[] = [];
  for (let index = 0; index < digits.length; index += 2) {
    bytes.push((digits[index]! << 4) | digits[index + 1]!);
  }
  if (bytes.length > MAX_FRAME_BYTES) {
    return `报文过长（${bytes.length} 字节，上限 ${MAX_FRAME_BYTES}）`;
  }
  return bytes;
}

/** 字节数组 → 大写、空格分隔的十六进制（与宿主报告里的写法一致）。 */
export function formatBytes(bytes: readonly number[]): string {
  return bytes.map((byte) => byte.toString(16).toUpperCase().padStart(2, "0")).join(" ");
}

/** 预览用截断：`06 FB 00 …`（`maxBytes` 之后省略）。 */
export function shortHex(hex: string, maxBytes = 8): string {
  const parts = hex.split(" ").filter(Boolean);
  if (parts.length <= maxBytes) return hex;
  return `${parts.slice(0, maxBytes).join(" ")} …`;
}

/** 时长文案：`20 秒` / `1 分 05 秒`。 */
export function formatDurationMs(ms: number): string {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  if (totalSeconds < 60) return `${totalSeconds} 秒`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return seconds === 0 ? `${minutes} 分` : `${minutes} 分 ${String(seconds).padStart(2, "0")} 秒`;
}
