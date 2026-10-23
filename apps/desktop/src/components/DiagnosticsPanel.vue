
<script setup lang="ts">
import { RotateCcw } from "@lucide/vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { humanizeAgo } from "../lib/time";
import type { DiagnosticsDto } from "../lib/types";

const props = defineProps<{
  diagnostics: DiagnosticsDto;
  nowMs: number;
}>();

const emit = defineEmits<{
  (e: "restart", transport: "hid" | "ble"): void;
  /** 调试用：把首次引导遮罩重新叫出来（宿主决定是否落盘）。 */
  (e: "replayOnboarding"): void;
}>();

const levelClass: Record<string, string> = {
  error: "text-destructive font-medium",
  warn: "text-amber-600 dark:text-amber-400 font-medium",
  info: "text-muted-foreground",
};

const levelLabel: Record<string, string> = {
  error: "错误",
  warn: "警告",
  info: "信息",
};

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

/**
 * 坏帧诊断的 detail 是「成因 + 原始字节」（`cmd_mismatch(...) · 6B: 06 FA …`），
 * 属于机器数据：按等宽字体渲染，字节间距才对得齐、好逐位核对。
 */
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
          <span class="text-muted-foreground text-xs">
            {{ diagnostics.transports.length }} 个线程
          </span>
        </div>
      </CardHeader>

      <CardContent class="px-4">
        <div
          v-if="diagnostics.transports.length === 0"
          class="text-muted-foreground rounded-lg border border-dashed px-3 py-6 text-center text-xs"
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
                  <span class="text-muted-foreground font-mono text-[11px] break-all">
                    {{ familyLabel(t.family) }}
                  </span>
                </div>

                <div class="mt-2 flex flex-wrap items-center gap-2">
                  <Badge
                    :variant="statusVariant(t.status)"
                    :class="statusClass(t.status)"
                  >
                    {{ statusLabel[t.status] ?? t.status }}
                  </Badge>
                  <span class="text-muted-foreground text-xs">
                    重启 {{ t.restarts }} 次
                  </span>
                  <span class="text-muted-foreground text-xs">
                    队列 {{ t.queuedEvents }}
                  </span>
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
              <p class="text-destructive mb-1 text-[11px] font-medium">
                最近错误
              </p>
              <p
                class="text-destructive font-mono text-xs break-all whitespace-pre-wrap select-text"
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
          <span class="text-muted-foreground text-xs">
            {{ diagnostics.notifications.length }} 条记录
          </span>
        </div>
      </CardHeader>

      <CardContent class="px-4">
        <div
          v-if="diagnostics.notifications.length === 0"
          class="text-muted-foreground rounded-lg border border-dashed px-3 py-6 text-center text-xs"
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
                      : 'border-amber-500/40 text-amber-600 font-normal dark:text-amber-400'
                  "
                >
                  {{ n.armed ? "待触发" : "已触发" }}
                </Badge>
              </div>
            </div>

            <div
              v-if="n.lastNotifiedAtMs || n.lastResult"
              class="text-muted-foreground mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs"
            >
              <span v-if="n.lastNotifiedAtMs">
                通知时间：{{ ago(n.lastNotifiedAtMs) }}
              </span>
              <span v-if="n.lastResult" class="break-all">
                结果：{{ n.lastResult }}
              </span>
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
          <span class="text-muted-foreground text-xs">
            {{ diagnostics.events.length }} 条事件
          </span>
        </div>
      </CardHeader>

      <CardContent class="px-4">
        <div
          v-if="diagnostics.events.length === 0"
          class="text-muted-foreground rounded-lg border border-dashed px-3 py-6 text-center text-xs"
        >
          暂无诊断事件
        </div>

        <ul v-else class="flex flex-col">
          <li
            v-for="(event, index) in diagnostics.events"
            :key="`${event.atMs}-${index}`"
            class="min-w-0 border-b border-border/50 py-3 first:pt-0 last:border-b-0 last:pb-0"
          >
            <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
              <span class="text-muted-foreground text-xs tabular-nums">
                {{ ago(event.atMs) }}
              </span>

              <Badge
                variant="outline"
                :class="levelClass[event.level] ?? 'text-muted-foreground'"
              >
                {{ levelLabel[event.level] ?? event.level }}
              </Badge>

              <span class="font-mono text-xs break-all" :class="levelClass[event.level]">
                {{ event.code }}
              </span>
            </div>

            <p
              class="mt-2 min-w-0 text-xs leading-5 text-foreground/90 break-all whitespace-pre-wrap select-text"
              :class="detailClass(event.code)"
            >
              <span v-if="event.deviceId" class="text-muted-foreground font-mono">
                {{ event.deviceId }} ·
              </span>
              {{ event.detail }}
            </p>
          </li>
        </ul>
      </CardContent>
    </Card>

    <!-- 调试区（红色边框）：只做前端重放，不写设置、不动启动项 -->
    <Card class="border-destructive/60 min-w-0 py-4">
      <CardHeader class="px-4 pb-0">
        <div class="flex items-center justify-between gap-3">
          <CardTitle class="text-destructive text-sm font-semibold">调试区</CardTitle>
          <Badge
            variant="outline"
            class="border-destructive/40 text-destructive px-1.5 py-0 text-[10px] font-normal"
          >
            请明白你要做什么
          </Badge>
        </div>
      </CardHeader>

      <CardContent class="px-4">
        <Button
          type="button"
          variant="outline"
          size="sm"
          class="border-destructive/50 text-destructive hover:bg-destructive/10 hover:text-destructive mt-3 w-full"
          @click="emit('replayOnboarding')"
        >
          <RotateCcw aria-hidden="true" />
          重现首次引导页
        </Button>
      </CardContent>
    </Card>
  </div>
</template>
