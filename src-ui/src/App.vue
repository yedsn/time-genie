<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, onUnmounted, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { hideHoverAfterKeyboardClose, closeMainWindow, getSettings, getStorageMode, isCurrentWindowFocused, onCloudSyncStateChanged, onCurrentWindowFocusChanged, onOpenUnassignedResolution, onWorkDataChanged, wakeCloudConnection, type StorageModeSnapshot } from "./services/tauri";
import { useWorkdayStore } from "./store";
import { stopCompletionFeedback } from "./services/completionFeedback";
import { applyAppTheme, getCachedAppTheme, isAppTheme } from "./services/theme";
import { partitionWorkDataRefreshDomains, shouldClearDirectCloudMemory, shouldRefreshDirectCloudData, workDataRefreshPlan } from "./services/workDataRefresh";
import { shouldRefreshTimeHistory } from "./services/timeEntryHistoryCore";
import { decideWorkDataInvalidation } from "./services/workDataInvalidationCore";
import { shouldCalibrateTimer, TimerCalibrationSingleFlight } from "./services/timerCalibrationCore";

applyAppTheme(getCachedAppTheme());

const store = useWorkdayStore();
const asyncViewOptions = (loader: () => Promise<unknown>) => ({
  loader: loader as () => Promise<{ default: object }>,
  loadingComponent: { template: '<div class="view-loading">正在加载…</div>' },
  delay: 80,
});
const MainWorkbench = defineAsyncComponent(asyncViewOptions(() => import("./views/MainWorkbench.vue")));
const HoverPanel = defineAsyncComponent(asyncViewOptions(() => import("./views/HoverPanel.vue")));
const params = new URLSearchParams(window.location.search);
const view = computed(() => params.get("view") ?? "main");
let unlistenWindowFocus: (() => void) | undefined;
let unlistenWorkDataChanged: (() => void) | undefined;
let unlistenCloudSyncStateChanged: (() => void) | undefined;
let unlistenOpenUnassignedResolution: (() => void) | undefined;
let unmounted = false;
let mainWindowFocused = false;
let activationCheckArmed = true;
let activationSequence = 0;
const pendingRefreshDomains = new Set<string>();
let pendingTimerStoppedEntryId = "";
let refreshTimer: number | undefined;
let refreshRunning = false;
let catchUpTimer: number | undefined;
const catchUpSingleFlight = new TimerCalibrationSingleFlight();
let lastCatchUpAt = Date.now();
let lastWakeHeartbeatAt = Date.now();
const currentWindowLabel = getCurrentWindow().label;
let currentWorkspaceEpoch = 0;
let appliedChangeSeq = 0;
let currentWorkspaceId = "";
let wasDirectCloud = false;
let directCloudAccessState = "";

function clearPendingDomainRefresh() {
  pendingRefreshDomains.clear();
  pendingTimerStoppedEntryId = "";
  if (refreshTimer) window.clearTimeout(refreshTimer);
  refreshTimer = undefined;
}

function applyCloudAccessState(state: StorageModeSnapshot) {
  const isDirectCloud = state.mode === "cloud" && state.storageProtocol === "direct_v2";
  if ((wasDirectCloud && !isDirectCloud) || (currentWorkspaceId && state.workspaceId && state.workspaceId !== currentWorkspaceId)) {
    clearPendingDomainRefresh();
    store.clearCloudBusinessMemory();
    currentWorkspaceEpoch = 0;
    appliedChangeSeq = 0;
  }
  if (currentWorkspaceEpoch && state.workspaceEpoch && state.workspaceEpoch !== currentWorkspaceEpoch) {
    clearPendingDomainRefresh();
    store.clearCloudBusinessMemory();
    appliedChangeSeq = 0;
  }
  wasDirectCloud = isDirectCloud;
  currentWorkspaceId = state.workspaceId || currentWorkspaceId;
  currentWorkspaceEpoch = state.workspaceEpoch || currentWorkspaceEpoch;
  if (!state.storageProtocol) return;
  if (state.mode !== "cloud" || state.storageProtocol !== "direct_v2") {
    directCloudAccessState = "";
    return;
  }
  const previousAccessState = directCloudAccessState;
  directCloudAccessState = state.cloudAccessState;
  if (state.cloudAccessState === "online_writable") {
    if (shouldRefreshDirectCloudData(previousAccessState, state.cloudAccessState)) void catchUpCloudData();
    return;
  }
  if (shouldClearDirectCloudMemory(state.cloudAccessState)) {
    store.clearCloudBusinessMemory();
  }
}

async function catchUpCloudData() {
  lastCatchUpAt = Date.now();
  try {
    await catchUpSingleFlight.run(async () => {
      await Promise.all([
        store.loadTimeData(true),
        store.loadUnassignedState(false, true),
      ]);
    });
  } catch {
    // The backend reports the connection state; keep the loaded page visible.
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key !== "Escape" || event.repeat) return;
  if (view.value === "hover") {
    event.preventDefault();
    void hideHoverAfterKeyboardClose();
    return;
  }
  if (view.value === "main") {
    event.preventDefault();
    void closeMainWindow();
  }
}

async function promptPendingUnassignedTimeOnce(sequence: number) {
  if (view.value !== "main" || !activationCheckArmed) return;
  activationCheckArmed = false;
  await store.loadUnassignedState();
  if (!mainWindowFocused || sequence !== activationSequence) return;
  store.promptUnassignedResolution();
}

function handleMainWindowFocus(focused: boolean) {
  mainWindowFocused = focused;
  activationSequence += 1;
  if (!focused) {
    activationCheckArmed = true;
    return;
  }
  void catchUpCloudData();
  void promptPendingUnassignedTimeOnce(activationSequence);
}

function handleVisibilityChange() {
  if (document.visibilityState === "visible") void catchUpCloudData();
}

function handleNetworkOnline() {
  if (view.value !== "main") return;
  void wakeCloudConnection();
  void catchUpCloudData();
}

function scheduleDomainRefresh(domains: string[], timerStoppedEntryId?: string) {
  domains.forEach((domain) => pendingRefreshDomains.add(domain));
  if (timerStoppedEntryId) pendingTimerStoppedEntryId = timerStoppedEntryId;
  if (refreshTimer || refreshRunning) return;
  refreshTimer = window.setTimeout(() => {
    refreshTimer = undefined;
    void flushDomainRefresh();
  }, 80);
}

function hasVisiblePendingRefresh() {
  const { visible } = partitionWorkDataRefreshDomains(
    pendingRefreshDomains,
    store.activePage,
    view.value === "main",
  );
  return visible.length > 0;
}

async function flushDomainRefresh() {
  if (refreshRunning) return;
  refreshRunning = true;
  try {
    while (pendingRefreshDomains.size || pendingTimerStoppedEntryId) {
      const { visible } = partitionWorkDataRefreshDomains(
        pendingRefreshDomains,
        store.activePage,
        view.value === "main",
      );
      const timerStoppedEntryId = pendingTimerStoppedEntryId;
      if (!visible.length && !timerStoppedEntryId) break;
      visible.forEach((domain) => pendingRefreshDomains.delete(domain));
      const domains = new Set(visible);
      pendingTimerStoppedEntryId = "";
      const plan = workDataRefreshPlan(domains, view.value === "main", store.activePage);
      const loads: Promise<unknown>[] = [];
      if (plan.workspace) loads.push(store.loadWorkspaceData());
      if (plan.time) {
        loads.push(store.loadTimeData());
        if (view.value === "main" && shouldRefreshTimeHistory(store.activePage, plan.time)) loads.push(store.loadTimeHistory());
      }
      if (plan.reports) loads.push(store.loadReports());
      if (plan.unassigned) loads.push(store.loadUnassignedState());
      await Promise.all(loads);
      if (plan.todayOverview) await store.loadTodayOverview();
      if (view.value === "main" && timerStoppedEntryId) await store.openTimerStopConfirmation(timerStoppedEntryId);
    }
  } finally {
    refreshRunning = false;
    if (pendingTimerStoppedEntryId || hasVisiblePendingRefresh()) scheduleDomainRefresh([]);
  }
}

watch(() => store.activePage, () => scheduleDomainRefresh([]));

onMounted(() => {
  document.body.dataset.view = view.value;
  void getSettings().then((settings) => {
    const theme = settings.device.app_theme;
    applyAppTheme(isAppTheme(theme) ? theme : "forest", true);
  }).catch(() => undefined);
  store.startClock(view.value === "hover" ? "hover" : "main");
  void getStorageMode().then(applyCloudAccessState).catch(() => undefined);
  void onCloudSyncStateChanged(applyCloudAccessState).then((unlisten) => {
    if (unmounted) unlisten();
    else unlistenCloudSyncStateChanged = unlisten;
  });
  void onWorkDataChanged((payload) => {
    const decision = decideWorkDataInvalidation({
      ...payload,
      changeSeq: payload.changeSeq ?? payload.revision,
    }, { currentWindowLabel, workspaceEpoch: currentWorkspaceEpoch, appliedChangeSeq });
    if (decision.action === "ignore") return;
    if (decision.action === "reset") {
      clearPendingDomainRefresh();
      store.clearCloudBusinessMemory();
      currentWorkspaceEpoch = payload.workspaceEpoch ?? currentWorkspaceEpoch;
      appliedChangeSeq = 0;
    }
    appliedChangeSeq = Math.max(appliedChangeSeq, decision.changeSeq);
    store.invalidateCloudDomains(decision.domains, decision.changeSeq, payload.workspaceEpoch);
    scheduleDomainRefresh(decision.domains, payload.timerStoppedEntryId);
  }).then((unlisten) => {
    if (unmounted) unlisten();
    else unlistenWorkDataChanged = unlisten;
  });
  window.addEventListener("keydown", onKeydown, true);
  window.addEventListener("online", handleNetworkOnline);
  document.addEventListener("visibilitychange", handleVisibilityChange);
  catchUpTimer = window.setInterval(() => {
    const now = Date.now();
    const wakeGap = now - lastWakeHeartbeatAt;
    lastWakeHeartbeatAt = now;
    if (document.visibilityState === "visible" && shouldCalibrateTimer(now, lastCatchUpAt, wakeGap, true)) {
      void catchUpCloudData();
    }
  }, 5_000);
  if (view.value === "main") {
    void onOpenUnassignedResolution(async () => {
      await store.loadUnassignedState(true);
      if (store.unassignedSessionId && store.unassignedSeconds > 0) store.promptUnassignedResolution(true);
    }).then((unlisten) => {
      if (unmounted) unlisten();
      else unlistenOpenUnassignedResolution = unlisten;
    });
    void onCurrentWindowFocusChanged(handleMainWindowFocus).then((unlisten) => {
      if (unmounted) unlisten();
      else unlistenWindowFocus = unlisten;
    });
    void isCurrentWindowFocused().then((focused) => {
      if (!unmounted) handleMainWindowFocus(focused);
    });
  }
});

onUnmounted(() => {
  unmounted = true;
  unlistenWindowFocus?.();
  unlistenWorkDataChanged?.();
  unlistenCloudSyncStateChanged?.();
  unlistenOpenUnassignedResolution?.();
  if (refreshTimer) window.clearTimeout(refreshTimer);
  if (catchUpTimer) window.clearInterval(catchUpTimer);
  store.stopClock();
  window.removeEventListener("keydown", onKeydown, true);
  window.removeEventListener("online", handleNetworkOnline);
  document.removeEventListener("visibilitychange", handleVisibilityChange);
  stopCompletionFeedback();
});
</script>

<template>
  <HoverPanel v-if="view === 'hover'" />
  <MainWorkbench v-else />
</template>

<style>
.view-loading {
  display: grid;
  min-height: 100vh;
  place-items: center;
  color: var(--el-text-color-secondary);
  font-size: 13px;
}
</style>
