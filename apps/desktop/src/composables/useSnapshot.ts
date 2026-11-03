import { onBeforeUnmount, onMounted, ref } from "vue";
import { getSnapshot, subscribeSnapshot } from "../lib/ipc";
import { anchorOffset, serverNow } from "../lib/time";
import type { Snapshot } from "../lib/types";

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
