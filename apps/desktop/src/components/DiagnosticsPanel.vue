<script setup lang="ts">
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { humanizeAgo } from "../lib/time";
import type { DiagnosticsDto } from "../lib/types";

const props = defineProps<{ diagnostics: DiagnosticsDto; nowMs: number }>();
const emit = defineEmits<{ (e: "restart", transport: "hid" | "ble"): void }>();

const levelClass: Record<string, string> = {
  error: "text-destructive",
  warn: "text-amber-600 dark:text-amber-400",
  info: "text-muted-foreground",
};

const statusLabel: Record<string, string> = {
  running: "运行中",
  down: "已停止",
  unavailable: "采集不可用",
};

function ago(atMs: number) {
  return humanizeAgo(Math.max(0, props.nowMs - atMs));
}

function transportName(t: string) {
  return t === "hid" ? "2.4G" : "蓝牙";
}

/**
 * 家族 ID 的展示名：内部 ID 带厂商标识，诊断页统一收敛成协议简称。
 * 未登记的家族原样显示，避免新增 Provider 时界面失去辨识能力。
 */
const FAMILY_LABELS: Record<string, string> = {
  "hid-nearlink-ajazz": "hid-vbas",
};

function familyLabel(family: string) {
  return FAMILY_LABELS[family] ?? family;
}
</script>

<template>
  <div class="flex flex-col gap-4 pb-4">
    <Card class="gap-3 py-4">
      <CardHeader class="px-4">
        <CardTitle class="text-sm">采集线程组</CardTitle>
      </CardHeader>
      <CardContent class="px-4">
        <ul class="flex flex-col gap-2">
          <li
            v-for="t in diagnostics.transports"
            :key="t.transport"
            class="rounded-lg border px-3 py-2"
          >
            <div class="flex items-center justify-between gap-2">
              <div class="min-w-0">
                <p class="flex items-baseline gap-1.5 text-sm font-medium">
                  {{ transportName(t.transport) }}
                  <span class="text-muted-foreground font-mono text-[11px]">{{ familyLabel(t.family) }}</span>
                </p>
                <p class="text-muted-foreground mt-1 flex flex-wrap items-center gap-1.5 text-xs">
                  <Badge
                    :variant="t.status === 'running' ? 'secondary' : 'outline'"
                    :class="t.status === 'running' ? 'font-normal' : 'border-destructive/40 text-destructive font-normal'"
                  >
                    {{ statusLabel[t.status] ?? t.status }}
                  </Badge>
                  <span>重启 {{ t.restarts }} 次 · 队列 {{ t.queuedEvents }}</span>
                </p>
              </div>
              <Button type="button" variant="outline" size="sm" @click="emit('restart', t.transport)">
                重启
              </Button>
            </div>
            <p
              v-if="t.lastError"
              class="text-destructive mt-1 font-mono text-[11px] break-all select-text"
            >
              {{ t.lastError }}
            </p>
          </li>
        </ul>
      </CardContent>
    </Card>

    <Card class="gap-3 py-4">
      <CardHeader class="px-4">
        <CardTitle class="text-sm">通知触发节点</CardTitle>
      </CardHeader>
      <CardContent class="px-4">
        <div v-if="diagnostics.notifications.length === 0" class="text-muted-foreground text-xs">
          暂无记录
        </div>
        <ul v-else class="flex flex-col gap-1.5">
          <li
            v-for="n in diagnostics.notifications"
            :key="`${n.stableId}-${n.threshold}`"
            class="bg-muted/60 flex items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-xs"
          >
            <span class="min-w-0 truncate font-mono">{{ n.stableId }}</span>
            <span class="text-muted-foreground flex shrink-0 items-center gap-1.5">
              {{ n.threshold }}%
              <Badge v-if="!n.armed" variant="outline" class="font-normal">已触发</Badge>
              <template v-if="n.lastNotifiedAtMs"> · {{ ago(n.lastNotifiedAtMs) }}</template>
              <template v-if="n.lastResult"> · {{ n.lastResult }}</template>
            </span>
          </li>
        </ul>
      </CardContent>
    </Card>

    <Card class="gap-3 py-4">
      <CardHeader class="px-4">
        <CardTitle class="text-sm">运行环境</CardTitle>
      </CardHeader>
      <CardContent class="px-4">
        <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
          <dt class="text-muted-foreground">版本</dt>
          <dd class="font-mono select-text">{{ diagnostics.version }}</dd>
          <dt class="text-muted-foreground">数据目录</dt>
          <dd class="font-mono break-all select-text">{{ diagnostics.dataRoot }}</dd>
          <dt class="text-muted-foreground">开机自启</dt>
          <dd>{{ diagnostics.autostartRegistered ? "已注册" : "未注册" }}</dd>
        </dl>
      </CardContent>
    </Card>

    <Card class="gap-3 py-4">
      <CardHeader class="px-4">
        <CardTitle class="text-sm">诊断事件</CardTitle>
      </CardHeader>
      <CardContent class="px-4">
        <div v-if="diagnostics.events.length === 0" class="text-muted-foreground text-xs">
          暂无事件
        </div>
        <ul v-else class="flex flex-col gap-1">
          <li
            v-for="(event, index) in diagnostics.events"
            :key="`${event.atMs}-${index}`"
            class="flex items-start gap-2 text-xs"
          >
            <span class="text-muted-foreground w-14 shrink-0 tabular-nums">{{ ago(event.atMs) }}</span>
            <span class="w-[9.5rem] shrink-0 font-mono break-all" :class="levelClass[event.level]">
              {{ event.code }}
            </span>
            <span class="min-w-0 break-all select-text">
              {{ event.deviceId ? `${event.deviceId} · ` : "" }}{{ event.detail }}
            </span>
          </li>
        </ul>
      </CardContent>
    </Card>
  </div>
</template>
