<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow, type Window as TauriWindow } from "@tauri-apps/api/window";
import { Minus, X } from "@lucide/vue";
import { Button } from "@/components/ui/button";
import BrandMark from "@/components/BrandMark.vue";
import { hideToTray, quitApp } from "@/lib/ipc";

/**
 * 自绘标题栏（`decorations:false`，系统边框不画）。约束：
 * 1. 拖拽靠 `data-tauri-drag-region`：宿主只认命中元素上的属性，故品牌区子元素 `pointer-events-none` 让点击落回 `<header>`。
 * 2. 窗口按钮须落在拖拽区外（`<header>` 内 `ml-auto` 一组），否则「点了没反应」。
 * 3. 「最小化到托盘」= 宿主 `hide_to_tray`：隐藏窗口 + 强制落盘，进程与采集线程照旧跑，托盘图标还在。
 * 4. 「退出程序」= 宿主 `quit_app` → `app.exit(0)`，由 `RunEvent::Exit` 停监管线程并落盘。
 *    两个按钮都必须走命令而非 `window.hide()/close()`：前端自己调会绕开落盘；且 `close()` 被宿主 `prevent_close`
 *    截成「收托盘」，在这里用就成了「点了退出结果只关掉窗口」。
 * 5. 指针状态需主动作废（见 `POINTER_STALE`）。
 * 无最大化（配置 `maximizable:false`，双击标题栏最大化亦失效），仅最小化到托盘与退出。窗口按钮用 lucide 的 `Minus`/`X`，品牌徽标为 `BrandMark`（与应用图标同源）。
 */

/**
 * 「按钮底色卡住」根因与解法。
 * WebView2（Chromium）`:hover`/`:active` 由最后一次真实指针事件算并缓存；窗口隐藏/最小化时不参与命中测试，浏览器收不到 `mouseleave`/`mouseup`，缓存的「悬停/按下」留着。窗口再现时无新指针输入→不失效，底色停在按下版；真实 `pointermove` 到达才重算。故不能指望浏览器自清，须在窗口重获焦点时主动作废：给 `<html>` 挂 `.pointer-stale`，按钮悬停/按下背景写成 `[html:not(.pointer-stale)_&]:hover:…`，挂类即失配回透明，真实 `pointerdown`/`pointermove` 到达（指针可信）再摘。
 * 注：Button ghost 变体自带 `hover:bg-accent`，须显式 `hover:bg-transparent` 抵消（tailwind-merge 去重），带守卫的工具类特异性更高压过它。
 * 用真实指针事件而非合成 `dispatchEvent`：合成事件不更新浏览器内部 hover 状态。
 */
const POINTER_STALE = "pointer-stale";

/** 非 Tauri 环境（纯 `vite dev` / `vite preview`）没有窗口 API：窗口按钮退化为无操作，只保留焦点监听这一路。 */
const appWindow: TauriWindow | null = isTauri() ? getCurrentWindow() : null;

let unlisten: (() => void) | null = null;

function warn(action: string, e: unknown) {
  console.warn(`窗口${action}失败`, e);
}

function markPointerStale() {
  document.documentElement.classList.add(POINTER_STALE);
}

function clearPointerStale() {
  document.documentElement.classList.remove(POINTER_STALE);
}

function minimizeToTray() {
  if (!isTauri()) return;
  void hideToTray().catch((e) => warn("最小化到托盘", e));
}

function quit() {
  if (!isTauri()) return;
  void quitApp().catch((e) => warn("退出", e));
}

onMounted(async () => {
  markPointerStale();

  window.addEventListener("pointerdown", clearPointerStale, true);
  window.addEventListener("pointermove", clearPointerStale, true);
  window.addEventListener("focus", markPointerStale);

  if (!appWindow) return;
  try {
    unlisten = await appWindow.onFocusChanged(({ payload: focused }) => {
      if (focused) markPointerStale();
    });
  } catch (e) {
    warn("焦点监听", e);
  }
});

onBeforeUnmount(() => {
  unlisten?.();
  unlisten = null;
  window.removeEventListener("pointerdown", clearPointerStale, true);
  window.removeEventListener("pointermove", clearPointerStale, true);
  window.removeEventListener("focus", markPointerStale);
});
</script>

<template>
  <header
    data-tauri-drag-region
    class="relative z-50 flex h-10 shrink-0 items-center bg-background/70 pl-3 backdrop-blur-md dark:bg-white/[0.03]"
  >
    <div data-tauri-drag-region class="pointer-events-none flex min-w-0 items-center gap-2">
      <BrandMark class="size-[18px] shrink-0" />
      <span class="truncate text-xs font-semibold tracking-tight">VoltNest</span>
    </div>

    <div class="ml-auto flex h-full shrink-0 items-stretch">
      <Button
        type="button"
        variant="ghost"
        size="icon"
        class="h-full w-[42px] rounded-none text-muted-foreground hover:bg-transparent hover:text-foreground dark:hover:bg-transparent [html:not(.pointer-stale)_&]:hover:bg-neutral-900/[0.07] [html:not(.pointer-stale)_&]:active:bg-neutral-900/[0.12] dark:[html:not(.pointer-stale)_&]:hover:bg-white/[0.09] dark:[html:not(.pointer-stale)_&]:active:bg-white/[0.14]"
        title="最小化到托盘"
        aria-label="最小化到托盘"
        @click="minimizeToTray"
      >
        <Minus :size="13" :stroke-width="1.75" aria-hidden="true" />
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        class="h-full w-[42px] rounded-none text-muted-foreground hover:bg-transparent hover:text-foreground dark:hover:bg-transparent [html:not(.pointer-stale)_&]:hover:bg-[#c42b1c] [html:not(.pointer-stale)_&]:hover:text-white [html:not(.pointer-stale)_&]:active:bg-[#b0261a] [html:not(.pointer-stale)_&]:active:text-white"
        title="退出程序"
        aria-label="退出程序"
        @click="quit"
      >
        <X :size="14" :stroke-width="1.75" aria-hidden="true" />
      </Button>
    </div>
  </header>
</template>
