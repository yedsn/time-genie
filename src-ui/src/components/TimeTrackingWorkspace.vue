<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { Check, ChevronDown, ChevronRight, CircleDot, Coffee, Download, Pause, Play, Plus, Split, TimerReset, Trash2 } from "lucide-vue-next";
import { save } from "@tauri-apps/plugin-dialog";
import { useWorkdayStore, type TimeEntry, type TimeEntryHistoryDay } from "../store";
import TaskSelectControl from "./TaskSelectControl.vue";
import { playCompletionFeedback } from "../services/completionFeedback";
import { historyDateLabel, historyPaginationState } from "../services/timeEntryHistoryCore";
import { executeTimeAllocationExport, previewTimeAllocationExport, type TimeAllocationExportPreview } from "../services/tauri";
import { defaultExportDraft, ensureXlsxExtension, exportRangeError, LatestRequestGuard } from "../services/timeAllocationExportCore";

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
const emit = defineEmits<{
  openEntryAllocation: [entryId: string, subjectId?: string];
}>();
const expandedEntryId = ref(store.selectedEntryId);
const manualOpen = ref(false);
const manualTaskId = ref(store.selectedTaskId);
const manualMinutes = ref(30);
const manualNote = ref("");
const manualCompleteTask = ref(false);
const recentManualEntryId = ref("");
const correctionDrafts = reactive<Record<string, TimeCorrectionDraft>>({});
const manualTaskControl = ref<TaskSelectControlInstance>();
const manualSaving = ref(false);
const correctionSavingId = ref("");
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

watch(() => store.runningEntry?.id, (currentId, previousId) => {
  if (!currentId && previousId) expandedEntryId.value = store.selectedEntryId;
});

watch(() => store.selectedEntryId, (entryId) => {
  const entry = store.selectedEntry;
  if (!entry || entry.state !== "ended") return;
  expandedEntryId.value = entry.id;
  if (entry.workDate === store.workspaceToday) ensureCorrectionDraft(entry);
  void nextTick(() => {
    document.querySelector<HTMLElement>(`[data-entry-id="${entry.id}"]`)?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  });
});

watch(() => store.timeHistoryGroups.flatMap((group) => group.entries.map((entry) => entry.id)), (entryIds) => {
  const visibleIds = new Set(entryIds);
  for (const entryId of Object.keys(correctionDrafts)) {
    if (!visibleIds.has(entryId)) delete correctionDrafts[entryId];
  }
  if (expandedEntryId.value && !visibleIds.has(expandedEntryId.value)) expandedEntryId.value = "";
  const selected = store.selectedEntry;
  if (selected?.state === "ended" && visibleIds.has(selected.id)) {
    expandedEntryId.value = selected.id;
    if (selected.workDate === store.workspaceToday) ensureCorrectionDraft(selected);
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
  if (entry.workDate === store.workspaceToday) ensureCorrectionDraft(entry);
}

function entryAllocationSummary(entry: TimeEntry) {
  if (entry.kind === "break") return ["休息时间"];
  if (!entry.allocations?.length) return [entry.allocated ? `已归属 ${entry.allocated} 分钟` : "未归属"];
  return entry.allocations.map((allocation) => `${store.taskDisplayLabel(allocation.taskId)} · ${allocation.minutes} 分钟`);
}

function entrySubjectId(entry: TimeEntry) {
  const allocationSubjectId = entry.allocations?.find((allocation) => allocation.subjectId)?.subjectId;
  if (allocationSubjectId) return allocationSubjectId;
  const taskId = entry.allocations?.[0]?.taskId ?? entry.defaultTask;
  return [...store.todayPlanTasks, ...store.tasks].find((task) => task.id === taskId)?.subjectId ?? store.selectedSubjectId;
}

function openEntryAllocation(entry: TimeEntry) {
  if (entry.workDate !== store.workspaceToday) return;
  emit("openEntryAllocation", entry.id, entrySubjectId(entry));
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
  } catch (error) {
    if (error === "cancel" || error === "close") return;
    const message = errorText(error, "撤销实际调整失败");
    if (message.startsWith("HISTORICAL_RECORD_READ_ONLY:")) {
      await store.refreshWorkspaceCalendar();
      await store.loadTimeHistory(store.timeHistoryPage);
      ElMessage.warning(message.replace(/^HISTORICAL_RECORD_READ_ONLY:\s*/, ""));
      return;
    }
    ElMessage.error(message);
  }
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

async function saveCorrection(entry: TimeEntry) {
  const draft = ensureCorrectionDraft(entry);
  const startedAt = new Date(draft.startedAt).getTime();
  const endedAt = new Date(draft.endedAt).getTime();
  if (!Number.isFinite(startedAt) || !Number.isFinite(endedAt) || endedAt <= startedAt) {
    ElMessage.warning("结束时间必须晚于开始时间");
    return;
  }
  correctionSavingId.value = entry.id;
  try {
    await store.correctTimeEntry(entry.id, startedAt, endedAt, draft.note);
    delete correctionDrafts[entry.id];
    ElMessage.success("时间记录已修正");
  } catch (error) {
    const message = errorText(error, "时间记录修正失败");
    if (message.startsWith("HISTORICAL_RECORD_READ_ONLY:")) {
      delete correctionDrafts[entry.id];
      expandedEntryId.value = "";
      await store.refreshWorkspaceCalendar();
      await store.loadTimeHistory(store.timeHistoryPage);
      ElMessage.warning(message.replace(/^HISTORICAL_RECORD_READ_ONLY:\s*/, ""));
    } else {
      ElMessage.error(message);
    }
  } finally {
    correctionSavingId.value = "";
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

function errorText(error: unknown, fallback: string) {
  if (typeof error === "string" && error.trim()) return error;
  return error instanceof Error ? error.message : fallback;
}

</script>

<template>
  <section class="time-workspace">
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
    <div v-else-if="!store.timeHistoryGroups.length" class="time-history-empty">还没有时间记录，开始执行事项或手动补录后会显示在这里。</div>
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
              <button v-if="group.workDate === store.workspaceToday" class="icon-button subtle" type="button" title="删除并撤销此调整" @click="removeActualAdjustment(adjustment)"><Trash2 :size="14" /></button>
              <span v-else class="time-entry-readonly-mark">历史只读</span>
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
              <div v-if="entry.workDate === store.workspaceToday" class="time-correction-row">
                <label><span>开始</span><input v-model="ensureCorrectionDraft(entry).startedAt" type="datetime-local" /></label>
                <label><span>结束</span><input v-model="ensureCorrectionDraft(entry).endedAt" type="datetime-local" /></label>
                <label class="time-correction-note"><span>备注</span><input v-model="ensureCorrectionDraft(entry).note" placeholder="可选" /></label>
                <button class="secondary-button compact" type="button" :disabled="correctionSavingId === entry.id" @click="saveCorrection(entry)">修正时间</button>
              </div>
              <div class="time-entry-allocation-summary">
                <div class="allocation-summary-line">
                  <span><Split :size="14" />已保存归属</span>
                  <strong>{{ entry.allocated }} / {{ entry.minutes }} 分钟</strong>
                </div>
                <div class="time-entry-allocation-tags">
                  <span v-for="item in entryAllocationSummary(entry)" :key="item">{{ item }}</span>
                </div>
                <div v-if="entry.workDate === store.workspaceToday" class="inline-allocation-actions">
                  <span>归属、休息或丢弃都在执行事项面板中处理。</span>
                  <button class="primary-button compact" type="button" @click="openEntryAllocation(entry)"><Split :size="14" />调整归属</button>
                </div>
                <div v-else class="time-entry-history-readonly">历史记录只读，仅展示已保存的时间与归属。</div>
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
