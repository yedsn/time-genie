<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { Check, ChevronDown, ChevronRight, CircleDot, Clock3, Coffee, Download, Eraser, Pause, Play, Plus, RotateCcw, Split, Square, TimerReset, Trash2 } from "lucide-vue-next";
import { save } from "@tauri-apps/plugin-dialog";
import { useWorkdayStore, type TimeEntry, type TimeEntryHistoryDay } from "../store";
import TaskSelectControl, { type TaskSelectIntent } from "./TaskSelectControl.vue";
import { playCompletionFeedback } from "../services/completionFeedback";
import { clampPartialAllocationMinutes } from "../services/allocationMath";
import { historyDateLabel, historyPaginationState } from "../services/timeEntryHistoryCore";
import { executeTimeAllocationExport, previewTimeAllocationExport, type TimeAllocationExportPreview } from "../services/tauri";
import { defaultExportDraft, ensureXlsxExtension, exportRangeError, LatestRequestGuard } from "../services/timeAllocationExportCore";

type AllocationDraft = {
  id: string;
  taskId: string;
  minutes: number;
  completeTask: boolean;
};

type TimeCorrectionDraft = {
  startedAt: string;
  endedAt: string;
  note: string;
};

type TaskSelectControlInstance = {
  commit: () => string | undefined;
  focus: () => void;
};

const store = useWorkdayStore();
const expandedEntryId = ref(store.selectedEntryId);
const manualOpen = ref(false);
const manualTaskId = ref(store.selectedTaskId);
const manualMinutes = ref(30);
const manualNote = ref("");
const manualCompleteTask = ref(false);
const timerNote = ref("");
const recentManualEntryId = ref("");
const allocationDrafts = reactive<Record<string, AllocationDraft[]>>({});
const correctionDrafts = reactive<Record<string, TimeCorrectionDraft>>({});
const timerTaskControl = ref<TaskSelectControlInstance>();
const manualTaskControl = ref<TaskSelectControlInstance>();
const allocationTaskControls = new Map<string, TaskSelectControlInstance>();
const timerBusy = ref(false);
const manualSaving = ref(false);
const allocationSavingId = ref("");
const exportOpen = ref(false);
const exportPreviewLoading = ref(false);
const exportSaving = ref(false);
const exportPreview = ref<TimeAllocationExportPreview>();
const exportError = ref("");
const exportDraft = reactive({
  startDate: "",
  endDate: "",
  includeSpecialTime: false,
});
const exportRequestGuard = new LatestRequestGuard();
let exportPreviewTimer: ReturnType<typeof window.setTimeout> | undefined;

const manualMinutesValid = computed(() => Number.isFinite(Number(manualMinutes.value)) && Number(manualMinutes.value) >= 1);
const historyPagination = computed(() => historyPaginationState(store.timeHistoryPage, store.timeHistoryTotalDays));
const exportValidationError = computed(() => exportRangeError(exportDraft.startDate, exportDraft.endDate));
const exportReady = computed(() => (exportPreview.value?.rowCount ?? 0) > 0
  && !exportValidationError.value
  && !exportPreviewLoading.value
  && !exportSaving.value);

const elapsedSeconds = computed(() => {
  const entry = store.runningEntry;
  return entry ? store.entryDurationSeconds(entry) : 0;
});

const clockText = computed(() => formatClock(elapsedSeconds.value));

function setAllocationTaskControl(allocationId: string, component: unknown) {
  if (component && typeof component === "object" && "commit" in component) {
    allocationTaskControls.set(allocationId, component as TaskSelectControlInstance);
  } else {
    allocationTaskControls.delete(allocationId);
  }
}

watch(() => store.runningEntry?.id, (currentId, previousId) => {
  if (!currentId && previousId) expandedEntryId.value = store.selectedEntryId;
});

watch(() => store.selectedEntryId, (entryId) => {
  const entry = store.selectedEntry;
  if (!entry || entry.state !== "ended") return;
  expandedEntryId.value = entry.id;
  ensureDraft(entry);
  ensureCorrectionDraft(entry);
  void nextTick(() => {
    document.querySelector<HTMLElement>(`[data-entry-id="${entry.id}"]`)?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  });
});

watch(() => store.timeHistoryGroups.flatMap((group) => group.entries.map((entry) => entry.id)), (entryIds) => {
  const visibleIds = new Set(entryIds);
  for (const entryId of Object.keys(allocationDrafts)) {
    if (!visibleIds.has(entryId)) delete allocationDrafts[entryId];
  }
  for (const entryId of Object.keys(correctionDrafts)) {
    if (!visibleIds.has(entryId)) delete correctionDrafts[entryId];
  }
  if (expandedEntryId.value && !visibleIds.has(expandedEntryId.value)) expandedEntryId.value = "";
  const selected = store.selectedEntry;
  if (selected?.state === "ended" && visibleIds.has(selected.id)) {
    expandedEntryId.value = selected.id;
    ensureDraft(selected);
    ensureCorrectionDraft(selected);
    void nextTick(() => {
      document.querySelector<HTMLElement>(`[data-entry-id="${selected.id}"]`)?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    });
  }
});

onMounted(() => {
  void store.loadTimeHistory(store.timeHistoryPage).catch((error) => {
    ElMessage.error(errorText(error, "时间记录加载失败"));
  });
});

onBeforeUnmount(() => {
  if (exportPreviewTimer !== undefined) window.clearTimeout(exportPreviewTimer);
  exportRequestGuard.invalidate();
});

watch(
  () => [exportDraft.startDate, exportDraft.endDate, exportDraft.includeSpecialTime] as const,
  () => {
    if (exportOpen.value) scheduleExportPreview();
  },
);

watch(manualOpen, (open) => {
  if (!open) return;
  manualTaskId.value = store.selectedTaskId;
  manualMinutes.value = defaultManualMinutes(manualTaskId.value);
});

watch(manualTaskId, (taskId) => {
  if (!manualOpen.value) return;
  manualMinutes.value = defaultManualMinutes(taskId);
});

watch(() => store.selectedTaskId, (taskId) => {
  if (!manualOpen.value) manualTaskId.value = taskId;
});

function formatClock(seconds: number) {
  const hours = Math.floor(seconds / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  const rest = seconds % 60;
  return [hours, minutes, rest].map((value) => String(value).padStart(2, "0")).join(":");
}

function formatMinutes(minutes: number) {
  if (minutes < 60) return `${minutes} 分钟`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? `${hours} 小时 ${rest} 分钟` : `${hours} 小时`;
}

function formatAllocationProgress(entry: TimeEntry) {
  if (entry.kind === "break") return formatMinutes(entryMinutes(entry));
  return `${entry.allocated} / ${entryMinutes(entry)} 分钟`;
}

function defaultManualMinutes(taskId?: string) {
  const task = store.tasks.find((item) => item.id === taskId);
  return task?.todayEstimate ?? task?.estimate ?? 30;
}

function entryMinutes(entry: TimeEntry) {
  return Math.ceil(store.entryDurationSeconds(entry) / 60);
}

function formatTime(timestamp?: number) {
  if (!timestamp) return "现在";
  return new Intl.DateTimeFormat("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false }).format(timestamp);
}

function entryStatus(entry: TimeEntry) {
  if (entry.kind === "break") return "休息";
  if (entry.state === "running") return "进行中";
  if (entry.state === "paused") return "已暂停";
  if (entry.allocated >= entry.minutes && entry.minutes > 0) return "已归属";
  return "待确认";
}

function entryStatusClass(entry: TimeEntry) {
  if (entry.kind === "break") return "break";
  if (entry.state === "running") return "running";
  if (entry.state === "paused") return "paused";
  return entry.allocated >= entry.minutes && entry.minutes > 0 ? "settled" : "pending";
}

function toggleEntry(entry: TimeEntry) {
  store.selectedEntryId = entry.id;
  if (entry.state !== "ended") return;
  expandedEntryId.value = expandedEntryId.value === entry.id ? "" : entry.id;
  ensureDraft(entry);
  ensureCorrectionDraft(entry);
}

function ensureDraft(entry: TimeEntry) {
  if (allocationDrafts[entry.id]) return allocationDrafts[entry.id];
  allocationDrafts[entry.id] = entry.allocations?.length
    ? entry.allocations.map((allocation, index) => ({
      id: allocation.id ?? `${entry.id}-allocation-${index + 1}`,
      taskId: allocation.taskId,
      minutes: allocation.minutes,
      completeTask: false,
    }))
    : [{
      id: `${entry.id}-allocation-1`,
      taskId: entry.defaultTask ?? store.selectedTaskId,
      minutes: entry.allocated,
      completeTask: false,
    }];
  return allocationDrafts[entry.id];
}

function formatHistoryDate(workDate: string) {
  return historyDateLabel(workDate, store.workspaceToday);
}

function formatHistorySummary(group: TimeEntryHistoryDay) {
  return group.entryCount + " 段 · 原始工作 " + group.allocatedMinutes + " 分钟 · 实际调整 " + (group.adjustmentMinutes >= 0 ? "+" : "") + group.adjustmentMinutes + " 分钟 · 最终实际 " + group.finalActualMinutes + " 分钟 · 待确认 " + group.pendingMinutes + " 分钟";
}

async function removeActualAdjustment(adjustment: TimeEntryHistoryDay["adjustments"][number]) {
  const message = "确定删除“" + adjustment.taskTitle + "”的 " + (adjustment.deltaMinutes >= 0 ? "+" : "") + adjustment.deltaMinutes + " 分钟实际调整吗？";
  try {
    await ElMessageBox.confirm(message, "删除实际调整", { confirmButtonText: "删除", cancelButtonText: "取消", type: "warning" });
    await store.removeActualAdjustment(adjustment.id, adjustment.version);
    ElMessage.success("实际调整已撤销");
  } catch (error) { if (error !== "cancel" && error !== "close") ElMessage.error(errorText(error, "撤销实际调整失败")); }
}

async function openExportDialog() {
  try {
    if (!store.workspaceToday) await store.refreshWorkspaceCalendar();
    Object.assign(exportDraft, defaultExportDraft(store.workspaceToday));
    exportPreview.value = undefined;
    exportError.value = "";
    await nextTick();
    exportOpen.value = true;
    await refreshExportPreview();
  } catch (error) {
    ElMessage.error(errorText(error, "无法打开导出设置"));
  }
}

function scheduleExportPreview() {
  if (exportPreviewTimer !== undefined) window.clearTimeout(exportPreviewTimer);
  exportRequestGuard.invalidate();
  exportPreview.value = undefined;
  exportError.value = exportValidationError.value;
  if (exportValidationError.value) {
    exportPreviewLoading.value = false;
    return;
  }
  exportPreviewLoading.value = true;
  exportPreviewTimer = window.setTimeout(() => {
    exportPreviewTimer = undefined;
    void refreshExportPreview();
  }, 250);
}

async function refreshExportPreview() {
  if (exportPreviewTimer !== undefined) {
    window.clearTimeout(exportPreviewTimer);
    exportPreviewTimer = undefined;
  }
  if (exportValidationError.value) {
    exportRequestGuard.invalidate();
    exportPreview.value = undefined;
    exportError.value = exportValidationError.value;
    exportPreviewLoading.value = false;
    return;
  }
  const requestId = exportRequestGuard.next();
  exportPreviewLoading.value = true;
  exportError.value = "";
  try {
    const preview = await previewTimeAllocationExport({ ...exportDraft });
    if (!exportRequestGuard.isCurrent(requestId)) return;
    exportPreview.value = preview;
  } catch (error) {
    if (!exportRequestGuard.isCurrent(requestId)) return;
    exportPreview.value = undefined;
    exportError.value = errorText(error, "无法预览导出内容");
  } finally {
    if (exportRequestGuard.isCurrent(requestId)) exportPreviewLoading.value = false;
  }
}

function closeExportDialog() {
  if (exportSaving.value) return;
  exportOpen.value = false;
  exportRequestGuard.invalidate();
  if (exportPreviewTimer !== undefined) {
    window.clearTimeout(exportPreviewTimer);
    exportPreviewTimer = undefined;
  }
}

function handleExportClosed() {
  exportRequestGuard.invalidate();
  exportPreviewLoading.value = false;
  if (exportPreviewTimer !== undefined) {
    window.clearTimeout(exportPreviewTimer);
    exportPreviewTimer = undefined;
  }
}

async function exportTimeAllocations() {
  if (!exportReady.value || !exportPreview.value) return;
  exportSaving.value = true;
  try {
    const target = await save({
      title: "导出工时归属",
      defaultPath: exportPreview.value.defaultFileName,
      filters: [{ name: "Excel 工作簿", extensions: ["xlsx"] }],
    });
    if (!target) return;
    const result = await executeTimeAllocationExport({
      ...exportDraft,
      targetPath: ensureXlsxExtension(target),
    });
    exportOpen.value = false;
    ElMessage.success(`已导出 ${result.rowCount} 行、${result.totalMinutes} 分钟到 ${result.path}`);
  } catch (error) {
    exportError.value = errorText(error, "工时导出失败，请重新选择保存位置");
    ElMessage.error(exportError.value);
  } finally {
    exportSaving.value = false;
  }
}

async function changeHistoryPage(page: number) {
  try {
    await store.selectTimeHistoryPage(page);
  } catch (error) {
    ElMessage.error(errorText(error, "时间记录翻页失败"));
  }
}

function ensureCorrectionDraft(entry: TimeEntry) {
  correctionDrafts[entry.id] ??= {
    startedAt: toDateTimeLocal(entry.startedAt),
    endedAt: toDateTimeLocal(entry.endedAt ?? Date.now()),
    note: entry.note,
  };
  return correctionDrafts[entry.id];
}

function allocationTotal(entry: TimeEntry) {
  return ensureDraft(entry).reduce((sum, item) => sum + Math.max(0, Number(item.minutes) || 0), 0);
}

function allocationRemaining(entry: TimeEntry) {
  return entry.minutes - allocationTotal(entry);
}

function addAllocation(entry: TimeEntry) {
  const drafts = ensureDraft(entry);
  drafts.push({
    id: `${entry.id}-allocation-${Date.now()}`,
    taskId: store.selectedTaskId,
    minutes: Math.max(0, allocationRemaining(entry)),
    completeTask: false,
  });
}

function removeAllocation(entry: TimeEntry, allocationId: string) {
  const drafts = ensureDraft(entry);
  if (drafts.length === 1) return;
  const index = drafts.findIndex((item) => item.id === allocationId);
  if (index < 0) return;
  drafts.splice(index, 1);
}

function updateAllocationMinutes(entry: TimeEntry, index: number, event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  const drafts = ensureDraft(entry);
  const nextMinutes = clampPartialAllocationMinutes(
    drafts.map((item) => item.minutes),
    index,
    input.value,
  );
  nextMinutes.forEach((minutes, currentIndex) => drafts[currentIndex].minutes = minutes);
  input.value = String(nextMinutes[index]);
}

async function saveAllocation(entry: TimeEntry) {
  for (const draft of ensureDraft(entry)) {
    const taskId = allocationTaskControls.get(draft.id)?.commit();
    if (taskId) draft.taskId = taskId;
  }
  allocationSavingId.value = entry.id;
  try {
    const result = await store.allocate(entry.id, ensureDraft(entry).map((draft) => ({
      taskId: draft.taskId,
      minutes: Math.max(0, Number(draft.minutes) || 0),
      completeTask: draft.completeTask,
    })));
    delete allocationDrafts[entry.id];
    delete correctionDrafts[entry.id];
    expandedEntryId.value = "";
    ElMessage.success("工时归属已保存");
    if (result?.completedCount) playCompletionFeedback({ completedCount: result.completedCount });
  } catch (error) {
    ElMessage.error(errorText(error, "工时归属保存失败"));
  } finally {
    allocationSavingId.value = "";
  }
}

async function clearAllocation(entry: TimeEntry) {
  for (const draft of ensureDraft(entry)) draft.minutes = 0;
  ElMessage.info("已清空草稿用时，点击确认归属后保存");
}

async function markEntryDisposition(entry: TimeEntry, disposition: "break" | "discard") {
  allocationSavingId.value = entry.id;
  try {
    await store.setTimeEntryDisposition(entry.id, disposition);
    delete allocationDrafts[entry.id];
    delete correctionDrafts[entry.id];
    expandedEntryId.value = "";
    ElMessage.success(disposition === "break" ? "已标记为休息时间" : "已标记为无效时间");
  } catch (error) {
    ElMessage.error(errorText(error, disposition === "break" ? "休息时间保存失败" : "无效时间保存失败"));
  } finally {
    allocationSavingId.value = "";
  }
}

async function submitTimerIntent(intent: TaskSelectIntent) {
  timerBusy.value = true;
  try {
    const result = await store.transitionTimer(intent, timerNote.value);
    timerNote.value = "";
    if (result.previousStopResult && !result.noOp) ElMessage.success("已切换事项，上一段计时可稍后确认归属");
  } catch (error) {
    ElMessage.error(errorText(error, store.runningEntry ? "切换计时失败" : "开始计时失败"));
  } finally {
    timerBusy.value = false;
  }
}

async function saveCorrection(entry: TimeEntry) {
  const draft = ensureCorrectionDraft(entry);
  const startedAt = new Date(draft.startedAt).getTime();
  const endedAt = new Date(draft.endedAt).getTime();
  if (!Number.isFinite(startedAt) || !Number.isFinite(endedAt) || endedAt <= startedAt) {
    ElMessage.warning("结束时间必须晚于开始时间");
    return;
  }
  allocationSavingId.value = entry.id;
  try {
    await store.correctTimeEntry(entry.id, startedAt, endedAt, draft.note);
    delete correctionDrafts[entry.id];
    delete allocationDrafts[entry.id];
    ElMessage.success("时间记录已修正");
  } catch (error) {
    ElMessage.error(errorText(error, "时间记录修正失败"));
  } finally {
    allocationSavingId.value = "";
  }
}

function toDateTimeLocal(timestamp: number) {
  const date = new Date(timestamp);
  const offset = date.getTimezoneOffset() * 60_000;
  return new Date(timestamp - offset).toISOString().slice(0, 16);
}

async function submitManualEntry() {
  const taskId = manualTaskControl.value?.commit() ?? manualTaskId.value;
  const minutes = Number(manualMinutes.value);
  if (!Number.isFinite(minutes) || minutes < 1) {
    ElMessage.warning("补录用时不能少于 1 分钟");
    return;
  }
  manualSaving.value = true;
  try {
    const result = await store.addManualEntry(taskId || undefined, Math.round(minutes), manualNote.value.trim(), manualCompleteTask.value);
    const entryId = result.entryId;
    recentManualEntryId.value = entryId;
    manualOpen.value = false;
    manualMinutes.value = defaultManualMinutes(manualTaskId.value);
    manualNote.value = "";
    manualCompleteTask.value = false;
    ElMessage.success("已补录用时");
    if (result.completedCount) playCompletionFeedback({ completedCount: result.completedCount });
    void nextTick(() => {
      document.querySelector<HTMLElement>(`[data-entry-id="${entryId}"]`)?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    });
    window.setTimeout(() => {
      if (recentManualEntryId.value === entryId) recentManualEntryId.value = "";
    }, 1_800);
  } catch (error) {
    ElMessage.error(errorText(error, "手动补录失败"));
  } finally {
    manualSaving.value = false;
  }
}

async function runTimerAction(action: () => Promise<unknown>, fallback: string) {
  timerBusy.value = true;
  try {
    await action();
  } catch (error) {
    ElMessage.error(errorText(error, fallback));
  } finally {
    timerBusy.value = false;
  }
}

function errorText(error: unknown, fallback: string) {
  if (typeof error === "string" && error.trim()) return error;
  return error instanceof Error ? error.message : fallback;
}

async function focusTaskSelector() {
  if (store.runningEntry) return;
  await nextTick();
  timerTaskControl.value?.focus();
}

defineExpose({ focusTaskSelector });
</script>

<template>
  <section class="time-workspace">
    <div
      class="time-focus-panel"
      :class="{
        active: store.runningEntry?.state === 'running',
        paused: store.runningEntry?.state === 'paused'
      }"
    >
      <div class="time-focus-overview">
        <div class="time-focus-state">
          <span class="time-focus-icon"><Clock3 :size="18" /></span>
          <div>
            <span>{{ store.runningEntry?.state === 'paused' ? '计时已暂停' : store.runningEntry ? '正在计时' : '准备开始' }}</span>
            <strong>{{ store.runningEntry ? store.timeEntryLabel(store.runningEntry) : '选择一个事项' }}</strong>
          </div>
        </div>
        <time>{{ store.runningEntry ? clockText : '00:00:00' }}</time>
      </div>

      <div class="time-focus-controls">
        <div class="time-focus-selector">
          <span>{{ store.runningEntry ? '切换事项' : '计时事项' }}</span>
          <TaskSelectControl
            ref="timerTaskControl"
            v-model="store.selectedTaskId"
            variant="focus"
            intent-mode
            :busy="timerBusy"
            :placeholder="store.runningEntry ? '选择或输入新事项并切换' : '选择或输入新事项'"
            :aria-label="store.runningEntry ? '切换计时事项' : '选择计时事项'"
            @submit="submitTimerIntent"
          />
          <input v-model="timerNote" class="time-focus-note" placeholder="备注（可选）" aria-label="计时备注" />
        </div>
        <div class="time-focus-actions">
          <button v-if="store.runningEntry?.state === 'running'" class="secondary-button" :disabled="timerBusy" @click="runTimerAction(store.pauseTimer, '暂停计时失败')"><Pause :size="15" fill="currentColor" />暂停</button>
          <button v-else-if="store.runningEntry?.state === 'paused'" class="primary-button" :disabled="timerBusy" @click="runTimerAction(store.resumeTimer, '继续计时失败')"><Play :size="15" fill="currentColor" />继续计时</button>
          <button v-if="store.runningEntry" class="danger-button" :disabled="timerBusy" @click="runTimerAction(store.stopTimer, '结束计时失败')"><Square :size="13" fill="currentColor" />结束计时</button>
        </div>
      </div>
    </div>

    <div class="time-history-head">
      <div>
        <h2>时间记录</h2>
        <span>共 {{ store.timeHistoryTotalDays }} 个有记录日期 · 每页 7 天</span>
      </div>
      <div class="time-history-actions">
        <button class="secondary-button compact" type="button" @click="openExportDialog"><Download :size="14" />导出</button>
        <button class="secondary-button compact" type="button" @click="manualOpen = !manualOpen"><Plus :size="14" />手动补录</button>
      </div>
    </div>

    <form v-if="manualOpen" class="manual-entry-row" @submit.prevent="submitManualEntry">
      <TaskSelectControl
        ref="manualTaskControl"
        v-model="manualTaskId"
        placeholder="选择或输入临时事项"
        aria-label="补录事项"
      />
      <label><span>分钟</span><input v-model.number="manualMinutes" min="1" type="number" /></label>
      <input v-model="manualNote" placeholder="备注（可选）" />
      <label class="allocation-complete-toggle manual-complete-toggle" title="保存补录工时时同时完成这个事项">
        <input v-model="manualCompleteTask" type="checkbox" />
        <span>同时完成</span>
      </label>
      <button class="primary-button compact" type="submit" :disabled="!manualMinutesValid || manualSaving"><Check :size="14" />{{ manualSaving ? '保存中' : '保存' }}</button>
    </form>

    <div v-if="store.timeHistoryLoading && !store.timeHistoryGroups.length" class="time-history-empty">正在加载时间记录…</div>
    <div v-else-if="!store.timeHistoryGroups.length" class="time-history-empty">还没有时间记录，开始计时或手动补录后会显示在这里。</div>
    <div v-else class="time-history-groups">
      <section v-for="group in store.timeHistoryGroups" :key="group.workDate" class="time-history-group">
        <header class="time-history-group-head">
          <strong>{{ formatHistoryDate(group.workDate) }}</strong>
          <span>{{ formatHistorySummary(group) }}</span>
        </header>
        <div class="time-entry-list">
          <article v-for="adjustment in group.adjustments" :key="adjustment.id" class="time-entry actual-adjustment-history-item">
            <div class="time-entry-summary">
              <span class="time-entry-icon ended"><TimerReset :size="15" /></span>
              <span class="time-entry-range">{{ formatTime(adjustment.createdAt) }}</span>
              <span class="time-entry-title"><strong>{{ adjustment.taskTitle }}</strong><small>{{ adjustment.note || '无备注' }}</small></span>
              <span class="time-entry-duration actual-adjustment-history-delta">{{ adjustment.deltaMinutes >= 0 ? '+' : '' }}{{ adjustment.deltaMinutes }} 分钟</span>
              <span class="time-entry-status ended">实际调整</span>
              <button class="icon-button subtle" type="button" title="删除并撤销此调整" @click="removeActualAdjustment(adjustment)"><Trash2 :size="14" /></button>
            </div>
          </article>
          <article
            v-for="entry in group.entries"
            :key="entry.id"
            class="time-entry"
            :class="{ expanded: expandedEntryId === entry.id, 'just-added': recentManualEntryId === entry.id }"
            :data-entry-id="entry.id"
          >
            <button class="time-entry-summary" type="button" @click="toggleEntry(entry)">
              <span class="time-entry-icon" :class="entryStatusClass(entry)">
                <Play v-if="entry.state === 'running'" :size="14" fill="currentColor" />
                <Pause v-else-if="entry.state === 'paused'" :size="14" fill="currentColor" />
                <Coffee v-else-if="entry.kind === 'break'" :size="15" />
                <CircleDot v-else :size="16" />
              </span>
              <span class="time-entry-range">{{ formatTime(entry.startedAt) }} - {{ formatTime(entry.endedAt) }}</span>
              <span class="time-entry-title"><strong>{{ store.timeEntryLabel(entry) }}</strong><small>{{ entry.note || '无备注' }}</small></span>
              <span class="time-entry-duration">{{ formatAllocationProgress(entry) }}</span>
              <span class="time-entry-status" :class="entryStatusClass(entry)">{{ entryStatus(entry) }}</span>
              <ChevronDown v-if="entry.state === 'ended' && expandedEntryId === entry.id" :size="15" />
              <ChevronRight v-else-if="entry.state === 'ended'" :size="15" />
              <span v-else class="time-entry-live" aria-hidden="true"></span>
            </button>

            <div v-if="entry.state === 'ended' && expandedEntryId === entry.id" class="inline-allocation-editor">
              <div class="time-correction-row">
                <label><span>开始</span><input v-model="ensureCorrectionDraft(entry).startedAt" type="datetime-local" /></label>
                <label><span>结束</span><input v-model="ensureCorrectionDraft(entry).endedAt" type="datetime-local" /></label>
                <label class="time-correction-note"><span>备注</span><input v-model="ensureCorrectionDraft(entry).note" placeholder="可选" /></label>
                <button class="secondary-button compact" type="button" :disabled="allocationSavingId === entry.id" @click="saveCorrection(entry)">修正时间</button>
              </div>
              <div class="allocation-summary-line">
                <span><Split :size="14" />分配 {{ allocationTotal(entry) }} / {{ entry.minutes }} 分钟</span>
                <strong>
                  {{ allocationRemaining(entry) >= 0 ? `剩余 ${allocationRemaining(entry)} 分钟` : `超出 ${Math.abs(allocationRemaining(entry))} 分钟` }}
                </strong>
              </div>
              <div v-for="(allocation, allocationIndex) in ensureDraft(entry)" :key="allocation.id" class="inline-allocation-row">
                <TaskSelectControl
                  :ref="(component) => setAllocationTaskControl(allocation.id, component)"
                  v-model="allocation.taskId"
                  variant="compact"
                  placeholder="选择或输入临时事项"
                  aria-label="分配事项"
                />
                <label><input :value="allocation.minutes" min="0" type="number" @input="updateAllocationMinutes(entry, allocationIndex, $event)" /><span>分钟</span></label>
                <label class="allocation-complete-toggle" title="保存工时时同时完成这个事项">
                  <input v-model="allocation.completeTask" type="checkbox" />
                  <span>同时完成</span>
                </label>
                <button class="icon-button subtle" type="button" title="删除此分配" :disabled="ensureDraft(entry).length === 1" @click="removeAllocation(entry, allocation.id)"><Trash2 :size="14" /></button>
              </div>
              <button class="inline-add-allocation" type="button" @click="addAllocation(entry)"><Plus :size="14" />添加事项</button>
              <div class="inline-allocation-actions">
                <div class="inline-allocation-secondary-actions">
                  <button class="secondary-button compact" type="button" :disabled="allocationSavingId === entry.id" @click="markEntryDisposition(entry, 'break')"><Coffee :size="14" />休息时间</button>
                  <button class="secondary-button compact discard" type="button" :disabled="allocationSavingId === entry.id" @click="markEntryDisposition(entry, 'discard')"><Trash2 :size="14" />无效时间</button>
                </div>
                <div class="inline-allocation-primary-actions">
                  <button class="secondary-button compact" type="button" :disabled="allocationSavingId === entry.id || allocationTotal(entry) <= 0" @click="clearAllocation(entry)"><Eraser :size="14" />清空用时</button>
                  <button class="secondary-button compact" type="button" :disabled="allocationSavingId === entry.id" @click="ensureDraft(entry).forEach((draft) => draft.minutes = draft.taskId === (entry.defaultTask ?? store.selectedTaskId) ? entry.minutes : 0)"><RotateCcw :size="14" />填满用时</button>
                  <button class="primary-button compact" type="button" :disabled="allocationSavingId === entry.id" @click="saveAllocation(entry)"><Check :size="14" />确认归属</button>
                </div>
              </div>
            </div>
          </article>
        </div>
      </section>
    </div>

    <nav v-if="store.timeHistoryTotalDays > 0" class="time-history-pagination" aria-label="时间记录分页">
      <button class="secondary-button compact" type="button" :disabled="store.timeHistoryLoading || !historyPagination.canGoPrevious" @click="changeHistoryPage(store.timeHistoryPage - 1)">上一页</button>
      <span>第 {{ store.timeHistoryPage }} / {{ store.timeHistoryTotalPages }} 页 · 共 {{ store.timeHistoryTotalDays }} 个日期</span>
      <button class="secondary-button compact" type="button" :disabled="store.timeHistoryLoading || !historyPagination.canGoNext" @click="changeHistoryPage(store.timeHistoryPage + 1)">下一页</button>
    </nav>

    <el-dialog
      v-model="exportOpen"
      class="time-export-dialog"
      width="min(480px, calc(100vw - 28px))"
      title="导出工时归属"
      :close-on-click-modal="!exportSaving"
      :close-on-press-escape="!exportSaving"
      :show-close="!exportSaving"
      @closed="handleExportClosed"
    >
      <div class="time-export-form">
        <div class="time-export-range">
          <label><span>开始日期</span><input v-model="exportDraft.startDate" type="date" /></label>
          <label><span>结束日期</span><input v-model="exportDraft.endDate" type="date" /></label>
        </div>
        <label class="time-export-special">
          <input v-model="exportDraft.includeSpecialTime" type="checkbox" />
          <span><strong>包含特殊时间</strong><small>同时导出未归属时间和休息记录</small></span>
        </label>
        <div class="time-export-preview" :class="{ error: !!exportError }">
          <template v-if="exportValidationError">
            <strong>日期范围需要调整</strong>
            <span>{{ exportValidationError }}</span>
          </template>
          <template v-else-if="exportPreviewLoading">
            <strong>正在统计…</strong>
            <span>根据当前选择计算最终 Excel 内容</span>
          </template>
          <template v-else-if="exportError">
            <strong>暂时无法导出</strong>
            <span>{{ exportError }}</span>
          </template>
          <template v-else-if="exportPreview">
            <strong>{{ exportPreview.rowCount ? `预计导出 ${exportPreview.rowCount} 行` : '没有可导出的工时' }}</strong>
            <span>{{ exportPreview.rowCount ? `合计 ${exportPreview.totalMinutes} 分钟，只统计最终写入的行` : '调整日期，或选择包含特殊时间后再试' }}</span>
          </template>
        </div>
      </div>
      <template #footer>
        <div class="time-export-footer">
          <span>导出不会触发云端同步或增加待保存项</span>
          <div>
            <button class="secondary-button compact" type="button" :disabled="exportSaving" @click="closeExportDialog">取消</button>
            <button class="primary-button compact" type="button" :disabled="!exportReady" @click="exportTimeAllocations"><Download :size="14" />{{ exportSaving ? '正在导出' : '选择位置并导出' }}</button>
          </div>
        </div>
      </template>
    </el-dialog>
  </section>
</template>
