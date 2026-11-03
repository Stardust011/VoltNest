<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from "vue";
import { Activity, BatteryMedium, CircleAlert, Settings } from "@lucide/vue";
import DeviceCard from "./components/DeviceCard.vue";
import DiagnosticsPanel from "./components/DiagnosticsPanel.vue";
import ManualAddPanel from "./components/ManualAddPanel.vue";
import OnboardingFlow from "./components/OnboardingFlow.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import TitleBar from "./components/TitleBar.vue";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import { useSnapshot } from "./composables/useSnapshot";
import {
  completeOnboarding,
  getDiagnostics,
  getSettings,
  listManualDevices,
  removeManualDevice,
  restartTransport,
  setAutostart,
  subscribeDiagnostics,
  updateSettings,
} from "./lib/ipc";
import type {
  CardDto,
  DiagnosticsDto,
  ManualDeviceDto,
  OnboardingChoice,
  SettingsDto,
} from "./lib/types";

type View = "devices" | "settings" | "diagnostics";

const view = ref<View>("devices");
const { snapshot, nowMs, connected } = useSnapshot();

const settings = ref<SettingsDto | null>(null);
const diagnostics = ref<DiagnosticsDto | null>(null);

/** 手动上报的设备：独立于快照（不参与采集），故单独拉取与维护。 */
const manualDevices = ref<ManualDeviceDto[]>([]);
/** 生成上报数据面板是否打开。 */
const addingDevice = ref(false);
/** 等待确认移除的记录（只留确认框要用的字段）。 */
const removeTarget = ref<{ id: string; name: string } | null>(null);
/**
 * 确认框开关，与 `removeTarget` 分开存。
 *
 * `AlertDialogAction` 点击时会先触发 reka 的关闭（`update:open false`），
 * 若开关与目标同一个变量，确认逻辑拿到的就是 `null` —— 表现就是「点了移除什么都没发生」。
 */
const removeOpen = ref(false);
const busy = ref(false);
const error = ref<string | null>(null);

/**
 * 调试重放开关：诊断页点「重现首次引导」时置 true，只让遮罩重新出现。
 *
 * 这是纯 UI 状态，不碰 `settings.onboardingCompleted` —— 关掉遮罩后
 * 首次引导的生命周期判定仍与磁盘上的设置一致。
 */
const replayOnboarding = ref(false);

/** 诊断刷新的合并窗口：一串线程组状态变化只拉一次。 */
const DIAGNOSTICS_REFRESH_MS = 120;

/**
 * 诊断拉取序号。`get_diagnostics` 是异步往返，先发的请求可能后返回；
 * 只有最后一次请求的响应允许落到界面上，避免旧数据把新数据盖回去。
 */
let diagnosticsSeq = 0;
let diagnosticsTimer: number | undefined;
let unlistenDiagnostics: (() => void) | undefined;
let disposed = false;

const navItems: Array<{ id: View; label: string; icon: Component }> = [
  { id: "devices", label: "设备", icon: BatteryMedium },
  { id: "settings", label: "设置", icon: Settings },
  { id: "diagnostics", label: "诊断", icon: Activity },
];

/**
 * 面板行：快照里的 `cards` 只覆盖「HID + BLE 同一型号」的合并对，
 * 其余设备各自成行。行的顺序直接沿用 `devices` 的数组顺序 —— 排序统一在
 * Rust 决定，前端不得重排。
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

/** 顶部状态条：只在真的连上宿主后才显示版本号等信息；台数按面板行算（合并卡算一台）。 */
const statusText = computed(() => {
  if (!connected.value) return "正在连接…";
  const count = rows.value.length;
  return count === 0 ? "尚未发现设备" : `正在监控 ${count} 台设备`;
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

async function loadManualDevices() {
  try {
    manualDevices.value = await listManualDevices();
  } catch (e) {
    fail(e);
  }
}

/** 上报数据生成成功：刷新列表并保留面板（用户可能还要复制报告）。 */
async function onManualSaved() {
  await loadManualDevices();
}

/** 移除一条手动设备记录：先弹确认框（记录与报告文件一起删）。 */
function doRemoveManual(id: string) {
  const device = manualDevices.value.find((item) => item.id === id);
  removeTarget.value = { id, name: device?.name.trim() || id };
  removeOpen.value = true;
}

/** reka 关闭弹窗时只关开关；目标留到确认/下次打开时清理，避免抢在 @click 之前置空。 */
function setRemoveOpen(open: boolean) {
  removeOpen.value = open;
}

/** 确认移除：先关弹窗再落盘，重复点击不会发两次请求。 */
async function confirmRemoveManual() {
  const target = removeTarget.value;
  removeTarget.value = null;
  removeOpen.value = false;
  if (!target) return;
  try {
    await removeManualDevice(target.id);
    await loadManualDevices();
  } catch (e) {
    fail(e);
  }
}

async function loadDiagnostics() {
  const seq = ++diagnosticsSeq;
  try {
    const next = await getDiagnostics();
    if (seq === diagnosticsSeq) diagnostics.value = next;
  } catch (e) {
    fail(e);
  }
}

/**
 * 收到宿主的「诊断数据已变」信号后重新拉取。
 *
 * 重启次数由监管线程异步推进（后台退避重启、手动重启都在那边完成），
 * 以前只有切到诊断页时才拉一次，所以数值变化在界面上是看不见的。
 * 同一串变化合并成一次请求，且只在诊断页可见时拉，不为看不见的页面做无谓 IPC。
 */
function scheduleDiagnosticsRefresh() {
  if (view.value !== "diagnostics" || diagnosticsTimer !== undefined) return;
  diagnosticsTimer = window.setTimeout(() => {
    diagnosticsTimer = undefined;
    void loadDiagnostics();
  }, DIAGNOSTICS_REFRESH_MS);
}

/**
 * 建立诊断订阅并拉取首帧。
 *
 * 顺序与快照一致：先订阅 → 再拉全量。反过来写会漏掉
 * 「拉取已返回、订阅尚未建立」这段时间内发生的线程组变化。
 */
async function startDiagnosticsFeed() {
  try {
    const off = await subscribeDiagnostics(scheduleDiagnosticsRefresh);
    if (disposed) {
      off();
      return;
    }
    unlistenDiagnostics = off;
  } catch (e) {
    fail(e);
  }
  await loadDiagnostics();
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
    if (actual !== enabled)
      error.value = enabled ? "无法写入启动项，请检查系统策略" : "无法移除启动项";
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

/**
 * 引导结束。
 *
 * 正常路径仍交给宿主落盘（`complete_onboarding`：写设置 + 注册 Run 项）。
 * 调试重放只收起遮罩：调试入口一旦落盘，等于隔三差五改用户的启动项。
 */
function onOnboardingDone(choice: OnboardingChoice) {
  if (replayOnboarding.value) {
    replayOnboarding.value = false;
    return;
  }
  void finishOnboarding(choice);
}

watch(view, (next) => {
  if (next !== "diagnostics") return;
  if (diagnosticsTimer !== undefined) {
    window.clearTimeout(diagnosticsTimer);
    diagnosticsTimer = undefined;
  }
  void loadDiagnostics();
});

onMounted(() => {
  void loadSettings();
  void startDiagnosticsFeed();
  void loadManualDevices();
});

onBeforeUnmount(() => {
  disposed = true;
  unlistenDiagnostics?.();
  if (diagnosticsTimer !== undefined) window.clearTimeout(diagnosticsTimer);
});
</script>

<template>
  <div class="flex h-screen flex-col overflow-hidden bg-muted/40 select-none dark:bg-background">
    <TitleBar />

    <nav class="flex shrink-0 items-center gap-1 border-b px-3 py-1.5">
      <Button
        v-for="item in navItems"
        :key="item.id"
        :variant="view === item.id ? 'secondary' : 'ghost'"
        size="sm"
        type="button"
        @click="view = item.id"
      >
        <component :is="item.icon" aria-hidden="true" />
        {{ item.label }}
      </Button>
      <Badge variant="outline" class="ml-auto max-w-[55%] truncate font-normal">
        {{ statusText }}
      </Badge>
    </nav>

    <main class="mx-auto min-h-0 w-full max-w-[560px] flex-1 overflow-y-auto px-5 pt-4">
      <template v-if="view === 'devices'">
        <div v-if="connected" class="flex flex-col gap-2.5 pb-5">
          <template v-if="rows.length > 0">
            <DeviceCard
              v-for="row in rows"
              :key="row.key"
              :device="deviceById.get(row.deviceId)!"
              :now-ms="nowMs"
              :inactive-text="row.inactiveText"
            />
          </template>

          <div v-else class="flex flex-col items-center justify-center gap-2 py-10 text-center">
            <div class="size-3 rounded-full border-2 border-muted-foreground/40" />
            <p class="text-sm font-medium">还没有发现设备</p>
            <p class="max-w-[280px] text-xs leading-relaxed text-muted-foreground">
              插上 2.4G 接收器，或在系统设置里把蓝牙耳机连接一次，设备会自己出现在这里。
            </p>
          </div>

          <p class="text-center text-xs leading-relaxed text-muted-foreground">
            若未自动识别，请切换为蓝牙模式或前往「诊断页」生成上报数据。
          </p>
        </div>
        <div v-else class="py-16 text-center text-sm text-muted-foreground">正在连接宿主进程…</div>
      </template>

      <SettingsPanel
        v-else-if="view === 'settings' && settings"
        :settings="settings"
        @change="applySettings"
        @autostart="applyAutostart"
      />

      <DiagnosticsPanel
        v-else-if="view === 'diagnostics' && diagnostics"
        :diagnostics="diagnostics"
        :now-ms="nowMs"
        :manual-devices="manualDevices"
        @restart="doRestart"
        @replay-onboarding="replayOnboarding = true"
        @add-manual="addingDevice = true"
        @remove-manual="doRemoveManual"
      />

      <div v-else class="py-16 text-center text-sm text-muted-foreground">正在读取…</div>
    </main>

    <AlertDialog :open="removeOpen" @update:open="setRemoveOpen">
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>移除「{{ removeTarget?.name }}」？</AlertDialogTitle>
          <AlertDialogDescription>
            这条记录和它的报告文件都会删除，无法恢复。如需留档，请先「复制报告」。
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>取消</AlertDialogCancel>
          <AlertDialogAction
            :class="buttonVariants({ variant: 'destructive' })"
            @click="confirmRemoveManual"
          >
            移除
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>

    <ManualAddPanel v-if="addingDevice" @close="addingDevice = false" @saved="onManualSaved" />

    <OnboardingFlow
      v-if="settings && (!settings.onboardingCompleted || replayOnboarding)"
      :busy="busy"
      @done="onOnboardingDone"
    />

    <Alert
      v-if="error"
      variant="destructive"
      class="fixed bottom-4 left-1/2 z-50 w-[calc(100%-2rem)] max-w-[420px] -translate-x-1/2 bg-popover shadow-lg"
    >
      <CircleAlert />
      <AlertDescription>
        <div class="flex w-full items-start gap-3">
          <span class="min-w-0 flex-1 break-all select-text">{{ error }}</span>
          <Button
            variant="ghost"
            size="sm"
            class="-my-1 h-7 shrink-0 px-2 text-destructive"
            type="button"
            @click="error = null"
          >
            关闭
          </Button>
        </div>
      </AlertDescription>
    </Alert>
  </div>
</template>
