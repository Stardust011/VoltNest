<script setup lang="ts">
import { humanizeAgo } from "../lib/time";
import type { DiagnosticsDto } from "../lib/types";

const props = defineProps<{ diagnostics: DiagnosticsDto; nowMs: number }>();
const emit = defineEmits<{ (e: "restart", transport: "hid" | "ble"): void }>();

const levelClass: Record<string, string> = {
  error: "text-red-600 dark:text-red-400",
  warn: "text-amber-600 dark:text-amber-400",
  info: "text-neutral-500 dark:text-neutral-400",
};

const statusLabel: Record<string, string> = {
  running: "运行中",
  down: "已停止",
  unavailable: "采集不可用",
};

function ago(atMs: number) {
  return humanizeAgo(Math.max(0, props.nowMs - atMs));
}

function transportName(t: string) {
  return t === "hid" ? "2.4G" : "蓝牙";
}
</script>

<template>
  <div class="flex flex-col gap-4 pb-4">
    <section class="rounded-2xl border border-black/5 bg-white/90 p-4 dark:border-white/10 dark:bg-white/[0.04]">
      <h3 class="text-[13px] font-semibold">采集线程组</h3>
      <ul class="mt-3 flex flex-col gap-2">
        <li
          v-for="t in diagnostics.transports"
          :key="t.transport"
          class="rounded-xl border border-black/5 px-3 py-2 dark:border-white/10"
        >
          <div class="flex items-center justify-between gap-2">
            <div class="min-w-0">
              <p class="text-[12.5px] font-medium">
                {{ transportName(t.transport) }}
                <span class="ml-1 font-mono text-[10.5px] text-neutral-400">{{ t.family }}</span>
              </p>
              <p class="mt-0.5 text-[11px] text-neutral-400">
                {{ statusLabel[t.status] ?? t.status }} · 重启 {{ t.restarts }} 次 · 队列 {{ t.queuedEvents }}
              </p>
            </div>
            <button
              type="button"
              class="shrink-0 rounded-lg border border-black/10 px-2 py-1 text-[11.5px] transition hover:bg-black/[0.04] dark:border-white/15 dark:hover:bg-white/[0.06]"
              @click="emit('restart', t.transport)"
            >
              重启
            </button>
          </div>
          <p v-if="t.lastError" class="selectable mt-1 break-all font-mono text-[10.5px] text-orange-600 dark:text-orange-400">
            {{ t.lastError }}
          </p>
        </li>
      </ul>
    </section>

    <section class="rounded-2xl border border-black/5 bg-white/90 p-4 dark:border-white/10 dark:bg-white/[0.04]">
      <h3 class="text-[13px] font-semibold">通知状态</h3>
      <p class="mt-1 text-[11px] text-neutral-400">
        用来二分「系统吞了通知」还是「我们没发」：`armed` 为否说明这一档还没恢复武装。
      </p>
      <div v-if="diagnostics.notifications.length === 0" class="mt-2 text-[12px] text-neutral-400">
        暂无记录
      </div>
      <ul v-else class="mt-2 flex flex-col gap-1.5">
        <li
          v-for="n in diagnostics.notifications"
          :key="`${n.stableId}-${n.threshold}`"
          class="flex items-center justify-between gap-2 rounded-lg bg-black/[0.03] px-2 py-1.5 text-[11.5px] dark:bg-white/[0.06]"
        >
          <span class="min-w-0 truncate font-mono">{{ n.stableId }}</span>
          <span class="shrink-0 text-neutral-500 dark:text-neutral-400">
            {{ n.threshold }}% · {{ n.armed ? "已武装" : "已触发" }}
            <template v-if="n.lastNotifiedAtMs"> · {{ ago(n.lastNotifiedAtMs) }}</template>
            <template v-if="n.lastResult"> · {{ n.lastResult }}</template>
          </span>
        </li>
      </ul>
    </section>

    <section class="rounded-2xl border border-black/5 bg-white/90 p-4 dark:border-white/10 dark:bg-white/[0.04]">
      <h3 class="text-[13px] font-semibold">运行环境</h3>
      <dl class="mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[11.5px]">
        <dt class="text-neutral-400">版本</dt>
        <dd class="selectable font-mono">{{ diagnostics.version }}</dd>
        <dt class="text-neutral-400">数据目录</dt>
        <dd class="selectable break-all font-mono">{{ diagnostics.dataRoot }}</dd>
        <dt class="text-neutral-400">开机自启</dt>
        <dd>{{ diagnostics.autostartRegistered ? "已注册" : "未注册" }}</dd>
        <dt class="text-neutral-400">AUMID</dt>
        <dd>{{ diagnostics.aumidRegistered ? "已写入 HKCU" : "未写入" }}</dd>
      </dl>
    </section>

    <section class="rounded-2xl border border-black/5 bg-white/90 p-4 dark:border-white/10 dark:bg-white/[0.04]">
      <h3 class="text-[13px] font-semibold">诊断事件</h3>
      <div v-if="diagnostics.events.length === 0" class="mt-2 text-[12px] text-neutral-400">暂无事件</div>
      <ul v-else class="mt-2 flex flex-col gap-1">
        <li
          v-for="(event, index) in diagnostics.events"
          :key="`${event.atMs}-${index}`"
          class="flex items-start gap-2 text-[11.5px]"
        >
          <span class="w-14 shrink-0 tabular-nums text-neutral-400">{{ ago(event.atMs) }}</span>
          <span class="w-[9.5rem] shrink-0 break-all font-mono" :class="levelClass[event.level]">{{ event.code }}</span>
          <span class="selectable min-w-0 break-all text-neutral-600 dark:text-neutral-300">
            {{ event.deviceId ? `${event.deviceId} · ` : "" }}{{ event.detail }}
          </span>
        </li>
      </ul>
    </section>
  </div>
</template>
