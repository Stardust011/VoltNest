<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { reactiveOmit } from "@vueuse/core";
import { SelectGroup, type SelectGroupProps, useForwardProps } from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps<SelectGroupProps & { class?: HTMLAttributes["class"] }>();

const delegatedProps = reactiveOmit(props, "class");

const forwardedProps = useForwardProps(delegatedProps);
</script>

<template>
  <SelectGroup
    data-slot="select-group"
    v-bind="forwardedProps"
    :class="cn('p-1 w-full', props.class)"
  >
    <slot />
  </SelectGroup>
</template>
