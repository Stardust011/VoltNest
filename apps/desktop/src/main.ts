import { createApp } from "vue";
import App from "./App.vue";
import "./style.css";

/**
 * 主题跟随系统：shadcn-vue 的深色令牌挂在 `.dark` 上，`dark:` 变体也由
 * `@custom-variant dark (&:is(.dark *))` 绑定到该类。这里把
 * `prefers-color-scheme` 映射成 `<html class="dark">`，视觉行为与原先用
 * 媒体查询时一致：系统换主题，窗口立刻跟着换。
 */
const scheme = window.matchMedia("(prefers-color-scheme: dark)");

function applyScheme(dark: boolean) {
  document.documentElement.classList.toggle("dark", dark);
}

applyScheme(scheme.matches);
scheme.addEventListener("change", (event) => applyScheme(event.matches));

createApp(App).mount("#app");
