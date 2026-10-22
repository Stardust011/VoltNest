// `vitest/config` 复用 Vite 的 defineConfig，并额外声明 `test` 字段的类型。
import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";

// Tauri 期望固定端口；host 只监听回环，避免把开发服务器暴露到局域网。
export default defineConfig({
  plugins: [vue(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 5273,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "chrome105",
    sourcemap: false,
    outDir: "dist",
  },
  test: {
    // 只测纯函数：不引入 jsdom / happy-dom（规格 §14.1）。
    environment: "node",
    include: ["src/tests/**/*.spec.ts"],
  },
});
