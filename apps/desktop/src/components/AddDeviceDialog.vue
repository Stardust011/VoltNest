<script setup lang="ts">
import { onMounted, ref } from "vue";
import { Plus, RefreshCw } from "@lucide/vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { addDevice, listDeviceCandidates } from "../lib/ipc";
import { transportLabel } from "../lib/format";
import type { DeviceCandidateDto } from "../lib/types";

const emit = defineEmits<{
  close: [];
  changed: [];
}>();

const candidates = ref<DeviceCandidateDto[]>([]);
const loading = ref(true);
const busy = ref<string | null>(null);
const error = ref<string | null>(null);

async function load() {
  loading.value = true;
  error.value = null;
  try {
    candidates.value = await listDeviceCandidates();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

/** 添加或恢复一台设备：成功后刷新候选（重复添加由宿主拒绝，这里只回显原因）。 */
async function add(candidate: DeviceCandidateDto) {
  if (busy.value) return;
  busy.value = candidate.stableId;
  error.value = null;
  try {
    await addDevice(candidate.stableId);
    emit("changed");
    await load();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = null;
  }
}

function actionLabel(candidate: DeviceCandidateDto): string {
  if (candidate.removed) return "恢复";
  return candidate.inList ? "已在列表" : "添加";
}

function addable(candidate: DeviceCandidateDto): boolean {
  return candidate.removed || !candidate.inList;
}

onMounted(load);
</script>

<template>
  <Dialog :open="true" @update:open="(open: boolean) => !open && emit('close')">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>添加设备</DialogTitle>
        <DialogDescription>暂未适配的设备请去诊断页上报请求支持。</DialogDescription>
      </DialogHeader>

      <div v-if="loading" class="py-6 text-center text-sm text-muted-foreground">正在枚举设备…</div>

      <ul
        v-else-if="candidates.length > 0"
        class="flex max-h-[320px] flex-col gap-2 overflow-y-auto"
      >
        <li
          v-for="candidate in candidates"
          :key="candidate.stableId"
          class="flex items-center gap-3 rounded-md border px-3 py-2"
        >
          <div class="min-w-0 flex-1">
            <p class="truncate text-sm font-medium">{{ candidate.displayName }}</p>
            <p class="truncate font-mono text-xs text-muted-foreground">
              {{ candidate.vidPid ?? candidate.stableId }}
            </p>
          </div>

          <Badge variant="secondary" class="shrink-0 px-1.5 py-0 text-[10px] font-normal">
            {{ transportLabel(candidate.transport) }}
          </Badge>

          <Button
            size="sm"
            variant="outline"
            type="button"
            class="shrink-0"
            :disabled="busy !== null || !addable(candidate)"
            @click="add(candidate)"
          >
            <RefreshCw v-if="candidate.removed" data-icon="inline-start" />
            <Plus v-else data-icon="inline-start" />
            {{ actionLabel(candidate) }}
          </Button>
        </li>
      </ul>

      <div v-else class="flex flex-col items-center gap-1 py-6 text-center">
        <p class="text-sm font-medium">没有可添加的设备</p>
        <p class="max-w-[320px] text-xs leading-relaxed text-muted-foreground">
          请先接入设备并保持通电，然后重新打开这个窗口。
        </p>
      </div>

      <p v-if="error" class="text-xs text-destructive">{{ error }}</p>

      <DialogFooter>
        <Button variant="outline" type="button" @click="emit('close')">关闭</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
