import { createApp } from "vue";
import App from "./App.vue";
import "./style.css";

// 主题跟随系统：shadcn-vue 深色令牌挂在 `.dark`，把 `prefers-color-scheme` 映射成 `<html class="dark">`，系统换主题即跟随。
const scheme = window.matchMedia("(prefers-color-scheme: dark)");

function applyScheme(dark: boolean) {
  document.documentElement.classList.toggle("dark", dark);
}

applyScheme(scheme.matches);
scheme.addEventListener("change", (event) => applyScheme(event.matches));

createApp(App).mount("#app");
