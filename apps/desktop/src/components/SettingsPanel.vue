<script setup lang="ts">
import { computed } from "vue";
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

function setMode(mode: NotificationMode) {
  patch({ notifications: { ...props.settings.notifications, mode } });
}

function setThreshold(index: 0 | 1, raw: string) {
  const value = Number.parseInt(raw, 10);
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
</script>

<template>
  <div class="flex flex-col gap-4 pb-4">
    <section class="rounded-2xl border border-black/5 bg-white/90 p-4 dark:border-white/10 dark:bg-white/[0.04]">
      <h3 class="text-[13px] font-semibold">过期灵敏度</h3>
      <p class="mt-1 text-[12px] text-neutral-500 dark:text-neutral-400">
        作用于整条过期链（变黄 → 读取失败 → 离线），三个阈值同比缩放。
      </p>
      <div class="mt-3 grid grid-cols-3 gap-2">
        <button
          v-for="opt in sensitivityOptions"
          :key="opt.value"
          type="button"
          class="rounded-xl border px-2 py-2 text-left transition"
          :class="settings.sensitivity === opt.value
            ? 'border-transparent bg-neutral-900 text-white dark:bg-white dark:text-neutral-900'
            : 'border-black/10 bg-transparent hover:bg-black/[0.03] dark:border-white/15 dark:hover:bg-white/[0.06]'"
          @click="setSensitivity(opt.value)"
        >
          <span class="block text-[13px] font-medium">{{ opt.label }}</span>
          <span class="mt-0.5 block text-[10.5px] opacity-70">{{ opt.hint }}</span>
        </button>
      </div>
    </section>

    <section class="rounded-2xl border border-black/5 bg-white/90 p-4 dark:border-white/10 dark:bg-white/[0.04]">
      <h3 class="text-[13px] font-semibold">低电量通知</h3>
      <div class="mt-3 flex items-center justify-between gap-3">
        <span class="text-[12.5px] text-neutral-600 dark:text-neutral-300">通知方式</span>
        <select
          class="rounded-lg border border-black/10 bg-white px-2 py-1 text-[12.5px] dark:border-white/15 dark:bg-neutral-900"
          :value="settings.notifications.mode"
          @change="setMode(($event.target as HTMLSelectElement).value as NotificationMode)"
        >
          <option v-for="opt in modeOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
        </select>
      </div>
      <div class="mt-3 flex items-center justify-between gap-3">
        <span class="text-[12.5px] text-neutral-600 dark:text-neutral-300">提醒阈值</span>
        <div class="flex items-center gap-1.5">
          <input
            type="number"
            min="5"
            max="95"
            class="w-16 rounded-lg border border-black/10 bg-white px-2 py-1 text-center text-[12.5px] tabular-nums dark:border-white/15 dark:bg-neutral-900"
            :value="thresholds[0]"
            @change="setThreshold(0, ($event.target as HTMLInputElement).value)"
          />
          <span class="text-neutral-400">/</span>
          <input
            type="number"
            min="5"
            max="95"
            class="w-16 rounded-lg border border-black/10 bg-white px-2 py-1 text-center text-[12.5px] tabular-nums dark:border-white/15 dark:bg-neutral-900"
            :value="thresholds[1]"
            @change="setThreshold(1, ($event.target as HTMLInputElement).value)"
          />
          <span class="text-[12.5px] text-neutral-400">%</span>
        </div>
      </div>
      <p class="mt-2 text-[11.5px] text-neutral-400">
        跌破阈值才提醒；回升到「阈值 + 5%」才重新武装；同一档 30 分钟内最多一次。
      </p>
      <button
        type="button"
        class="mt-3 w-full rounded-xl border border-black/10 py-1.5 text-[12.5px] font-medium transition hover:bg-black/[0.04] disabled:opacity-50 dark:border-white/15 dark:hover:bg-white/[0.06]"
        :disabled="busy"
        @click="emit('test-notification')"
      >
        发送测试通知
      </button>
    </section>

    <section class="rounded-2xl border border-black/5 bg-white/90 p-4 dark:border-white/10 dark:bg-white/[0.04]">
      <h3 class="text-[13px] font-semibold">启动</h3>
      <label class="mt-3 flex cursor-pointer items-start justify-between gap-3">
        <span>
          <span class="block text-[12.5px]">开机时自动启动</span>
          <span class="mt-0.5 block text-[11px] text-neutral-400">写入 HKCU 的 Run 项，可随时在此关闭</span>
        </span>
        <input
          type="checkbox"
          class="mt-1 h-4 w-4 accent-neutral-900 dark:accent-white"
          :checked="settings.autostart"
          @change="emit('autostart', ($event.target as HTMLInputElement).checked)"
        />
      </label>
      <label class="mt-3 flex cursor-pointer items-start justify-between gap-3">
        <span>
          <span class="block text-[12.5px]">启动时最小化到托盘</span>
          <span class="mt-0.5 block text-[11px] text-neutral-400">仅影响手动启动；自启路径始终静默</span>
        </span>
        <input
          type="checkbox"
          class="mt-1 h-4 w-4 accent-neutral-900 dark:accent-white"
          :checked="settings.startMinimized"
          @change="patch({ startMinimized: ($event.target as HTMLInputElement).checked })"
        />
      </label>
    </section>

    <section class="rounded-2xl border border-black/5 bg-white/90 p-4 dark:border-white/10 dark:bg-white/[0.04]">
      <h3 class="text-[13px] font-semibold">设备采集</h3>
      <p class="mt-1 text-[11.5px] text-neutral-400">
        采集线程异常退出后可手动重启；重启不会丢失设备状态。
      </p>
      <div class="mt-3 grid grid-cols-2 gap-2">
        <button
          type="button"
          class="rounded-xl border border-black/10 py-1.5 text-[12.5px] transition hover:bg-black/[0.04] dark:border-white/15 dark:hover:bg-white/[0.06]"
          @click="emit('restart', 'hid')"
        >
          重启 2.4G 采集
        </button>
        <button
          type="button"
          class="rounded-xl border border-black/10 py-1.5 text-[12.5px] transition hover:bg-black/[0.04] dark:border-white/15 dark:hover:bg-white/[0.06]"
          @click="emit('restart', 'ble')"
        >
          重启蓝牙采集
        </button>
      </div>
    </section>
  </div>
</template>
