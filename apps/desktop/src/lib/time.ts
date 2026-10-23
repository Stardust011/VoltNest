// 时间显示（W-4）。以快照 `serverNowMs` 为锚点、用本地单调计时器 `performance.now()` 推算时间，收到新快照即重新对齐。
// 禁止 `Date.now() - lastPushAt`：系统时钟跳变会让界面误显「3 小时前」，而那正是判断过期的依据。

// 相对时间文案，分档与 Rust 侧 `humanize_ago` 一致。
export function humanizeAgo(elapsedMs: number): string {
  if (!Number.isFinite(elapsedMs) || elapsedMs < 0) return "刚刚";
  if (elapsedMs < 60_000) return "刚刚";
  if (elapsedMs < 3_600_000) return `${Math.floor(elapsedMs / 60_000)} 分钟前`;
  if (elapsedMs < 86_400_000) return `${Math.floor(elapsedMs / 3_600_000)} 小时前`;
  return `${Math.floor(elapsedMs / 86_400_000)} 天前`;
}

// 建立「本地时钟 → 宿主时钟」锚点。
export function anchorOffset(serverNowMs: number, localMonotonicMs: number): number {
  return serverNowMs - localMonotonicMs;
}

// 由锚点推算当前宿主时间。
export function serverNow(offset: number, localMonotonicMs: number): number {
  return offset + localMonotonicMs;
}

// 设备「多久前有过有效读数」；无读数返回 `null`（界面显「—」）。
export function ageSince(lastValidAtMs: number | null, now: number): number | null {
  if (lastValidAtMs === null) return null;
  return Math.max(0, now - lastValidAtMs);
}
