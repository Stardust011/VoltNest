
<script setup lang="ts">
/** 开关（shadcn-vue `switch`，底层 reka-ui `SwitchRoot`）。语义上是「立即生效的开关」而非「待提交的勾选」，故无指示器，仅滑块位移动画。 */
import type { HTMLAttributes } from "vue";
import { reactiveOmit } from "@vueuse/core";
import {
  SwitchRoot,
  SwitchThumb,
  useForwardPropsEmits,
  type SwitchRootEmits,
  type SwitchRootProps,
} from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps<SwitchRootProps & { class?: HTMLAttributes["class"] }>();

const emits = defineEmits<SwitchRootEmits>();

const delegatedProps = reactiveOmit(props, "class");

const forwarded = useForwardPropsEmits(delegatedProps, emits);
</script>

<template>
  <SwitchRoot
    v-bind="forwarded"
    :class="
      cn(
        'peer data-[state=checked]:bg-primary data-[state=unchecked]:bg-input focus-visible:border-ring focus-visible:ring-ring/50 inline-flex h-[1.15rem] w-8 shrink-0 items-center rounded-full border border-transparent shadow-xs transition-all outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50',
        props.class,
      )
    "
  >
    <SwitchThumb
      data-slot="switch-thumb"
      class="bg-background pointer-events-none block size-4 rounded-full shadow-sm ring-0 transition-transform data-[state=checked]:translate-x-[calc(100%-2px)] data-[state=unchecked]:translate-x-0"
    />
  </SwitchRoot>
</template>
