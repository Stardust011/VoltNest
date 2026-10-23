/**
 * Prettier 只负责排版，不承担代码质量判断：类型交给 `vue-tsc`，行为交给 `vitest`。
 *
 * 与 Rust 侧对齐：`rustfmt.toml` 用 `max_width = 100` 与 Unix 换行，这里保持同一套
 * 观感，避免同一个仓库里两种行长与两种换行符。
 *
 * @type {import("prettier").Config}
 * @see https://prettier.io/docs/options
 */
export default {
  printWidth: 100,
  tabWidth: 2,
  useTabs: false,
  semi: true,
  singleQuote: false,
  quoteProps: "as-needed",
  trailingComma: "all",
  bracketSpacing: true,
  bracketSameLine: false,
  arrowParens: "always",
  endOfLine: "lf",

  // Vue 单文件组件：保持 <script>/<style> 与模板同级缩进，否则 diff 噪声大。
  vueIndentScriptAndStyle: false,

  // Tailwind v4 的类名顺序靠 `@import "tailwindcss"` 的入口 CSS 推导，
  // 因此必须指向真实样式表，否则插件会退回内置的 v3 默认顺序。
  plugins: ["prettier-plugin-tailwindcss"],
  tailwindStylesheet: "./src/style.css",
  // `cn()` / `cva()` 里的字符串同样是 Tailwind 类名，一并参与排序。
  tailwindFunctions: ["cn", "cva"],

  overrides: [
    {
      // Markdown 里手写的换行是有意的（说明性文档常按句断行），不要重排。
      files: "*.md",
      options: { proseWrap: "preserve" },
    },
  ],
};
