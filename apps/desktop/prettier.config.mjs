import base from "../../prettier.config.mjs";

/**
 * 桌面端排版：继承仓库基线，追加 Tailwind 类名排序。
 *
 * @type {import("prettier").Config}
 * @see https://prettier.io/docs/options
 */
export default {
  ...base,

  // Vue 单文件组件：<script>/<style> 与模板同级缩进。
  vueIndentScriptAndStyle: false,

  // Tailwind v4 的类名顺序由入口 CSS 推导，必须指向真实样式表，
  // 否则插件退回内置的 v3 默认顺序。
  plugins: ["prettier-plugin-tailwindcss"],
  tailwindStylesheet: "./src/style.css",
  tailwindFunctions: ["cn", "cva"],
};
