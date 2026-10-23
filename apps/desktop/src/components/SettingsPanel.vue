<script setup lang="ts">
import { computed, ref, watch, type Component } from "vue";
import { BatteryLow, BatteryMedium } from "@lucide/vue";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  applyThresholdEdit,
  parseThresholdInput,
  validateThresholdPair,
  THRESHOLD_MAX,
  THRESHOLD_MIN,
  type ThresholdIndex,
} from "../lib/thresholds";
import type { NotificationMode, Sensitivity, SettingsDto } from "../lib/types";

const props = defineProps<{ settings: SettingsDto }>();
const emit = defineEmits<{
  (e: "change", settings: SettingsDto): void;
  (e: "autostart", enabled: boolean): void;
}>();

const sensitivityOptions: Array<{ value: Sensitivity; label: string }> = [
  { value: "sensitive", label: "快" },
  { value: "standard", label: "标准（推荐）" },
  { value: "loose", label: "慢" },
];

const modeOptions: Array<{ value: NotificationMode; label: string }> = [
  { value: "system", label: "系统通知" },
  { value: "trayBubble", label: "托盘气泡" },
  { value: "off", label: "关闭" },
];

/**
 * 两档提醒的展示信息。
 *
 * `id` / `errorId` 必须是稳定且唯一的：输入框靠 `id` 与 Label 的 `for` 配对，
 * 错误文本靠 `errorId` 被 `aria-describedby` 引用。
 */
interface ThresholdRow {
  index: ThresholdIndex;
  id: string;
  errorId: string;
  label: string;
  hint: string;
  icon: Component;
}

const thresholdRows: readonly ThresholdRow[] = [
  {
    index: 0,
    id: "threshold-first",
    errorId: "threshold-first-error",
    label: "第一档提醒",
    hint: "电量较低，首次提醒",
    icon: BatteryMedium,
  },
  {
    index: 1,
    id: "threshold-second",
    errorId: "threshold-second-error",
    label: "第二档提醒",
    hint: "电量更低，再次提醒",
    icon: BatteryLow,
  },
];

const thresholds = computed(() => props.settings.notifications.lowThresholds);

/**
 * 输入框的临时文本。
 *
 * 不能直接把 `thresholds` 喂给输入框：正式值是数字，空串和小数没法表达，
 * 一旦把非法输入写回设置，宿主回传的旧值会把用户正在输的内容顶掉。
 * 所以这里保留一份字符串缓冲，合法且失焦时才通过 `setThreshold` 落到正式值。
 */
const thresholdInputs = ref<[string, string]>([
  String(thresholds.value[0]),
  String(thresholds.value[1]),
]);

/** 当前聚焦的档位：用来避免宿主回传时把用户正在编辑的缓冲覆盖掉。 */
const focused = ref<ThresholdIndex | null>(null);

/** 实时校验结果：输入过程中就能变红，不用等失焦。 */
const thresholdErrors = computed<[string | null, string | null]>(() =>
  validateThresholdPair(thresholdInputs.value),
);

/**
 * 宿主回传的正式值变化时校准缓冲。
 *
 * 正在聚焦且内容已被用户改过（与上一版正式值不同）时不覆盖，
 * 否则会在光标处打断输入；失焦提交时还会再对齐一次。
 */
watch(
  thresholds,
  (next, previous) => {
    const buffered: [string, string] = [thresholdInputs.value[0], thresholdInputs.value[1]];
    for (const index of [0, 1] as const) {
      const untouched = buffered[index] === String(previous?.[index]);
      if (focused.value === index && !untouched) continue;
      buffered[index] = String(next[index]);
    }
    thresholdInputs.value = buffered;
  },
  { deep: true },
);

function patch(partial: Partial<SettingsDto>) {
  emit("change", { ...props.settings, ...partial });
}

/** Select 回调的取值类型：reka-ui 允许任意可接受值，这里只认字符串项。 */
function setSensitivity(value: unknown) {
  if (typeof value !== "string") return;
  patch({ sensitivity: value as Sensitivity });
}

/** 同上：只接受预置的通知方式字符串。 */
function setMode(mode: unknown) {
  if (typeof mode !== "string") return;
  patch({ notifications: { ...props.settings.notifications, mode: mode as NotificationMode } });
}

/**
 * 写正式阈值。自动修正顺序（含 5 / 95 边界）在 `applyThresholdEdit` 里，
 * 非整数直接放弃，正式值保持原样。
 */
function setThreshold(index: ThresholdIndex, raw: string | number) {
  const next = applyThresholdEdit(thresholds.value, index, raw);
  if (!next) return;
  patch({ notifications: { ...props.settings.notifications, lowThresholds: next } });
}

/** 输入只更新缓冲，不写正式值：空串和半成品输入必须留在输入框里。 */
function onThresholdInput(index: ThresholdIndex, value: string) {
  if (index === 0) thresholdInputs.value = [value, thresholdInputs.value[1]];
  else thresholdInputs.value = [thresholdInputs.value[0], value];
}

/** 把缓冲恢复到某一档的正式值（提交被拒绝时用）。 */
function resetThresholdInput(index: ThresholdIndex) {
  const value = String(thresholds.value[index]);
  onThresholdInput(index, value);
}

/**
 * 失焦提交。
 *
 * - 单档非法（空 / 非整数 / 越界）：不写正式值，缓冲退回正式值，
 *   否则红框里会长期停着一个从未生效过的数字；
 * - 单档合法但两档顺序不成立：交给 `setThreshold` 按原有顺序修正，
 *   修正结果立即回填到两个输入框，红框随之消失；
 * - 两档结果与正式值逐位相同：不产生多余的写盘。
 */
function commitThreshold(index: ThresholdIndex) {
  const raw = thresholdInputs.value[index];
  const value = parseThresholdInput(raw);
  if (value === null || value < THRESHOLD_MIN || value > THRESHOLD_MAX) {
    resetThresholdInput(index);
    return;
  }
  const next = applyThresholdEdit(thresholds.value, index, value);
  if (!next) {
    resetThresholdInput(index);
    return;
  }
  // 逐位比较：自动修正可能只挪动了另一档（例如盘上本就是非法组合时）。
  if (next[0] !== thresholds.value[0] || next[1] !== thresholds.value[1]) {
    setThreshold(index, value);
  }
  // 立即按预期正式值对齐两档；宿主回传后由 watch 再校准一次。
  thresholdInputs.value = [String(next[0]), String(next[1])];
}

function onThresholdBlur(index: ThresholdIndex) {
  focused.value = null;
  commitThreshold(index);
}

/** 回车等价于失焦提交，避免键盘用户必须先点别处才生效。 */
function onThresholdEnter(event: KeyboardEvent) {
  (event.target as HTMLInputElement | null)?.blur();
}

function onThresholdFocus(index: ThresholdIndex) {
  focused.value = index;
}

/** Checkbox 的 modelValue 还可能是 "indeterminate"，这里只认布尔。 */
function onCheckbox(value: boolean | "indeterminate", apply: (checked: boolean) => void) {
  apply(value === true);
}
</script>

<template>
  <div class="flex flex-col gap-4 pb-4">
    <Card class="gap-3 py-4">
      <CardHeader class="px-4">
        <div class="flex items-center justify-between gap-3">
          <CardTitle class="text-sm">数据过期速度</CardTitle>
          <Select :model-value="settings.sensitivity" @update:model-value="setSensitivity">
            <SelectTrigger id="expiry-speed" aria-label="数据过期速度" size="sm" class="w-[9rem]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="opt in sensitivityOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      </CardHeader>
    </Card>

    <Card class="gap-3 py-4">
      <CardHeader class="px-4">
        <CardTitle class="text-sm">低电量通知</CardTitle>
        <p class="text-muted-foreground text-xs">电量下降时，按以下档位依次提醒。</p>
      </CardHeader>
      <CardContent class="flex flex-col gap-3 px-4">
        <div class="flex items-center justify-between gap-3">
          <Label for="notification-mode" class="text-muted-foreground text-xs font-normal">
            通知方式
          </Label>
          <Select :model-value="settings.notifications.mode" @update:model-value="setMode">
            <SelectTrigger id="notification-mode" size="sm" class="w-[9rem]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="opt in modeOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <!--
          两档各占一行，不各占一张卡片：同一张 Card 里保持纵向列表，
          左侧是图标 + 名称/说明，右侧是百分比输入框 + 外置的 %。
        -->
        <div v-for="row in thresholdRows" :key="row.id" class="flex flex-col gap-1">
          <div class="flex items-start justify-between gap-3">
            <div class="flex min-w-0 flex-1 items-start gap-2">
              <component :is="row.icon" class="text-destructive/70 mt-0.5 size-4 shrink-0" />
              <div class="min-w-0">
                <Label :for="row.id" class="text-xs font-medium">{{ row.label }}</Label>
                <p class="text-muted-foreground mt-0.5 text-[11px] leading-snug">{{ row.hint }}</p>
              </div>
            </div>
            <div class="flex shrink-0 items-center gap-1.5">
              <Input
                :id="row.id"
                type="text"
                inputmode="numeric"
                autocomplete="off"
                :aria-label="`${row.label}电量（百分比）`"
                :aria-invalid="thresholdErrors[row.index] ? 'true' : undefined"
                :aria-describedby="thresholdErrors[row.index] ? row.errorId : undefined"
                :model-value="thresholdInputs[row.index]"
                :class="[
                  'h-8 w-14 px-2 text-right text-xs tabular-nums',
                  thresholdErrors[row.index] ? 'text-destructive' : '',
                ]"
                @update:model-value="(value) => onThresholdInput(row.index, String(value))"
                @focus="onThresholdFocus(row.index)"
                @blur="onThresholdBlur(row.index)"
                @keydown.enter="onThresholdEnter"
              />
              <span class="text-muted-foreground text-xs" aria-hidden="true">%</span>
            </div>
          </div>
          <!-- 错误行常驻占位，出现/消失不会让上下内容跳动。 -->
          <div class="flex min-h-4 justify-end">
            <p
              v-if="thresholdErrors[row.index]"
              :id="row.errorId"
              class="text-destructive text-right text-[11px] leading-tight"
            >
              {{ thresholdErrors[row.index] }}
            </p>
          </div>
        </div>
      </CardContent>
    </Card>

    <Card class="gap-3 py-4">
      <CardHeader class="px-4">
        <CardTitle class="text-sm">启动</CardTitle>
      </CardHeader>
      <CardContent class="flex flex-col gap-3 px-4">
        <div class="flex items-center justify-between gap-3">
          <Label for="autostart" class="flex-1 cursor-pointer text-sm font-normal">
            开机时自动启动
          </Label>
          <Checkbox
            id="autostart"
            :model-value="settings.autostart"
            @update:model-value="(value) => onCheckbox(value, (checked) => emit('autostart', checked))"
          />
        </div>

        <div class="flex items-center justify-between gap-3">
          <Label for="start-minimized" class="flex-1 cursor-pointer text-sm font-normal">
            启动时最小化到托盘
          </Label>
          <Checkbox
            id="start-minimized"
            :model-value="settings.startMinimized"
            @update:model-value="
              (value) => onCheckbox(value, (checked) => patch({ startMinimized: checked }))
            "
          />
        </div>
      </CardContent>
    </Card>
  </div>
</template>
