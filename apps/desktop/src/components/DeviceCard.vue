<script setup lang="ts">
import { computed } from "vue";
import { Link2, Pencil, Split, Trash2, Zap } from "@lucide/vue";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
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
import type { CardDto, DeviceDto } from "../lib/types";

const props = defineProps<{
  device: DeviceDto;
  nowMs: number;
  /** 该行对应的合并卡；无合并时为空。 */
  card?: CardDto;
  /** 合并卡的非活跃成员说明行；有值时替代普通副行。 */
  inactiveText?: string;
  /** 没有其它设备可合并时菜单项置灰。 */
  canMerge: boolean;
}>();

const emit = defineEmits<{
  rename: [];
  merge: [];
  separate: [];
  remove: [];
}>();

const tone = computed(() => toneFor(props.device.state));

/** 无本次读数的首屏先用 state.json 缓存值，避免空白进度条，数据到达即换实时值（`displayedBattery`）。 */
const shownBattery = computed(() => displayedBattery(props.device));

const progress = computed(() => ringProgress(shownBattery.value));
const color = computed(() =>
  batteryColor(shownBattery.value, props.device.isCharging === true, props.device.state),
);

/** 仅 Rust 判出具体类型时存在（`unknown`→`null`）；卡片已改图标，文案仅供无障碍标签，判定细节见诊断页。 */
const typeLabel = computed(() => deviceTypeLabel(props.device.deviceType));

/** 取代「键盘/鼠标/耳机」文字标签；类型不可知按链路兜底（见 `lib/device-icon.ts`），此组件只渲染。 */
const typeIcon = computed(() => deviceTypeIcon(props.device.deviceType, props.device.transport));

/** 图标无文字，用无障碍标签补语义（类型优先，否则链路）。 */
const typeIconLabel = computed(() => typeLabel.value ?? transportLabel(props.device.transport));

/** 合并卡用合并名，单机用设备名。 */
const title = computed(() => props.card?.displayName || props.device.displayName);

/** 用户手动合并的卡片：副行标「已合并」，自动合并卡不标。 */
const userMerged = computed(() => props.card?.userMerged === true);

/** 副行：合并卡用非活跃链路说明（Rust 生成）；主数字为缓存值时补「上次读数 · 时间」；其余按充电状态。 */
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
  <ContextMenu>
    <ContextMenuTrigger as-child>
      <Card class="group py-3 transition-shadow hover:shadow-md" @contextmenu.stop>
        <CardContent class="flex items-center gap-3 px-4">
          <div
            class="flex size-8 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground"
            role="img"
            :aria-label="typeIconLabel"
            data-testid="device-type-icon"
          >
            <component :is="typeIcon" class="size-4" aria-hidden="true" />
          </div>

          <!-- 电量区域 -->
          <div class="flex w-16 shrink-0 flex-col gap-2">
            <div
              class="flex h-6 items-center gap-1 text-lg leading-none font-semibold whitespace-nowrap tabular-nums"
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
              <h2 class="truncate text-sm leading-5 font-semibold">
                {{ title }}
              </h2>

              <Badge variant="secondary" class="shrink-0 px-1.5 py-0 text-[10px] font-normal">
                {{ transportLabel(device.transport) }}
              </Badge>
            </div>

            <p
              class="mt-1 flex min-w-0 items-center gap-1.5 text-xs leading-4 text-muted-foreground"
            >
              <span v-if="userMerged" class="flex shrink-0 items-center gap-1">
                <Link2 class="size-3" aria-hidden="true" />
                已合并
              </span>
              <span class="truncate">{{ secondary }}</span>
            </p>
          </div>

          <!-- 状态区域 -->
          <div class="flex max-w-[8rem] shrink-0 flex-col items-end gap-1.5">
            <Badge variant="secondary" :class="[tone.chip, 'rounded-full whitespace-nowrap']">
              <span class="mr-1.5 size-1.5 shrink-0 rounded-full" :class="tone.dot" />
              {{ stateLabel(device.state) }}
            </Badge>

            <span
              v-if="device.lastError"
              class="max-w-full truncate text-right font-mono text-[10px] leading-3 text-orange-600 dark:text-orange-400"
              :title="device.lastError"
              >{{ device.lastError }}</span
            >
          </div>
        </CardContent>
      </Card>
    </ContextMenuTrigger>

    <ContextMenuContent class="w-44">
      <ContextMenuItem @select="emit('rename')">
        <Pencil aria-hidden="true" />
        改名
      </ContextMenuItem>
      <ContextMenuItem :disabled="!canMerge" @select="emit('merge')">
        <Link2 aria-hidden="true" />
        与其它设备合并
      </ContextMenuItem>
      <ContextMenuItem v-if="card" @select="emit('separate')">
        <Split aria-hidden="true" />
        {{ userMerged ? "分离合并" : "分离设备" }}
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem variant="destructive" @select="emit('remove')">
        <Trash2 aria-hidden="true" />
        删除设备
      </ContextMenuItem>
    </ContextMenuContent>
  </ContextMenu>
</template>
