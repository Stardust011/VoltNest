<script setup lang="ts">
import { computed, ref, type Component } from "vue";
import {
  Headphones,
  Layers,
  LoaderCircle,
  Mouse,
  PanelTop,
  Power,
  ShieldCheck,
  Zap,
} from "@lucide/vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import type { OnboardingChoice } from "../lib/types";

/**
 * 首次引导。
 * 判定只看 `settings.onboardingCompleted`，与 settings.json 是否存在无关——文件损坏被重建时 `recovered()` 置其为 `true`，避免用户反复见引导。
 * 开机自启默认关，须在此显式勾选；页面不写业务状态，只把选择交宿主落盘并注册 Run 项。
 * 头部共用「品牌行+步骤进度+标题」，两步只换标题与内容：一为产品介绍，二为两项启动设置。
 */
defineProps<{ busy: boolean }>();
const emit = defineEmits<{ (e: "done", choice: OnboardingChoice): void }>();

const step = ref<0 | 1>(0);
const autostart = ref(false);
const startMinimized = ref(false);

/** 两步式进度：第一步走一半，第二步走满。 */
const progress = computed(() => (step.value === 0 ? 50 : 100));

interface Feature {
  icon: Component;
  title: string;
  text: string;
}

const FEATURES: Feature[] = [
  {
    icon: Layers,
    title: "设备统一展示",
    text: "整合 2.4G 接收器与蓝牙设备，同一设备只出现一次。",
  },
  {
    icon: ShieldCheck,
    title: "可靠的状态提示",
    text: "数据过期、读取失败、设备离线时明确显示实际状态。",
  },
  {
    icon: PanelTop,
    title: "托盘驻留",
    text: "关闭窗口后应用继续运行，可从系统托盘重新打开。",
  },
];

/** 第一步设备面板为静态示意：数值写死、不接 `useSnapshot()`，引导阶段未必已采到数据，真实读数会误导。 */
interface DemoDevice {
  icon: Component;
  name: string;
  battery: number;
}

const DEMO_DEVICES: DemoDevice[] = [
  { icon: Mouse, name: "无线鼠标", battery: 82 },
  { icon: Headphones, name: "无线耳机", battery: 46 },
];

function finish() {
  emit("done", { autostart: autostart.value, startMinimized: startMinimized.value });
}
</script>

<template>
  <!-- 遮罩自标题栏下方起：引导期间标题栏仍可拖动/最小化/收进托盘。 -->
  <div
    class="fixed inset-x-0 top-10 bottom-0 z-40 flex overflow-y-auto bg-background px-4 py-4"
    role="dialog"
    aria-labelledby="onboarding-title"
  >
    <!-- 用 `m-auto` 而非 `items-center`：内容超窗高时仍可滚到顶，不被裁切。 -->
    <Card class="m-auto w-full max-w-[400px] gap-0 overflow-hidden py-0 shadow-lg">
      <CardHeader class="gap-0 px-6 pt-4 pb-4">
        <div class="flex items-center justify-between gap-3">
          <div class="flex min-w-0 items-center gap-2">
            <!-- 品牌标识复用标题栏方块徽标，强化「同一应用」连续性。 -->
            <span
              class="grid size-6 shrink-0 place-items-center rounded-md bg-primary text-primary-foreground"
            >
              <Zap :size="13" :stroke-width="2.5" aria-hidden="true" />
            </span>
            <span class="text-xs font-semibold tracking-tight">VoltNest</span>
            <span class="truncate text-[11px] text-muted-foreground">外设电量监控</span>
          </div>
          <span class="shrink-0 text-[11px] font-medium text-muted-foreground tabular-nums">
            {{ step + 1 }} / 2
          </span>
        </div>

        <Progress
          :model-value="progress"
          class="mt-3 h-1 bg-emerald-500/15 [&>div]:bg-emerald-500 [&>div]:motion-reduce:transition-none"
          :aria-label="`引导进度 ${step + 1} / 2`"
        />

        <CardTitle v-if="step === 0" id="onboarding-title" class="mt-3.5 text-lg leading-snug">
          让每一份电量，都尽在掌握
        </CardTitle>
        <CardTitle v-else id="onboarding-title" class="mt-3.5 text-lg leading-snug">
          按照你的习惯，设置启动方式
        </CardTitle>

        <p class="mt-1 text-[11px] leading-relaxed text-muted-foreground">
          <template v-if="step === 0"> 鼠标、耳机电量统一收在一处，读数过期也不冒充。 </template>
          <template v-else> 两项都可以先跳过，之后在设置页随时修改。 </template>
        </p>
      </CardHeader>

      <CardContent class="px-6 pb-4">
        <!-- 第一步：产品介绍 -->
        <section v-if="step === 0" class="flex flex-col gap-3.5">
          <!-- 示意面板：数值写死，非真实设备数据。 -->
          <div class="rounded-lg border bg-muted/40 p-3">
            <div class="flex items-center justify-between gap-2">
              <span class="text-[11px] font-medium">设备面板</span>
              <Badge
                variant="outline"
                class="bg-background px-1.5 py-0 text-[10px] font-normal text-muted-foreground"
              >
                界面示意
              </Badge>
            </div>

            <div class="mt-2.5 flex flex-col gap-2" aria-hidden="true">
              <div
                v-for="device in DEMO_DEVICES"
                :key="device.name"
                class="flex items-center gap-2.5"
              >
                <span
                  class="grid size-7 shrink-0 place-items-center rounded-md border bg-background text-muted-foreground"
                >
                  <component :is="device.icon" :size="14" aria-hidden="true" />
                </span>
                <div class="min-w-0 flex-1">
                  <div class="flex items-baseline justify-between gap-2">
                    <span class="truncate text-[11px] font-medium">{{ device.name }}</span>
                    <span
                      class="shrink-0 text-[11px] font-semibold text-emerald-700 tabular-nums dark:text-emerald-300"
                    >
                      {{ device.battery }}%
                    </span>
                  </div>
                  <Progress
                    :model-value="device.battery"
                    class="mt-1 h-1 bg-emerald-500/15 [&>div]:bg-emerald-500"
                  />
                </div>
              </div>
            </div>

            <p class="mt-2 text-[10px] leading-relaxed text-muted-foreground">
              以上为示意数据，非真实设备读数。
            </p>
          </div>

          <ul class="flex flex-col gap-2.5">
            <li v-for="feature in FEATURES" :key="feature.title" class="flex items-start gap-2.5">
              <span
                class="mt-px grid size-6 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground"
              >
                <component :is="feature.icon" :size="13" aria-hidden="true" />
              </span>
              <div class="min-w-0">
                <p class="text-xs leading-tight font-medium">{{ feature.title }}</p>
                <p class="mt-1 text-[11px] leading-relaxed text-muted-foreground">
                  {{ feature.text }}
                </p>
              </div>
            </li>
          </ul>
        </section>

        <!-- 第二步：启动设置（两项独立，默认关闭） -->
        <section v-else class="flex flex-col gap-2.5">
          <div
            class="flex items-start gap-3 rounded-lg border bg-muted/40 p-3"
            :class="busy ? 'opacity-60' : ''"
          >
            <span
              class="grid size-7 shrink-0 place-items-center rounded-md border bg-background text-muted-foreground"
            >
              <Power :size="14" aria-hidden="true" />
            </span>
            <Label
              for="onboarding-autostart"
              class="min-w-0 flex-1 cursor-pointer flex-col items-start gap-0"
            >
              <span class="text-xs leading-tight font-medium">开机时自动启动</span>
              <span
                id="onboarding-autostart-hint"
                class="mt-1 text-[11px] leading-relaxed font-normal text-muted-foreground"
              >
                登录 Windows 后自动运行；开启后会为当前用户注册启动项。
              </span>
            </Label>
            <Checkbox
              id="onboarding-autostart"
              class="mt-0.5"
              aria-label="开机时自动启动"
              aria-describedby="onboarding-autostart-hint"
              :disabled="busy"
              :model-value="autostart"
              @update:model-value="(value) => (autostart = value === true)"
            />
          </div>

          <div
            class="flex items-start gap-3 rounded-lg border bg-muted/40 p-3"
            :class="busy ? 'opacity-60' : ''"
          >
            <span
              class="grid size-7 shrink-0 place-items-center rounded-md border bg-background text-muted-foreground"
            >
              <PanelTop :size="14" aria-hidden="true" />
            </span>
            <Label
              for="onboarding-start-minimized"
              class="min-w-0 flex-1 cursor-pointer flex-col items-start gap-0"
            >
              <span class="text-xs leading-tight font-medium">启动时最小化到托盘</span>
              <span
                id="onboarding-start-minimized-hint"
                class="mt-1 text-[11px] leading-relaxed font-normal text-muted-foreground"
              >
                启动后不显示主窗口，只保留托盘图标。
              </span>
            </Label>
            <Checkbox
              id="onboarding-start-minimized"
              class="mt-0.5"
              aria-label="启动时最小化到托盘"
              aria-describedby="onboarding-start-minimized-hint"
              :disabled="busy"
              :model-value="startMinimized"
              @update:model-value="(value) => (startMinimized = value === true)"
            />
          </div>
        </section>
      </CardContent>

      <CardFooter class="px-6 pb-4">
        <div class="flex w-full flex-col gap-3 border-t pt-4">
          <Button v-if="step === 0" type="button" class="w-full" @click="step = 1"> 继续 </Button>
          <template v-else>
            <div class="flex w-full gap-2">
              <Button type="button" variant="outline" :disabled="busy" @click="step = 0">
                返回
              </Button>
              <Button
                type="button"
                class="flex-1"
                :disabled="busy"
                :aria-busy="busy"
                @click="finish"
              >
                <LoaderCircle
                  v-if="busy"
                  class="size-3.5 animate-spin motion-reduce:animate-none"
                  aria-hidden="true"
                />
                {{ busy ? "正在保存设置…" : "完成，开始使用" }}
              </Button>
            </div>
            <p class="text-[11px] leading-relaxed text-muted-foreground">
              两项设置之后都可以在「设置」页修改。
            </p>
          </template>
        </div>
      </CardFooter>
    </Card>
  </div>
</template>
