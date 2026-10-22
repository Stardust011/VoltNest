<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import DeviceCard from "./components/DeviceCard.vue";
import DiagnosticsPanel from "./components/DiagnosticsPanel.vue";
import OnboardingFlow from "./components/OnboardingFlow.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import { useSnapshot } from "./composables/useSnapshot";
import {
  completeOnboarding,
  getDiagnostics,
  getSettings,
  restartTransport,
  setAutostart,
  testNotification,
  updateSettings,
} from "./lib/ipc";
import type { CardDto, DiagnosticsDto, OnboardingChoice, SettingsDto } from "./lib/types";

type View = "devices" | "settings" | "diagnostics";

const view = ref<View>("devices");
const { snapshot, nowMs, connected } = useSnapshot();

const settings = ref<SettingsDto | null>(null);
const diagnostics = ref<DiagnosticsDto | null>(null);
const busy = ref(false);
const error = ref<string | null>(null);

const navItems: Array<{ id: View; label: string }> = [
  { id: "devices", label: "设备" },
  { id: "settings", label: "设置" },
  { id: "diagnostics", label: "诊断" },
];

/**
 * 面板行：快照里的 `cards` 只覆盖「HID + BLE 同一型号」的合并对，
 * 其余设备各自成行。行的顺序直接沿用 `devices` 的数组顺序 —— 排序在
 * Rust 决定（W-3），前端不得重排。
 *
 * 合并卡的非活跃成员不再单独出行，它的信息以副行文案出现在主成员卡上。
 */
interface Row {
  key: string;
  deviceId: string;
  inactiveText?: string;
}

const rows = computed<Row[]>(() => {
  const snap = snapshot.value;
  if (!snap) return [];
  const byPrimary = new Map<string, CardDto>();
  const absorbed = new Set<string>();
  for (const card of snap.cards) {
    byPrimary.set(card.primaryDeviceId, card);
    for (const id of card.memberDeviceIds) {
      if (id !== card.primaryDeviceId) absorbed.add(id);
    }
  }
  const result: Row[] = [];
  for (const device of snap.devices) {
    if (absorbed.has(device.stableId)) continue;
    const card = byPrimary.get(device.stableId);
    result.push({
      key: card?.cardId ?? device.stableId,
      deviceId: device.stableId,
      inactiveText: card?.inactiveMemberText,
    });
  }
  return result;
});

const deviceById = computed(() => {
  const map = new Map<string, NonNullable<typeof snapshot.value>["devices"][number]>();
  for (const device of snapshot.value?.devices ?? []) map.set(device.stableId, device);
  return map;
});

/** 顶部状态条：只在真的连上宿主后才显示版本号等信息。 */
const statusText = computed(() => {
  if (!connected.value) return "正在连接…";
  const count = snapshot.value?.devices.length ?? 0;
  return count === 0 ? "尚未发现设备" : `已跟踪 ${count} 台设备`;
});

function fail(e: unknown) {
  error.value = e instanceof Error ? e.message : String(e);
}

async function loadSettings() {
  try {
    settings.value = await getSettings();
  } catch (e) {
    fail(e);
  }
}

async function loadDiagnostics() {
  try {
    diagnostics.value = await getDiagnostics();
  } catch (e) {
    fail(e);
  }
}

async function applySettings(next: SettingsDto) {
  busy.value = true;
  try {
    settings.value = await updateSettings(next);
  } catch (e) {
    fail(e);
    await loadSettings();
  } finally {
    busy.value = false;
  }
}

/**
 * 自启开关是唯一「以宿主返回值为准」的设置项：写注册表可能失败
 * （组策略、杀软拦截），此时必须把开关拨回真实状态，而不是相信本地意图。
 */
async function applyAutostart(enabled: boolean) {
  busy.value = true;
  try {
    const actual = await setAutostart(enabled);
    if (settings.value) settings.value = { ...settings.value, autostart: actual };
    if (actual !== enabled) error.value = enabled ? "无法写入启动项，请检查系统策略" : "无法移除启动项";
  } catch (e) {
    fail(e);
    await loadSettings();
  } finally {
    busy.value = false;
  }
}

async function doRestart(transport: "hid" | "ble") {
  busy.value = true;
  try {
    await restartTransport(transport);
    if (view.value === "diagnostics") await loadDiagnostics();
  } catch (e) {
    fail(e);
  } finally {
    busy.value = false;
  }
}

async function doTestNotification() {
  busy.value = true;
  try {
    await testNotification();
    if (view.value === "diagnostics") await loadDiagnostics();
  } catch (e) {
    fail(e);
  } finally {
    busy.value = false;
  }
}

async function finishOnboarding(choice: OnboardingChoice) {
  busy.value = true;
  try {
    settings.value = await completeOnboarding(choice);
  } catch (e) {
    fail(e);
  } finally {
    busy.value = false;
  }
}

watch(view, (next) => {
  if (next === "diagnostics") void loadDiagnostics();
});

onMounted(() => {
  void loadSettings();
  void loadDiagnostics();
});
</script>

<template>
  <div class="flex h-full flex-col">
    <header
      class="flex items-center justify-between border-b border-black/5 px-5 py-3 dark:border-white/[0.07]"
    >
      <div class="flex items-baseline gap-2.5">
        <span class="text-[15px] font-semibold tracking-tight">VoltNest</span>
        <span class="text-[11.5px] text-neutral-400">{{ statusText }}</span>
      </div>
      <nav class="flex items-center gap-1">
        <button
          v-for="item in navItems"
          :key="item.id"
          type="button"
          class="rounded-lg px-3 py-1.5 text-[12.5px] transition"
          :class="view === item.id
            ? 'bg-black/[0.06] font-medium text-neutral-900 dark:bg-white/[0.1] dark:text-white'
            : 'text-neutral-500 hover:bg-black/[0.03] dark:text-neutral-400 dark:hover:bg-white/[0.06]'"
          @click="view = item.id"
        >
          {{ item.label }}
        </button>
      </nav>
    </header>

    <main class="min-h-0 flex-1 overflow-y-auto px-5 pt-4">
      <template v-if="view === 'devices'">
        <div v-if="rows.length > 0" class="flex flex-col gap-2.5 pb-5">
          <DeviceCard
            v-for="row in rows"
            :key="row.key"
            :device="deviceById.get(row.deviceId)!"
            :now-ms="nowMs"
            :inactive-text="row.inactiveText"
          />
        </div>
        <div v-else-if="connected" class="flex flex-col items-center justify-center gap-2 py-16 text-center">
          <div class="h-3 w-3 rounded-full border-2 border-neutral-300 dark:border-neutral-600" />
          <p class="text-[13px] font-medium">还没有发现设备</p>
          <p class="max-w-[280px] text-[11.5px] leading-relaxed text-neutral-400">
            插上 2.4G 接收器，或在系统设置里把蓝牙耳机连接一次，设备会自己出现在这里。
          </p>
        </div>
        <div v-else class="py-16 text-center text-[12.5px] text-neutral-400">正在连接宿主进程…</div>
      </template>

      <SettingsPanel
        v-else-if="view === 'settings' && settings"
        :settings="settings"
        :busy="busy"
        @change="applySettings"
        @autostart="applyAutostart"
        @test-notification="doTestNotification"
        @restart="doRestart"
      />

      <DiagnosticsPanel
        v-else-if="view === 'diagnostics' && diagnostics"
        :diagnostics="diagnostics"
        :now-ms="nowMs"
        @restart="doRestart"
      />

      <div v-else class="py-16 text-center text-[12.5px] text-neutral-400">正在读取…</div>
    </main>

    <OnboardingFlow
      v-if="settings && !settings.onboardingCompleted"
      :busy="busy"
      @done="finishOnboarding"
    />

    <div
      v-if="error"
      class="fixed bottom-4 left-1/2 flex max-w-[420px] -translate-x-1/2 items-start gap-2 rounded-xl border border-red-500/20 bg-red-50 px-3.5 py-2.5 text-[12px] text-red-700 shadow-lg dark:border-red-400/25 dark:bg-red-950/70 dark:text-red-200"
    >
      <span class="selectable flex-1">{{ error }}</span>
      <button type="button" class="shrink-0 text-red-500 hover:text-red-700" @click="error = null">关闭</button>
    </div>
  </div>
</template>
