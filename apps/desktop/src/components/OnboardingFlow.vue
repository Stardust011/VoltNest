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
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
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
 *
 * 视觉上共用一个「品牌行 + 步骤进度 + 标题」的头部，两步只换标题与内容：
 * 第一步是产品介绍（设备面板示意 + 三条能力），第二步是两项启动设置。
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

/**
 * 第一步的设备面板：纯静态示意。
 *
 * 数值是写死的常量，不接 `useSnapshot()`、不读真实电量 —— 引导阶段宿主
 * 未必已经采到数据，拿真实读数当「好看的首屏」会变成误导。
 */
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
  <!-- 遮罩从自绘标题栏下方开始：引导期间标题栏仍可拖动 / 最小化 / 收进托盘。 -->
  <div
    class="bg-background fixed inset-x-0 bottom-0 top-10 z-40 flex overflow-y-auto px-4 py-4"
    role="dialog"
    aria-labelledby="onboarding-title"
  >
    <!-- `m-auto` 而非 `items-center`：内容高于窗口时仍能滚到顶部，不被裁切。 -->
    <Card class="m-auto w-full max-w-[400px] gap-0 overflow-hidden py-0 shadow-lg">
      <CardHeader class="gap-0 px-6 pt-4 pb-4">
        <div class="flex items-center justify-between gap-3">
          <div class="flex min-w-0 items-center gap-2">
            <!-- 品牌标识：与自绘标题栏同一个方块徽标，保证「同一个应用」的连续性。 -->
            <span
              class="bg-primary text-primary-foreground grid size-6 shrink-0 place-items-center rounded-md"
            >
              <Zap :size="13" :stroke-width="2.5" aria-hidden="true" />
            </span>
            <span class="text-xs font-semibold tracking-tight">VoltNest</span>
            <span class="text-muted-foreground truncate text-[11px]">外设电量监控</span>
          </div>
          <span class="text-muted-foreground shrink-0 text-[11px] font-medium tabular-nums">
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

        <p class="text-muted-foreground mt-1 text-[11px] leading-relaxed">
          <template v-if="step === 0">
            鼠标、耳机电量统一收在一处，读数过期也不冒充。
          </template>
          <template v-else>
            两项都可以先跳过，之后在设置页随时修改。
          </template>
        </p>
      </CardHeader>

      <CardContent class="px-6 pb-4">
        <!-- 第一步：产品介绍 -->
        <section v-if="step === 0" class="flex flex-col gap-3.5">
          <!-- 示意面板：数值写死，标注清楚「不是真实设备数据」。 -->
          <div class="bg-muted/40 rounded-lg border p-3">
            <div class="flex items-center justify-between gap-2">
              <span class="text-[11px] font-medium">设备面板</span>
              <Badge
                variant="outline"
                class="bg-background text-muted-foreground px-1.5 py-0 text-[10px] font-normal"
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
                  class="bg-background text-muted-foreground grid size-7 shrink-0 place-items-center rounded-md border"
                >
                  <component :is="device.icon" :size="14" aria-hidden="true" />
                </span>
                <div class="min-w-0 flex-1">
                  <div class="flex items-baseline justify-between gap-2">
                    <span class="truncate text-[11px] font-medium">{{ device.name }}</span>
                    <span
                      class="shrink-0 text-[11px] font-semibold tabular-nums text-emerald-700 dark:text-emerald-300"
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

            <p class="text-muted-foreground mt-2 text-[10px] leading-relaxed">
              以上为界面示意数据，不代表真实设备读数。
            </p>
          </div>

          <ul class="flex flex-col gap-2.5">
            <li
              v-for="feature in FEATURES"
              :key="feature.title"
              class="flex items-start gap-2.5"
            >
              <span
                class="bg-muted text-muted-foreground mt-px grid size-6 shrink-0 place-items-center rounded-md"
              >
                <component :is="feature.icon" :size="13" aria-hidden="true" />
              </span>
              <div class="min-w-0">
                <p class="text-xs font-medium leading-tight">{{ feature.title }}</p>
                <p class="text-muted-foreground mt-1 text-[11px] leading-relaxed">
                  {{ feature.text }}
                </p>
              </div>
            </li>
          </ul>
        </section>

        <!-- 第二步：启动设置（两项互相独立，默认关闭） -->
        <section v-else class="flex flex-col gap-2.5">
          <div
            class="bg-muted/40 flex items-start gap-3 rounded-lg border p-3"
            :class="busy ? 'opacity-60' : ''"
          >
            <span
              class="bg-background text-muted-foreground grid size-7 shrink-0 place-items-center rounded-md border"
            >
              <Power :size="14" aria-hidden="true" />
            </span>
            <Label
              for="onboarding-autostart"
              class="min-w-0 flex-1 cursor-pointer flex-col items-start gap-0"
            >
              <span class="text-xs font-medium leading-tight">开机时自动启动</span>
              <span
                id="onboarding-autostart-hint"
                class="text-muted-foreground mt-1 text-[11px] leading-relaxed font-normal"
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
            class="bg-muted/40 flex items-start gap-3 rounded-lg border p-3"
            :class="busy ? 'opacity-60' : ''"
          >
            <span
              class="bg-background text-muted-foreground grid size-7 shrink-0 place-items-center rounded-md border"
            >
              <PanelTop :size="14" aria-hidden="true" />
            </span>
            <Label
              for="onboarding-start-minimized"
              class="min-w-0 flex-1 cursor-pointer flex-col items-start gap-0"
            >
              <span class="text-xs font-medium leading-tight">启动时最小化到托盘</span>
              <span
                id="onboarding-start-minimized-hint"
                class="text-muted-foreground mt-1 text-[11px] leading-relaxed font-normal"
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
          <Button v-if="step === 0" type="button" class="w-full" @click="step = 1">
            继续
          </Button>
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
            <p class="text-muted-foreground text-[11px] leading-relaxed">
              两项设置之后都可以在「设置」页修改。
            </p>
          </template>
        </div>
      </CardFooter>
    </Card>
  </div>
</template>
