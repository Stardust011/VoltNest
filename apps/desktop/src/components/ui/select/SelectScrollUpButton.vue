<script setup lang="ts">
import { ChevronUp } from "@lucide/vue";
import { SelectScrollUpButton, type SelectScrollUpButtonProps, useForwardProps } from "reka-ui";
import { cn } from "@/lib/utils";
import type { HTMLAttributes } from "vue";

const props = defineProps<SelectScrollUpButtonProps & { class?: HTMLAttributes["class"] }>();

const forwardedProps = useForwardProps(props);
</script>

<template>
  <SelectScrollUpButton
    data-slot="select-scroll-up-button"
    v-bind="forwardedProps"
    :class="cn('flex cursor-default items-center justify-center py-1', props.class)"
  >
    <slot>
      <ChevronUp class="size-4" />
    </slot>
  </SelectScrollUpButton>
</template>
