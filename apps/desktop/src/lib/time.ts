/**
 * 时间显示（W-4）。
 *
 * 快照携带 `serverNowMs`：前端以它为锚点、用**本地单调计时器**
 * （`performance.now()`）推算当前时间，收到新快照时重新对齐。
 *
 * 禁止写成 `Date.now() - lastPushAt`：系统时钟跳变会让界面显示「3 小时前」，
 * 而那正是我们用来判断设备是否过期的依据。
 */

/** 相对时间文案；与 Rust 侧 `humanize_ago` 保持一致的分档。 */
export function humanizeAgo(elapsedMs: number): string {
  if (!Number.isFinite(elapsedMs) || elapsedMs < 0) return "刚刚";
  if (elapsedMs < 60_000) return "刚刚";
  if (elapsedMs < 3_600_000) return `${Math.floor(elapsedMs / 60_000)} 分钟前`;
  if (elapsedMs < 86_400_000) return `${Math.floor(elapsedMs / 3_600_000)} 小时前`;
  return `${Math.floor(elapsedMs / 86_400_000)} 天前`;
}

/**
 * 建立「本地时钟 → 宿主时钟」的锚点。
 *
 * @param serverNowMs 快照里的宿主 UTC 毫秒
 * @param localMonotonicMs 收到快照时的 `performance.now()`
 */
export function anchorOffset(serverNowMs: number, localMonotonicMs: number): number {
  return serverNowMs - localMonotonicMs;
}

/** 由锚点推算当前宿主时间。 */
export function serverNow(offset: number, localMonotonicMs: number): number {
  return offset + localMonotonicMs;
}

/** 设备「多久前有过有效读数」；无读数返回 `null`（界面显示「—」）。 */
export function ageSince(lastValidAtMs: number | null, now: number): number | null {
  if (lastValidAtMs === null) return null;
  return Math.max(0, now - lastValidAtMs);
}
