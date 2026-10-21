<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { AlertCircle, Check, CheckCircle2, CornerDownRight, GripVertical, IndentDecrease, IndentIncrease, ListTodo, LoaderCircle, MessageSquareText, MoreHorizontal, Play, Plus, Search, Trash2 } from "lucide-vue-next";
import { useWorkdayStore, type Task } from "../store";
import type { ActualAdjustmentSnapshotTaskRecord } from "../services/tauri";
import {
  executionPanelAction,
  executionPanelCurrentStatus,
  executionPanelPresentation,
  executionPanelRows,
  executionPanelSourceTasks,
  type ExecutionPanelMode,
  type ExecutionPanelTaskRow,
} from "../services/executionTaskPanelCore";
import { contextMenuPosition, taskSubtreeIds } from "../services/planTaskInteractionCore";
import { createTaskTreeSortable } from "../services/taskTreeSortable";
import { parseActualMinutes } from "../services/quickActualAdjustmentCore";
import { readRememberedPanelSubjectId, rememberPanelSubjectId } from "../services/executionPanelSubjectPreference";
import {
  beginTimerDialogSave,
  createTimerDialogDraft,
  resolveTimerDialogSubject,
  settleTimerDialogSave,
  timerDialogDraftDirty,
  updateTimerDialogDraft,
  type TimerDialogDraft,
  type TimerDialogDraftField,
} from "../services/timerTaskDialogCore";

type PanelTask = Task & { totalMinutes: number; completedToday?: boolean; adjustmentFingerprint?: string; timerState?: "running" | "paused"; isLeaf?: boolean; canAcceptChildren?: boolean };

const props = withDefaults(defineProps<{ modelValue: boolean; contextSubjectId?: string; timerNote?: string; mode?: ExecutionPanelMode; entryId?: string }>(), {
  mode: "execute",
});
const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  goPlan: [subjectId: string];
  timerStarted: [];
}>();

const store = useWorkdayStore();
const subjectId = ref("");
const query = ref("");
const searchRef = ref<HTMLInputElement>();
const rowsRef = ref<HTMLElement>();
const returnFocusElement = ref<HTMLElement>();
const drafts = reactive<Record<string, TimerDialogDraft>>({});
const saveChains = new Map<string, Promise<boolean>>();
const timerBusyTaskId = ref("");
const recentTaskId = ref("");
const noteTaskId = ref("");
const moreTaskId = ref("");
const closing = ref(false);
const moving = ref(false);
const actualTaskId = ref("");
const actualTargetText = ref("");
const actualNote = ref("");
const actualError = ref("");
const actualSaving = ref(false);
const allocationDrafts = reactive<Record<string, { minutes: number; completeTask: boolean }>>({});
const allocationTaskTitles = reactive<Record<string, string>>({});
const allocationSaving = ref(false);
const allocationError = ref("");
const allocationOpenedSessionId = ref("");
const allocationOpenedSessionVersion = ref(0);
const allocationOpenedEntryVersion = ref(0);
const allocationBaseline = ref("");
const newTask = reactive({ open: false, parentId: "", targetId: "", placement: "after" as "before" | "after", title: "", saving: false, error: "" });
const rowContextMenuOpen = ref(false);
const rowContextTaskId = ref("");
const rowContextMenuLeft = ref(0);
const rowContextMenuTop = ref(0);
const todayPickerOpen = ref(false);
const todayPickerQuery = ref("");
const todayPickerSelected = ref<string[]>([]);
const moreDrafts = reactive<Record<string, { estimate: string; planDate: string; recurrence: "none" | "daily" | "weekdays" | "weekly"; recurrenceStart: string; recurrenceWeekdays: number[] }>>({});
const weekdayLabels = ["一", "二", "三", "四", "五", "六", "日"];

const activeTask = computed(() => [...store.todayPlanTasks, ...store.tasks].find((task) => task.id === store.runningEntry?.defaultTask));
const presentation = computed(() => executionPanelPresentation(props.mode));
const snapshot = computed(() => store.executionSnapshots[subjectId.value]);
const panelInitialLoading = computed(() => props.mode === "execute"
  ? store.todayPlanLoading
  : !snapshot.value && (store.workspaceLoading || store.executionSnapshotLoading[subjectId.value]));
function snapshotTaskToPanelTask(task: ActualAdjustmentSnapshotTaskRecord): PanelTask {
  const live = [...store.todayPlanTasks, ...store.tasks].find((item) => item.id === task.id);
  return {
    id: task.id,
    subjectId: task.subjectId,
    parentId: task.parentId,
    title: task.title,
    project: live?.project ?? "",
    planDate: task.plannedDate,
    estimate: task.estimateMinutes,
    todayEstimate: task.todayEstimateMinutes,
    dailyEstimateVersion: task.dailyEstimateVersion,
    dayPlanState: task.dayPlanState,
    dayPlanVersion: task.dayPlanVersion,
    todayMember: task.todayMember,
    todayContext: task.todayContext,
    actual: task.totalMinutes,
    totalMinutes: task.totalMinutes,
    directMinutes: task.directMinutes,
    status: task.status,
    note: task.note,
    sortOrder: task.sortOrder,
    completedAt: task.completedAt,
    version: task.version,
    selectable: task.selectable,
    executionState: task.status === "done" ? "done" : task.timerState ?? (task.directMinutes > 0 ? "started" : "not_started"),
    recurrence: live?.recurrence,
    occurrenceDate: live?.occurrenceDate,
    occurrenceOrigin: live?.occurrenceOrigin,
    occurrenceVersion: live?.occurrenceVersion,
    completedToday: task.completedToday,
    adjustmentFingerprint: task.adjustmentFingerprint,
    timerState: task.timerState,
    isLeaf: task.isLeaf,
    canAcceptChildren: task.canAcceptChildren,
  };
}
const panelTasks = computed<PanelTask[]>(() => {
  if (props.mode === "execute") {
    const todayPlanPanelTasks = store.todayPlanTasks
      .map((task) => ({ ...task, totalMinutes: task.actual, completedToday: task.status === "done" }));
    return executionPanelSourceTasks(todayPlanPanelTasks, subjectId.value, props.mode);
  }
  return snapshot.value?.tasks.map(snapshotTaskToPanelTask) ?? store.tasks.map((task) => ({ ...task, totalMinutes: task.actual, completedToday: false }));
});
const rows = computed(() => executionPanelRows<PanelTask>(panelTasks.value, subjectId.value, query.value));
const openLeafCount = computed(() => rows.value.filter((row) => !row.hasChildren && !row.task.completedToday).length);
const completedLeafCount = computed(() => rows.value.filter((row) => !row.hasChildren && row.task.completedToday).length);
const currentStatus = computed(() => executionPanelCurrentStatus({
  taskId: store.runningEntry?.defaultTask,
  taskTitle: activeTask.value?.title,
  entryLabel: store.runningEntry?.label,
  entryState: store.runningEntry?.state,
  candidateTaskIds: panelTasks.value.map((task) => task.id),
}));
const currentSubject = computed(() => store.subjects.find((subject) => subject.id === subjectId.value));
const todayPlannedIds = computed(() => new Set(store.todayPlanTasks
  .filter((task) => task.todayMember && task.dayInstanceSourceTaskId)
  .map((task) => task.dayInstanceSourceTaskId!)));
const todayPickerCandidates = computed(() => {
  const keyword = todayPickerQuery.value.trim().toLocaleLowerCase();
  return store.tasks
    .filter((task) => task.subjectId === subjectId.value && task.status === "open" && task.selectable && !todayPlannedIds.value.has(task.id))
    .filter((task) => !keyword || `${task.pathLabel ?? task.title} ${task.note ?? ""}`.toLocaleLowerCase().includes(keyword));
});
const actualTask = computed(() => panelTasks.value.find((task) => task.id === actualTaskId.value));
const actualBaseline = computed(() => actualTask.value?.directMinutes ?? 0);
const actualParsed = computed(() => parseActualMinutes(actualTargetText.value));
const actualDelta = computed(() => actualParsed.value.ok ? actualParsed.value.minutes - actualBaseline.value : 0);
const allocationEntry = computed(() => props.entryId ? store.timeHistoryGroups.flatMap((group) => group.entries).find((entry) => entry.id === props.entryId) ?? store.entries.find((entry) => entry.id === props.entryId) : undefined);
const allocationRequiredMinutes = computed(() => props.mode === "unassigned-allocation"
  ? Math.max(1, Math.ceil(store.unassignedSeconds / 60))
  : Math.max(0, allocationEntry.value?.minutes ?? 0));
const allocationTotalMinutes = computed(() => Object.values(allocationDrafts).reduce((sum, draft) => sum + Math.max(0, Math.round(Number(draft.minutes) || 0)), 0));
const allocationRemainingMinutes = computed(() => allocationRequiredMinutes.value - allocationTotalMinutes.value);
const allocationDraftSignature = computed(() => JSON.stringify(Object.entries(allocationDrafts)
  .map(([taskId, draft]) => [taskId, Math.max(0, Math.round(Number(draft.minutes) || 0)), Boolean(draft.completeTask)] as const)
  .filter(([, minutes, completeTask]) => minutes > 0 || completeTask)
  .sort(([left], [right]) => left.localeCompare(right))));
const allocationDirty = computed(() => presentation.value.showAllocation && allocationDraftSignature.value !== allocationBaseline.value);
const selectedAllocations = computed(() => Object.entries(allocationDrafts)
  .map(([taskId, draft]) => ({
    taskId,
    title: panelTasks.value.find((task) => task.id === taskId)?.title
      ?? allocationTaskTitles[taskId]
      ?? allocationEntry.value?.allocations?.find((allocation) => allocation.taskId === taskId)?.taskTitle
      ?? store.taskDisplayLabel(taskId),
    minutes: Math.max(0, Math.round(Number(draft.minutes) || 0)),
    completeTask: Boolean(draft.completeTask),
  }))
  .filter((item) => item.minutes > 0 || item.completeTask));
const allocationPeriodLabel = computed(() => {
  if (props.mode === "unassigned-allocation") {
    const startedAt = store.unassignedFirstStartedAt ?? store.unassignedStartedAt;
    if (!startedAt) return "未归属时间";
    const endedAt = store.unassignedStartedAt ? undefined : store.unassignedLastEndedAt;
    return `${formatTime(startedAt)} - ${endedAt ? formatTime(endedAt) : "现在"}`;
  }
  const entry = allocationEntry.value;
  return entry ? `${formatTime(entry.startedAt)} - ${entry.endedAt ? formatTime(entry.endedAt) : "现在"}` : "时间记录";
});

watch([() => props.modelValue, () => props.mode, () => props.entryId], async ([open]) => {
  if (!open) { closeRowContextMenu(); return; }
  closeRowContextMenu();
  returnFocusElement.value = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  subjectId.value = resolveTimerDialogSubject({
    subjects: store.subjects,
    contextSubjectId: props.contextSubjectId,
    rememberedSubjectId: readRememberedPanelSubjectId(),
    selectedSubjectId: store.selectedSubjectId,
    activeTaskSubjectId: activeTask.value?.subjectId,
  });
  query.value = "";
  for (const taskId of Object.keys(drafts)) delete drafts[taskId];
  for (const taskId of Object.keys(moreDrafts)) delete moreDrafts[taskId];
  noteTaskId.value = "";
  moreTaskId.value = "";
  resetNewTask();
  initializeAllocationDrafts();
  allocationBaseline.value = allocationDraftSignature.value;
  allocationOpenedSessionId.value = props.mode === "unassigned-allocation" ? store.unassignedSessionId : "";
  allocationOpenedSessionVersion.value = props.mode === "unassigned-allocation" ? store.unassignedVersion : 0;
  allocationOpenedEntryVersion.value = props.mode === "entry-allocation" ? allocationEntry.value?.version ?? 0 : 0;
  if (props.mode === "execute") await store.loadTodayPlan(subjectId.value);
  else await store.loadExecutionSnapshot(subjectId.value);
}, { immediate: true });

watch(() => store.subjects.map((subject) => subject.id).join("|"), () => {
  if (!store.subjects.some((subject) => subject.id === subjectId.value)) {
    subjectId.value = resolveTimerDialogSubject({
      subjects: store.subjects,
      rememberedSubjectId: readRememberedPanelSubjectId(),
      selectedSubjectId: store.selectedSubjectId,
    });
  }
});

watch(subjectId, (currentSubjectId) => rememberPanelSubjectId(currentSubjectId));

function subtreeIds(taskId: string) {
  return taskSubtreeIds(rows.value.map((row) => row.task), taskId);
}

const treeSortable = createTaskTreeSortable({
  container: () => rowsRef.value,
  rowSelector: ".timer-task-row[data-timer-task-id]",
  handleSelector: ".timer-task-drag-handle",
  taskIdAttribute: "data-timer-task-id",
  disabled: () => Boolean(query.value.trim()),
  subtreeIds,
  parentId: (taskId) => rows.value.find((row) => row.task.id === taskId)?.task.parentId,
  onStart: () => { moving.value = true; },
  async onIntent(taskId, intent) {
    try {
      const task = panelTasks.value.find((item) => item.id === taskId);
      if (!task) throw new Error("事项已不存在");
      await store.moveExecutionTask(task.id, task.version, task.subjectId, intent.parentId, intent.previousSiblingId, intent.nextSiblingId);
    } catch (error) {
      ElMessage.warning(errorText(error, "事项移动失败"));
      if (props.mode === "execute") await store.loadTodayPlan(subjectId.value);
      else await store.loadExecutionSnapshot(subjectId.value);
    }
  },
  onInvalid: () => { ElMessage.warning("该落点会产生三级事项，请选择一级或同级位置"); },
  async onSettled() {
    moving.value = false;
    await nextTick();
    treeSortable.mount();
  },
});

watch([() => props.modelValue, rows, query], async ([open]) => {
  if (!open) { treeSortable.destroy(); return; }
  await nextTick();
  treeSortable.mount();
}, { flush: "post" });

onMounted(() => {
  document.addEventListener("pointerdown", closeRowContextMenuForPointer);
  document.addEventListener("keydown", closeRowContextMenuForKey);
  window.addEventListener("scroll", closeRowContextMenu, true);
});

onBeforeUnmount(() => {
  treeSortable.destroy();
  document.removeEventListener("pointerdown", closeRowContextMenuForPointer);
  document.removeEventListener("keydown", closeRowContextMenuForKey);
  window.removeEventListener("scroll", closeRowContextMenu, true);
});

function prepareSubtreeDrag(taskId: string) {
  treeSortable.prepareSubtree(taskId);
}

async function keyboardMove(task: PanelTask, direction: "indent" | "outdent") {
  try {
    const ordered = rows.value.map((row) => row.task);
    if (direction === "indent") {
      const index = ordered.findIndex((item) => item.id === task.id);
      const previousRoot = ordered.slice(0, index).reverse().find((item) => !item.parentId);
      if (!previousRoot) throw new Error("前面没有可作为父事项的一级事项");
      await store.moveExecutionTask(task.id, task.version, task.subjectId, previousRoot.id);
    } else {
      if (!task.parentId) return;
      const parentIndex = ordered.findIndex((item) => item.id === task.parentId);
      const nextRoot = ordered.slice(parentIndex + 1).find((item) => !item.parentId);
      await store.moveExecutionTask(task.id, task.version, task.subjectId, undefined, task.parentId, nextRoot?.id);
    }
  } catch (error) {
    ElMessage.warning(errorText(error, "层级调整失败"));
  }
}

async function confirmAddExistingTodayTasks() {
  if (!todayPickerSelected.value.length) return;
  try {
    await store.addTasksToToday([...todayPickerSelected.value]);
    await store.loadTodayPlan(subjectId.value);
    todayPickerSelected.value = [];
    todayPickerQuery.value = "";
    todayPickerOpen.value = false;
  } catch (error) {
    ElMessage.error(errorText(error, "加入今日失败"));
  }
}

function draftFor(task: PanelTask) {
  const existing = drafts[task.id];
  if (existing) return existing;
  const draft = createTimerDialogDraft(task);
  drafts[task.id] = draft;
  return draft;
}

function actionFor(task: PanelTask, hasChildren: boolean) {
  return executionPanelAction({
    mode: props.mode,
    taskId: task.id,
    taskTitle: draftFor(task).title || task.title || "未命名事项",
    hasChildren,
    completedToday: task.completedToday,
    activeTaskId: store.runningEntry?.defaultTask,
    activeState: store.runningEntry?.state,
  });
}

function updateDraft(task: PanelTask, field: TimerDialogDraftField, value: string) {
  updateTimerDialogDraft(draftFor(task), field, value);
}

function initializeAllocationDrafts() {
  for (const taskId of Object.keys(allocationDrafts)) delete allocationDrafts[taskId];
  for (const taskId of Object.keys(allocationTaskTitles)) delete allocationTaskTitles[taskId];
  allocationError.value = "";
  if (props.mode !== "entry-allocation") return;
  for (const allocation of allocationEntry.value?.allocations ?? []) {
    allocationDrafts[allocation.taskId] = { minutes: allocation.minutes, completeTask: false };
    if (allocation.taskTitle) allocationTaskTitles[allocation.taskId] = allocation.taskTitle;
  }
}

function allocationDraft(taskId: string, title?: string) {
  if (title) allocationTaskTitles[taskId] = title;
  return allocationDrafts[taskId] ??= { minutes: 0, completeTask: false };
}

function allocateRemaining(taskId: string, title?: string) {
  const remaining = Math.max(0, allocationRemainingMinutes.value);
  if (!remaining) return;
  allocationDraft(taskId, title).minutes += remaining;
  allocationError.value = "";
}

function clearAllocationDrafts() {
  for (const draft of Object.values(allocationDrafts)) {
    draft.minutes = 0;
    draft.completeTask = false;
  }
}

function clearAllocationDraft(taskId: string) {
  const draft = allocationDraft(taskId);
  draft.minutes = 0;
  draft.completeTask = false;
}

async function saveAllocationDrafts() {
  if (!presentation.value.showAllocation || allocationSaving.value) return;
  const allocations = Object.entries(allocationDrafts)
    .map(([taskId, draft]) => ({ taskId, minutes: Math.max(0, Math.round(Number(draft.minutes) || 0)), completeTask: draft.completeTask }))
    .filter((allocation) => allocation.minutes > 0 || allocation.completeTask);
  if (!allocations.length) { allocationError.value = "请至少分配 1 分钟到事项"; return; }
  if (allocationRemainingMinutes.value !== 0) {
    allocationError.value = allocationRemainingMinutes.value > 0 ? `还剩 ${allocationRemainingMinutes.value} 分钟未分配` : `已超出 ${Math.abs(allocationRemainingMinutes.value)} 分钟`;
    return;
  }
  allocationSaving.value = true;
  allocationError.value = "";
  try {
    if (props.mode === "unassigned-allocation") {
      if (store.unassignedSessionId !== allocationOpenedSessionId.value || store.unassignedVersion !== allocationOpenedSessionVersion.value) {
        throw new Error("UNASSIGNED_CONTEXT_CHANGED: 未归属时间已变化，请重新打开后分配");
      }
      const saved = await store.resolveUnassignedTime("work", allocations);
      if (!saved) throw new Error("未归属时间已变化，请刷新后重试");
      ElMessage.success("未归属时间已分配到事项");
    } else if (allocationEntry.value) {
      if (allocationEntry.value.version !== allocationOpenedEntryVersion.value) {
        throw new Error("TIME_ENTRY_CONTEXT_CHANGED: 时间记录已变化，请重新打开后调整");
      }
      await store.allocate(allocationEntry.value.id, allocations);
      ElMessage.success("时间归属已保存");
    } else {
      throw new Error("时间记录已不存在");
    }
    emit("update:modelValue", false);
  } catch (error) {
    allocationError.value = errorText(error, "时间分配保存失败");
    if (props.mode === "unassigned-allocation") await store.loadUnassignedState();
    if (allocationError.value.startsWith("HISTORICAL_RECORD_READ_ONLY:") || allocationError.value.startsWith("TIME_ENTRY_CONTEXT_CHANGED:")) {
      await store.loadTimeHistory(store.timeHistoryPage);
      ElMessage.warning(allocationError.value.replace(/^[A-Z_]+:\s*/, ""));
      emit("update:modelValue", false);
    }
  } finally {
    allocationSaving.value = false;
  }
}

async function resolveAllocationDisposition(disposition: "break" | "discard") {
  if (!presentation.value.showBreakDiscard || allocationSaving.value) return;
  allocationSaving.value = true;
  try {
    if (props.mode === "unassigned-allocation") {
      if (store.unassignedSessionId !== allocationOpenedSessionId.value || store.unassignedVersion !== allocationOpenedSessionVersion.value) {
        throw new Error("UNASSIGNED_CONTEXT_CHANGED: 未归属时间已变化，请重新打开后处理");
      }
      const saved = await store.resolveUnassignedTime(disposition);
      if (!saved) throw new Error("未归属时间已变化，请刷新后重试");
    } else if (allocationEntry.value) {
      if (allocationEntry.value.version !== allocationOpenedEntryVersion.value) {
        throw new Error("TIME_ENTRY_CONTEXT_CHANGED: 时间记录已变化，请重新打开后调整");
      }
      await store.setTimeEntryDisposition(allocationEntry.value.id, disposition);
    } else {
      throw new Error("时间记录已不存在");
    }
    emit("update:modelValue", false);
    ElMessage.success(disposition === "break" ? "已记录为休息时间" : "已丢弃这段时间");
  } catch (error) {
    allocationError.value = errorText(error, "未归属时间处理失败");
    if (allocationError.value.startsWith("HISTORICAL_RECORD_READ_ONLY:") || allocationError.value.startsWith("TIME_ENTRY_CONTEXT_CHANGED:")) {
      await store.loadTimeHistory(store.timeHistoryPage);
      ElMessage.warning(allocationError.value.replace(/^[A-Z_]+:\s*/, ""));
      emit("update:modelValue", false);
    }
  } finally {
    allocationSaving.value = false;
  }
}

async function openActualAdjustment(task: PanelTask, hasChildren: boolean) {
  if (hasChildren || !presentation.value.actualEditable) return;
  actualTaskId.value = task.id;
  actualTargetText.value = String(task.directMinutes);
  actualNote.value = "";
  actualError.value = "";
  try {
    const latestSnapshot = await store.loadActualAdjustmentSnapshot(subjectId.value);
    if (actualTaskId.value !== task.id) return;
    const latestTask = latestSnapshot?.tasks.find((item) => item.id === task.id && item.isLeaf);
    if (!latestTask) throw new Error("事项实际基线已失效，请刷新后重试");
    actualTargetText.value = String(latestTask.directMinutes);
  } catch (error) {
    actualError.value = errorText(error, "今日实际加载失败");
  }
}

function closeActualAdjustment() {
  if (actualSaving.value) return;
  actualTaskId.value = "";
  actualTargetText.value = "";
  actualNote.value = "";
  actualError.value = "";
}

function actualAdjustmentDirty() {
  return Boolean(actualTaskId.value && actualParsed.value.ok && actualParsed.value.minutes !== actualBaseline.value);
}

async function setActualAdjustmentVisible(task: PanelTask, visible: boolean) {
  if (visible) { await openActualAdjustment(task, false); return; }
  if (actualSaving.value) return;
  if (actualAdjustmentDirty()) {
    try {
      await ElMessageBox.confirm("今日实际还有未保存修改，确定放弃吗？", "放弃今日实际调整", { confirmButtonText: "放弃", cancelButtonText: "继续编辑", type: "warning" });
    } catch { return; }
  }
  closeActualAdjustment();
}

async function saveActualAdjustment() {
  const task = actualTask.value;
  const parsed = actualParsed.value;
  actualSaving.value = true;
  actualError.value = "";
  try {
  const actualSnapshot = await store.loadActualAdjustmentSnapshot(subjectId.value);
  const actualSnapshotTask = actualSnapshot?.tasks.find((item) => item.id === task?.id && item.isLeaf);
  if (!task || !actualSnapshot || !actualSnapshotTask || !parsed.ok || !actualSnapshotTask.adjustmentFingerprint) {
    actualError.value = parsed.ok ? "事项实际基线已失效，请刷新后重试" : parsed.message;
    return;
  }
  if (parsed.minutes === actualSnapshotTask.directMinutes) { closeActualAdjustment(); return; }
    await store.saveActualAdjustmentBatch({
      subjectId: actualSnapshotTask.subjectId,
      workDate: actualSnapshot.workDate,
      dayInstanceDate: props.mode === "execute" ? store.workspaceToday : undefined,
      clientRequestId: crypto.randomUUID(),
      batchId: crypto.randomUUID(),
      note: actualNote.value.trim() || undefined,
      items: [{
        taskId: actualSnapshotTask.id,
        expectedTaskVersion: actualSnapshotTask.version,
        baselineMinutes: actualSnapshotTask.directMinutes,
        targetMinutes: parsed.minutes,
        deltaMinutes: parsed.minutes - actualSnapshotTask.directMinutes,
        adjustmentFingerprint: actualSnapshotTask.adjustmentFingerprint,
      }],
      newTasks: [],
    });
    if (props.mode === "execute") await store.loadTodayPlan(subjectId.value);
    else await store.loadExecutionSnapshot(subjectId.value);
    actualTaskId.value = "";
    ElMessage.success("今日实际已调整");
  } catch (error) {
    actualError.value = errorText(error, "今日实际调整失败");
    if (actualError.value.startsWith("HISTORICAL_RECORD_READ_ONLY:")) {
      const message = actualError.value.replace(/^HISTORICAL_RECORD_READ_ONLY:\s*/, "");
      closeActualAdjustment();
      await store.refreshWorkspaceCalendar();
      await Promise.all([store.loadExecutionSnapshot(subjectId.value), store.loadTimeHistory(store.timeHistoryPage)]);
      ElMessage.warning(message);
    }
  } finally {
    actualSaving.value = false;
  }
}

function activeSegmentMinutes(task: PanelTask) {
  if (store.runningEntry?.defaultTask !== task.id) return 0;
  return Math.max(0, Math.ceil(store.entryDurationSeconds(store.runningEntry) / 60));
}

function errorText(error: unknown, fallback: string) {
  if (typeof error === "string" && error.trim()) return error;
  return error instanceof Error ? error.message : fallback;
}

async function focusTaskControl(taskId: string, field?: TimerDialogDraftField) {
  await nextTick();
  await new Promise<void>((resolve) => window.requestAnimationFrame(() => window.requestAnimationFrame(() => resolve())));
  const selector = field
    ? `[data-timer-task-id=\"${CSS.escape(taskId)}\"] [data-timer-task-field=\"${field}\"]`
    : `[data-timer-task-id=\"${CSS.escape(taskId)}\"] .timer-task-primary-action`;
  document.querySelector<HTMLElement>(selector)?.focus();
}

function restoreTriggerFocus() {
  const target = returnFocusElement.value;
  returnFocusElement.value = undefined;
  if (target?.isConnected) target.focus();
}

async function focusInitialControl() {
  await nextTick();
  searchRef.value?.focus();
}

function enqueueSave(taskId: string, operation: () => Promise<boolean>) {
  const previous = saveChains.get(taskId) ?? Promise.resolve(true);
  const next = previous.catch(() => false).then(operation);
  saveChains.set(taskId, next);
  void next.finally(() => {
    if (saveChains.get(taskId) === next) saveChains.delete(taskId);
  });
  return next;
}

async function saveField(task: PanelTask, field: TimerDialogDraftField) {
  const draft = draftFor(task);
  if (!timerDialogDraftDirty(draft, field)) return !draft.errors[field];
  const token = beginTimerDialogSave(draft, field);
  const value = draft[field];
  return enqueueSave(task.id, async () => {
    let mutatedStoreTitle = false;
    try {
      const source = task.dayInstanceSourceTaskId
        ? store.tasks.find((item) => item.id === task.dayInstanceSourceTaskId)
        : undefined;
      let applyToFuture = false;
      if (props.mode === "execute" && source?.recurrence) {
        const scope = await ElMessageBox.confirm(
          `要把“${task.title}”的修改应用到今后的重复事项吗？`,
          "修改重复事项",
          { confirmButtonText: "修改今后全部", cancelButtonText: "仅修改今天", distinguishCancelAndClose: true, type: "info" },
        ).then(() => "future" as const).catch((action) => action === "cancel" ? "today" as const : "closed" as const);
        if (scope === "closed") return false;
        applyToFuture = scope === "future";
      }
      if (field === "title") {
        const title = value.trim();
        if (!title) throw new Error("事项名称不能为空");
        const target = [...store.todayPlanTasks, ...store.tasks].find((item) => item.id === task.id);
        if (target) { target.title = title; mutatedStoreTitle = true; }
        await store.saveExecutionTaskTitle(task.id, task.version, title, task.subjectId, applyToFuture);
        settleTimerDialogSave(draft, token, { ok: true, savedValue: title });
      } else {
        await store.saveExecutionTaskNote(task.id, task.version, value, task.subjectId, applyToFuture);
        settleTimerDialogSave(draft, token, { ok: true, savedValue: value });
      }
      return true;
    } catch (error) {
      settleTimerDialogSave(draft, token, { ok: false, message: errorText(error, "保存失败") });
      if (field === "title" && mutatedStoreTitle) {
        if (props.mode === "execute") await store.loadTodayPlan(task.subjectId);
        else await store.loadWorkspaceData();
      }
      if (timerBusyTaskId.value === task.id) await focusTaskControl(task.id, field);
      return false;
    }
  });
}

async function flushTask(task: PanelTask) {
  for (const field of ["title", "note"] as const) {
    while (timerDialogDraftDirty(draftFor(task), field)) {
      if (!await saveField(task, field)) return false;
    }
  }
  const pending = saveChains.get(task.id);
  return pending ? await pending : true;
}

async function startOrSwitch(task: PanelTask, hasChildren: boolean) {
  const action = actionFor(task, hasChildren);
  if (!action.actionable || timerBusyTaskId.value) return;
  if (action.mode === "allocate") { allocateRemaining(task.id, task.title); return; }
  timerBusyTaskId.value = task.id;
  let focusActionAfterFailure = false;
  try {
    if (!await flushTask(task)) return;
    const result = await store.transitionTimer({
      taskId: task.id,
      reopenCompletedTaskVersion: action.mode === "continue" ? task.version : undefined,
    }, props.timerNote ?? "");
    if (result.previousStopResult && !result.noOp) ElMessage.success("已切换事项，上一段计时可稍后确认归属");
    emit("timerStarted");
    emit("update:modelValue", false);
  } catch (error) {
    const draft = draftFor(task);
    draft.errors.title = errorText(error, action.mode === "switch" ? "切换计时失败" : "开始计时失败");
    await Promise.all([store.loadTimeData(), store.loadWorkspaceData(), store.loadUnassignedState()]);
    focusActionAfterFailure = true;
  } finally {
    timerBusyTaskId.value = "";
    if (focusActionAfterFailure) await focusTaskControl(task.id);
  }
}

async function selectSubject(nextSubjectId: string) {
  if (nextSubjectId === subjectId.value) return;
  if (actualAdjustmentDirty()) {
    try {
      await ElMessageBox.confirm("今日实际还有未保存修改，切换主体会放弃这些修改。", "切换主体", { confirmButtonText: "放弃并切换", cancelButtonText: "继续编辑", type: "warning" });
      closeActualAdjustment();
    } catch { return; }
  }
  if (newTask.open && newTask.title.trim()) {
    try {
      await ElMessageBox.confirm("新增事项还没有保存，切换主体会放弃该草稿。", "切换主体", { confirmButtonText: "放弃并切换", cancelButtonText: "继续编辑", type: "warning" });
    } catch { return; }
  }
  const dirtyTasks = panelTasks.value.filter((task) => task.subjectId === subjectId.value).filter((task) => {
    const draft = drafts[task.id];
    return draft && (["title", "note"] as const).some((field) => timerDialogDraftDirty(draft, field));
  });
  for (const task of dirtyTasks) if (!await flushTask(task)) return;
  subjectId.value = nextSubjectId;
  query.value = "";
  resetNewTask();
  if (props.mode === "execute") await store.loadTodayPlan(nextSubjectId);
  else await store.loadExecutionSnapshot(nextSubjectId);
}

function openNewTask(parentId = "") {
  closeRowContextMenu();
  newTask.open = true;
  newTask.parentId = parentId;
  newTask.targetId = "";
  newTask.placement = "after";
  newTask.title = "";
  newTask.error = "";
}

function openRelativeTask(task: PanelTask, placement: "before" | "after") {
  closeRowContextMenu();
  newTask.open = true;
  newTask.parentId = task.parentId ?? "";
  newTask.targetId = task.id;
  newTask.placement = placement;
  newTask.title = "";
  newTask.error = "";
}

function resetNewTask() {
  newTask.open = false;
  newTask.parentId = "";
  newTask.targetId = "";
  newTask.placement = "after";
  newTask.title = "";
  newTask.saving = false;
  newTask.error = "";
}

async function createNewTask(startAfterCreate = false) {
  const title = newTask.title.trim();
  if (!title) { newTask.error = "事项名称不能为空"; return; }
  newTask.saving = true;
  newTask.error = "";
  try {
    if (startAfterCreate && props.mode === "execute" && !newTask.targetId) {
      const parent = newTask.parentId ? store.todayPlanTasks.find((task) => task.id === newTask.parentId) : undefined;
      const result = await store.transitionTimer({
        newTask: {
          subjectId: subjectId.value,
          parentId: newTask.parentId || undefined,
          expectedParentVersion: parent?.version,
          title,
          dayInstanceDate: store.workspaceToday,
        },
      }, props.timerNote ?? "");
      resetNewTask();
      if (result.previousStopResult && !result.noOp) ElMessage.success("已切换事项，上一段计时可稍后确认归属");
      emit("timerStarted");
      emit("update:modelValue", false);
      return;
    }
    const taskId = newTask.targetId
      ? await store.createTaskRelative(newTask.targetId, newTask.placement, title)
      : await (async () => {
        const tempId = store.addTaskSelectionDraft(title, subjectId.value, newTask.parentId || undefined);
        if (!tempId) throw new Error("新事项创建失败");
        const tempTask = store.tasks.find((task) => task.id === tempId);
        if (!tempTask) throw new Error("新事项创建失败");
        const savedTaskId = await store.saveTask(tempId);
        if (!savedTaskId) throw new Error("新事项保存失败");
        return savedTaskId;
      })();
    if (props.mode === "execute") {
      try {
        await store.addTasksToToday([taskId]);
        await store.loadTodayPlan(subjectId.value);
      } catch (error) {
        throw new Error(`事项已创建，但加入今日失败：${errorText(error, "请稍后重试")}`);
      }
    } else {
      await store.loadExecutionSnapshot(subjectId.value);
    }
    recentTaskId.value = taskId;
    resetNewTask();
    window.setTimeout(() => { if (recentTaskId.value === taskId) recentTaskId.value = ""; }, 1_200);
    if (startAfterCreate) {
      const result = await store.transitionTimer({ taskId }, props.timerNote ?? "");
      if (result.previousStopResult && !result.noOp) ElMessage.success("已切换事项，上一段计时可稍后确认归属");
      emit("timerStarted");
      emit("update:modelValue", false);
    }
  } catch (error) {
    newTask.error = errorText(error, "新事项保存失败");
  } finally {
    newTask.saving = false;
  }
}

function rowContextTask() {
  return panelTasks.value.find((task) => task.id === rowContextTaskId.value);
}

function openRowContextMenu(event: MouseEvent, task: PanelTask) {
  event.preventDefault();
  event.stopPropagation();
  const position = contextMenuPosition(event.clientX, event.clientY, 190, 198, window.innerWidth, window.innerHeight);
  rowContextTaskId.value = task.id;
  rowContextMenuLeft.value = position.left;
  rowContextMenuTop.value = position.top;
  rowContextMenuOpen.value = true;
}

function closeRowContextMenu() {
  rowContextMenuOpen.value = false;
  rowContextTaskId.value = "";
}

function closeRowContextMenuForPointer(event: PointerEvent) {
  if (!(event.target as HTMLElement).closest(".task-row-context-menu")) closeRowContextMenu();
}

function closeRowContextMenuForKey(event: KeyboardEvent) {
  if (event.key === "Escape") closeRowContextMenu();
}

async function duplicateCurrentRow(task: PanelTask) {
  closeRowContextMenu();
  try {
    const taskId = await store.duplicateSingleTask(task.id);
    if (!taskId) throw new Error("事项复制失败");
    if (props.mode === "execute") {
      await store.addTasksToToday([taskId]);
      await store.loadTodayPlan(subjectId.value);
    } else {
      await store.loadExecutionSnapshot(subjectId.value);
    }
    recentTaskId.value = taskId;
    window.setTimeout(() => { if (recentTaskId.value === taskId) recentTaskId.value = ""; }, 1_200);
    ElMessage.success("已复制事项");
  } catch (error) {
    ElMessage.error(errorText(error, "事项复制失败"));
  }
}

function runRowContextCommand(command: "create" | "before" | "after" | "duplicate" | "delete") {
  const task = rowContextTask();
  if (!task) { closeRowContextMenu(); return; }
  if (command === "duplicate") { void duplicateCurrentRow(task); return; }
  if (command === "delete") { closeRowContextMenu(); void deleteTask(task); return; }
  if (command === "create") { openNewTask(); return; }
  openRelativeTask(task, command);
}

function moreDraftFor(task: Task) {
  return moreDrafts[task.id] ??= {
    estimate: task.estimate == null ? "" : String(task.estimate),
    planDate: task.planDate ?? "",
    recurrence: task.recurrence?.frequency ?? "none",
    recurrenceStart: task.recurrence?.effectiveStart ?? store.workspaceToday,
    recurrenceWeekdays: task.recurrence?.weekdaysMask
      ? Array.from({ length: 7 }, (_, index) => index).filter((index) => Boolean(task.recurrence!.weekdaysMask! & (1 << index)))
      : [],
  };
}

async function saveMore(task: PanelTask) {
  const draft = moreDraftFor(task);
  try {
    const estimate = draft.estimate.trim() ? Number(draft.estimate) : undefined;
    if (estimate !== undefined && (!Number.isFinite(estimate) || estimate < 0)) throw new Error("整体预计需为非负分钟");
    const source = task.dayInstanceSourceTaskId
      ? store.tasks.find((item) => item.id === task.dayInstanceSourceTaskId)
      : undefined;
    const currentTitle = draftFor(task).title.trim() || task.title;
    const currentNote = draftFor(task).note.trim();
    const recurringContentChanged = currentTitle !== task.title
      || currentNote !== (task.note ?? "")
      || (estimate === undefined ? task.estimate != null : Math.round(estimate) !== task.estimate);
    let applyToFuture = false;
    if (props.mode === "execute" && source?.recurrence && recurringContentChanged) {
      const scope = await ElMessageBox.confirm(
        `要把“${task.title}”的设置应用到今后的重复事项吗？`,
        "修改重复事项",
        { confirmButtonText: "修改今后全部", cancelButtonText: "仅修改今天", distinguishCancelAndClose: true, type: "info" },
      ).then(() => "future" as const).catch((action) => action === "cancel" ? "today" as const : "closed" as const);
      if (scope === "closed") return;
      applyToFuture = scope === "future";
    }
    const hadLiveTask = [...store.todayPlanTasks, ...store.tasks].some((item) => item.id === task.id);
    await store.saveExecutionTaskDetails({
      id: task.id,
      subjectId: task.subjectId,
      expectedVersion: task.version,
      title: currentTitle,
      plannedDate: draft.planDate || undefined,
      estimateMinutes: estimate === undefined ? undefined : Math.round(estimate),
      note: currentNote,
      projectName: task.project,
    }, applyToFuture);
    const refreshedLiveTask = [...store.todayPlanTasks, ...store.tasks].find((item) => item.id === task.id);
    if (draft.recurrence === "none") {
      if (refreshedLiveTask?.recurrence) await store.setTaskRecurrence(task.id, undefined);
    } else {
      if (!hadLiveTask || !refreshedLiveTask) throw new Error("今天已完成事项不能在此修改重复设置，请继续事项后再调整");
      const weekdaysMask = draft.recurrence === "weekly"
        ? draft.recurrenceWeekdays.reduce((mask, weekday) => mask | (1 << weekday), 0)
        : undefined;
      if (draft.recurrence === "weekly" && !weekdaysMask) throw new Error("每周重复至少选择一天");
      await store.setTaskRecurrence(task.id, { frequency: draft.recurrence, weekdaysMask, effectiveStart: draft.recurrenceStart || store.workspaceToday });
    }
    if (props.mode === "execute") await store.loadTodayPlan(task.subjectId);
    else await store.loadExecutionSnapshot(task.subjectId);
    moreTaskId.value = "";
    ElMessage.success("事项设置已保存");
  } catch (error) {
    ElMessage.error(errorText(error, "事项设置保存失败"));
  }
}

function containsActiveTask(task: Task) {
  const activeId = store.runningEntry?.defaultTask;
  if (!activeId) return false;
  if (task.id === activeId) return true;
  let current = panelTasks.value.find((item) => item.id === activeId);
  while (current?.parentId) {
    if (current.parentId === task.id) return true;
    current = panelTasks.value.find((item) => item.id === current?.parentId);
  }
  return false;
}

async function completeTask(task: Task) {
  if (task.status === "done") return;
  if (containsActiveTask(task)) { ElMessage.warning("请先结束或切换当前计时"); return; }
  try {
    await store.commitTaskToggle(task);
    await Promise.all([store.loadWorkspaceData(), store.loadTodayOverview(), props.mode === "execute" ? store.loadTodayPlan(task.subjectId) : store.loadExecutionSnapshot(task.subjectId)]);
    moreTaskId.value = "";
  } catch (error) { ElMessage.error(errorText(error, "完成事项失败")); }
}

async function deleteTask(task: Task) {
  if (containsActiveTask(task)) { ElMessage.warning("请先结束或切换当前计时"); return; }
  try {
    await ElMessageBox.confirm(`确定删除“${task.title || "未命名事项"}”及其子项吗？`, "删除事项", { confirmButtonText: "删除", cancelButtonText: "取消", type: "warning", customClass: "work-confirm-dialog", confirmButtonClass: "work-confirm-danger" });
    await store.deleteExecutionTask(task.id, task.version, task.subjectId);
    moreTaskId.value = "";
  } catch (error) {
    if (error !== "cancel" && error !== "close") ElMessage.error(errorText(error, "删除事项失败"));
  }
}

async function flushAllDrafts() {
  const dirtyTasks = panelTasks.value.filter((task) => {
    const draft = drafts[task.id];
    return draft && (["title", "note"] as const).some((field) => timerDialogDraftDirty(draft, field));
  });
  for (const task of dirtyTasks) if (!await flushTask(task)) return false;
  return true;
}

async function goPlan() {
  if (!await canCloseDialog()) return;
  emit("update:modelValue", false);
  emit("goPlan", subjectId.value);
}

async function canCloseDialog() {
  if (closing.value) return false;
  closing.value = true;
  try {
    if (newTask.open && newTask.title.trim()) {
      try {
        await ElMessageBox.confirm("新增事项还没有保存，确定放弃吗？", "放弃新增事项", { confirmButtonText: "放弃", cancelButtonText: "继续编辑", type: "warning" });
      } catch { return false; }
    }
    if (actualAdjustmentDirty()) {
      try {
        await ElMessageBox.confirm("今日实际还有未保存修改，确定放弃吗？", "放弃今日实际调整", { confirmButtonText: "放弃", cancelButtonText: "继续编辑", type: "warning" });
        closeActualAdjustment();
      } catch { return false; }
    }
    if (allocationDirty.value) {
      try {
        await ElMessageBox.confirm("时间分配还有未保存修改，确定放弃吗？", "放弃时间分配", { confirmButtonText: "放弃", cancelButtonText: "继续编辑", type: "warning" });
      } catch { return false; }
    }
    return await flushAllDrafts();
  } finally {
    closing.value = false;
  }
}

async function closeDialog() {
  if (await canCloseDialog()) emit("update:modelValue", false);
}

async function beforeClose(done: () => void) {
  if (await canCloseDialog()) done();
}

function formatMinutes(minutes: number) {
  const safe = Math.max(0, Math.round(minutes));
  if (safe < 60) return `${safe}min`;
  const hours = Math.floor(safe / 60);
  const rest = safe % 60;
  return rest ? `${hours}h${rest}min` : `${hours}h`;
}

function estimateCellLabel(row: ExecutionPanelTaskRow<PanelTask>) {
  if (row.totalTodayEstimate <= 0) return `${row.task.title}未设置预计`;
  const value = formatMinutes(row.totalTodayEstimate);
  if (row.hasChildren) return `${row.task.title}子事项预计合计${value}`;
  if (row.estimateSource === "overall") return `${row.task.title}${row.task.recurrence ? "每次预计" : "整体预计"}${value}，今天未单独设置`;
  return `${row.task.title}今日预计${value}`;
}

function formatTime(timestamp: number) {
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
    timeZone: store.workspaceTimezone,
  }).format(timestamp);
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    class="timer-task-dialog"
    modal-class="timer-task-dialog-overlay"
    :title="presentation.title"
    width="min(960px, 96vw)"
    append-to-body
    destroy-on-close
    :before-close="beforeClose"
    @opened="focusInitialControl"
    @closed="restoreTriggerFocus"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <section v-if="props.mode === 'unassigned-allocation'" class="timer-task-allocation-overview" :class="{ over: allocationRemainingMinutes < 0 }" aria-label="未归属时间分配进度">
      <div class="timer-task-allocation-context">
        <span>待处理 · 未归属时间</span>
        <strong>{{ allocationPeriodLabel }}</strong>
        <small>请把这段时间分配到下方事项，也可以记为休息或直接丢弃。</small>
      </div>
      <div class="timer-task-allocation-metrics">
        <div class="timer-task-allocation-metric total">
          <span>总时长</span>
          <strong>{{ allocationRequiredMinutes }}</strong>
          <small>分钟</small>
        </div>
        <div class="timer-task-allocation-metric pending">
          <span>{{ allocationRemainingMinutes >= 0 ? '待分配' : '超出' }}</span>
          <strong>{{ Math.abs(allocationRemainingMinutes) }}</strong>
          <small>分钟</small>
        </div>
        <div class="timer-task-allocation-metric assigned">
          <span>已分配</span>
          <strong>{{ allocationTotalMinutes }}</strong>
          <small>分钟</small>
        </div>
      </div>
    </section>

    <div class="timer-task-dialog-toolbar">
      <div class="timer-task-subject-tabs" role="tablist" aria-label="执行事项主体">
        <button v-for="subject in store.subjects" :key="subject.id" type="button" role="tab" :aria-selected="subjectId === subject.id" :class="{ active: subjectId === subject.id }" @click="selectSubject(subject.id)">{{ subject.name }}</button>
      </div>
      <label class="timer-task-search"><Search :size="15" /><input ref="searchRef" v-model="query" placeholder="搜索当前主体事项" aria-label="搜索执行事项" /></label>
      <button v-if="props.mode === 'execute'" class="secondary-button compact" type="button" @click="todayPickerOpen = true"><ListTodo :size="14" />从进行中加入</button>
    </div>

    <div class="timer-task-dialog-summary">
      <span><strong>{{ currentSubject?.name ?? '当前主体' }}</strong></span>
      <span>{{ openLeafCount }} 个可执行事项<template v-if="completedLeafCount"> · {{ completedLeafCount }} 个今天已完成</template></span>
      <span v-if="props.mode === 'entry-allocation'">{{ allocationPeriodLabel }} · 共 {{ allocationRequiredMinutes }} 分钟 · 已分配 {{ allocationTotalMinutes }} · {{ allocationRemainingMinutes >= 0 ? `待分配 ${allocationRemainingMinutes}` : `超出 ${Math.abs(allocationRemainingMinutes)}` }} 分钟</span>
      <span v-else-if="props.mode === 'execute'">名称和备注会自动保存 · 今日预计请在待办页调整</span>
    </div>
    <div v-if="props.mode === 'execute' && currentStatus" class="timer-task-current-status">
      <div><span class="storage-state-dot online"></span><strong>{{ currentStatus.title }}</strong><small>{{ currentStatus.stateLabel }}</small></div>
      <span v-if="currentStatus.outsideTodayPlan">当前事项未加入今天计划，仅显示计时状态</span>
    </div>
    <div v-if="presentation.showAllocation && selectedAllocations.length" class="timer-task-allocation-selection" aria-label="当前已选时间分配">
      <span v-for="item in selectedAllocations" :key="item.taskId"><strong>{{ item.title }}</strong><template v-if="item.minutes > 0">{{ item.minutes }} 分钟</template><template v-if="item.minutes > 0 && item.completeTask"> · </template><template v-if="item.completeTask">完成</template><button type="button" :aria-label="`移除${item.title}的分配与完成选择`" @click="clearAllocationDraft(item.taskId)">×</button></span>
    </div>
    <div class="timer-task-table-tools"><button class="secondary-button compact" type="button" @click="openNewTask()"><Plus :size="14" />新增事项</button></div>

    <div class="timer-task-list" :class="{ allocation: presentation.showAllocation }" aria-live="polite" :aria-busy="store.workspaceLoading || store.executionSnapshotLoading[subjectId]">
      <div class="timer-task-list-head" aria-hidden="true"><span>事项</span><span>今日预计</span><span>{{ presentation.showAllocation ? '已确认实际' : '已确认实际' }}</span><span>{{ presentation.showAllocation ? '本次分配' : '操作' }}</span></div>
      <div v-if="panelInitialLoading" class="timer-task-empty timer-task-loading" role="status"><LoaderCircle :size="22" class="spin" /><strong>正在加载事项…</strong><span>列表准备好后即可编辑或开始执行。</span></div>

      <div v-else-if="newTask.open" class="timer-task-row new-task" :class="{ child: newTask.parentId }">
        <div class="timer-task-name-cell"><CornerDownRight v-if="newTask.parentId" :size="14" /><input v-model="newTask.title" placeholder="新事项名称" aria-label="新事项名称" @keydown.enter="createNewTask(false)" /></div>
        <span class="timer-task-estimate-cell readonly empty">—</span>
        <span class="timer-task-actual-cell">{{ newTask.targetId ? (newTask.placement === 'before' ? '插入上方' : '插入下方') : '新事项' }}</span>
        <div class="timer-task-actions"><button class="secondary-button compact" type="button" :disabled="newTask.saving" @click="resetNewTask">取消</button><button class="secondary-button compact" type="button" :disabled="newTask.saving" @click="createNewTask(false)">保存</button><button v-if="presentation.showTimerActions" class="primary-button compact" type="button" :disabled="newTask.saving" @click="createNewTask(true)"><LoaderCircle v-if="newTask.saving" :size="13" class="spin" /><Play v-else :size="13" fill="currentColor" />开始</button></div>
        <small v-if="newTask.error" class="timer-task-row-error">{{ newTask.error }}</small>
      </div>

      <div v-if="!panelInitialLoading && rows.length" ref="rowsRef" class="timer-task-sortable-rows" :class="{ moving }">
        <div v-for="row in rows" :key="row.task.id" class="timer-task-row" :data-timer-task-id="row.task.id" :data-depth="row.depth" :class="[{ parent: row.hasChildren, child: row.depth > 0, recent: recentTaskId === row.task.id, active: store.runningEntry?.defaultTask === row.task.id, completed: row.task.completedToday }, `state-${actionFor(row.task, row.hasChildren).mode}`]">
          <div class="timer-task-name-cell" :style="{ '--task-depth': String(row.depth) }">
            <button class="timer-task-drag-handle" type="button" :disabled="Boolean(query.trim())" :class="{ active: rowContextTaskId === row.task.id }" :title="query.trim() ? '清空搜索后调整顺序和层级' : '拖动排序；右键可复制、插入或删除事项'" :aria-label="`${row.task.title}拖动排序，右键打开行菜单`" @pointerdown="prepareSubtreeDrag(row.task.id)" @contextmenu="openRowContextMenu($event, row.task)"><GripVertical :size="14" /></button>
            <CornerDownRight v-if="row.depth" :size="14" />
            <input data-timer-task-field="title" :value="draftFor(row.task).title" :aria-label="`事项名称：${row.task.title}`" @input="updateDraft(row.task, 'title', ($event.target as HTMLInputElement).value)" @blur="saveField(row.task, 'title')" @keydown.enter.prevent="($event.target as HTMLInputElement).blur()" />
            <small v-if="row.hasChildren">{{ row.task.parentId ? '子组' : '父事项' }}<template v-if="row.totalTodayEstimate > 0"> · {{ formatMinutes(row.totalTodayEstimate) }} 预计</template></small>
            <small v-else-if="draftFor(row.task).saving.title">保存中...</small>
            <small v-else-if="draftFor(row.task).errors.title" class="error">{{ draftFor(row.task).errors.title }}</small>
            <small v-else-if="draftFor(row.task).saved.title">已保存</small>
          </div>

          <span class="timer-task-estimate-cell readonly" :class="{ summary: row.hasChildren, empty: row.totalTodayEstimate <= 0 }" :aria-label="estimateCellLabel(row)">
            <strong>{{ row.totalTodayEstimate > 0 ? formatMinutes(row.totalTodayEstimate) : '—' }}</strong>
            <small v-if="row.hasChildren && row.totalTodayEstimate > 0">汇总</small>
            <small v-else-if="row.estimateSource === 'overall' && !row.task.recurrence">整体预计</small>
          </span>

          <div class="timer-task-actual-cell">
            <el-popover v-if="!row.hasChildren && presentation.actualEditable" :visible="actualTaskId === row.task.id" placement="bottom" :width="340" trigger="click" @update:visible="setActualAdjustmentVisible(row.task, $event)">
              <template #reference><button type="button" :aria-label="`调整${row.task.title}今日实际`">{{ formatMinutes(row.confirmedMinutes) }}</button></template>
              <div v-if="actualTaskId === row.task.id" class="actual-adjustment-inline">
                <strong>{{ row.task.title }} · 今日实际</strong>
                <div class="actual-adjustment-inline-summary"><span>当前已确认 <b>{{ formatMinutes(actualBaseline) }}</b></span><span v-if="activeSegmentMinutes(row.task)">当前本段 <b>{{ formatMinutes(activeSegmentMinutes(row.task)) }}</b>，尚未计入</span></div>
                <label><span>目标分钟</span><input v-model="actualTargetText" inputmode="numeric" aria-label="目标实际分钟" /></label>
                <div class="actual-adjustment-inline-delta">本次变化 <strong>{{ actualDelta >= 0 ? '+' : '' }}{{ actualDelta }} 分钟</strong></div>
                <label><span>备注（可选）</span><input v-model="actualNote" placeholder="例如：按工作记录核对" /></label>
                <small v-if="!actualParsed.ok" class="error">{{ actualParsed.message }}</small>
                <small v-else-if="actualError" class="error">{{ actualError }}</small>
                <div class="actual-adjustment-inline-actions"><button class="secondary-button compact" type="button" :disabled="actualSaving" @click="closeActualAdjustment">取消</button><button class="primary-button compact" type="button" :disabled="actualSaving || !actualParsed.ok || actualDelta === 0" @click="saveActualAdjustment">{{ actualSaving ? '保存中...' : '确认调整' }}</button></div>
              </div>
            </el-popover>
            <strong v-else>{{ row.hasChildren ? '汇总 ' : '' }}{{ formatMinutes(row.confirmedMinutes) }}</strong>
            <small v-if="activeSegmentMinutes(row.task)">+ 本次 {{ formatMinutes(activeSegmentMinutes(row.task)) }}</small>
          </div>

          <div class="timer-task-actions">
            <button v-if="!row.task.parentId" class="icon-button subtle" type="button" title="缩进为上一个一级事项的子项" :aria-label="`${row.task.title}增加层级`" @click="keyboardMove(row.task, 'indent')"><IndentIncrease :size="14" /></button>
            <button v-else class="icon-button subtle" type="button" title="移到一级" :aria-label="`${row.task.title}减少层级`" @click="keyboardMove(row.task, 'outdent')"><IndentDecrease :size="14" /></button>
            <el-popover :visible="noteTaskId === row.task.id" placement="bottom" :width="300" trigger="click" @update:visible="noteTaskId = $event ? row.task.id : ''">
              <template #reference><button class="icon-button subtle" type="button" :class="{ filled: Boolean(draftFor(row.task).note.trim()) }" :title="draftFor(row.task).note.trim() ? '编辑备注' : '添加备注'" :aria-label="`${row.task.title}备注`"><MessageSquareText :size="14" /></button></template>
              <div class="timer-task-note-popover"><strong>{{ row.task.title }} · 备注</strong><textarea data-timer-task-field="note" :value="draftFor(row.task).note" rows="4" @input="updateDraft(row.task, 'note', ($event.target as HTMLTextAreaElement).value)"></textarea><small v-if="draftFor(row.task).errors.note" class="error">{{ draftFor(row.task).errors.note }}</small><div><button class="secondary-button compact" type="button" @click="noteTaskId = ''">关闭</button><button class="primary-button compact" type="button" :disabled="Boolean(draftFor(row.task).saving.note)" @click="saveField(row.task, 'note').then((ok) => { if (ok) noteTaskId = '' })"><Check :size="13" />保存</button></div></div>
            </el-popover>

            <label v-if="presentation.showAllocation && !row.hasChildren" class="timer-task-allocation-input"><input v-model.number="allocationDraft(row.task.id, row.task.title).minutes" min="0" :max="allocationRequiredMinutes" type="number" :aria-label="`${row.task.title}本次分配分钟`" /><span>分</span></label>
            <label v-if="presentation.showAllocation && !row.hasChildren" class="timer-task-complete-toggle" title="保存归属时同时完成事项；无需为该事项分配时间"><input v-model="allocationDraft(row.task.id, row.task.title).completeTask" type="checkbox" /><span>完成</span></label>
            <button v-if="!row.hasChildren" class="timer-task-primary-action" :class="actionFor(row.task, false).mode" type="button" :disabled="!actionFor(row.task, false).actionable || Boolean(timerBusyTaskId)" :aria-label="actionFor(row.task, false).ariaLabel" @click="startOrSwitch(row.task, false)"><LoaderCircle v-if="timerBusyTaskId === row.task.id" :size="13" class="spin" /><Play v-else-if="presentation.showTimerActions && actionFor(row.task, false).actionable" :size="13" fill="currentColor" />{{ actionFor(row.task, false).label }}</button>

            <el-popover :visible="moreTaskId === row.task.id" placement="bottom-end" :width="330" trigger="click" @update:visible="moreTaskId = $event ? row.task.id : ''">
              <template #reference><button class="icon-button subtle" type="button" :aria-label="`${row.task.title}更多操作`" title="更多操作"><MoreHorizontal :size="15" /></button></template>
              <div class="timer-task-more-popover"><strong>{{ row.task.title }}</strong><label><span>整体预计</span><div><input v-model="moreDraftFor(row.task).estimate" inputmode="numeric" /><small>分钟</small></div></label><label><span>预计开始</span><input v-model="moreDraftFor(row.task).planDate" type="date" /></label><label v-if="!row.hasChildren && store.tasks.some((task) => task.id === row.task.id)"><span>重复设置</span><select v-model="moreDraftFor(row.task).recurrence"><option value="none">不重复</option><option value="daily">每天</option><option value="weekdays">工作日</option><option value="weekly">每周</option></select></label><label v-if="!row.hasChildren && store.tasks.some((task) => task.id === row.task.id) && moreDraftFor(row.task).recurrence !== 'none'"><span>生效日期</span><input v-model="moreDraftFor(row.task).recurrenceStart" type="date" /></label><div v-if="!row.hasChildren && store.tasks.some((task) => task.id === row.task.id) && moreDraftFor(row.task).recurrence === 'weekly'" class="timer-task-weekdays"><span>重复星期</span><div><label v-for="(label, index) in weekdayLabels" :key="label"><input v-model="moreDraftFor(row.task).recurrenceWeekdays" type="checkbox" :value="index" />{{ label }}</label></div></div><small v-if="row.task.completedToday && !store.tasks.some((task) => task.id === row.task.id)">继续事项后可修改重复设置</small><div class="timer-task-more-actions"><button v-if="row.task.status !== 'done'" class="secondary-button compact" type="button" @click="completeTask(row.task)"><CheckCircle2 :size="13" />完成</button><button class="danger-button compact" type="button" @click="deleteTask(row.task)"><Trash2 :size="13" />删除</button><span></span><button class="primary-button compact" type="button" @click="saveMore(row.task)">保存设置</button></div></div>
            </el-popover>
          </div>
        </div>
      </div>
      <small v-if="query.trim()" class="timer-task-search-drag-hint">搜索状态下不能拖动；清空搜索后可调整顺序和层级。</small>

      <div v-if="!panelInitialLoading && !rows.length && !newTask.open" class="timer-task-empty"><AlertCircle :size="22" /><strong>{{ query ? '没有匹配事项' : (props.mode === 'execute' ? '今日还没有事项' : '当前主体暂无可执行事项') }}</strong><span>{{ query ? '换个关键词，或清空搜索查看全部事项。' : (props.mode === 'execute' ? '可以新建今日事项，或从进行中加入今日计划。' : '可以直接在这里新建事项。') }}</span><div v-if="!query"><button class="primary-button compact" type="button" @click="openNewTask()"><Plus :size="14" />新增事项</button><button v-if="props.mode === 'execute'" class="secondary-button compact" type="button" @click="todayPickerOpen = true">从进行中加入</button></div></div>
    </div>

    <template #footer><div class="timer-task-dialog-footer"><button class="secondary-button compact" type="button" @click="goPlan"><ListTodo :size="14" />前往待办页</button><span class="timer-task-allocation-error">{{ allocationError }}</span><template v-if="presentation.showAllocation"><button class="secondary-button compact" type="button" :disabled="allocationSaving || !selectedAllocations.length" @click="clearAllocationDrafts">清空</button><button v-if="presentation.showBreakDiscard" class="secondary-button compact" type="button" :disabled="allocationSaving" @click="resolveAllocationDisposition('break')">记为休息</button><button v-if="presentation.showBreakDiscard" class="danger-button compact" type="button" :disabled="allocationSaving" @click="resolveAllocationDisposition('discard')">丢弃</button><button class="primary-button" type="button" :disabled="allocationSaving" @click="saveAllocationDrafts">{{ allocationSaving ? '保存中...' : presentation.primaryLabel }}</button></template><button v-else class="secondary-button" type="button" :disabled="closing" @click="closeDialog">关闭</button></div></template>
  </el-dialog>

  <el-dialog v-model="todayPickerOpen" title="从进行中加入今日" width="min(640px, 92vw)" append-to-body>
    <input v-model="todayPickerQuery" class="today-plan-search" placeholder="搜索事项或父级路径" />
    <div class="today-plan-candidates"><label v-for="task in todayPickerCandidates" :key="task.id"><input v-model="todayPickerSelected" type="checkbox" :value="task.id" /><span><strong>{{ task.title }}</strong><small>{{ task.pathLabel }}</small></span></label><p v-if="!todayPickerCandidates.length">没有可加入的进行中事项</p></div>
    <template #footer><button class="secondary-button" type="button" @click="todayPickerOpen = false">取消</button><button class="primary-button" type="button" :disabled="!todayPickerSelected.length" @click="confirmAddExistingTodayTasks">加入今日（{{ todayPickerSelected.length }}）</button></template>
  </el-dialog>

  <Teleport to="body">
    <div v-if="rowContextMenuOpen" class="plan-task-context-menu task-row-context-menu" role="menu" :style="{ left: `${rowContextMenuLeft}px`, top: `${rowContextMenuTop}px` }" @pointerdown.stop>
      <button type="button" role="menuitem" @click="runRowContextCommand('duplicate')">复制事项</button>
      <button type="button" role="menuitem" @click="runRowContextCommand('create')">新增事项</button>
      <span class="plan-task-context-divider"></span>
      <button type="button" role="menuitem" @click="runRowContextCommand('before')">在上方插入行</button>
      <button type="button" role="menuitem" @click="runRowContextCommand('after')">在下方插入行</button>
      <span class="plan-task-context-divider"></span>
      <button class="danger" type="button" role="menuitem" @click="runRowContextCommand('delete')">删除事项</button>
    </div>
  </Teleport>

</template>
