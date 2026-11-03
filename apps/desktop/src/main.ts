import { createApp } from "vue";
import App from "./App.vue";
import "./style.css";

const scheme = window.matchMedia("(prefers-color-scheme: dark)");

function applyScheme(dark: boolean) {
  document.documentElement.classList.toggle("dark", dark);
}

applyScheme(scheme.matches);
scheme.addEventListener("change", (event) => applyScheme(event.matches));

createApp(App).mount("#app");
