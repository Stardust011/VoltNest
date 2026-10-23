<script setup lang="ts">
import { RotateCcw } from "@lucide/vue";
import { computed, ref } from "vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { humanizeAgo } from "../lib/time";
import { visibleDiagEvents } from "../lib/format";
import type { DiagnosticsDto } from "../lib/types";

const props = defineProps<{
  diagnostics: DiagnosticsDto;
  nowMs: number;
}>();

const emit = defineEmits<{
  (e: "restart", transport: "hid" | "ble"): void;
  /** 调试：重新唤起首次引导遮罩（是否落盘由宿主决定）。 */
  (e: "replayOnboarding"): void;
}>();

const levelClass: Record<string, string> = {
  error: "text-destructive font-medium",
  warn: "text-amber-600 dark:text-amber-400 font-medium",
  info: "text-muted-foreground",
  debug: "text-muted-foreground/70",
};

const levelLabel: Record<string, string> = {
  error: "错误",
  warn: "警告",
  info: "信息",
  debug: "调试",
};

/** `debug` 默认折叠（逐条细节，展开才看）；纯前端状态不落盘，开关置于底部「调试区」以免混入正常诊断。 */
const showDebug = ref(false);
const visibleEvents = computed(() => visibleDiagEvents(props.diagnostics.events, showDebug.value));

/** `debug` 级记录总数，只数原始列表、不随开关变化：若按「总数-可见数」算，展开后差值归零会让开关自消失。 */
const debugCount = computed(
  () => props.diagnostics.events.filter((event) => event.level === "debug").length,
);

/** 折叠时须在标题处点明「还有多少条 debug 未展开」，否则列表看起来像缺了记录（开关已移到调试区）。 */
const eventCountText = computed(() => {
  const shown = visibleEvents.value.length;
  return !showDebug.value && debugCount.value > 0
    ? `${shown} 条事件 · 另 ${debugCount.value} 条调试未展开`
    : `${shown} 条事件`;
});

/** 调试区开关说明（无 debug 记录时说明拨了也无效）。 */
const debugHint = computed(() =>
  debugCount.value > 0
    ? `「诊断事件」中展开 ${debugCount.value} 条 debug 级记录（逐条枚举，噪声大）。`
    : "当前无 debug 级记录。",
);

const statusLabel: Record<string, string> = {
  running: "运行中",
  down: "已停止",
  unavailable: "采集不可用",
};

function ago(atMs: number) {
  return humanizeAgo(Math.max(0, props.nowMs - atMs));
}

function transportName(transport: string) {
  return transport === "hid" ? "2.4G" : "蓝牙";
}

function familyLabel(family: string) {
  return family;
}

function statusVariant(status: string) {
  return status === "running" ? "secondary" : "outline";
}

function statusClass(status: string) {
  return status === "running"
    ? "font-normal"
    : "border-destructive/40 text-destructive font-normal";
}

/** 坏帧 detail 为「成因+原始字节」，属机器数据，用等宽字体对齐以便逐位核对。 */
function detailClass(code: string) {
  return code === "reading_unreadable" ? "font-mono text-[11px]" : "";
}
</script>

<template>
  <div class="flex w-full min-w-0 flex-col gap-4 pb-4">
    <!-- 采集线程 -->
    <Card class="min-w-0 py-4">
      <CardHeader class="px-4 pb-0">
        <div class="flex items-center justify-between gap-3">
          <CardTitle class="text-sm font-semibold">采集线程组</CardTitle>
          <span class="text-xs text-muted-foreground">
            {{ diagnostics.transports.length }} 个线程
          </span>
        </div>
      </CardHeader>

      <CardContent class="px-4">
        <div
          v-if="diagnostics.transports.length === 0"
          class="rounded-lg border border-dashed px-3 py-6 text-center text-xs text-muted-foreground"
        >
          暂无采集线程
        </div>

        <ul v-else class="flex flex-col gap-3">
          <li
            v-for="t in diagnostics.transports"
            :key="t.transport"
            class="min-w-0 rounded-lg border bg-card/50 p-3"
          >
            <div class="flex min-w-0 items-start justify-between gap-3">
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <span class="text-sm font-semibold">
                    {{ transportName(t.transport) }}
                  </span>
                  <span class="font-mono text-[11px] break-all text-muted-foreground">
                    {{ familyLabel(t.family) }}
                  </span>
                </div>

                <div class="mt-2 flex flex-wrap items-center gap-2">
                  <Badge :variant="statusVariant(t.status)" :class="statusClass(t.status)">
                    {{ statusLabel[t.status] ?? t.status }}
                  </Badge>
                  <span class="text-xs text-muted-foreground"> 重启 {{ t.restarts }} 次 </span>
                  <span class="text-xs text-muted-foreground"> 队列 {{ t.queuedEvents }} </span>
                </div>
              </div>

              <Button
                type="button"
                variant="outline"
                size="sm"
                class="shrink-0"
                @click="emit('restart', t.transport)"
              >
                重启
              </Button>
            </div>

            <div
              v-if="t.lastError"
              class="mt-3 rounded-md border border-destructive/20 bg-destructive/5 p-2.5"
            >
              <p class="mb-1 text-[11px] font-medium text-destructive">最近错误</p>
              <p
                class="font-mono text-xs break-all whitespace-pre-wrap text-destructive select-text"
              >
                {{ t.lastError }}
              </p>
            </div>
          </li>
        </ul>
      </CardContent>
    </Card>

    <!-- 运行环境 -->
    <Card class="min-w-0 py-4">
      <CardHeader class="px-4 pb-0">
        <CardTitle class="text-sm font-semibold">运行环境</CardTitle>
      </CardHeader>

      <CardContent class="px-4">
        <dl class="flex flex-col divide-y divide-border/50 text-xs">
          <div class="grid grid-cols-[5rem_minmax(0,1fr)] gap-3 py-2 first:pt-0 last:pb-0">
            <dt class="text-muted-foreground">版本</dt>
            <dd class="font-mono break-all select-text">
              {{ diagnostics.version }}
            </dd>
          </div>

          <div class="grid grid-cols-[5rem_minmax(0,1fr)] gap-3 py-2">
            <dt class="text-muted-foreground">数据目录</dt>
            <dd class="font-mono break-all select-text">
              {{ diagnostics.dataRoot }}
            </dd>
          </div>

          <div class="grid grid-cols-[5rem_minmax(0,1fr)] gap-3 py-2 last:pb-0">
            <dt class="text-muted-foreground">开机自启</dt>
            <dd>
              <Badge
                :variant="diagnostics.autostartRegistered ? 'secondary' : 'outline'"
                :class="
                  diagnostics.autostartRegistered
                    ? 'font-normal'
                    : 'font-normal text-muted-foreground'
                "
              >
                {{ diagnostics.autostartRegistered ? "已注册" : "未注册" }}
              </Badge>
            </dd>
          </div>
        </dl>
      </CardContent>
    </Card>

    <!-- 通知触发节点 -->
    <Card class="min-w-0 py-4">
      <CardHeader class="px-4 pb-0">
        <div class="flex items-center justify-between gap-3">
          <CardTitle class="text-sm font-semibold">通知触发节点</CardTitle>
          <span class="text-xs text-muted-foreground">
            {{ diagnostics.notifications.length }} 条记录
          </span>
        </div>
      </CardHeader>

      <CardContent class="px-4">
        <div
          v-if="diagnostics.notifications.length === 0"
          class="rounded-lg border border-dashed px-3 py-6 text-center text-xs text-muted-foreground"
        >
          暂无通知记录
        </div>

        <ul v-else class="flex flex-col gap-2">
          <li
            v-for="n in diagnostics.notifications"
            :key="`${n.stableId}-${n.threshold}`"
            class="min-w-0 rounded-lg border bg-card/50 p-3"
          >
            <div class="flex min-w-0 items-start justify-between gap-3">
              <span class="min-w-0 flex-1 font-mono text-xs break-all select-text">
                {{ n.stableId }}
              </span>

              <div class="flex shrink-0 items-center gap-2">
                <span class="font-mono text-sm font-semibold tabular-nums">
                  {{ n.threshold }}%
                </span>
                <Badge
                  :variant="n.armed ? 'secondary' : 'outline'"
                  :class="
                    n.armed
                      ? 'font-normal'
                      : 'border-amber-500/40 font-normal text-amber-600 dark:text-amber-400'
                  "
                >
                  {{ n.armed ? "待触发" : "已触发" }}
                </Badge>
              </div>
            </div>

            <div
              v-if="n.lastNotifiedAtMs || n.lastResult"
              class="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground"
            >
              <span v-if="n.lastNotifiedAtMs"> 通知时间：{{ ago(n.lastNotifiedAtMs) }} </span>
              <span v-if="n.lastResult" class="break-all"> 结果：{{ n.lastResult }} </span>
            </div>
          </li>
        </ul>
      </CardContent>
    </Card>

    <!-- 诊断事件 -->
    <Card class="min-w-0 py-4">
      <CardHeader class="px-4 pb-0">
        <div class="flex items-center justify-between gap-3">
          <CardTitle class="text-sm font-semibold">诊断事件</CardTitle>
          <span class="text-xs text-muted-foreground">
            {{ eventCountText }}
          </span>
        </div>
      </CardHeader>

      <CardContent class="px-4">
        <div
          v-if="visibleEvents.length === 0"
          class="rounded-lg border border-dashed px-3 py-6 text-center text-xs text-muted-foreground"
        >
          暂无诊断事件
        </div>

        <ul v-else class="flex flex-col">
          <li
            v-for="(event, index) in visibleEvents"
            :key="`${event.atMs}-${index}`"
            class="min-w-0 border-b border-border/50 py-3 first:pt-0 last:border-b-0 last:pb-0"
          >
            <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
              <span class="text-xs text-muted-foreground tabular-nums">
                {{ ago(event.atMs) }}
              </span>

              <Badge variant="outline" :class="levelClass[event.level] ?? 'text-muted-foreground'">
                {{ levelLabel[event.level] ?? event.level }}
              </Badge>

              <span class="font-mono text-xs break-all" :class="levelClass[event.level]">
                {{ event.code }}
              </span>
            </div>

            <p
              class="mt-2 min-w-0 text-xs leading-5 break-all whitespace-pre-wrap text-foreground/90 select-text"
              :class="detailClass(event.code)"
            >
              <span v-if="event.deviceId" class="font-mono text-muted-foreground">
                {{ event.deviceId }} ·
              </span>
              {{ event.detail }}
            </p>
          </li>
        </ul>
      </CardContent>
    </Card>

    <!-- 调试区（红色边框）：只做前端重放，不写设置、不动启动项 -->
    <Card class="min-w-0 border-destructive/60 py-4">
      <CardHeader class="px-4 pb-0">
        <div class="flex items-center justify-between gap-3">
          <CardTitle class="text-sm font-semibold text-destructive">调试区</CardTitle>
          <Badge
            variant="outline"
            class="border-destructive/40 px-1.5 py-0 text-[10px] font-normal text-destructive"
          >
            请明白你要做什么
          </Badge>
        </div>
      </CardHeader>

      <CardContent class="flex flex-col gap-3 px-4">
        <!--
          调试开关（原「显示调试」按钮的位置在诊断事件卡里，现挪到调试区并改成开关）：
          它控制的是上方「诊断事件」是否展开 debug 级记录，纯前端状态、不落盘。
        -->
        <div
          class="flex items-start justify-between gap-3 rounded-lg border border-destructive/30 bg-destructive/5 p-3"
        >
          <div class="min-w-0 flex-1">
            <Label for="show-debug-events" class="cursor-pointer text-xs font-medium">
              显示调试信息
            </Label>
            <p class="mt-1 text-[11px] leading-snug text-muted-foreground">
              {{ debugHint }}
            </p>
          </div>

          <Switch
            id="show-debug-events"
            v-model="showDebug"
            aria-label="显示调试信息"
            class="mt-0.5 shrink-0 data-[state=checked]:bg-destructive"
          />
        </div>

        <Button
          type="button"
          variant="outline"
          size="sm"
          class="w-full border-destructive/50 text-destructive hover:bg-destructive/10 hover:text-destructive"
          @click="emit('replayOnboarding')"
        >
          <RotateCcw aria-hidden="true" />
          重现首次引导页
        </Button>
      </CardContent>
    </Card>
  </div>
</template>
