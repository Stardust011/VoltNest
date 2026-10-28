/**
 * 全仓排版基线：仓库根目录的 Markdown / JSON / YAML。
 * apps/desktop/prettier.config.mjs 继承本文件并追加 Tailwind 插件。
 *
 * 与 rustfmt.toml 对齐：100 列、Unix 换行。
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

  overrides: [
    {
      // Markdown 手写换行有语义，不重排段落。
      files: "*.md",
      options: { proseWrap: "preserve" },
    },
  ],
};
