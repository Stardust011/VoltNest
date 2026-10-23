import { onBeforeUnmount, onMounted, ref } from "vue";
import { getSnapshot, subscribeSnapshot } from "../lib/ipc";
import { anchorOffset, serverNow } from "../lib/time";
import type { Snapshot } from "../lib/types";

// 快照订阅（W-6 / W-7）。顺序不可颠倒：先订阅事件→再 get_snapshot()，丢弃 `rev <= snapshot.rev` 的缓冲事件。反之会稳定丢失启动瞬间首变（Tauri 事件在 webview 未就绪时永久丢失）。
export function useSnapshot() {
  const snapshot = ref<Snapshot | null>(null);
  const nowMs = ref(Date.now());
  const connected = ref(false);

  let offset = 0;
  let localRev = -1;
  let ready = false;
  let buffered: Snapshot[] = [];
  let timer: number | undefined;
  let unlisten: (() => void) | undefined;

  function apply(snap: Snapshot) {
    if (snap.rev <= localRev) return;
    localRev = snap.rev;
    snapshot.value = snap;
    // 以快照为锚点对齐本地单调计时器，绝不用 Date.now() 相减（W-4）。
    offset = anchorOffset(snap.serverNowMs, performance.now());
    nowMs.value = serverNow(offset, performance.now());
  }

  async function start() {
    unlisten = await subscribeSnapshot((snap) => {
      if (ready) apply(snap);
      else buffered.push(snap);
    });
    apply(await getSnapshot());
    ready = true;
    // 缓冲事件按 rev 排序后逐个应用，rev 过滤保证幂等。
    buffered.sort((a, b) => a.rev - b.rev).forEach(apply);
    buffered = [];
    connected.value = true;
    timer = window.setInterval(() => {
      nowMs.value = serverNow(offset, performance.now());
    }, 1000);
  }

  async function refresh() {
    apply(await getSnapshot());
  }

  onMounted(() => {
    void start();
  });

  onBeforeUnmount(() => {
    unlisten?.();
    if (timer !== undefined) window.clearInterval(timer);
  });

  return { snapshot, nowMs, connected, refresh };
}
