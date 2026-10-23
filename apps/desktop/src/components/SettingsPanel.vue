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

/** 两档提醒展示信息；`id`/`errorId` 须稳定唯一：输入框凭 `id` 配 Label 的 `for`，错误文本凭 `errorId` 被 `aria-describedby` 引用。 */
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

/** 输入框的文本缓冲：正式值为数字，空串/小数无法表达；直接写回会让宿主回传的旧值顶掉用户正在输的内容，故合法且失焦才经 `setThreshold` 落正式值。 */
const thresholdInputs = ref<[string, string]>([
  String(thresholds.value[0]),
  String(thresholds.value[1]),
]);

/** 实时校验：输入即变红，不等失焦。 */
const thresholdErrors = computed<[string | null, string | null]>(() =>
  validateThresholdPair(thresholdInputs.value),
);

/** 宿主回传正式值变化时同步缓冲：只更新「未改过」的镜像，避免回传把红框里的半成品悄悄替换（也算自动调整）。 */
watch(
  thresholds,
  (next, previous) => {
    const buffered: [string, string] = [thresholdInputs.value[0], thresholdInputs.value[1]];
    for (const index of [0, 1] as const) {
      if (buffered[index] !== String(previous?.[index])) continue;
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

/** 写正式阈值；越界/顺序兜底在 `applyThresholdEdit`（0/95 边界会挪另一档），非整数直接放弃，正式值不变。 */
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

/** 失焦提交，不做任何自动调整：非法值既不写正式值也不改用户输入/回退旧值，红框保留等用户改；仅整对合法且与正式值不同才走 `setThreshold`。 */
function commitThreshold(index: ThresholdIndex) {
  if (thresholdErrors.value[index] !== null) return;
  const value = parseThresholdInput(thresholdInputs.value[index]);
  if (value === null || value === thresholds.value[index]) return;
  setThreshold(index, value);
}

function onThresholdBlur(index: ThresholdIndex) {
  commitThreshold(index);
}

/** 回车等价于失焦提交，避免键盘用户必须先点别处才生效。 */
function onThresholdEnter(event: KeyboardEvent) {
  (event.target as HTMLInputElement | null)?.blur();
}

/** 数字框仍会接收 `e`/`+`/`-`/`.`，直接挡掉：只取整百分比；不拦 Ctrl/Alt/Meta 组合键。 */
function onThresholdKeydown(event: KeyboardEvent) {
  if (event.ctrlKey || event.metaKey || event.altKey) return;
  if (["e", "E", "+", "-", ".", ","].includes(event.key)) event.preventDefault();
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
          <CardTitle class="text-sm">刷新速率</CardTitle>
          <Select :model-value="settings.sensitivity" @update:model-value="setSensitivity">
            <SelectTrigger id="refresh-rate" aria-label="刷新速率" size="sm" class="w-[9rem]">
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
        <p class="text-xs text-muted-foreground">电量下降时，按以下档位依次提醒。</p>
      </CardHeader>
      <CardContent class="flex flex-col gap-3 px-4">
        <div class="flex items-center justify-between gap-3">
          <Label for="notification-mode" class="text-xs font-normal text-muted-foreground">
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
            <div class="flex min-w-0 flex-1 items-center gap-2">
              <component :is="row.icon" class="size-4 shrink-0 text-foreground" />
              <div class="min-w-0">
                <Label :for="row.id" class="text-xs font-medium">{{ row.label }}</Label>
                <p class="mt-0.5 text-[11px] leading-snug text-muted-foreground">{{ row.hint }}</p>
              </div>
            </div>
            <div class="flex shrink-0 items-center gap-1.5">
              <Input
                :id="row.id"
                type="number"
                min="0"
                max="95"
                step="1"
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
                @blur="onThresholdBlur(row.index)"
                @keydown="onThresholdKeydown"
                @keydown.enter="onThresholdEnter"
              />
              <span class="text-xs text-muted-foreground" aria-hidden="true">%</span>
            </div>
          </div>
          <!-- 错误行常驻占位，出现/消失不会让上下内容跳动。 -->
          <div class="flex min-h-4 justify-end">
            <p
              v-if="thresholdErrors[row.index]"
              :id="row.errorId"
              class="text-right text-[11px] leading-tight text-destructive"
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
            @update:model-value="
              (value) => onCheckbox(value, (checked) => emit('autostart', checked))
            "
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
