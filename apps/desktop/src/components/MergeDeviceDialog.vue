<script setup lang="ts">
import { ref } from "vue";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { mergeDevices } from "../lib/ipc";

const props = defineProps<{
  /** 卡片锚点设备。 */
  primary: { id: string; name: string };
  /** 其它可合并的设备（可多选）。 */
  others: Array<{ id: string; name: string }>;
  /** 锚点已在合并组里：并入原组，而不是新建。 */
  merged?: boolean;
}>();

const emit = defineEmits<{
  close: [];
  merged: [];
}>();

/** 默认名取 HID 上报的名称，用户可改。 */
const name = ref(props.primary.name);
/** 选中的其它设备，允许一次并入多台（三台及以上）。 */
const selected = ref<string[]>([]);
const busy = ref(false);
const error = ref<string | null>(null);

function toggle(id: string, checked: boolean) {
  const next = new Set(selected.value);
  if (checked) next.add(id);
  else next.delete(id);
  selected.value = [...next];
}

async function confirm() {
  if (busy.value) return;
  if (selected.value.length === 0) {
    error.value = "请选择要合并的设备";
    return;
  }
  busy.value = true;
  error.value = null;
  try {
    await mergeDevices(props.primary.id, selected.value, name.value);
    emit("merged");
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <Dialog :open="true" @update:open="(open: boolean) => !open && emit('close')">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{{ merged ? "追加到合并组" : "合并设备" }}</DialogTitle>
        <DialogDescription>
          {{
            merged
              ? `所选设备将并入「${primary.name}」所在的合并组；解散时整组一起恢复为独立设备。`
              : `「${primary.name}」将与所选设备显示为同一台，电量取当前活跃的一方。`
          }}
        </DialogDescription>
      </DialogHeader>

      <div class="flex flex-col gap-4">
        <div class="flex flex-col gap-2">
          <Label>并入「{{ primary.name }}」的设备</Label>
          <div
            class="max-h-56 overflow-y-auto rounded-md border p-1"
            role="group"
            aria-label="可合并的设备"
          >
            <Label
              v-for="(item, index) in others"
              :key="item.id"
              :for="`merge-target-${index}`"
              class="cursor-pointer rounded-sm px-2 py-1.5 text-sm font-normal hover:bg-accent"
            >
              <Checkbox
                :id="`merge-target-${index}`"
                :model-value="selected.includes(item.id)"
                @update:model-value="(v: boolean | 'indeterminate') => toggle(item.id, v === true)"
              />
              <span class="truncate">{{ item.name }}</span>
            </Label>
          </div>
          <p class="text-xs text-muted-foreground">
            已选 {{ selected.length }} 台，可同时并入多台设备。
          </p>
        </div>

        <div class="flex flex-col gap-2">
          <Label for="merge-name">合并后的名称</Label>
          <Input id="merge-name" v-model="name" maxlength="60" @keyup.enter="confirm" />
        </div>

        <p v-if="error" class="text-xs text-destructive">{{ error }}</p>
      </div>

      <DialogFooter>
        <Button variant="outline" type="button" :disabled="busy" @click="emit('close')">
          取消
        </Button>
        <Button type="button" :disabled="busy" @click="confirm">合并</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
