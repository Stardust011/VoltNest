<script setup lang="ts">
import { computed } from "vue";
import { Zap } from "@lucide/vue";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import { deviceTypeIcon } from "../lib/device-icon";
import {
  batteryColor,
  deviceTypeLabel,
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
 * 类型文案：只有 Rust 判定出具体类型时才存在（`unknown` → `null`）。
 *
 * 卡片上不再显示这段文字（已换成图标），只留下给图标的无障碍标签用；
 * 判定来源见诊断页的 `device_classified` 记录，界面不重复解释。
 */
const typeLabel = computed(() => deviceTypeLabel(props.device.deviceType));

/**
 * 类型图标：取代原来的「键盘 / 鼠标 / 耳机」文字标签。
 *
 * 类型不可知时按链路兜底（2.4G → `Link`、蓝牙 → `BluetoothConnected`），
 * 规则集中在 `lib/device-icon.ts`，这里只做渲染。
 */
const typeIcon = computed(() => deviceTypeIcon(props.device.deviceType, props.device.transport));

/** 图标没有可见文字，用无障碍标签补回语义（类型已知说类型，否则说链路）。 */
const typeIconLabel = computed(() => typeLabel.value ?? transportLabel(props.device.transport));

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

<Card class="group py-3 transition-shadow hover:shadow-md">
  <CardContent class="flex items-center gap-3 px-4">

    <!-- 类型图标（最左侧）：类型不可知时退化为链路图标 -->
    <div
      class="bg-muted text-muted-foreground flex size-8 shrink-0 items-center justify-center rounded-md"
      role="img"
      :aria-label="typeIconLabel"
      data-testid="device-type-icon"
    >
      <component :is="typeIcon" class="size-4" aria-hidden="true" />
    </div>

    <!-- 电量区域 -->
    <div class="flex w-16 shrink-0 flex-col gap-2">
      <div
        class="flex h-6 items-center gap-1 whitespace-nowrap text-lg font-semibold leading-none tabular-nums"
        :style="{ color }"
      >
        <span>
          {{ shownBattery === null ? "—" : shownBattery
          }}<span v-if="shownBattery !== null" class="text-xs font-medium">%</span>
        </span>

        <Zap
          v-if="device.isCharging === true"
          class="size-3.5 shrink-0"
          aria-hidden="true"
          data-testid="charging-bolt"
        />
      </div>

      <Progress
        :model-value="progress"
        class="h-1.5 [&>div]:bg-current"
        :style="{ color }"
        :aria-label="shownBattery === null ? '电量未知' : `电量 ${shownBattery}%`"
      />
    </div>

    <!-- 设备信息 -->
    <div class="min-w-0 flex-1 self-center">
      <div class="flex min-w-0 items-center gap-2">
        <h2 class="truncate text-sm font-semibold leading-5">
          {{ device.displayName }}
        </h2>

        <Badge
          variant="secondary"
          class="shrink-0 px-1.5 py-0 text-[10px] font-normal"
        >
          {{ transportLabel(device.transport) }}
        </Badge>
      </div>

      <p class="mt-1 truncate text-xs leading-4 text-muted-foreground">
        {{ secondary }}
      </p>
    </div>

    <!-- 状态区域 -->
    <div class="flex max-w-[8rem] shrink-0 flex-col items-end gap-1.5">
      <Badge
        variant="secondary"
        :class="[tone.chip, 'rounded-full whitespace-nowrap']"
      >
        <span
          class="mr-1.5 size-1.5 shrink-0 rounded-full"
          :class="tone.dot"
        />
        {{ stateLabel(device.state) }}
      </Badge>

      <span
        v-if="device.lastError"
        class="max-w-full truncate text-right font-mono text-[10px] leading-3 text-orange-600 dark:text-orange-400"
        :title="device.lastError"
      >{{ device.lastError }}</span>
    </div>

  </CardContent>
</Card>

</template>
