<script setup lang="ts">
/**
 * 品牌标识：与应用图标同源（`src-tauri/icons/generate.py` 生成的位图）。
 *
 * 三件套：近黑圆角底、渐变的「V + 闪电」、底色里一圈开口朝下的环。这里用内联 SVG
 * 复刻同一套几何（归一化到 0..100），好处是任意尺寸都锐利，且不额外打包图片。
 *
 * 改形状要同时改 `generate.py` 与这里——两处坐标一一对应，注释里标注了对应关系。
 * 尺寸完全由外部 `class` 控制（`size-[18px]` 之类），本组件不自带宽高。
 */
import { useId } from "vue";

// 标题栏与引导页会同时挂载，渐变 id 必须逐实例唯一，否则后挂载的那个会引用到
// 前一个实例的 <defs>，在 SSR / 卸载场景下拿到已消失的节点。
const gradientId = `vn-brand-gradient-${useId()}`;
</script>

<template>
  <svg viewBox="0 0 100 100" role="img" aria-label="VoltNest" class="block">
    <defs>
      <!-- 与 generate.py 的 GRAD_FROM / GRAD_TO 一致；略微右倾，避免整块死板。 -->
      <linearGradient
        :id="gradientId"
        x1="44"
        y1="17"
        x2="56"
        y2="74"
        gradientUnits="userSpaceOnUse"
      >
        <stop offset="0" stop-color="#c8f46e" />
        <stop offset="1" stop-color="#22b2ac" />
      </linearGradient>
    </defs>

    <!-- 底：TILE / TILE_R -->
    <rect width="100" height="100" rx="23.2" fill="#141518" />

    <!-- 环：RING_R=31.2, RING_W=3.6，开口 RING_GAP_DEG×2=44° 朝正下。
         周长 2πr=196.03，缺口弧长 196.03×44/360=23.96，故 dash 172.07 / gap 23.96，
         旋转 112° 让缺口落在 6 点方向。 -->
    <circle
      cx="50"
      cy="50"
      r="31.2"
      fill="none"
      stroke="#4b515c"
      stroke-width="3.6"
      stroke-dasharray="172.07 23.96"
      transform="rotate(112 50 50)"
    />

    <!-- V：V_LEFT / V_APEX / V_RIGHT，笔画宽度 V_HALF_W×2=11.2，谷底圆角。 -->
    <path
      d="M29.6 30 L50 68.8 L70.4 30"
      fill="none"
      :stroke="`url(#${gradientId})`"
      stroke-width="11.2"
      stroke-linejoin="round"
      stroke-linecap="butt"
    />

    <!-- 闪电：BOLT_LOCAL 按 BOLT_ORIGIN(50,15) / BOLT_SIZE(28,36) 映射。 -->
    <polygon
      points="68.14,15 65.6,29.18 77.22,29.18 78,31.34 61.28,51 59.24,49.6 62.4,37 50.87,37"
      :fill="`url(#${gradientId})`"
    />
  </svg>
</template>
