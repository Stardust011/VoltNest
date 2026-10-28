/**
 * 提交信息规范：Conventional Commits，与仓库历史用法保持一致。
 *
 * @type {import("@commitlint/types").UserConfig}
 */
export default {
  extends: ["@commitlint/config-conventional"],
  rules: {
    // 只放行仓库实际使用的类型
    "type-enum": [
      2,
      "always",
      [
        "feat",
        "fix",
        "docs",
        "style",
        "refactor",
        "perf",
        "test",
        "build",
        "ci",
        "chore",
        "revert",
      ],
    ],
    "header-max-length": [2, "always", 100],
    "scope-case": [2, "always", ["lower-case", "kebab-case", "snake-case"]],
    // 中文标题不需要首字母大写与句号
    "subject-case": [0],
    "subject-full-stop": [0],
  },
};
