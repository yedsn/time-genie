<script setup lang="ts">
import { computed, onMounted, onUnmounted } from "vue";
import MainWorkbench from "./views/MainWorkbench.vue";
import HoverPanel from "./views/HoverPanel.vue";
import { hideHoverAfterKeyboardClose, closeMainWindow, getSettings, isCurrentWindowFocused, onCurrentWindowFocusChanged, onWorkDataChanged } from "./services/tauri";
import { useWorkdayStore } from "./store";
import { stopCompletionFeedback } from "./services/completionFeedback";
import { applyAppTheme, getCachedAppTheme, isAppTheme } from "./services/theme";
import { workDataRefreshPlan } from "./services/workDataRefresh";

applyAppTheme(getCachedAppTheme());

const store = useWorkdayStore();
const params = new URLSearchParams(window.location.search);
const view = computed(() => params.get("view") ?? "main");
let unlistenWindowFocus: (() => void) | undefined;
let unlistenWorkDataChanged: (() => void) | undefined;
let unmounted = false;
let mainWindowFocused = false;
let activationCheckArmed = true;
let activationSequence = 0;
const pendingRefreshDomains = new Set<string>();
let pendingTimerStoppedEntryId = "";
let refreshTimer: number | undefined;
let refreshRunning = false;

function onKeydown(event: KeyboardEvent) {
  if (event.key !== "Escape" || event.repeat) return;
  event.preventDefault();
  if (view.value === "main" && store.unassignedDialogOpen) return;
  if (view.value === "hover") {
    void hideHoverAfterKeyboardClose();
    return;
  }
  if (view.value === "main") void closeMainWindow();
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
  void promptPendingUnassignedTimeOnce(activationSequence);
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

async function flushDomainRefresh() {
  if (refreshRunning) return;
  refreshRunning = true;
  try {
    while (pendingRefreshDomains.size || pendingTimerStoppedEntryId) {
      const domains = new Set(pendingRefreshDomains);
      const timerStoppedEntryId = pendingTimerStoppedEntryId;
      pendingRefreshDomains.clear();
      pendingTimerStoppedEntryId = "";
      const plan = workDataRefreshPlan(domains, view.value === "main");
      const loads: Promise<unknown>[] = [];
      if (plan.workspace) loads.push(store.loadWorkspaceData());
      if (plan.time) loads.push(store.loadTimeData());
      if (plan.reports) loads.push(store.loadReports());
      if (plan.unassigned) loads.push(store.loadUnassignedState());
      await Promise.all(loads);
      if (plan.todayOverview) await store.loadTodayOverview();
      if (view.value === "main" && timerStoppedEntryId) await store.openTimerStopConfirmation(timerStoppedEntryId);
    }
  } finally {
    refreshRunning = false;
    if (pendingRefreshDomains.size || pendingTimerStoppedEntryId) scheduleDomainRefresh([]);
  }
}

onMounted(() => {
  document.body.dataset.view = view.value;
  void getSettings().then((settings) => {
    const theme = settings.device.app_theme;
    applyAppTheme(isAppTheme(theme) ? theme : "forest", true);
  }).catch(() => undefined);
  store.startClock(view.value === "main");
  void onWorkDataChanged((payload) => {
    scheduleDomainRefresh(payload.domains, payload.timerStoppedEntryId);
  }).then((unlisten) => {
    if (unmounted) unlisten();
    else unlistenWorkDataChanged = unlisten;
  });
  window.addEventListener("keydown", onKeydown);
  if (view.value === "main") {
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
  if (refreshTimer) window.clearTimeout(refreshTimer);
  store.stopClock();
  window.removeEventListener("keydown", onKeydown);
  stopCompletionFeedback();
});
</script>

<template>
  <HoverPanel v-if="view === 'hover'" />
  <MainWorkbench v-else />
</template>
