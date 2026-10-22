<script setup lang="ts">
import { computed } from "vue";
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

const thresholds = computed(() => props.settings.notifications.lowThresholds);

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

function setThreshold(index: 0 | 1, raw: string | number) {
  const value = Number.parseInt(String(raw), 10);
  if (Number.isNaN(value)) return;
  const clamped = Math.min(95, Math.max(5, value));
  const next: [number, number] = [thresholds.value[0], thresholds.value[1]];
  next[index] = clamped;
  // 高阈值必须严格大于低阈值，否则同一档会重复触发。
  if (next[0] <= next[1]) {
    if (index === 0) next[0] = next[1] + 1;
    else next[1] = next[0] - 1;
  }
  patch({ notifications: { ...props.settings.notifications, lowThresholds: next } });
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
      </CardHeader>
      <CardContent class="px-4">
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

        <div class="mt-3 flex items-center justify-between gap-3">
          <Label for="threshold-low" class="text-muted-foreground text-xs font-normal">
            提醒电量
          </Label>
          <div class="flex items-center gap-1.5">
            <Input
              id="threshold-low"
              type="number"
              min="5"
              max="95"
              class="h-8 w-16 text-center text-xs tabular-nums"
              :model-value="thresholds[0]"
              @change="setThreshold(0, ($event.target as HTMLInputElement).value)"
            />
            <span class="text-muted-foreground text-xs">→</span>
            <Input
              type="number"
              min="5"
              max="95"
              aria-label="第二档提醒电量"
              class="h-8 w-16 text-center text-xs tabular-nums"
              :model-value="thresholds[1]"
              @change="setThreshold(1, ($event.target as HTMLInputElement).value)"
            />
            <span class="text-muted-foreground text-xs">%</span>
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
