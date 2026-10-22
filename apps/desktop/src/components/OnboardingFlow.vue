<script setup lang="ts">
import { ref } from "vue";
import type { OnboardingChoice } from "../lib/types";

/**
 * 首次引导（规格 §12「启动」）。
 *
 * 判定条件只有一个：`settings.onboardingCompleted`，与 settings.json 是否
 * 存在无关 —— 文件损坏被重建时 `recovered()` 会把该标志置为 `true`，
 * 避免用过即忘的用户反复看到引导。
 *
 * 开机自启默认关闭，必须在此显式勾选；引导页面不写业务状态，
 * 只把用户选择交给宿主（由宿主落盘并注册 Run 项）。
 */
defineProps<{ busy: boolean }>();
const emit = defineEmits<{ (e: "done", choice: OnboardingChoice): void }>();

const step = ref<0 | 1>(0);
const autostart = ref(false);
const startMinimized = ref(false);

function finish() {
  emit("done", { autostart: autostart.value, startMinimized: startMinimized.value });
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-[#f5f6f8] px-5 dark:bg-[#0b0f14]">
    <div
      class="w-full max-w-[420px] rounded-3xl border border-black/5 bg-white p-7 shadow-[0_12px_40px_rgba(16,24,40,0.10)] dark:border-white/10 dark:bg-white/[0.04]"
    >
      <div class="flex items-center gap-2">
        <span class="h-2 w-2 rounded-full bg-emerald-500" />
        <span class="text-[11.5px] font-medium tracking-wide text-neutral-400 uppercase">VoltNest</span>
      </div>

      <template v-if="step === 0">
        <h1 class="mt-4 text-[19px] leading-snug font-semibold">
          把鼠标、耳机的电量<br />放在一个安心的位置
        </h1>
        <ul class="mt-4 flex flex-col gap-2 text-[12.5px] text-neutral-600 dark:text-neutral-300">
          <li class="flex gap-2">
            <span class="text-neutral-400">·</span>
            <span>2.4G 接收器与蓝牙电量统一展示，同一设备只出现一次。</span>
          </li>
          <li class="flex gap-2">
            <span class="text-neutral-400">·</span>
            <span>读数过期会逐级退化（变黄 → 读取失败 → 离线），不会假装还是旧数字。</span>
          </li>
          <li class="flex gap-2">
            <span class="text-neutral-400">·</span>
            <span>关闭窗口只是收进托盘，随时可以从托盘图标看电量。</span>
          </li>
        </ul>
        <button
          type="button"
          class="mt-6 w-full rounded-xl bg-neutral-900 py-2 text-[13px] font-medium text-white transition hover:bg-neutral-800 dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-200"
          @click="step = 1"
        >
          继续
        </button>
      </template>

      <template v-else>
        <h1 class="mt-4 text-[19px] leading-snug font-semibold">再确认两件小事</h1>
        <label class="mt-5 flex cursor-pointer items-start justify-between gap-3">
          <span>
            <span class="block text-[12.5px]">开机时自动启动</span>
            <span class="mt-0.5 block text-[11px] text-neutral-400">
              默认关闭。开启后写入当前用户的启动项，启动过程静默到托盘
            </span>
          </span>
          <input
            type="checkbox"
            class="mt-1 h-4 w-4 accent-neutral-900 dark:accent-white"
            :checked="autostart"
            @change="autostart = ($event.target as HTMLInputElement).checked"
          />
        </label>
        <label class="mt-4 flex cursor-pointer items-start justify-between gap-3">
          <span>
            <span class="block text-[12.5px]">启动时最小化到托盘</span>
            <span class="mt-0.5 block text-[11px] text-neutral-400">手动启动时也不弹窗，只留托盘图标</span>
          </span>
          <input
            type="checkbox"
            class="mt-1 h-4 w-4 accent-neutral-900 dark:accent-white"
            :checked="startMinimized"
            @change="startMinimized = ($event.target as HTMLInputElement).checked"
          />
        </label>
        <div class="mt-6 flex gap-2">
          <button
            type="button"
            class="rounded-xl border border-black/10 px-4 py-2 text-[13px] transition hover:bg-black/[0.04] dark:border-white/15 dark:hover:bg-white/[0.06]"
            :disabled="busy"
            @click="step = 0"
          >
            返回
          </button>
          <button
            type="button"
            class="flex-1 rounded-xl bg-neutral-900 py-2 text-[13px] font-medium text-white transition hover:bg-neutral-800 disabled:opacity-50 dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-200"
            :disabled="busy"
            @click="finish"
          >
            {{ busy ? "正在保存…" : "开始使用" }}
          </button>
        </div>
        <p class="mt-3 text-[11px] text-neutral-400">这两项随时可以在设置页修改。</p>
      </template>
    </div>
  </div>
</template>
