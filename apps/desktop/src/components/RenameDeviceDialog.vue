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
import { renameDevice } from "../lib/ipc";

const props = defineProps<{
  /** 设备 `stableId` 或合并卡 ID。 */
  target: string;
  /** 当前显示名，作为输入框初值。 */
  value: string;
  /** 是否为合并卡（决定文案与空值校验）。 */
  merged: boolean;
}>();

const emit = defineEmits<{
  close: [];
  renamed: [];
}>();

const name = ref(props.value);
const busy = ref(false);
const error = ref<string | null>(null);

async function confirm() {
  if (busy.value) return;
  const trimmed = name.value.trim();
  if (props.merged && trimmed === "") {
    error.value = "合并名称不能为空";
    return;
  }
  busy.value = true;
  error.value = null;
  try {
    await renameDevice(props.target, trimmed);
    emit("renamed");
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <Dialog :open="true" @update:open="(open: boolean) => !open && emit('close')">
    <DialogContent class="max-w-[440px]">
      <DialogHeader>
        <DialogTitle>{{ merged ? "重命名合并卡" : "设备改名" }}</DialogTitle>
        <DialogDescription>
          {{
            merged
              ? "只改这张卡片的显示名，成员设备本身各自的名字不受影响。"
              : "留空并保存可恢复设备的上报名称。"
          }}
        </DialogDescription>
      </DialogHeader>

      <div class="flex flex-col gap-1.5">
        <Label for="rename-input">名称</Label>
        <Input id="rename-input" v-model="name" maxlength="60" @keyup.enter="confirm" />
        <p v-if="error" class="text-xs text-destructive">{{ error }}</p>
      </div>

      <DialogFooter>
        <Button variant="outline" size="sm" type="button" :disabled="busy" @click="emit('close')">
          取消
        </Button>
        <Button size="sm" type="button" :disabled="busy" @click="confirm">保存</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
