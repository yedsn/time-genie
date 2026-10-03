<script setup lang="ts">
import { computed, onMounted, onUnmounted } from "vue";
import MainWorkbench from "./views/MainWorkbench.vue";
import HoverPanel from "./views/HoverPanel.vue";
import { hideHoverAfterKeyboardClose, closeMainWindow, getSettings, isCurrentWindowFocused, onCurrentWindowFocusChanged, onWorkDataChanged } from "./services/tauri";
import { useWorkdayStore } from "./store";
import { stopCompletionFeedback } from "./services/completionFeedback";
import { applyAppTheme, getCachedAppTheme, isAppTheme } from "./services/theme";

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

onMounted(() => {
  document.body.dataset.view = view.value;
  void getSettings().then((settings) => {
    const theme = settings.device.app_theme;
    applyAppTheme(isAppTheme(theme) ? theme : "forest", true);
  }).catch(() => undefined);
  store.startClock(view.value === "main");
  void onWorkDataChanged((payload) => {
    const refresh = view.value === "main"
      ? Promise.all([store.loadWorkspaceData(), store.loadTimeData(), store.loadReports(), store.loadUnassignedState()]).then(() => store.loadTodayOverview())
      : Promise.all([store.loadWorkspaceData(), store.loadTimeData(), store.loadUnassignedState()]);
    void refresh.then(() => {
      if (view.value === "main" && payload.timerStoppedEntryId) {
        void store.openTimerStopConfirmation(payload.timerStoppedEntryId);
      }
    });
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
  store.stopClock();
  window.removeEventListener("keydown", onKeydown);
  stopCompletionFeedback();
});
</script>

<template>
  <HoverPanel v-if="view === 'hover'" />
  <MainWorkbench v-else />
</template>
