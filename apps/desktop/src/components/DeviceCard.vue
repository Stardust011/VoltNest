<script setup lang="ts">
import { computed } from "vue";
import { Zap } from "@lucide/vue";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import {
  batteryColor,
  displayedBattery,
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

/**
 * 展示用读数：启动瞬间还没有本次读数（`state === "unknown"`）时先用 state.json 的缓存值，
 * 免得首屏出现「— + 空进度条」，数据一到就自动换成实时值（`displayedBattery`）。
 */
const shownBattery = computed(() => displayedBattery(props.device));

const progress = computed(() => ringProgress(shownBattery.value));
const color = computed(() =>
  batteryColor(shownBattery.value, props.device.isCharging === true, props.device.state),
);

/**
 * 副行：
 * 1. 合并卡 → 非活跃链路说明（W-2，由 Rust 生成）；
 * 2. 主数字是缓存值 → 补「上次读数 · 时间」，让「等待读数」有明确来源；
 * 3. 其余按充电状态。
 */
const secondary = computed(() => {
  if (props.inactiveText) return props.inactiveText;
  if (props.device.battery === null && props.device.lastKnownBattery !== null) {
    const at = props.device.lastKnownAtMs;
    const when = at === null ? "" : humanizeAgo(Math.max(0, props.nowMs - at));
    return when ? `上次读数 · ${when}` : "上次读数";
  }
  return secondaryLine(props.device);
});
</script>

<template>
  <Card class="gap-0 py-3.5 transition-shadow hover:shadow-md">
    <CardContent class="flex items-center gap-3.5 px-4">
      <!-- 电量读数：数值 + Progress，颜色沿用 batteryColor 的语义（充电 / 低电 / 状态）。 -->
      <div class="flex w-14 shrink-0 flex-col gap-1.5">
        <!-- 按基线对齐：数字坐在自身基线上，SVG 的基线就是它的下边缘，
             所以闪电的底边与数字底边齐平 —— 用 items-center 反而会因为
             数字没有下伸部而看起来高半格。尺寸取 0.8em，字形高度刚好接近大写高。 -->
        <span
          class="flex items-baseline gap-0.5 text-lg leading-none font-semibold tabular-nums"
          :style="{ color }"
        >
          <span>
            {{ shownBattery === null ? "—" : shownBattery
            }}<span v-if="shownBattery !== null" class="text-[0.6em] font-medium">%</span>
          </span>
          <!-- 充电：电量旁的闪电（lucide zap），与托盘绿环里的闪电是同一个符号。 -->
          <Zap
            v-if="device.isCharging === true"
            class="h-[0.8em] w-[0.8em] shrink-0"
            aria-hidden="true"
            data-testid="charging-bolt"
          />
        </span>
        <Progress
          :model-value="progress"
          class="h-1.5 [&>div]:bg-current"
          :style="{ color }"
          :aria-label="shownBattery === null ? '电量未知' : `电量 ${shownBattery}%`"
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
