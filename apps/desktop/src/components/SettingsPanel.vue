<script setup lang="ts">
import { computed } from "vue";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
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

const props = defineProps<{ settings: SettingsDto; busy: boolean }>();
const emit = defineEmits<{
  (e: "change", settings: SettingsDto): void;
  (e: "autostart", enabled: boolean): void;
  (e: "test-notification"): void;
  (e: "restart", transport: "hid" | "ble"): void;
}>();

const sensitivityOptions: Array<{ value: Sensitivity; label: string; hint: string }> = [
  { value: "sensitive", label: "敏感", hint: "更快变黄，适合盯盘" },
  { value: "standard", label: "标准", hint: "推荐" },
  { value: "loose", label: "宽松", hint: "更迟钝，适合闲置设备" },
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

function setSensitivity(value: Sensitivity) {
  patch({ sensitivity: value });
}

/** Select 回调的取值类型：reka-ui 允许任意可接受值，这里只认字符串项。 */
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
      <CardHeader class="gap-1 px-4">
        <CardTitle class="text-sm">过期灵敏度</CardTitle>
        <CardDescription class="text-xs">
          作用于整条过期链（变黄 → 读取失败 → 离线），三个阈值同比缩放。
        </CardDescription>
      </CardHeader>
      <CardContent class="px-4">
        <div class="grid grid-cols-3 gap-2" role="group" aria-label="过期灵敏度">
          <Button
            v-for="opt in sensitivityOptions"
            :key="opt.value"
            type="button"
            :variant="settings.sensitivity === opt.value ? 'default' : 'outline'"
            :aria-pressed="settings.sensitivity === opt.value"
            class="h-auto flex-col items-start gap-0.5 px-2 py-2 text-left"
            @click="setSensitivity(opt.value)"
          >
            <span class="text-sm font-medium">{{ opt.label }}</span>
            <span class="text-[11px] font-normal opacity-70">{{ opt.hint }}</span>
          </Button>
        </div>
      </CardContent>
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
            <SelectTrigger id="notification-mode" size="sm" class="w-[8.5rem]">
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
          <Label for="threshold-low" class="text-muted-foreground text-xs font-normal">提醒阈值</Label>
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
            <span class="text-muted-foreground text-xs">/</span>
            <Input
              type="number"
              min="5"
              max="95"
              aria-label="高提醒阈值"
              class="h-8 w-16 text-center text-xs tabular-nums"
              :model-value="thresholds[1]"
              @change="setThreshold(1, ($event.target as HTMLInputElement).value)"
            />
            <span class="text-muted-foreground text-xs">%</span>
          </div>
        </div>

        <p class="text-muted-foreground mt-2 text-xs">
          跌破阈值才提醒；回升到「阈值 + 5%」才重新武装；同一档 30 分钟内最多一次。
        </p>
        <Button
          type="button"
          variant="outline"
          class="mt-3 w-full"
          :disabled="busy"
          @click="emit('test-notification')"
        >
          发送测试通知
        </Button>
      </CardContent>
    </Card>

    <Card class="gap-3 py-4">
      <CardHeader class="px-4">
        <CardTitle class="text-sm">启动</CardTitle>
      </CardHeader>
      <CardContent class="flex flex-col gap-3 px-4">
        <div class="flex items-start justify-between gap-3">
          <Label for="autostart" class="flex flex-1 cursor-pointer flex-col items-start gap-0.5 font-normal">
            <span class="text-sm">开机时自动启动</span>
            <span class="text-muted-foreground text-xs font-normal">
              写入 HKCU 的 Run 项，可随时在此关闭
            </span>
          </Label>
          <Checkbox
            id="autostart"
            class="mt-0.5"
            :model-value="settings.autostart"
            @update:model-value="(value) => onCheckbox(value, (checked) => emit('autostart', checked))"
          />
        </div>

        <div class="flex items-start justify-between gap-3">
          <Label
            for="start-minimized"
            class="flex flex-1 cursor-pointer flex-col items-start gap-0.5 font-normal"
          >
            <span class="text-sm">启动时最小化到托盘</span>
            <span class="text-muted-foreground text-xs font-normal">
              仅影响手动启动；自启路径始终静默
            </span>
          </Label>
          <Checkbox
            id="start-minimized"
            class="mt-0.5"
            :model-value="settings.startMinimized"
            @update:model-value="
              (value) => onCheckbox(value, (checked) => patch({ startMinimized: checked }))
            "
          />
        </div>
      </CardContent>
    </Card>

    <Card class="gap-3 py-4">
      <CardHeader class="gap-1 px-4">
        <CardTitle class="text-sm">设备采集</CardTitle>
        <CardDescription class="text-xs">
          采集线程异常退出后可手动重启；重启不会丢失设备状态。
        </CardDescription>
      </CardHeader>
      <CardContent class="px-4">
        <div class="grid grid-cols-2 gap-2">
          <Button type="button" variant="outline" @click="emit('restart', 'hid')">
            重启 2.4G 采集
          </Button>
          <Button type="button" variant="outline" @click="emit('restart', 'ble')">
            重启蓝牙采集
          </Button>
        </div>
      </CardContent>
    </Card>
  </div>
</template>
