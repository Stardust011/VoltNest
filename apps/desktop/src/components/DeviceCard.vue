<script setup lang="ts">
import { computed } from "vue";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import {
  batteryColor,
  batteryText,
  ringProgress,
  secondaryLine,
  stateLabel,
  toneFor,
  transportLabel,
} from "../lib/format";
import { humanizeAgo } from "../lib/time";
import type { DeviceDto } from "../lib/types";

const props = defineProps<{
  device: DeviceDto;
  nowMs: number;
  /** 合并卡的非活跃成员说明行；有值时替代普通副行。 */
  inactiveText?: string;
}>();

const tone = computed(() => toneFor(props.device.state));
const progress = computed(() => ringProgress(props.device.battery));
const color = computed(() => batteryColor(props.device.battery, props.device.isCharging === true, props.device.state));

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
  <Card class="gap-0 py-3.5 transition-shadow hover:shadow-md">
    <CardContent class="flex items-center gap-3.5 px-4">
      <!-- 电量读数：数值 + Progress，颜色沿用 batteryColor 的语义（充电 / 低电 / 状态）。 -->
      <div class="flex w-14 shrink-0 flex-col gap-1.5">
        <span class="text-lg leading-none font-semibold tabular-nums" :style="{ color }">
          {{ device.battery === null ? "—" : device.battery
          }}<span v-if="device.battery !== null" class="text-[0.6em] font-medium">%</span>
        </span>
        <Progress
          :model-value="progress"
          class="h-1.5 [&>div]:bg-current"
          :style="{ color }"
          :aria-label="`电量 ${batteryText(device)}`"
        />
      </div>

      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <h2 class="truncate text-sm font-semibold">{{ device.displayName }}</h2>
          <Badge variant="secondary" class="shrink-0 font-normal">
            {{ transportLabel(device.transport) }}
          </Badge>
        </div>
        <p class="text-muted-foreground mt-1 truncate text-xs">{{ secondary }}</p>
      </div>

      <div class="flex flex-col items-end gap-1.5">
        <Badge variant="secondary" :class="[tone.chip, 'rounded-full']">
          <span class="size-1.5 rounded-full" :class="tone.dot" />
          {{ stateLabel(device.state) }}
        </Badge>
        <span
          v-if="device.lastError"
          class="max-w-[7rem] truncate font-mono text-[10px] text-orange-600 select-text dark:text-orange-400"
          :title="device.lastError"
        >{{ device.lastError }}</span>
      </div>
    </CardContent>
  </Card>
</template>
