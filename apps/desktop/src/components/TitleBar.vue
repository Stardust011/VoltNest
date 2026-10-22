<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow, type Window as TauriWindow } from "@tauri-apps/api/window";
import { Minus, X, Zap } from "@lucide/vue";
import { Button } from "@/components/ui/button";

/**
 * 应用自绘标题栏（窗口侧 `decorations: false`，系统边框整体不画）。
 *
 * 四条约束值得写下来：
 *
 * 1. **拖拽靠 `data-tauri-drag-region`**：宿主只认「命中的那个元素」上的属性，
 *    所以品牌区的子元素统一 `pointer-events-none`，让点击落回 `<header>`。
 * 2. **窗口按钮必须落在拖拽区之外**：它们是 `<header>` 内 `ml-auto` 那一组，
 *    自身不带 drag 属性，否则会变成「点了没反应」。
 * 3. **关闭 = 收进托盘**：真正把窗口藏起来的是宿主 `on_window_event` 里的
 *    `prevent_close` + `hide`，前端只发出关闭请求，不自己调 `hide()`
 *    ——那样会绕开退出前的落盘。
 * 4. **指针状态要能作废**：见下面的 `POINTER_STALE`。
 *
 * 不做最大化 / 还原：配置里 `maximizable: false`，因此宿主内置的
 * 「双击标题栏最大化」也一并失效，这里只剩最小化与关闭两个按钮。
 * 图标全部来自 lucide（https://lucide.dev/icons/）：`Minus` / `X`，
 * 徽标用 `Zap`（电量）。
 */

/**
 * 「按钮底色卡住」的根因与解法。
 *
 * WebView2（Chromium）的 `:hover` / `:active` 不是每次重绘都重算的：它由
 * **最后一次真实指针事件**算出来，缓存在渲染进程里。窗口被隐藏到托盘或
 * 最小化时，窗口不再参与命中测试，浏览器收不到 `mouseleave`（也没有
 * `mouseup` 兜底），缓存里那个「悬停/按下」的结论就留在了元素上。窗口再
 * 出现时没有新的指针输入 → 没有任何东西让它失效，于是底色一直停在按下的
 * 那一版；鼠标一进窗口，真实 `pointermove` 到达，才被重算回正常。
 *
 * 结论：**不能指望浏览器自己把这份缓存擦掉**，只能在窗口重新获得焦点时
 * 主动作废。做法是给 `<html>` 挂 `.pointer-stale`：两个按钮的悬停 / 按下
 * 背景都写成 Tailwind 任意变体 `[html:not(.pointer-stale)_&]:hover:…`
 * ——挂上类，这组工具类立刻失配（底色回到透明常态），等真正的
 * `pointerdown` / `pointermove` 到达（说明指针位置重新可信）再摘掉。
 *
 * 注意：Button 的 ghost 变体自带 `hover:bg-accent`，所以每个按钮还要显式
 * 写 `hover:bg-transparent` 把它抵消（`cn` 里的 tailwind-merge 会去重）；
 * 带守卫的那份工具类特异性更高，守卫成立时压过它。
 *
 * 这里刻意用「真实的指针事件」而不是合成的 `dispatchEvent`：合成事件不会
 * 更新浏览器内部的 hover 状态，只有真输入才算数。
 */
const POINTER_STALE = "pointer-stale";

/** 非 Tauri 环境（纯 `vite dev` / `vite preview`）没有窗口 API，按钮退化为无操作。 */
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

function minimize() {
  if (!appWindow) return;
  void appWindow.minimize().catch((e) => warn("最小化", e));
}

function closeToTray() {
  if (!appWindow) return;
  void appWindow.close().catch((e) => warn("关闭", e));
}

onMounted(async () => {
  // 首帧同样不可信：光标位置未知，先按「没有悬停」画。
  markPointerStale();

  // 真实指针输入到达 = 光标位置重新可信，把状态交还给 CSS。
  // 捕获阶段监听，避免被子元素的 stopPropagation 截断。
  window.addEventListener("pointerdown", clearPointerStale, true);
  window.addEventListener("pointermove", clearPointerStale, true);
  // 窗口重新获得焦点（任务栏还原 / 托盘唤起）时，屏幕上的状态已经过期。
  window.addEventListener("focus", markPointerStale);

  if (!appWindow) return;
  try {
    // 宿主侧焦点事件兜底：隐藏窗口期间浏览器未必会派发 `window.focus`。
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
    class="bg-background/70 relative z-50 flex h-10 shrink-0 items-center pl-3 backdrop-blur-md dark:bg-white/[0.03]"
  >
    <div data-tauri-drag-region class="pointer-events-none flex min-w-0 items-center gap-2">
      <span
        class="bg-primary text-primary-foreground grid size-[18px] shrink-0 place-items-center rounded-[6px]"
      >
        <Zap :size="11" :stroke-width="2.5" aria-hidden="true" />
      </span>
      <span class="truncate text-xs font-semibold tracking-tight">VoltNest</span>
    </div>

    <div class="ml-auto flex h-full shrink-0 items-stretch">
      <Button
        type="button"
        variant="ghost"
        size="icon"
        class="text-muted-foreground h-full w-[42px] rounded-none hover:bg-transparent hover:text-foreground dark:hover:bg-transparent [html:not(.pointer-stale)_&]:hover:bg-neutral-900/[0.07] [html:not(.pointer-stale)_&]:active:bg-neutral-900/[0.12] dark:[html:not(.pointer-stale)_&]:hover:bg-white/[0.09] dark:[html:not(.pointer-stale)_&]:active:bg-white/[0.14]"
        title="最小化"
        aria-label="最小化"
        @click="minimize"
      >
        <Minus :size="13" :stroke-width="1.75" aria-hidden="true" />
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        class="text-muted-foreground h-full w-[42px] rounded-none hover:bg-transparent hover:text-foreground dark:hover:bg-transparent [html:not(.pointer-stale)_&]:hover:bg-[#c42b1c] [html:not(.pointer-stale)_&]:hover:text-white [html:not(.pointer-stale)_&]:active:bg-[#b0261a] [html:not(.pointer-stale)_&]:active:text-white"
        title="关闭到托盘"
        aria-label="关闭到托盘"
        @click="closeToTray"
      >
        <X :size="14" :stroke-width="1.75" aria-hidden="true" />
      </Button>
    </div>
  </header>
</template>
