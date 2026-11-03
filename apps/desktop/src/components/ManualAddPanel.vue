<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import {
  ChevronLeft,
  CircleAlert,
  ClipboardCheck,
  Copy,
  LoaderCircle,
  Plug,
  Radio,
  RefreshCw,
  Save,
  Square,
  X,
} from "@lucide/vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  discardHidCapture,
  generateIssueReport,
  listHidTargets,
  saveManualDevice,
  startHidCapture,
  stopHidCapture,
  subscribeCapture,
} from "../lib/ipc";
import { copyText } from "../lib/clipboard";
import { formatDurationMs, parseHexFrames, shortHex } from "../lib/hex";
import type {
  CaptureEventDto,
  CaptureFrameDto,
  CaptureStatus,
  CaptureSummaryDto,
  HidCollectionDto,
  HidTargetDto,
  ManualDeviceDto,
  ReportDto,
} from "../lib/types";

/**
 * 生成上报数据（自动识别失败时的兜底）。挂在诊断页。
 *
 * 四步是硬顺序，每步都在缩小范围：
 * 1. **设备**：按 VID:PID 收敛（复合 HID 会同时暴露标准鼠标集合与厂商私有集合）；
 * 2. **集合**：选具体哪条 HID 通道，电量协议通常只在其中一条上；
 * 3. **采集**：填上驱动显示的电量再录，并且**允许录多段** —— 电量变化后的第二段
 *    才是定位电量字段的依据；
 * 4. **导出**：整理成可直接贴进 issue 的报告（宿主渲染，前端不拼文本）。
 *
 * 电量是「采样」的属性而不是设备的属性：同一台设备 80% 和 75% 各一段，两段的字节差异
 * 才有意义。因此采样列表里每条都带电量，报告里也按样本分节。
 */
const emit = defineEmits<{ (e: "close"): void; (e: "saved", device: ManualDeviceDto): void }>();

type Step = 1 | 2 | 3 | 4;

/** 一次已完成的采样（会话必须留到生成报告之后才能丢弃）。 */
interface CapturedSample {
  sessionId: number;
  battery: number | null;
  frames: number;
  summary: CaptureSummaryDto | null;
}

/** 录制时长选项：30 秒足够覆盖「拨一下滚轮 + 按一次键」。 */
const DURATIONS = [10_000, 30_000, 60_000, 180_000];

/** 界面上保留的最近帧数：够做肉眼确认，全量数据在报告里。 */
const PREVIEW_LIMIT = 40;

const step = ref<Step>(1);

const targets = ref<HidTargetDto[] | null>(null);
const targetsError = ref<string | null>(null);
const refreshing = ref(false);
const selectedTarget = ref<HidTargetDto | null>(null);
const selectedCollection = ref<HidCollectionDto | null>(null);

const durationMs = ref(30_000);
const queryText = ref("");
const queryIntervalMs = ref(1_000);
const batteryInput = ref("");

const status = ref<CaptureStatus | "idle">("idle");
const activeSessionId = ref<number | null>(null);
const total = ref(0);
const preview = ref<CaptureFrameDto[]>([]);
const summary = ref<CaptureSummaryDto | null>(null);
const captureError = ref<string | null>(null);
const samples = ref<CapturedSample[]>([]);
/** 本次录制的电量（开始时确定，结束时随帧一起入列）。 */
let recordingBattery: number | null = null;

const form = ref({ name: "", steps: "", expectation: "", note: "" });
const report = ref<ReportDto | null>(null);
const generating = ref(false);
const saving = ref(false);
const saved = ref<ManualDeviceDto | null>(null);
const busy = ref(false);
const copied = ref(false);
const error = ref<string | null>(null);

let unlisten: (() => void) | undefined;
let disposed = false;
let copiedTimer: number | undefined;

const stepTitles = ["设备", "集合", "采集", "导出"] as const;
const running = computed(() => status.value === "running");
const canRecord = computed(() => selectedCollection.value !== null && !busy.value);

const targetCountText = computed(() => {
  if (targets.value === null) return "正在枚举 HID 设备…";
  return `共 ${targets.value.length} 台设备`;
});

const collectionHint = computed(() => {
  const target = selectedTarget.value;
  if (!target) return null;
  if (target.privateCollections === 0) {
    return "这台设备只声明了标准 HID 集合（鼠标 / 键盘）。厂商电量协议通常挂在 0xFF00 以上的 UsagePage，若抓不到电量，请确认设备是否还有其它接口。";
  }
  return `这台设备有 ${target.privateCollections} 个厂商私有集合，建议优先选它们。`;
});

/** 抓包是否异常中断（用于把提示切成错误色）。 */
const interrupted = computed(() => status.value === "failed");

/** 中断原因后面必须给出下一步动作，否则用户只会重复点同一个集合。 */
function withSetSwitchHint(reason: string): string {
  const text = reason.trim().replace(/[。．.!！]+$/, "");
  return `${text || "录制中断"}。请切换其他集合后重试。`;
}

/** 录制结束后的统计提示：把「为什么没数据」直接说在脸上。 */
const finishHint = computed(() => {
  if (status.value === "failed") {
    return withSetSwitchHint(captureError.value ?? "录制中断");
  }
  const value = summary.value;
  if (!value) return null;
  if (value.frames === 0) {
    return "没有录到任何数据：设备可能不主动上报，请补一条查询报文后重录。";
  }
  if (value.truncated) {
    return "已达帧数上限，数据不完整；建议缩短录制时长或减少设备操作后重录。";
  }
  if (value.writeFailures > 0) {
    return `有 ${value.writeFailures} 条查询报文发送失败：${value.writeError ?? "设备不接受该报文"}`;
  }
  return null;
});

function fail(e: unknown) {
  error.value = e instanceof Error ? e.message : String(e);
}

async function refresh() {
  refreshing.value = true;
  targetsError.value = null;
  try {
    const list = await listHidTargets();
    targets.value = list;
    if (selectedTarget.value) {
      const fresh = list.find((item) => item.vidPid === selectedTarget.value?.vidPid);
      if (!fresh) {
        selectedTarget.value = null;
        selectedCollection.value = null;
      } else {
        selectedTarget.value = fresh;
        if (selectedCollection.value) {
          const stillThere = fresh.collections.find(
            (item) => item.path === selectedCollection.value?.path,
          );
          selectedCollection.value = stillThere ?? null;
        }
      }
    }
  } catch (e) {
    targets.value = [];
    targetsError.value = e instanceof Error ? e.message : String(e);
  } finally {
    refreshing.value = false;
  }
}

function chooseTarget(target: HidTargetDto) {
  selectedTarget.value = target;
  selectedCollection.value = target.collections.length === 1 ? target.collections[0]! : null;
  error.value = null;
  step.value = 2;
}

/**
 * 路径预览：保留头尾（Windows 的 vid/pid 在头部，Linux 的 hidraw 节点在尾部）。
 * 直接用 mid-ellipsis 而不是 CSS truncate：用户需要能一眼核对是哪条通道。
 */
function shortPath(path: string, head = 26, tail = 10): string {
  if (path.length <= head + tail + 1) return path;
  return `${path.slice(0, head)}…${path.slice(-tail)}`;
}

function chooseCollection(collection: HidCollectionDto) {
  selectedCollection.value = collection;
  if (!form.value.name.trim()) form.value.name = selectedTarget.value?.displayName ?? "";
}

/**
 * 电量输入 → 数值 / `null`（驱动未显示）/ `"invalid"`（越界或非数字）。
 *
 * 留空即视为「驱动没显示」：不再用复选框表达同一件事，少一个必答项。
 */
function resolveBattery(): number | null | "invalid" {
  const raw = batteryInput.value.trim();
  if (raw === "") return null;
  const value = Number(raw);
  if (!Number.isFinite(value) || value < 0 || value > 100) return "invalid";
  return Math.round(value);
}

/**
 * 会话号尚未回到前端时先缓存事件。
 *
 * 打开设备就可能立刻失败（被官方驱动独占），失败事件会先于 `start_hid_capture` 的响应
 * 抵达；直接按 id 过滤会把它丢掉，界面就会停在「正在采集」不动。
 */
let pendingCaptureEvents: CaptureEventDto[] = [];

function onCaptureEvent(event: CaptureEventDto) {
  if (activeSessionId.value === null) {
    pendingCaptureEvents.push(event);
    if (pendingCaptureEvents.length > 8) pendingCaptureEvents.shift();
    return;
  }
  applyCaptureEvent(event);
}

function applyCaptureEvent(event: CaptureEventDto) {
  if (event.sessionId !== activeSessionId.value) return;
  total.value = event.total;
  if (event.frames.length > 0) {
    preview.value = [...preview.value, ...event.frames].slice(-PREVIEW_LIMIT);
  }
  if (event.status === "running") return;

  status.value = event.status;
  summary.value = event.summary;
  if (event.message) captureError.value = event.message;
  if (event.total > 0) {
    samples.value = [
      ...samples.value,
      {
        sessionId: event.sessionId,
        battery: recordingBattery,
        frames: event.total,
        summary: event.summary,
      },
    ];
    activeSessionId.value = null;
    status.value = "idle";
    batteryInput.value = "";
  }
}

async function startRecording() {
  const collection = selectedCollection.value;
  if (!collection) return;
  const battery = resolveBattery();
  if (battery === "invalid") {
    captureError.value = "电量请填 0–100 之间的整数；驱动没有显示电量的，留空即可";
    return;
  }
  const parsed = parseHexFrames(queryText.value);
  if (parsed.error) {
    captureError.value = parsed.error;
    return;
  }

  busy.value = true;
  captureError.value = null;
  error.value = null;
  report.value = null;
  saved.value = null;
  preview.value = [];
  total.value = 0;
  summary.value = null;
  pendingCaptureEvents = [];
  activeSessionId.value = null;
  recordingBattery = battery;
  try {
    const started = await startHidCapture({
      path: collection.path,
      durationMs: durationMs.value,
      writeFrames: parsed.frames,
      writeIntervalMs: queryIntervalMs.value,
    });
    activeSessionId.value = started.sessionId;
    status.value = "running";
    for (const event of pendingCaptureEvents.splice(0)) {
      applyCaptureEvent(event);
    }
  } catch (e) {
    status.value = "idle";
    recordingBattery = null;
    fail(e);
  } finally {
    busy.value = false;
  }
}

async function stopRecording() {
  const id = activeSessionId.value;
  if (id === null) return;
  try {
    await stopHidCapture(id);
  } catch (e) {
    fail(e);
  }
}

/** 丢掉某条采样：会话释放，列表里移除。 */
async function removeSample(sample: CapturedSample) {
  samples.value = samples.value.filter((item) => item.sessionId !== sample.sessionId);
  try {
    await discardHidCapture(sample.sessionId);
  } catch {}
}

/** 丢弃所有仍被占用的会话（关面板 / 重来）。 */
async function discardSessions() {
  const ids = [
    ...(activeSessionId.value === null ? [] : [activeSessionId.value]),
    ...samples.value.map((sample) => sample.sessionId),
  ];
  activeSessionId.value = null;
  samples.value = [];
  pendingCaptureEvents = [];
  for (const id of ids) {
    try {
      await discardHidCapture(id);
    } catch {}
  }
}

function backToCollections() {
  if (running.value) return;
  step.value = 2;
}

function backToTargets() {
  if (running.value) return;
  step.value = 1;
}

/** 默认操作步骤：把「用户到底做了什么」拆成可填写的位置，避免一份什么都没说的报告。 */
function defaultSteps(): string {
  const name = form.value.name.trim() || selectedTarget.value?.displayName || "这台设备";
  const battery = samples.value
    .map((sample) => (sample.battery === null ? "未显示" : `${sample.battery}%`))
    .join(" → ");
  return [
    "1. 打开 VoltNest → 诊断 → 生成上报数据",
    `2. 选择「${name}」的 HID 集合，录制 ${formatDurationMs(durationMs.value)}`,
    `3. 采样时的驱动显示电量：${battery || "<填写电量>"}`,
    "4. 录制期间：<补充你做了什么，例如拨动滚轮 / 按一下按键 / 插拔数据线>",
    "5. 连接方式：<2.4G / 蓝牙 / 有线>",
  ].join("\n");
}

async function openReport() {
  if (samples.value.length === 0) return;
  step.value = 4;
  if (!form.value.steps.trim()) form.value.steps = defaultSteps();
  await regenerate();
}

function reportForm() {
  return {
    samples: samples.value.map((sample) => ({
      sessionId: sample.sessionId,
      batteryPercent: sample.battery,
    })),
    ...form.value,
  };
}

async function regenerate() {
  if (samples.value.length === 0) return;
  generating.value = true;
  error.value = null;
  try {
    report.value = await generateIssueReport(reportForm());
  } catch (e) {
    report.value = null;
    fail(e);
  } finally {
    generating.value = false;
  }
}

async function persist() {
  if (samples.value.length === 0) return;
  saving.value = true;
  error.value = null;
  try {
    const device = await saveManualDevice(reportForm());
    saved.value = device;
    emit("saved", device);
  } catch (e) {
    fail(e);
  } finally {
    saving.value = false;
  }
}

/** 复制报告正文。失败时提示手动全选，不假装成功。 */
async function copyReport() {
  const text = report.value?.markdown;
  if (!text) return;
  if (await copyText(text)) {
    markCopied();
  } else {
    error.value = "复制失败：请在下方报告预览里手动全选复制。";
  }
}

function markCopied() {
  copied.value = true;
  if (copiedTimer !== undefined) window.clearTimeout(copiedTimer);
  copiedTimer = window.setTimeout(() => (copied.value = false), 2_000);
}

async function close() {
  await discardSessions();
  emit("close");
}

onMounted(async () => {
  try {
    const off = await subscribeCapture(onCaptureEvent);
    if (disposed) {
      off();
      return;
    }
    unlisten = off;
  } catch (e) {
    fail(e);
  }
  await refresh();
});

onBeforeUnmount(() => {
  disposed = true;
  unlisten?.();
  if (copiedTimer !== undefined) window.clearTimeout(copiedTimer);
  void discardSessions();
});
</script>

<template>
  <div
    class="fixed inset-x-0 top-10 bottom-0 z-40 flex overflow-y-auto bg-background/95 px-3 py-3 backdrop-blur-[2px]"
    role="dialog"
    aria-labelledby="manual-add-title"
  >
    <Card class="m-auto w-full max-w-[460px] gap-0 overflow-hidden py-0 shadow-lg">
      <CardHeader class="gap-0 border-b px-4 py-3">
        <div class="flex items-center justify-between gap-2">
          <CardTitle id="manual-add-title" class="text-sm font-semibold">生成上报数据</CardTitle>
          <Button variant="ghost" size="sm" type="button" class="h-7 px-2" @click="close">
            <X aria-hidden="true" />
            关闭
          </Button>
        </div>

        <ol class="mt-2.5 flex items-center gap-1.5 text-[11px]">
          <li
            v-for="(title, index) in stepTitles"
            :key="title"
            class="flex min-w-0 flex-1 items-center gap-1.5"
          >
            <span
              class="flex size-4 shrink-0 items-center justify-center rounded-full border text-[10px] tabular-nums"
              :class="
                step === index + 1
                  ? 'border-primary bg-primary text-primary-foreground'
                  : step > index + 1
                    ? 'border-emerald-500/60 text-emerald-600 dark:text-emerald-400'
                    : 'text-muted-foreground'
              "
            >
              {{ index + 1 }}
            </span>
            <span
              class="truncate"
              :class="step === index + 1 ? 'font-medium' : 'text-muted-foreground'"
            >
              {{ title }}
            </span>
          </li>
        </ol>
      </CardHeader>

      <CardContent class="px-4 py-3">
        <!-- 第一步：按 VID:PID 选设备 -->
        <section v-if="step === 1" class="flex flex-col gap-3">
          <div class="flex items-center justify-between gap-2">
            <span class="text-[11px] text-muted-foreground">{{ targetCountText }}</span>
            <Button
              variant="outline"
              size="sm"
              type="button"
              class="h-7 px-2"
              :disabled="refreshing"
              @click="refresh"
            >
              <LoaderCircle v-if="refreshing" class="animate-spin" aria-hidden="true" />
              <RefreshCw v-else aria-hidden="true" />
              刷新
            </Button>
          </div>

          <div
            v-if="targetsError"
            class="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-xs text-destructive"
          >
            <p class="font-medium">枚举 HID 设备失败</p>
            <p class="mt-1 break-all whitespace-pre-wrap select-text">{{ targetsError }}</p>
          </div>

          <p v-else-if="targets === null" class="py-8 text-center text-xs text-muted-foreground">
            正在枚举…
          </p>

          <div
            v-else-if="targets.length === 0"
            class="rounded-lg border border-dashed px-3 py-6 text-center text-xs text-muted-foreground"
          >
            <Plug class="mx-auto mb-2 size-5 opacity-60" aria-hidden="true" />
            没有检测到 HID 设备。插好 2.4G 接收器 / 用数据线连接设备后点「刷新」。
          </div>

          <ul v-else class="flex max-h-[46vh] flex-col gap-1.5 overflow-y-auto pr-0.5">
            <li v-for="target in targets" :key="target.vidPid">
              <button
                type="button"
                class="w-full rounded-lg border bg-card/50 px-3 py-2 text-left transition-colors hover:border-primary/50 hover:bg-accent/40 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
                @click="chooseTarget(target)"
              >
                <div class="flex items-center justify-between gap-2">
                  <span class="truncate text-xs font-medium">{{ target.displayName }}</span>
                  <Badge
                    variant="outline"
                    class="shrink-0 px-1.5 py-0 text-[10px] font-normal"
                    :class="
                      target.privateCollections > 0
                        ? 'border-emerald-500/40 text-emerald-600 dark:text-emerald-400'
                        : 'text-muted-foreground'
                    "
                  >
                    {{ target.collections.length }} 个集合
                  </Badge>
                </div>
                <div class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px]">
                  <span class="font-mono text-muted-foreground">{{ target.vidPid }}</span>
                  <span v-if="target.manufacturer" class="text-muted-foreground">
                    {{ target.manufacturer }}
                  </span>
                </div>
                <p
                  v-if="target.serialSummary"
                  class="mt-0.5 truncate text-[10px] text-muted-foreground"
                >
                  SN {{ target.serialSummary }}
                </p>
              </button>
            </li>
          </ul>

          <p class="rounded-lg bg-muted/60 p-2.5 text-[11px] leading-relaxed text-muted-foreground">
            看不到你的设备？① 确认接收器插好、设备已开机；② 换成数据线连接再试一次； ③
            关闭厂商官方驱动后刷新（它可能独占设备）。
          </p>
        </section>

        <!-- 第二步：选集合 -->
        <section v-else-if="step === 2" class="flex flex-col gap-3">
          <div class="rounded-lg border bg-muted/40 p-2.5">
            <div class="flex items-center justify-between gap-2">
              <span class="truncate text-xs font-medium">{{ selectedTarget?.displayName }}</span>
              <Button
                variant="ghost"
                size="sm"
                type="button"
                class="h-6 shrink-0 px-1.5 text-[11px]"
                @click="backToTargets"
              >
                <ChevronLeft aria-hidden="true" />
                换设备
              </Button>
            </div>
            <p class="mt-0.5 font-mono text-[11px] break-all text-muted-foreground">
              {{ selectedTarget?.vidPid }}
            </p>
          </div>

          <p v-if="collectionHint" class="text-[11px] leading-relaxed text-muted-foreground">
            {{ collectionHint }}
          </p>

          <ul class="flex max-h-[42vh] flex-col gap-1.5 overflow-y-auto pr-0.5">
            <li v-for="collection in selectedTarget?.collections ?? []" :key="collection.path">
              <button
                type="button"
                class="w-full rounded-lg border px-3 py-2 text-left transition-colors focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
                :class="
                  selectedCollection?.path === collection.path
                    ? 'border-primary/60 bg-accent/60'
                    : 'bg-card/50 hover:border-primary/40 hover:bg-accent/30'
                "
                @click="chooseCollection(collection)"
              >
                <div class="flex items-center justify-between gap-2">
                  <span class="truncate font-mono text-[11px]">{{ collection.usageSummary }}</span>
                  <Badge
                    variant="outline"
                    class="shrink-0 px-1.5 py-0 text-[10px] font-normal"
                    :class="
                      collection.standardOnly
                        ? 'text-muted-foreground'
                        : 'border-emerald-500/40 text-emerald-600 dark:text-emerald-400'
                    "
                  >
                    {{ collection.standardOnly ? "标准集合" : "私有集合" }}
                  </Badge>
                </div>
                <div
                  class="mt-1 flex flex-wrap items-center gap-x-2 text-[10px] text-muted-foreground"
                >
                  <span>接口 {{ collection.interfaceNumber }}</span>
                  <span class="font-mono break-all">{{ shortPath(collection.path) }}</span>
                </div>
              </button>
            </li>
          </ul>

          <div class="flex justify-end">
            <Button type="button" size="sm" :disabled="!selectedCollection" @click="step = 3">
              下一步：采集
            </Button>
          </div>
        </section>

        <!-- 第三步：采集 -->
        <section v-else-if="step === 3" class="flex flex-col gap-3">
          <div class="rounded-lg border bg-muted/40 p-2.5">
            <div class="flex items-center justify-between gap-2">
              <span class="truncate text-xs font-medium">
                {{ selectedTarget?.displayName }} · {{ selectedCollection?.usageSummary }}
              </span>
              <Button
                variant="ghost"
                size="sm"
                type="button"
                class="h-6 shrink-0 px-1.5 text-[11px]"
                :disabled="running || busy"
                @click="backToCollections"
              >
                <ChevronLeft aria-hidden="true" />
                换集合
              </Button>
            </div>
            <p class="mt-0.5 font-mono text-[10px] break-all text-muted-foreground">
              {{ selectedCollection?.path }}
            </p>
          </div>

          <div class="flex flex-col gap-1.5">
            <Label for="manual-battery" class="text-[11px] font-medium">
              现在驱动 / 系统显示的电量
            </Label>
            <div class="flex items-center gap-2">
              <Input
                id="manual-battery"
                v-model="batteryInput"
                type="number"
                min="0"
                max="100"
                step="1"
                :disabled="running"
                class="h-8 w-24 text-xs tabular-nums"
                placeholder="80"
              />
              <span class="text-xs text-muted-foreground">%</span>
            </div>
            <p class="text-[10px] leading-relaxed text-muted-foreground">
              填的是官方驱动（或系统设置）里读到的值，用来和原始字节对照；驱动没有显示就留空。
            </p>
          </div>

          <div class="flex flex-col gap-1.5">
            <Label class="text-[11px] font-medium">录制时长</Label>
            <div class="flex gap-1.5">
              <Button
                v-for="value in DURATIONS"
                :key="value"
                type="button"
                size="sm"
                class="h-7 flex-1 px-0 text-[11px]"
                :variant="durationMs === value ? 'secondary' : 'outline'"
                :disabled="running"
                @click="durationMs = value"
              >
                {{ formatDurationMs(value) }}
              </Button>
            </div>
          </div>

          <div class="flex flex-col gap-1.5">
            <Label for="manual-query" class="text-[11px] font-medium">
              查询报文（可选，十六进制）
            </Label>
            <textarea
              id="manual-query"
              v-model="queryText"
              :disabled="running"
              spellcheck="false"
              rows="2"
              placeholder="留空 = 只监听。已知查询命令时每行一条，例如 06 FB 00 00 00 03"
              class="w-full resize-y rounded-md border border-input bg-transparent px-2.5 py-1.5 font-mono text-[11px] shadow-xs outline-none placeholder:font-sans placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:opacity-60 dark:bg-input/30"
            />
            <div class="flex items-center justify-between gap-2 text-[11px] text-muted-foreground">
              <span>发送间隔</span>
              <div class="flex items-center gap-1">
                <Input
                  v-model.number="queryIntervalMs"
                  type="number"
                  min="200"
                  max="10000"
                  step="100"
                  :disabled="running"
                  class="h-6 w-20 px-2 text-right text-[11px] tabular-nums"
                />
                <span>ms</span>
              </div>
            </div>
          </div>

          <div class="flex gap-1.5">
            <Button
              v-if="!running"
              type="button"
              size="sm"
              class="flex-1"
              :disabled="!canRecord"
              @click="startRecording"
            >
              <LoaderCircle v-if="busy" class="animate-spin" aria-hidden="true" />
              <Radio v-else aria-hidden="true" />
              {{ samples.length > 0 ? "再采一段" : "开始采集" }}
            </Button>
            <Button
              v-else
              type="button"
              size="sm"
              variant="outline"
              class="flex-1"
              @click="stopRecording"
            >
              <Square aria-hidden="true" />
              停止采集
            </Button>
          </div>

          <div v-if="running || total > 0" class="rounded-lg border bg-card/50 p-2.5">
            <div class="flex items-center justify-between gap-2">
              <span class="flex items-center gap-1.5 text-[11px] font-medium">
                <span
                  class="size-1.5 rounded-full"
                  :class="running ? 'animate-pulse bg-emerald-500' : 'bg-muted-foreground/50'"
                />
                {{ running ? "正在采集" : "本次采集结束" }}
              </span>
              <span class="text-[11px] text-muted-foreground tabular-nums">{{ total }} 帧</span>
            </div>
            <p class="mt-1 text-[11px] leading-relaxed text-muted-foreground">
              采集期间请：拨一下滚轮、按一次按键保持设备不在休眠状态，或插拔数据线触发设备上报电量。
            </p>
            <ul
              v-if="preview.length > 0"
              class="mt-1.5 max-h-28 overflow-y-auto rounded-md bg-muted/50 p-1.5 font-mono text-[10px] leading-4"
            >
              <li
                v-for="frame in preview"
                :key="`${frame.atMs}-${frame.hex}-${frame.direction}`"
                class="flex gap-1.5 whitespace-nowrap"
              >
                <span class="w-11 shrink-0 text-right text-muted-foreground tabular-nums">
                  {{ frame.atMs }}
                </span>
                <span
                  class="w-6 shrink-0 font-semibold"
                  :class="
                    frame.direction === 'IN'
                      ? 'text-emerald-600 dark:text-emerald-400'
                      : 'text-amber-600 dark:text-amber-400'
                  "
                >
                  {{ frame.direction }}
                </span>
                <span class="truncate">{{ shortHex(frame.hex, 16) }}</span>
              </li>
            </ul>
          </div>

          <div
            v-if="finishHint"
            class="rounded-lg border p-2.5 text-[11px] leading-relaxed"
            :class="
              interrupted
                ? 'border-destructive/30 bg-destructive/5 text-destructive'
                : 'border-amber-500/30 bg-amber-500/5 text-amber-700 dark:text-amber-400'
            "
          >
            {{ finishHint }}
          </div>

          <div v-if="samples.length > 0" class="flex flex-col gap-1.5">
            <div class="flex items-center justify-between gap-1">
              <span class="text-[11px] font-medium">已采集 {{ samples.length }} 段</span>
              <span class="truncate text-[10px] text-muted-foreground">
                电量顺序：{{
                  samples
                    .map((sample) => (sample.battery === null ? "未显示" : `${sample.battery}%`))
                    .join(" → ")
                }}
              </span>
            </div>
            <ul class="flex flex-col gap-1">
              <li
                v-for="(sample, index) in samples"
                :key="sample.sessionId"
                class="flex items-center justify-between gap-2 rounded-md border bg-card/50 px-2.5 py-1.5"
              >
                <span class="min-w-0 truncate text-[11px]">
                  样本 {{ index + 1 }} ·
                  <span class="font-medium">
                    {{ sample.battery === null ? "驱动未显示" : `${sample.battery}%` }}
                  </span>
                  · {{ sample.frames }} 帧
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  class="h-6 shrink-0 px-1.5 text-[10px] text-muted-foreground hover:text-destructive"
                  :disabled="running"
                  @click="removeSample(sample)"
                >
                  移除
                </Button>
              </li>
            </ul>
            <p
              v-if="samples.length === 1"
              class="rounded-lg border border-emerald-500/30 bg-emerald-500/5 p-2.5 text-[11px] leading-relaxed text-emerald-700 dark:text-emerald-400"
            >
              再等电量变化一次（比如掉
              1–5%），然后填上新电量、再采一段：两段对照才能看出哪个字节是电量。
            </p>
          </div>

          <div class="flex justify-end">
            <Button
              type="button"
              size="sm"
              :disabled="samples.length === 0 || running"
              @click="openReport"
            >
              下一步：导出报告
            </Button>
          </div>
        </section>

        <!-- 第四步：导出 -->
        <section v-else class="flex flex-col gap-3">
          <div class="flex flex-col gap-1.5">
            <Label for="manual-name" class="text-[11px] font-medium">设备型号</Label>
            <Input
              id="manual-name"
              v-model="form.name"
              class="h-8 text-xs"
              placeholder="例如 AJ159 NL"
            />
          </div>

          <div class="flex flex-col gap-1.5">
            <Label for="manual-steps" class="text-[11px] font-medium">操作步骤</Label>
            <textarea
              id="manual-steps"
              v-model="form.steps"
              rows="5"
              spellcheck="false"
              class="w-full resize-y rounded-md border border-input bg-transparent px-2.5 py-1.5 text-[11px] leading-relaxed shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 dark:bg-input/30"
            />
          </div>

          <div class="flex flex-col gap-1.5">
            <Label for="manual-expect" class="text-[11px] font-medium">期望行为</Label>
            <textarea
              id="manual-expect"
              v-model="form.expectation"
              rows="2"
              class="w-full resize-y rounded-md border border-input bg-transparent px-2.5 py-1.5 text-[11px] leading-relaxed shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 dark:bg-input/30"
              placeholder="设备出现在列表里，并显示电量。"
            />
          </div>

          <div class="flex flex-col gap-1.5">
            <Label for="manual-note" class="text-[11px] font-medium">补充说明（可选）</Label>
            <textarea
              id="manual-note"
              v-model="form.note"
              rows="2"
              class="w-full resize-y rounded-md border border-input bg-transparent px-2.5 py-1.5 text-[11px] leading-relaxed shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 dark:bg-input/30"
              placeholder="例如：2.4G 下不显示电量，有线正常。"
            />
          </div>

          <div class="flex flex-wrap gap-1.5">
            <Button
              type="button"
              size="sm"
              variant="outline"
              :disabled="generating"
              @click="regenerate"
            >
              <LoaderCircle v-if="generating" class="animate-spin" aria-hidden="true" />
              重新生成
            </Button>
            <Button type="button" size="sm" :disabled="!report" @click="copyReport">
              <ClipboardCheck v-if="copied" aria-hidden="true" />
              <Copy v-else aria-hidden="true" />
              {{ copied ? "已复制" : "复制报告" }}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="secondary"
              :disabled="saving || !!saved"
              @click="persist"
            >
              <LoaderCircle v-if="saving" class="animate-spin" aria-hidden="true" />
              <Save v-else aria-hidden="true" />
              {{ saved ? "已保存" : "保存到本地" }}
            </Button>
          </div>

          <div v-if="report" class="rounded-lg border bg-card/50 p-2.5">
            <p class="text-[11px] font-medium">报告预览</p>
            <p class="mt-0.5 text-[10px] leading-relaxed text-muted-foreground">
              点「复制报告」可直接贴进
              issue，此时不写任何文件；需要留档再点「保存到本地」，报告才会写到数据目录。
            </p>
            <pre
              class="mt-2 max-h-40 overflow-auto rounded-md bg-muted/60 p-2 font-mono text-[10px] leading-4 whitespace-pre-wrap select-text"
              >{{ report.markdown }}</pre>
          </div>

          <div v-if="saved" class="flex flex-col gap-1">
            <p class="text-[11px] text-emerald-600 dark:text-emerald-400">
              已保存「{{
                saved.name
              }}」：报告已写入本地，诊断页的「生成上报数据」列表里可以随时取回。
            </p>
            <p
              v-if="saved.reportPath"
              class="font-mono text-[10px] break-all text-muted-foreground select-text"
            >
              {{ saved.reportPath }}
            </p>
          </div>

          <div class="flex justify-end">
            <Button type="button" size="sm" variant="ghost" @click="close">完成</Button>
          </div>
        </section>

        <div
          v-if="error"
          class="mt-3 flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-2.5 text-[11px] text-destructive"
        >
          <CircleAlert class="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
          <span class="min-w-0 flex-1 break-all whitespace-pre-wrap select-text">{{ error }}</span>
        </div>
      </CardContent>
    </Card>
  </div>
</template>
