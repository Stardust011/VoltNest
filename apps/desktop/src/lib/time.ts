export function humanizeAgo(elapsedMs: number): string {
  if (!Number.isFinite(elapsedMs) || elapsedMs < 0) return "刚刚";
  if (elapsedMs < 60_000) return "刚刚";
  if (elapsedMs < 3_600_000) return `${Math.floor(elapsedMs / 60_000)} 分钟前`;
  if (elapsedMs < 86_400_000) return `${Math.floor(elapsedMs / 3_600_000)} 小时前`;
  return `${Math.floor(elapsedMs / 86_400_000)} 天前`;
}

export function anchorOffset(serverNowMs: number, localMonotonicMs: number): number {
  return serverNowMs - localMonotonicMs;
}

export function serverNow(offset: number, localMonotonicMs: number): number {
  return offset + localMonotonicMs;
}

export function ageSince(lastValidAtMs: number | null, now: number): number | null {
  if (lastValidAtMs === null) return null;
  return Math.max(0, now - lastValidAtMs);
}
