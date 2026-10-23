<script setup lang="ts">
/**
 * 品牌徽标：直接使用打包的位图，与应用图标同源。
 *
 * 图片来源是 `src-tauri/icons/generate.py` 里同一张 256×256 渲染结果
 * （`src/assets/brand-mark.png`），因此窗口内、任务栏、开始菜单、安装包看到的是
 * 同一个图形。要整体替换品牌形象时，只需换掉（或用脚本重新生成）这张图，
 * 不需要改任何组件代码。
 *
 * 尺寸由外部 `class` 控制（`size-[18px]` 之类）；`size-full` 让图片撑满外层，
 * 这样高 DPI 下由浏览器按需降采样，不会出现 1px 偏移。
 */
import brandMark from "@/assets/brand-mark.png";
</script>

<template>
  <span class="inline-block shrink-0">
    <img :src="brandMark" alt="VoltNest" class="block size-full select-none" draggable="false" />
  </span>
</template>
