<script setup lang="ts">
import { ref } from "vue";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
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
  <!-- 遮罩从自绘标题栏下方开始：引导期间标题栏仍可拖动 / 最小化 / 收进托盘。 -->
  <div
    class="bg-background fixed inset-x-0 bottom-0 top-10 z-40 flex items-center justify-center px-5"
  >
    <Card class="w-full max-w-[420px] gap-4 py-6 shadow-lg">
      <CardHeader class="gap-1 px-6">
        <div class="flex items-center gap-2">
          <span class="size-2 rounded-full bg-emerald-500" />
          <span class="text-muted-foreground text-xs font-medium tracking-wide uppercase">
            VoltNest
          </span>
        </div>
        <CardTitle v-if="step === 0" class="mt-3 text-lg leading-snug">
          把鼠标、耳机的电量<br />放在一个安心的位置
        </CardTitle>
        <CardTitle v-else class="mt-3 text-lg leading-snug">再确认两件小事</CardTitle>
      </CardHeader>

      <CardContent class="px-6">
        <ul v-if="step === 0" class="text-muted-foreground flex flex-col gap-2 text-xs">
          <li class="flex gap-2">
            <span>·</span>
            <span>2.4G 接收器与蓝牙电量统一展示，同一设备只出现一次。</span>
          </li>
          <li class="flex gap-2">
            <span>·</span>
            <span>读数过期会逐级退化（变黄 → 读取失败 → 离线），不会假装还是旧数字。</span>
          </li>
          <li class="flex gap-2">
            <span>·</span>
            <span>关闭窗口只是收进托盘，随时可以从托盘图标看电量。</span>
          </li>
        </ul>

        <div v-else class="flex flex-col gap-4">
          <div class="flex items-start justify-between gap-3">
            <Label
              for="onboarding-autostart"
              class="flex flex-1 cursor-pointer flex-col items-start gap-0.5 font-normal"
            >
              <span class="text-sm">开机时自动启动</span>
              <span class="text-muted-foreground text-xs font-normal">
                默认关闭。开启后写入当前用户的启动项，可随时关闭
              </span>
            </Label>
            <Checkbox
              id="onboarding-autostart"
              class="mt-0.5"
              :model-value="autostart"
              @update:model-value="(value) => (autostart = value === true)"
            />
          </div>

          <div class="flex items-start justify-between gap-3">
            <Label
              for="onboarding-start-minimized"
              class="flex flex-1 cursor-pointer flex-col items-start gap-0.5 font-normal"
            >
              <span class="text-sm">启动时最小化到托盘</span>
              <span class="text-muted-foreground text-xs font-normal">
                启动时不弹窗，只留托盘图标
              </span>
            </Label>
            <Checkbox
              id="onboarding-start-minimized"
              class="mt-0.5"
              :model-value="startMinimized"
              @update:model-value="(value) => (startMinimized = value === true)"
            />
          </div>
        </div>
      </CardContent>

      <CardFooter class="px-6">
        <Button v-if="step === 0" type="button" class="w-full" @click="step = 1">继续</Button>
        <div v-else class="flex w-full flex-col gap-3">
          <div class="flex w-full gap-2">
            <Button type="button" variant="outline" :disabled="busy" @click="step = 0">
              返回
            </Button>
            <Button type="button" class="flex-1" :disabled="busy" @click="finish">
              {{ busy ? "正在保存…" : "开始使用" }}
            </Button>
          </div>
          <p class="text-muted-foreground w-full text-xs">这两项随时可以在设置页修改。</p>
        </div>
      </CardFooter>
    </Card>
  </div>
</template>
