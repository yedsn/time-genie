<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { ArrowUpRight, Clock3, Pause, Play, RefreshCw, Square } from "lucide-vue-next";
import { ElMessage } from "element-plus";
import { useWorkdayStore } from "../store";
import {
  notifyWorkDataChanged,
  onTrayHoverShown,
  openMainOverview,
  openMainTimerTaskSelection,
  openMainUnassignedResolution,
} from "../services/tauri";
import { createSingleFlight, trayIdlePresentation } from "../services/trayHoverCore";

const store = useWorkdayStore();
const busy = ref(false);
const refreshing = ref(false);
const entry = computed(() => store.runningEntry);
const idlePresentation = computed(() => trayIdlePresentation({
  hasActiveEntry: Boolean(entry.value),
  unassignedSessionId: store.unassignedSessionId,
  unassignedSeconds: store.unassignedSeconds,
}));
const hasUnassignedTime = computed(() => idlePresentation.value === "unassigned");
let unlistenTrayHoverShown: (() => void) | undefined;
let unmounted = false;

function formatDuration(seconds: number) {
  const safeSeconds = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(safeSeconds / 3600);
  const minutes = Math.floor((safeSeconds % 3600) / 60);
  const rest = safeSeconds % 60;
  return [hours, minutes, rest].map((value) => String(value).padStart(2, "0")).join(":");
}

const elapsed = computed(() => formatDuration(entry.value ? store.entryDurationSeconds(entry.value) : 0));
const unassignedElapsed = computed(() => formatDuration(store.unassignedSeconds));

const runRefreshSingleFlight = createSingleFlight(async () => {
  refreshing.value = true;
  try {
    const results = await Promise.all([store.loadWorkspaceData(), store.loadTimeData(), store.loadUnassignedState()]);
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

async function openMain() {
  await openMainOverview();
}

async function openTimerSelection() {
  await openMainTimerTaskSelection();
}

async function openUnassigned() {
  if (!hasUnassignedTime.value) return;
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

async function stopTimerFromHover() {
  await store.stopTimer();
  await openMain();
}

onMounted(() => {
  void refreshTrayData();
  void onTrayHoverShown(() => {
    void refreshTrayData();
  }).then((unlisten) => {
    if (unmounted) unlisten();
    else unlistenTrayHoverShown = unlisten;
  });
});

onUnmounted(() => {
  unmounted = true;
  unlistenTrayHoverShown?.();
});
</script>

<template>
  <section class="hover-panel-window">
    <div class="hover-panel">
      <header>
        <div class="hover-panel-heading">
          <span class="live-dot" :class="{ paused: entry?.state === 'paused', idle: !entry }"></span>
          <div>
            <span>今日计时</span>
            <small>{{ entry ? (entry.state === 'paused' ? '本段已暂停' : '本段记录中') : '快速状态' }}</small>
          </div>
        </div>
        <div class="hover-header-actions">
          <button class="icon-button" title="刷新托盘数据" :disabled="refreshing" @click="refreshTrayData(true)">
            <RefreshCw :size="15" :class="{ 'sync-spin': refreshing }" />
          </button>
          <button class="icon-button" title="打开主窗口" @click="openMain"><ArrowUpRight :size="16" /></button>
        </div>
      </header>

      <div class="hover-timer" :class="{ 'is-idle': !entry, 'is-paused': entry?.state === 'paused' }">
        <template v-if="entry">
          <div class="timer-state">{{ entry.state === 'paused' ? '已暂停' : '正在记录' }}</div>
          <strong>{{ elapsed }}</strong>
          <div class="hover-task" :title="store.timeEntryLabel(entry)">{{ store.timeEntryLabel(entry) }}</div>
        </template>
        <template v-else>
          <div class="timer-state">{{ hasUnassignedTime ? '未归属时间 · 正在累计' : '未归属时间' }}</div>
          <strong>{{ hasUnassignedTime ? unassignedElapsed : '00:00:00' }}</strong>
          <div class="hover-idle-note">{{ hasUnassignedTime ? '可进入主窗口分配、记为休息或丢弃' : '当前没有需要处理的未归属时间' }}</div>
        </template>
      </div>

      <div class="hover-stats" aria-label="今日时间摘要">
        <span><small>今日已记录</small><b>{{ store.todayMinutes }} 分钟</b></span>
        <span><small>待归属</small><b class="accent-text">{{ store.pendingMinutes }} 分钟</b></span>
      </div>

      <div v-if="entry" class="hover-actions">
        <button class="secondary-button" :disabled="busy" @click="openTimerSelection"><Play :size="15" fill="currentColor" />切换执行事项</button>
        <button v-if="entry.state === 'running'" class="secondary-button" :disabled="busy" @click="runTimerAction(store.pauseTimer, '暂停计时失败')"><Pause :size="15" fill="currentColor" />暂停</button>
        <button v-else class="primary-button" :disabled="busy" @click="runTimerAction(store.resumeTimer, '继续计时失败')"><Play :size="15" fill="currentColor" />继续</button>
        <button class="danger-button" :disabled="busy" @click="runTimerAction(stopTimerFromHover, '结束计时失败')"><Square :size="14" fill="currentColor" />结束并处理</button>
      </div>
      <div v-else class="hover-actions hover-idle-actions">
        <button class="primary-button" @click="openTimerSelection"><Play :size="15" fill="currentColor" />开始执行事项</button>
        <button v-if="hasUnassignedTime" class="secondary-button" @click="openUnassigned"><Clock3 :size="14" />处理未归属时间</button>
        <button v-else class="secondary-button" @click="openMain"><ArrowUpRight :size="14" />打开主窗口</button>
      </div>
    </div>
  </section>
</template>
