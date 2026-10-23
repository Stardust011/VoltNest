<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { reactiveOmit } from "@vueuse/core";
import { Label, type LabelProps } from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps<LabelProps & { class?: HTMLAttributes["class"] }>();

/**
 * `for` 必须继续往下传，否则 `<label for>` 不会落到真实节点上，
 * 「点文字切换控件」与无障碍名称关联两件事一起失效。
 * 写法与 `ui/checkbox`、`ui/progress` 一致：用 reactiveOmit 摘掉 class，
 * 避免它与下面合并后的 class 重复。
 */
const delegatedProps = reactiveOmit(props, "class");
</script>

<template>
  <Label
    v-bind="delegatedProps"
    :class="
      cn(
        'flex items-center gap-2 text-sm leading-none font-medium select-none group-data-[disabled=true]:pointer-events-none group-data-[disabled=true]:opacity-50 peer-disabled:cursor-not-allowed peer-disabled:opacity-50',
        props.class,
      )
    "
  >
    <slot />
  </Label>
</template>
