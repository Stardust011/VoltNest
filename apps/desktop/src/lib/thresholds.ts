export const THRESHOLD_MIN = 0;
// 单档上限（含）。
export const THRESHOLD_MAX = 95;
// 两档最小间距。
export const THRESHOLD_MIN_GAP = 1;

export const RANGE_MESSAGE = "请输入 0～95 的整数";
export const ORDER_MESSAGE = "两档至少相差 1%";
export const SECOND_ABOVE_MESSAGE = "第二档不能高于第一档";
export const FIRST_BELOW_MESSAGE = "第一档不能低于第二档";

// 档位下标：0=第一档，1=第二档。
export type ThresholdIndex = 0 | 1;

// 两档阈值。
export type LowBatteryLevels = [number, number];

export function parseThresholdInput(raw: string): number | null {
  const text = raw.trim();
  if (!/^[+-]?\d+$/.test(text)) return null;
  const value = Number.parseInt(text, 10);
  return Number.isNaN(value) ? null : value;
}

export function clampThreshold(value: number): number {
  return Math.min(THRESHOLD_MAX, Math.max(THRESHOLD_MIN, value));
}

export function validateThresholdValue(raw: string): string | null {
  const value = parseThresholdInput(raw);
  if (value === null || value < THRESHOLD_MIN || value > THRESHOLD_MAX) {
    return RANGE_MESSAGE;
  }
  return null;
}

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
  const gap = (first as number) - (second as number);
  if (gap >= THRESHOLD_MIN_GAP) return errors;
  if (gap >= 0) {
    errors[0] = ORDER_MESSAGE;
    errors[1] = ORDER_MESSAGE;
  } else {
    errors[0] = FIRST_BELOW_MESSAGE;
    errors[1] = SECOND_ABOVE_MESSAGE;
  }
  return errors;
}

export function applyThresholdEdit(
  current: LowBatteryLevels,
  index: ThresholdIndex,
  raw: string | number,
): LowBatteryLevels | null {
  const parsed =
    typeof raw === "number" ? (Number.isInteger(raw) ? raw : null) : parseThresholdInput(raw);
  if (parsed === null) return null;

  const next: LowBatteryLevels = [current[0], current[1]];
  next[index] = clampThreshold(parsed);
  if (next[0] - next[1] >= THRESHOLD_MIN_GAP) return next;

  if (index === 0) {
    next[0] = Math.min(THRESHOLD_MAX, next[1] + THRESHOLD_MIN_GAP);
    if (next[0] - next[1] < THRESHOLD_MIN_GAP) {
      next[1] = next[0] - THRESHOLD_MIN_GAP;
    }
  } else {
    next[1] = Math.max(THRESHOLD_MIN, next[0] - THRESHOLD_MIN_GAP);
    if (next[0] - next[1] < THRESHOLD_MIN_GAP) {
      next[0] = next[1] + THRESHOLD_MIN_GAP;
    }
  }
  return next;
}
