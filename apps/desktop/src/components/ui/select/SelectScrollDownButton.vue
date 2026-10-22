<script setup lang="ts">
import { ChevronDown } from "@lucide/vue";
import { SelectScrollDownButton, type SelectScrollDownButtonProps, useForwardProps } from "reka-ui";
import { cn } from "@/lib/utils";
import type { HTMLAttributes } from "vue";

const props = defineProps<SelectScrollDownButtonProps & { class?: HTMLAttributes["class"] }>();

const forwardedProps = useForwardProps(props);
</script>

<template>
  <SelectScrollDownButton
    data-slot="select-scroll-down-button"
    v-bind="forwardedProps"
    :class="cn('flex cursor-default items-center justify-center py-1', props.class)"
  >
    <slot>
      <ChevronDown class="size-4" />
    </slot>
  </SelectScrollDownButton>
</template>
