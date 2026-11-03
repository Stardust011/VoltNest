<script setup lang="ts">
import { ref } from "vue";
import { Button } from "@/components/ui/button";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { mergeDevices } from "../lib/ipc";

const props = defineProps<{
  /** 卡片锚点设备。 */
  primary: { id: string; name: string };
  /** 其它可合并的设备。 */
  others: Array<{ id: string; name: string }>;
}>();

const emit = defineEmits<{
  close: [];
  merged: [];
}>();

/** 默认名取 HID 上报的名称，用户可改。 */
const name = ref(props.primary.name);
const target = ref(props.others[0]?.id ?? "");
const busy = ref(false);
const error = ref<string | null>(null);

async function confirm() {
  if (busy.value) return;
  if (!target.value) {
    error.value = "请选择要合并的设备";
    return;
  }
  if (target.value === props.primary.id) {
    error.value = "不能把设备与自身合并";
    return;
  }
  busy.value = true;
  error.value = null;
  try {
    await mergeDevices(props.primary.id, [target.value], name.value);
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
        <DialogTitle>合并设备</DialogTitle>
        <DialogDescription>
          「{{ primary.name }}」将与所选设备显示为同一台，电量取当前活跃的一方。
        </DialogDescription>
      </DialogHeader>

      <div class="flex flex-col gap-4">
        <div class="flex flex-col gap-2">
          <Label for="merge-target">合并到</Label>
          <Select v-model="target">
            <SelectTrigger id="merge-target" aria-label="合并目标设备" class="w-full">
              <SelectValue placeholder="选择设备" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="item in others" :key="item.id" :value="item.id">
                {{ item.name }}
              </SelectItem>
            </SelectContent>
          </Select>
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
