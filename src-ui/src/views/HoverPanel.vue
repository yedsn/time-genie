<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { ArrowUpRight, Clock3, Pause, Play, RefreshCw, Square } from "lucide-vue-next";
import { ElMessage } from "element-plus";
import { useWorkdayStore } from "../store";
import { notifyWorkDataChanged, onTrayHoverShown, openMainOverview, openMainUnassignedResolution, setHoverTaskSelectionMode } from "../services/tauri";
import { createSingleFlight, reconcileTrayTaskSelection, trayIdlePresentation } from "../services/trayHoverCore";
import TaskSelectControl from "../components/TaskSelectControl.vue";

const store = useWorkdayStore();
const busy = ref(false);
const refreshing = ref(false);
const selectionExpanded = ref(false);
const taskSelectRef = ref<{ closeSuggestions: () => void }>();
const selectedTaskId = ref(store.selectedTaskId);
const entry = computed(() => store.runningEntry);
const idlePresentation = computed(() => trayIdlePresentation({
  hasActiveEntry: Boolean(entry.value),
  unassignedSessionId: store.unassignedSessionId,
  unassignedSeconds: store.unassignedSeconds,
}));
const hasUnassignedTime = computed(() => idlePresentation.value === "unassigned");
let unlistenTrayHoverShown: (() => void) | undefined;
let unmounted = false;
let selectionTransition = Promise.resolve(true);

function formatDuration(seconds: number, includeSeconds = false) {
  const safeSeconds = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(safeSeconds / 3600);
  const minutes = Math.floor((safeSeconds % 3600) / 60);
  const rest = safeSeconds % 60;
  const base = String(hours).padStart(2, "0") + ":" + String(minutes).padStart(2, "0");
  return includeSeconds ? base + ":" + String(rest).padStart(2, "0") : base;
}

const elapsed = computed(() => formatDuration(entry.value ? store.entryDurationSeconds(entry.value) : 0));
const unassignedElapsed = computed(() => formatDuration(store.unassignedSeconds, true));

function reconcileSelection() {
  selectedTaskId.value = reconcileTrayTaskSelection(selectedTaskId.value, store.tasks);
}

const runRefreshSingleFlight = createSingleFlight(async () => {
  refreshing.value = true;
  try {
    const results = await Promise.all([store.loadWorkspaceData(), store.loadTimeData(), store.loadUnassignedState()]);
    reconcileSelection();
    return results.every((result) => result !== false);
  } finally {
    refreshing.value = false;
  }
});

async function refreshTrayData(manual = false) {
  const succeeded = await runRefreshSingleFlight();
  if (manual) {
    if (succeeded) ElMessage.success("托盘数据已刷新");
    else ElMessage.error("刷新失败，已保留上一次可用数据");
  }
  return succeeded;
}

function setSelectionExpanded(expanded: boolean) {
  selectionTransition = selectionTransition.then(async () => {
    if (selectionExpanded.value === expanded) return true;
    try {
      await setHoverTaskSelectionMode(expanded);
      selectionExpanded.value = expanded;
      return true;
    } catch (error) {
      if (expanded) ElMessage.error(error instanceof Error ? error.message : "无法展开事项列表");
      return false;
    }
  });
  return selectionTransition;
}

async function openTaskSuggestions(showSuggestions: () => void) {
  if (!(await setSelectionExpanded(true))) return;
  showSuggestions();
}

async function openMain() {
  await setSelectionExpanded(false);
  await openMainOverview();
}

async function openUnassigned() {
  if (!hasUnassignedTime.value) return;
  await setSelectionExpanded(false);
  await openMainUnassignedResolution();
}

async function runTimerAction(action: () => Promise<unknown>, fallback: string) {
  busy.value = true;
  try {
    await action();
    await notifyWorkDataChanged(["tasks", "time", "unassigned"], "tray-hover");
  } catch (error) {
    const message = typeof error === "string" ? error : error instanceof Error ? error.message : fallback;
    ElMessage.error(message);
  } finally {
    busy.value = false;
  }
}

async function startSelectedTimer() {
  if (!selectedTaskId.value) return;
  await setSelectionExpanded(false);
  await store.startTimer(selectedTaskId.value);
}

async function stopTimerFromHover() {
  await store.stopTimer();
  await openMain();
}

watch(() => store.tasks.map((task) => [task.id, task.selectable, task.parentId ?? ""].join(":")).join("|"), reconcileSelection);

onMounted(() => {
  void refreshTrayData();
  void onTrayHoverShown(() => {
    taskSelectRef.value?.closeSuggestions();
    selectionExpanded.value = false;
    void setHoverTaskSelectionMode(false);
    void refreshTrayData();
  }).then((unlisten) => {
    if (unmounted) unlisten();
    else unlistenTrayHoverShown = unlisten;
  });
});

onUnmounted(() => {
  unmounted = true;
  unlistenTrayHoverShown?.();
  if (selectionExpanded.value) void setHoverTaskSelectionMode(false);
});
</script>

<template>
  <section class="hover-panel-window" :class="{ 'is-selection-expanded': selectionExpanded }">
    <div class="hover-panel">
      <header>
        <div class="eyebrow"><span class="live-dot"></span>今日工作台</div>
        <div class="hover-header-actions">
          <button class="icon-button" title="刷新托盘数据" :disabled="refreshing" @click="refreshTrayData(true)">
            <RefreshCw :size="15" :class="{ 'sync-spin': refreshing }" />
          </button>
          <button class="icon-button" title="打开主窗口" @click="openMain"><ArrowUpRight :size="16" /></button>
        </div>
      </header>
      <div class="hover-timer" :class="{ 'is-idle': !entry }">
        <template v-if="entry">
          <div class="timer-state">{{ entry.state === 'paused' ? '已暂停' : '正在记录' }}</div>
          <strong>{{ elapsed }}</strong>
          <div class="hover-task" :title="store.timeEntryLabel(entry)">{{ store.timeEntryLabel(entry) }}</div>
        </template>
        <template v-else>
          <div class="hover-unassigned-summary" :class="{ 'is-empty': !hasUnassignedTime }">
            <div>
              <div class="timer-state">{{ hasUnassignedTime ? '未归属时间 · 正在累计' : '未归属时间' }}</div>
              <strong>{{ hasUnassignedTime ? unassignedElapsed : '暂无' }}</strong>
            </div>
            <button v-if="hasUnassignedTime" class="text-action" @click="openUnassigned"><Clock3 :size="14" />处理未归属时间</button>
          </div>
          <div class="hover-task-select">
            <TaskSelectControl
              ref="taskSelectRef"
              v-model="selectedTaskId"
              variant="tray"
              placeholder="选择要开始计时的事项"
              aria-label="悬浮窗计时事项"
              @suggestions-open="openTaskSuggestions"
              @suggestions-close="setSelectionExpanded(false)"
              @suggestion-selected="setSelectionExpanded(false)"
            />
          </div>
          <p v-if="hasUnassignedTime" class="hover-unassigned-warning">开始事项计时会清除这段未归属时间，不会自动分配。</p>
        </template>
      </div>
      <div class="hover-stats"><span>今日已记录 <b>{{ store.todayMinutes }} 分钟</b></span><span>待结算 <b class="accent-text">{{ store.pendingMinutes }} 分钟</b></span></div>
      <div class="hover-actions">
        <button v-if="!entry" class="primary-button" :disabled="busy || !selectedTaskId" @click="runTimerAction(startSelectedTimer, '开始计时失败')"><Play :size="15" fill="currentColor" /> 开始所选事项计时</button>
        <button v-else-if="entry.state === 'running'" class="secondary-button" :disabled="busy" @click="runTimerAction(store.pauseTimer, '暂停计时失败')"><Pause :size="15" fill="currentColor" /> 暂停</button>
        <button v-else class="secondary-button" :disabled="busy" @click="runTimerAction(store.resumeTimer, '继续计时失败')"><Play :size="15" fill="currentColor" /> 继续</button>
        <button v-if="entry" class="danger-button" :disabled="busy" @click="runTimerAction(stopTimerFromHover, '结束计时失败')"><Square :size="14" fill="currentColor" /> 结束本段</button>
      </div>
    </div>
  </section>
</template>
