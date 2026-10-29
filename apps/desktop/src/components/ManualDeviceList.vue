<script setup lang="ts">
import { computed, ref } from "vue";
import { ChevronDown, ClipboardCheck, Copy, FileText, Plus, RefreshCw } from "@lucide/vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { copyText } from "../lib/clipboard";
import { getManualReport } from "../lib/ipc";
import { humanizeAgo } from "../lib/time";
import type { ManualDeviceDto } from "../lib/types";

/**
 * 诊断页的「生成上报数据」区。
 *
 * 这些记录**不参与采集**：它们只代表「用户手动登记了一台 VoltNest 暂不认识的设备」，
 * 因此不显示实时电量、也不进托盘，避免让界面以为这台设备正在被监控。
 * 它的价值是绑住那份抓包报告（含各次采样时驱动显示的电量）—— 删掉记录不会删报告文件。
 */
const props = defineProps<{
  devices: ManualDeviceDto[];
  nowMs: number;
}>();

const emit = defineEmits<{
  (e: "add"): void;
  (e: "remove", id: string): void;
}>();

const expandedId = ref<string | null>(null);
const reportText = ref<string | null>(null);
const loadingId = ref<string | null>(null);
const copiedId = ref<string | null>(null);
const error = ref<string | null>(null);

const hasDevices = computed(() => props.devices.length > 0);

/** 采样摘要：`80% → 75%`，驱动没显示时写「未显示」。 */
function batteryTrail(device: ManualDeviceDto): string {
  if (device.samples.length === 0) return "—";
  return device.samples
    .map((sample) => (sample.batteryPercent === null ? "未显示" : `${sample.batteryPercent}%`))
    .join(" → ");
}

function ago(atMs: number | null): string {
  if (atMs === null) return "—";
  return humanizeAgo(Math.max(0, props.nowMs - atMs));
}

async function toggleReport(device: ManualDeviceDto) {
  error.value = null;
  if (expandedId.value === device.id) {
    expandedId.value = null;
    reportText.value = null;
    return;
  }
  if (!device.reportPath) {
    error.value = "这条记录没有报告文件（当时只保存了设备信息）。";
    return;
  }
  loadingId.value = device.id;
  try {
    reportText.value = await getManualReport(device.id);
    expandedId.value = device.id;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loadingId.value = null;
  }
}

async function copyReport(device: ManualDeviceDto) {
  error.value = null;
  loadingId.value = device.id;
  try {
    const text = await getManualReport(device.id);
    if (await copyText(text)) {
      copiedId.value = device.id;
      window.setTimeout(() => {
        if (copiedId.value === device.id) copiedId.value = null;
      }, 2_000);
    } else {
      error.value = "复制失败：展开报告后手动全选复制。";
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loadingId.value = null;
  }
}
</script>

<template>
  <Card class="min-w-0 py-4">
    <CardHeader class="px-4 pb-0">
      <div class="flex items-center justify-between gap-3">
        <CardTitle class="text-sm font-semibold">生成上报数据</CardTitle>
        <Button type="button" variant="outline" size="sm" class="h-7 px-2" @click="emit('add')">
          <Plus aria-hidden="true" />
          生成上报
        </Button>
      </div>
    </CardHeader>

    <CardContent class="px-4">
      <div
        v-if="!hasDevices"
        class="rounded-lg border border-dashed px-3 py-5 text-xs leading-relaxed text-muted-foreground"
      >
        软件没能自动识别的设备，可以在这里生成上报数据：<br />
        1. 先选设备与 HID 集合，<br />
        2. 按不同电量录几段原始数据，产出可直接贴进 issue 的报告。
      </div>

      <ul v-else class="flex flex-col gap-2">
        <li
          v-for="device in devices"
          :key="device.id"
          class="min-w-0 rounded-lg border bg-card/50 p-3"
        >
          <div class="flex min-w-0 items-start justify-between gap-2">
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
                <span class="truncate text-sm font-semibold">{{ device.name }}</span>
                <Badge
                  variant="outline"
                  class="px-1.5 py-0 text-[10px] font-normal text-muted-foreground"
                >
                  未自动识别
                </Badge>
              </div>
              <p class="mt-0.5 font-mono text-[11px] break-all text-muted-foreground">
                {{ device.vidPid }} · {{ device.usageSummary }}
              </p>
              <p class="mt-1 text-[11px] text-muted-foreground">
                {{ device.samples.length }} 次采样（{{ batteryTrail(device) }}）·
                {{ device.capturedFrames }} 帧 · 最近采集 {{ ago(device.lastCaptureAtMs) }}
              </p>
              <p v-if="device.note" class="mt-1 text-[11px] break-all text-muted-foreground">
                {{ device.note }}
              </p>
            </div>
          </div>

          <div class="mt-2 flex flex-wrap gap-1.5">
            <Button
              type="button"
              variant="outline"
              size="sm"
              class="h-7 px-2 text-[11px]"
              :disabled="loadingId === device.id"
              @click="toggleReport(device)"
            >
              <RefreshCw v-if="loadingId === device.id" class="animate-spin" aria-hidden="true" />
              <ChevronDown
                v-else
                :class="expandedId === device.id ? 'rotate-180' : ''"
                aria-hidden="true"
              />
              {{ expandedId === device.id ? "收起" : "查看报告" }}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              class="h-7 px-2 text-[11px]"
              :disabled="loadingId === device.id || !device.reportPath"
              @click="copyReport(device)"
            >
              <ClipboardCheck v-if="copiedId === device.id" aria-hidden="true" />
              <Copy v-else aria-hidden="true" />
              {{ copiedId === device.id ? "已复制" : "复制报告" }}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              class="h-7 px-2 text-[11px] text-destructive hover:bg-destructive/10 hover:text-destructive"
              @click="emit('remove', device.id)"
            >
              移除
            </Button>
          </div>

          <pre
            v-if="expandedId === device.id && reportText"
            class="mt-2 max-h-48 overflow-auto rounded-md bg-muted/60 p-2 font-mono text-[10px] leading-4 whitespace-pre-wrap select-text"
            >{{ reportText }}</pre>
          <p
            v-if="expandedId === device.id && device.reportPath"
            class="mt-1 font-mono text-[10px] break-all text-muted-foreground"
          >
            <FileText class="mr-1 inline size-3 align-[-2px]" aria-hidden="true" />{{
              device.reportPath
            }}
          </p>
        </li>
      </ul>

      <p
        v-if="error"
        class="mt-2 rounded-md border border-destructive/30 bg-destructive/5 p-2 text-[11px] break-all text-destructive select-text"
      >
        {{ error }}
      </p>
    </CardContent>
  </Card>
</template>
