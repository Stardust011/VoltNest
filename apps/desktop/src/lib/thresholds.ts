/**
 * 低电量两档阈值（`notifications.lowThresholds`）的纯逻辑：解析、校验、自动修正。
 *
 * 为什么单独成模块：设置页的输入框需要「临时字符串状态」，
 * 而项目只跑 node 环境的纯函数测试（规格 §14.1，不引入 jsdom），
 * 所以校验与修正顺序必须离开组件才能被覆盖。组件只负责把
 * `thresholdInputs` 画出来，业务规则全部在这里。
 *
 * 语义（与 Rust 侧一致）：`thresholds[0]` 是第一档（电量较高时先提醒），
 * `thresholds[1]` 是第二档（电量更低时再提醒），两档均在 0%～95%，
 * 且第一档必须比第二档至少高 1%。
 */

/** 单档下限（含）。0 是用户要求的合法下限，由 `dto::clamp_thresholds` 同步放开。 */
export const THRESHOLD_MIN = 0;
/** 单档上限（含）。 */
export const THRESHOLD_MAX = 95;
/** 两档之间的最小间距。 */
export const THRESHOLD_MIN_GAP = 1;

/** 单档错误文案：空、非整数、越界共用一句，避免用户猜。 */
export const RANGE_MESSAGE = "请输入 0～95 的整数";
/** 两档联动错误文案。 */
export const ORDER_MESSAGE = "两档至少相差 1%";

/** 档位下标：0 = 第一档，1 = 第二档。 */
export type ThresholdIndex = 0 | 1;

/** 两档阈值。 */
export type Thresholds = [number, number];

/**
 * 把输入框里的原始字符串解析成整数。
 *
 * 只接受十进制整数（允许前后空白与正负号）：空串、小数、`1e2`、`abc`
 * 一律返回 `null` —— 这些既不是「用户还没输完」，也不能被静默取整成别的值。
 */
export function parseThresholdInput(raw: string): number | null {
  const text = raw.trim();
  if (!/^[+-]?\d+$/.test(text)) return null;
  const value = Number.parseInt(text, 10);
  return Number.isNaN(value) ? null : value;
}

/** 把数值夹进合法区间（用于正式值兜底）。 */
export function clampThreshold(value: number): number {
  return Math.min(THRESHOLD_MAX, Math.max(THRESHOLD_MIN, value));
}

/**
 * 单档校验：返回错误文案，合法返回 `null`。
 *
 * 空 → 非法（不是「暂时没填」），因为设置页展示的就是当前生效值，
 * 空着无法表达任何有效阈值。
 */
export function validateThresholdValue(raw: string): string | null {
  const value = parseThresholdInput(raw);
  if (value === null || value < THRESHOLD_MIN || value > THRESHOLD_MAX) {
    return RANGE_MESSAGE;
  }
  return null;
}

/**
 * 两档联动校验：先逐档校验，两档都合法后再检查顺序。
 *
 * 顺序不满足时**两档一起**标红：这不是某一档单方面写错，
 * 而是这一对值不成立（谁高谁低由用户决定，界面不该替它选边）。
 */
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
  // 到这里两档都已确认是 0～95 的整数，可以断言非空。
  if ((first as number) - (second as number) < THRESHOLD_MIN_GAP) {
    errors[0] = ORDER_MESSAGE;
    errors[1] = ORDER_MESSAGE;
  }
  return errors;
}

/**
 * 把「某一档的新输入」落到两档正式值上，并在越界 / 顺序冲突时做兜底修正：
 *
 * - 编辑第一档：第一档让位，被抬到第二档之上（原实现 `next[0] = next[1] + 1`）；
 * - 编辑第二档：第二档让位，被压到第一档之下（原实现 `next[1] = next[0] - 1`）。
 *
 * 与原实现的**唯一**差别：当修正值撞到 0 / 95 边界时改为移动**另一档**，
 * 而不是把结果推出区间。旧实现先夹紧再修正，遇到第二档已是 95
 * （或第一档已是下限）时会算出 96 / 4，把非法值直接写进设置。
 *
 * 注意：设置页现在**不会**在失焦时自动修正用户输入 —— 非法输入直接不提交。
 * 这里的修正只作为写入口的兜底（例如盘上本来就是非法组合）。
 *
 * 输入不是整数时返回 `null`：调用方据此放弃这次提交，正式值保持不变。
 * 返回新数组，不修改传入的 `current`。
 */
export function applyThresholdEdit(
  current: Thresholds,
  index: ThresholdIndex,
  raw: string | number,
): Thresholds | null {
  const parsed =
    typeof raw === "number"
      ? Number.isInteger(raw)
        ? raw
        : null
      : parseThresholdInput(raw);
  if (parsed === null) return null;

  const next: Thresholds = [current[0], current[1]];
  next[index] = clampThreshold(parsed);
  if (next[0] - next[1] >= THRESHOLD_MIN_GAP) return next;

  if (index === 0) {
    // 第一档顶到 95 还压不住第二档时，只能把第二档往下挪。
    next[0] = Math.min(THRESHOLD_MAX, next[1] + THRESHOLD_MIN_GAP);
    if (next[0] - next[1] < THRESHOLD_MIN_GAP) {
      next[1] = next[0] - THRESHOLD_MIN_GAP;
    }
  } else {
    // 第二档退到 0 还让不开第一档时，只能把第一档往上挪。
    next[1] = Math.max(THRESHOLD_MIN, next[0] - THRESHOLD_MIN_GAP);
    if (next[0] - next[1] < THRESHOLD_MIN_GAP) {
      next[0] = next[1] + THRESHOLD_MIN_GAP;
    }
  }
  return next;
}
