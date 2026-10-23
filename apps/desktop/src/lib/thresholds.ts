// 低电量两档阈值（`notifications.lowThresholds`）纯逻辑：解析、校验、自动修正。
// 单独成模块因设置页需临时字符串态，且项目只跑 node 纯函数测试（不引 jsdom），校验须脱离组件方可覆盖。
// 语义与 Rust 一致：`[0]` 高电量先提醒、`[1]` 更低时再提醒，均 0~95% 且第一档至少高第二档 1%。

// 单档下限（含）。0 由 `dto::clamp_thresholds` 同步放开为合法值。
export const THRESHOLD_MIN = 0;
// 单档上限（含）。
export const THRESHOLD_MAX = 95;
// 两档最小间距。
export const THRESHOLD_MIN_GAP = 1;

// 单档错误文案：空/非整数/越界共用一句，避免用户猜。
export const RANGE_MESSAGE = "请输入 0～95 的整数";
// 两档相等：分不清改哪档，两句共用。
export const ORDER_MESSAGE = "两档至少相差 1%";
// 第二档高于第一档：直接说反了，比「至少相差 1%」更易照改。
export const SECOND_ABOVE_MESSAGE = "第二档不能高于第一档";
// 第一档低于第二档：与上句同一冲突的两侧。
export const FIRST_BELOW_MESSAGE = "第一档不能低于第二档";

// 档位下标：0=第一档，1=第二档。
export type ThresholdIndex = 0 | 1;

// 两档阈值。
export type Thresholds = [number, number];

// 解析输入框原始串为整数：仅接受十进制整数（允许前后空白与正负号），空串/小数/`1e2`/`abc` 一律返回 `null`——既非「未输完」也不可静默取整。
export function parseThresholdInput(raw: string): number | null {
  const text = raw.trim();
  if (!/^[+-]?\d+$/.test(text)) return null;
  const value = Number.parseInt(text, 10);
  return Number.isNaN(value) ? null : value;
}

// 把数值夹进合法区间（正式值兜底）。
export function clampThreshold(value: number): number {
  return Math.min(THRESHOLD_MAX, Math.max(THRESHOLD_MIN, value));
}

// 单档校验：返回错误文案，合法返回 `null`。空即非法——设置页展示当前生效值，空着无法表达有效阈值。
export function validateThresholdValue(raw: string): string | null {
  const value = parseThresholdInput(raw);
  if (value === null || value < THRESHOLD_MIN || value > THRESHOLD_MAX) {
    return RANGE_MESSAGE;
  }
  return null;
}

// 两档联动校验：先逐档校验，合法后再查顺序。顺序不成立则两档同标红（不替用户选边）；方向反说「第二档不能高于第一档」，相等才说「至少相差 1%」——后者要用户自算差值。
export function validateThresholdPair(
  raws: readonly [string, string],
): [string | null, string | null] {
  const first = parseThresholdInput(raws[0]);
  const second = parseThresholdInput(raws[1]);
  const errors: [string | null, string | null] = [
    validateThresholdValue(raws[0]),
    validateThresholdValue(raws[1]),
  ];
  if (errors[0] !== null || errors[1] !== null) return errors;
  // 到此两档已确认为 0~95 整数，可断言非空。
  const gap = (first as number) - (second as number);
  if (gap >= THRESHOLD_MIN_GAP) return errors;
  if (gap >= 0) {
    // 差距为正但不足 `THRESHOLD_MIN_GAP`：两档太近（即相等），两侧同句。
    errors[0] = ORDER_MESSAGE;
    errors[1] = ORDER_MESSAGE;
  } else {
    // 差距为负：第二档更高，方向反了。
    errors[0] = FIRST_BELOW_MESSAGE;
    errors[1] = SECOND_ABOVE_MESSAGE;
  }
  return errors;
}

// 将某档新输入落到两档正式值，越界/顺序冲突时兜底修正：编辑第一档则抬到第二档之上，编辑第二档则压到其下。
// 与原实现唯一差别：修正撞 0/95 边界时改移另一档而非推出区间——旧实现先夹紧再修正，遇 95/下限会算出 96/4 写进非法值。
// 设置页失焦不再自动修正，非法输入直接不提交；此处仅作写入口兜底（盘上本就非法的组合）。
// 输入非整数返回 `null`（调用方放弃提交，正式值不变）；返回新数组，不改 `current`。
export function applyThresholdEdit(
  current: Thresholds,
  index: ThresholdIndex,
  raw: string | number,
): Thresholds | null {
  const parsed =
    typeof raw === "number" ? (Number.isInteger(raw) ? raw : null) : parseThresholdInput(raw);
  if (parsed === null) return null;

  const next: Thresholds = [current[0], current[1]];
  next[index] = clampThreshold(parsed);
  if (next[0] - next[1] >= THRESHOLD_MIN_GAP) return next;

  if (index === 0) {
    // 第一档顶到 95 还压不住第二档，只能下移第二档。
    next[0] = Math.min(THRESHOLD_MAX, next[1] + THRESHOLD_MIN_GAP);
    if (next[0] - next[1] < THRESHOLD_MIN_GAP) {
      next[1] = next[0] - THRESHOLD_MIN_GAP;
    }
  } else {
    // 第二档退到 0 还让不开第一档，只能上移第一档。
    next[1] = Math.max(THRESHOLD_MIN, next[0] - THRESHOLD_MIN_GAP);
    if (next[0] - next[1] < THRESHOLD_MIN_GAP) {
      next[0] = next[1] + THRESHOLD_MIN_GAP;
    }
  }
  return next;
}
