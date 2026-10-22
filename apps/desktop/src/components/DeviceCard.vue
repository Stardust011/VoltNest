<script setup lang="ts">
import { computed } from "vue";
import BatteryRing from "./BatteryRing.vue";
import { secondaryLine, stateLabel, toneFor, transportLabel } from "../lib/format";
import { humanizeAgo } from "../lib/time";
import type { DeviceDto } from "../lib/types";

const props = defineProps<{
  device: DeviceDto;
  nowMs: number;
  /** 合并卡的非活跃成员说明行；有值时替代普通副行。 */
  inactiveText?: string;
}>();

const tone = computed(() => toneFor(props.device.state));

/**
 * 副行：
 * 1. 合并卡 → 非活跃链路说明（W-2，由 Rust 生成）；
 * 2. 无当前读数但有上次已知 → 「上次 X% · 时间」（§11 回填）；
 * 3. 其余按充电状态。
 */
const secondary = computed(() => {
  if (props.inactiveText) return props.inactiveText;
  if (props.device.battery === null && props.device.lastKnownBattery !== null) {
    const at = props.device.lastKnownAtMs;
    const when = at === null ? "" : humanizeAgo(Math.max(0, props.nowMs - at));
    return when ? `上次 ${props.device.lastKnownBattery}% · ${when}` : `上次 ${props.device.lastKnownBattery}%`;
  }
  return secondaryLine(props.device);
});
</script>

<template>
  <article
    class="rounded-2xl border border-black/5 bg-white/90 p-4 shadow-[0_1px_2px_rgba(16,24,40,0.04)] backdrop-blur transition hover:shadow-[0_4px_16px_rgba(16,24,40,0.08)] dark:border-white/10 dark:bg-white/[0.04]"
  >
    <div class="flex items-center gap-4">
      <BatteryRing
        :battery="device.battery"
        :charging="device.isCharging === true"
        :state="device.state"
        :size="60"
      />
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <h2 class="truncate text-[15px] font-semibold">{{ device.displayName }}</h2>
          <span class="shrink-0 rounded-md bg-black/[0.05] px-1.5 py-0.5 text-[11px] font-medium text-neutral-500 dark:bg-white/10 dark:text-neutral-400">
            {{ transportLabel(device.transport) }}
          </span>
        </div>
        <p class="mt-1 truncate text-[12.5px] text-neutral-500 dark:text-neutral-400">{{ secondary }}</p>
      </div>
      <div class="flex flex-col items-end gap-1.5">
        <span class="flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium" :class="tone.chip">
          <span class="h-1.5 w-1.5 rounded-full" :class="tone.dot" />
          {{ stateLabel(device.state) }}
        </span>
        <span
          v-if="device.lastError"
          class="selectable max-w-[7rem] truncate font-mono text-[10px] text-orange-600 dark:text-orange-400"
          :title="device.lastError"
        >{{ device.lastError }}</span>
      </div>
    </div>
  </article>
</template>
