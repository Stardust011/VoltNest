import type { ClassValue } from "clsx";
import { clsx } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * 合并 class：`clsx` 负责条件拼接，`twMerge` 负责消解 Tailwind 冲突。
 *
 * 这是 shadcn-vue 每个 UI 组件的公共入口 —— 组件的默认样式总在前、
 * 调用方传入的 `class` 总在后，因此调用方可以整体覆盖默认值，
 * 而不需要写任何自定义 CSS。
 */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
