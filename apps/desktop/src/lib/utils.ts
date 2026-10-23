import type { ClassValue } from "clsx";
import { clsx } from "clsx";
import { twMerge } from "tailwind-merge";

// 合并 class：`clsx` 条件拼接 + `twMerge` 消解 Tailwind 冲突。默认样式在前、调用方 class 在后，故可整体覆盖默认，无需自定义 CSS（shadcn-vue 公共入口）。
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
