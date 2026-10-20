<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { MdEditor } from "md-editor-v3";
import "md-editor-v3/lib/style.css";
import { CalendarDays, CheckCircle2, Circle, Clock3, Copy, CornerDownRight, DatabaseZap, FileInput, FilePenLine, GripVertical, IndentDecrease, IndentIncrease, ListMinus, ListPlus, MessageSquareText, Minus, Play, Plus, Radio, Repeat2, TimerReset, Trash2 } from "lucide-vue-next";
import type { Task } from "../store";
import { isActivePlanTask } from "../services/completedTaskHistoryCore";
import { contextMenuPosition, taskSubtreeIds, todayEstimateDraft } from "../services/planTaskInteractionCore";
import { createTaskTreeSortable } from "../services/taskTreeSortable";
import type { ActualAdjustmentBatchRequest, ActualAdjustmentBatchResult, ActualAdjustmentSnapshotRecord, ActualAdjustmentSnapshotTaskRecord } from "../services/tauri";
import { adjustQuickEstimateMinutes, parseQuickEstimateMinutes } from "../services/quickPlanEstimateCore";
import { actualAdjustmentSummary, eligibleActualAdjustmentParents, parentTargetTotal, parseActualMinutes, type ActualAdjustmentDraft, type ActualAdjustmentNewTaskDraft } from "../services/quickActualAdjustmentCore";
import { analyzeQuickPlanDraft, buildQuickPlanBatchRequest, createQuickPlanBaseline, createQuickPlanDraft, mergeScopedQuickPlanDraft, reconcileQuickPlanText, serializeQuickPlanLines, type QuickPlanBaselineTask, type QuickPlanBatchRequest, type QuickPlanDraftLine } from "../services/quickPlanTextCore";

const props = defineProps<{
  tasks: Task[];
  displayTasks?: Task[];
  todayStatusTasks?: Task[];
  todayMemberIds?: string[];
  mode?: "active" | "today";
  subjectId: string;
  subjectName: string;
  selectedId: string;
  runningTaskId?: string;
  runningTaskMinutes?: number;
  formatMinutes: (minutes: number) => string;
  todayDate: string;
  timezone: string;
  saveTodayImport: (request: { taskIds: string[]; estimates: Array<{ taskId: string; expectedTaskVersion: number; expectedDailyEstimateVersion?: number; estimateMinutes?: number }> }) => Promise<void>;
  createTask: (title: string, parentId?: string) => Promise<string>;
  createRelativeTask: (targetId: string, placement: "before" | "after", title: string) => Promise<string>;
  duplicateSingleTask: (taskId: string) => Promise<string | undefined>;
  saveTask: (taskId: string) => Promise<string | false>;
  savePlanBatch: (request: QuickPlanBatchRequest) => Promise<{ createdCount: number; updatedCount: number; deletedCount: number; todayPlanChangedCount: number; createdItems: Array<{ lineKey: string; taskId: string }> }>;
  loadActualSnapshot: () => Promise<ActualAdjustmentSnapshotRecord>;
  saveActualBatch: (request: ActualAdjustmentBatchRequest) => Promise<ActualAdjustmentBatchResult>;
  moveTask: (taskId: string, parentId?: string, previousSiblingId?: string, nextSiblingId?: string) => Promise<void>;
  loading?: boolean;
  error?: string;
}>();

const emit = defineEmits<{
  select: [id: string];
  save: [id: string];
  importPlan: [];
  syncSeaTable: [];
  add: [];
  addChild: [id: string];
  addRelative: [id: string, placement: "before" | "after"];
  duplicate: [id: string];
  delete: [id: string];
  toggle: [task: Task, event: MouseEvent];
  indent: [id: string];
  outdent: [id: string];
  startTimer: [id: string];
  move: [taskId: string, parentId?: string, previousSiblingId?: string, nextSiblingId?: string];
  saveNote: [taskId: string, note: string];
  saveTodayEstimate: [taskId: string, estimateMinutes?: number];
  addTodayTasks: [taskIds: string[]];
  removeToday: [taskId: string];
  setRecurrence: [taskId: string, recurrence?: { frequency: "daily" | "weekdays" | "weekly"; weekdaysMask?: number; effectiveStart: string }];
}>();

const listRef = ref<HTMLElement | null>(null);
const rowsRef = ref<HTMLElement | null>(null);
const todayImportRowsRef = ref<HTMLElement | null>(null);
const todayImportOpen = ref(false);
const todayImportQuery = ref("");
const todayImportSelection = ref<string[]>([]);
const todayImportEstimates = ref<Record<string, string>>({});
const todayImportErrors = ref<Record<string, string>>({});
const todayImportTaskTitles = ref<Record<string, string>>({});
const todayImportTitleErrors = ref<Record<string, string>>({});
const todayImportTitleSavingId = ref("");
const todayImportCreateParentId = ref("");
const todayImportCreateTargetId = ref("");
const todayImportCreatePlacement = ref<"before" | "after">("after");
const todayImportCreateTitle = ref("");
const todayImportCreateOpen = ref(false);
const todayImportCreateSaving = ref(false);
const todayImportCreateError = ref("");
const todayImportCreateInputRef = ref<HTMLInputElement | null>(null);
const todayImportSaving = ref(false);
const todayImportMoving = ref(false);
const todayImportEstimatePresets = [
  { minutes: 30, label: "30 分钟" },
  { minutes: 60, label: "1 小时" },
  { minutes: 90, label: "1 小时 30 分钟" },
  { minutes: 120, label: "2 小时" },
  { minutes: 150, label: "2 小时 30 分钟" },
];
const editingEstimateId = ref("");
const editingTodayEstimateId = ref("");
const todayEstimateDraftValue = ref<number>();
const levelChangedId = ref("");
const isSorting = ref(false);
const temporaryTaskId = ref("");
const listRenderVersion = ref(0);
const recurrenceDialogOpen = ref(false);
const recurrenceTaskId = ref("");
const recurrenceFrequency = ref<"none" | "daily" | "weekdays" | "weekly">("none");
const recurrenceStart = ref(props.todayDate);
const recurrenceWeekdays = ref<number[]>([]);
const noteDialogOpen = ref(false);
const noteTaskId = ref("");
const noteDraft = ref("");
const contextMenuOpen = ref(false);
const contextTaskId = ref("");
const contextMenuLeft = ref(0);
const contextMenuTop = ref(0);
const todayImportContextMenuOpen = ref(false);
const todayImportContextTaskId = ref("");
const todayImportContextMenuLeft = ref(0);
const todayImportContextMenuTop = ref(0);
const actualAdjustmentOpen = ref(false);
const actualAdjustmentSaving = ref(false);
const actualAdjustmentSnapshot = ref<ActualAdjustmentSnapshotRecord>();
const actualAdjustmentDrafts = ref<ActualAdjustmentDraft[]>([]);
const actualAdjustmentNewTasks = ref<ActualAdjustmentNewTaskDraft[]>([]);
const actualAdjustmentErrors = ref<Record<string, string>>({});
const actualAdjustmentNote = ref("");
const actualAdjustmentClientRequestId = ref("");
const actualAdjustmentBatchId = ref("");
const actualAdjustmentSubjectId = ref("");
const inlineActualTaskId = ref("");
const inlineActualSurface = ref<"list" | "today-import" | "">("");
const inlineActualTask = ref<ActualAdjustmentSnapshotTaskRecord>();
const inlineActualWorkDate = ref("");
const inlineActualTargetText = ref("");
const inlineActualNote = ref("");
const inlineActualError = ref("");
const inlineActualLoading = ref(false);
const inlineActualSaving = ref(false);
const quickPlanOpen = ref(false);
const quickPlanSaving = ref(false);
const quickPlanMode = ref<"edit" | "summary">("edit");
const quickPlanBaseline = ref<QuickPlanBaselineTask[]>([]);
const quickPlanSourceLines = ref<QuickPlanDraftLine[]>([]);
const quickPlanLines = ref<QuickPlanDraftLine[]>([]);
const quickPlanFullBaseline = ref<QuickPlanBaselineTask[]>([]);
const quickPlanFullSourceLines = ref<QuickPlanDraftLine[]>([]);
const quickPlanText = ref("");
const quickPlanConfirmingClose = ref(false);
const quickPlanPasteErrors = ref<Array<{ lineIndex: number; message: string }>>([]);
const quickPlanEditorRef = ref<{ getEditorView?: () => { state: { selection: { main: { from: number; to: number } } }; dispatch: (transaction: unknown) => void } } | undefined>(undefined);
let quickPlanTextVersion = 0;
let levelChangedTimer: number | undefined;

const quickPlanMarkdownToolbars = [
  "revoke",
  "next",
  "-",
  "bold",
  "underline",
  "italic",
  "strikeThrough",
  "unorderedList",
  "codeRow",
  "link",
  "=",
  "preview",
] as const;


const mode = computed(() => props.mode ?? "active");
const scopedDisplayTasks = computed(() => (props.displayTasks ?? []).filter((task) => task.subjectId === props.subjectId));
const todayStatusById = computed(() => new Map(
  (props.todayStatusTasks ?? [])
    .filter((task) => task.subjectId === props.subjectId)
    .map((task) => [task.id, task]),
));
const displayedTasks = computed(() => {
  if (!props.displayTasks) {
    if (!props.todayStatusTasks) return props.tasks;
    return props.tasks.map((task) => {
      const todayTask = todayStatusById.value.get(task.id);
      if (!todayTask) return task;
      return {
        ...task,
        status: todayTask.status,
        executionState: todayTask.executionState,
        completedAt: todayTask.completedAt,
        todayMember: todayTask.todayMember,
        recurrence: todayTask.recurrence ?? task.recurrence,
        occurrenceDate: todayTask.occurrenceDate,
        occurrenceOrigin: todayTask.occurrenceOrigin,
        occurrenceVersion: todayTask.occurrenceVersion,
      };
    });
  }
  const taskById = new Map(props.tasks.map((task) => [task.id, task]));
  const snapshotIds = new Set(scopedDisplayTasks.value.map((task) => task.id));
  const ordered = [...scopedDisplayTasks.value];
  if (!temporaryTaskId.value || snapshotIds.has(temporaryTaskId.value)) return ordered;
  const temporaryTask = taskById.get(temporaryTaskId.value);
  if (!temporaryTask) return ordered;
  const sourceIndex = props.tasks.findIndex((task) => task.id === temporaryTask.id);
  let previousSnapshotIndex = -1;
  for (let index = sourceIndex - 1; index >= 0; index -= 1) {
    if (snapshotIds.has(props.tasks[index].id)) {
      previousSnapshotIndex = index;
      break;
    }
  }
  const relativeNextSnapshotIndex = props.tasks
    .slice(sourceIndex + 1)
    .findIndex((task) => snapshotIds.has(task.id));
  if (previousSnapshotIndex >= 0) {
    const previousId = props.tasks[previousSnapshotIndex].id;
    const insertAt = ordered.findIndex((task) => task.id === previousId) + 1;
    ordered.splice(insertAt, 0, temporaryTask);
  } else if (relativeNextSnapshotIndex >= 0) {
    const nextId = props.tasks[sourceIndex + 1 + relativeNextSnapshotIndex].id;
    const insertAt = ordered.findIndex((task) => task.id === nextId);
    ordered.splice(insertAt, 0, temporaryTask);
  } else {
    ordered.push(temporaryTask);
  }
  return ordered;
});
const flatRows = computed(() => displayedTasks.value.map((task) => ({ task, depth: getDepth(task) })));
const pendingRows = computed(() => mode.value === "today"
  ? flatRows.value
  : flatRows.value.filter((row) => isActivePlanTask({ status: row.task.status, recurring: Boolean(row.task.recurrence) })));
const todayLoading = computed(() => mode.value === "today" && Boolean(props.loading));
const visiblePendingRows = computed(() => todayLoading.value ? [] : pendingRows.value);
const todayPlannedIds = computed(() => new Set(scopedDisplayTasks.value.filter((task) => task.todayMember).map((task) => task.id)));
const knownTodayMemberIds = computed(() => new Set([
  ...todayPlannedIds.value,
  ...(props.todayMemberIds ?? []),
]));
const activeImportTasks = computed(() => {
  const snapshotById = new Map(scopedDisplayTasks.value.map((task) => [task.id, task]));
  return props.tasks
    .filter((task) => isActivePlanTask({ status: task.status, recurring: Boolean(task.recurrence) }))
    .map((task) => ({ ...task, ...(snapshotById.get(task.id) ?? {}) }));
});
const todayImportRows = computed(() => {
  const keyword = todayImportQuery.value.trim().toLocaleLowerCase();
  const visibleIds = new Set<string>();
  if (keyword) {
    for (const task of activeImportTasks.value) {
      if (![task.title, task.pathLabel ?? "", task.note ?? ""].join(" ").toLocaleLowerCase().includes(keyword)) continue;
      visibleIds.add(task.id);
      let parentId = task.parentId;
      while (parentId) {
        visibleIds.add(parentId);
        parentId = props.tasks.find((candidate) => candidate.id === parentId)?.parentId;
      }
    }
  }
  return activeImportTasks.value
    .filter((task) => !keyword || visibleIds.has(task.id))
    .map((task) => ({ task, depth: getDepth(task), allocatable: !activeImportTasks.value.some((candidate) => candidate.parentId === task.id) }));
});
const todayMemberLeafTasks = computed(() => displayedTasks.value.filter((task) => (Boolean(task.todayMember) || task.version <= 0) && !displayedTasks.value.some((candidate) => candidate.parentId === task.id)));
const todayEstimatedTotalMinutes = computed(() => todayMemberLeafTasks.value.reduce((sum, task) => sum + (task.todayEstimate ?? 0), 0));
const todayActualTotalMinutes = computed(() => todayMemberLeafTasks.value.reduce((sum, task) => sum + task.directMinutes, 0));
const activeImportTaskIds = computed(() => new Set(activeImportTasks.value.map((task) => task.id)));
const todayImportFixedEstimateMinutes = computed(() => todayMemberLeafTasks.value.reduce((sum, task) => activeImportTaskIds.value.has(task.id) ? sum : sum + (task.todayEstimate ?? 0), 0));
const todayImportTotalMinutes = computed(() => todayImportFixedEstimateMinutes.value + todayImportSelection.value.reduce((sum, taskId) => {
  const parsed = parseQuickEstimateMinutes(todayImportEstimates.value[taskId] ?? "");
  return sum + (parsed.ok ? parsed.minutes ?? 0 : 0);
}, 0));
const actualAdjustmentSummaryValue = computed(() => actualAdjustmentSummary(actualAdjustmentDrafts.value, actualAdjustmentNewTasks.value));
const actualAdjustmentDirty = computed(() => actualAdjustmentSummaryValue.value.changes.length > 0 || actualAdjustmentNewTasks.value.length > 0);
const actualAdjustmentParents = computed(() => eligibleActualAdjustmentParents(actualAdjustmentSnapshot.value?.tasks ?? []));
const inlineActualBaseline = computed(() => inlineActualTask.value?.directMinutes ?? 0);
const inlineActualParsed = computed(() => parseActualMinutes(inlineActualTargetText.value));
const inlineActualDelta = computed(() => inlineActualParsed.value.ok ? inlineActualParsed.value.minutes - inlineActualBaseline.value : 0);
const quickPlanSummary = computed(() => {
  const summary = analyzeQuickPlanDraft(quickPlanBaseline.value, quickPlanLines.value);
  const contextErrors = mode.value === "today"
    ? summary.deleted.flatMap((item) => item.taskId && !todayPlannedIds.value.has(item.taskId)
      ? [{ lineIndex: quickPlanSourceLines.value.findIndex((line) => line.taskId === item.taskId), message: `父级上下文“${item.title}”需要保留；如需调整今日事项，请编辑其下的今日成员` }]
      : [])
    : [];
  return { ...summary, errors: [...quickPlanPasteErrors.value, ...summary.errors, ...contextErrors] };
});

function getDepth(task: Task) {
  let depth = 0;
  let parentId = task.parentId;
  while (parentId && depth < 8) {
    const parent = props.tasks.find((item) => item.id === parentId);
    if (!parent) break;
    depth += 1;
    parentId = parent.parentId;
  }
  return depth;
}

function getRootTask(task: Task) {
  let root = task;
  let parentId = task.parentId;
  while (parentId) {
    const parent = props.tasks.find((item) => item.id === parentId);
    if (!parent) break;
    root = parent;
    parentId = parent.parentId;
  }
  return root;
}

function childPlaceholderParent(rowIndex: number) {
  const row = pendingRows.value[rowIndex];
  if (!row) return;
  const root = getRootTask(row.task);
  if (root.status === "done" && !root.recurrence) return;
  if (root.recurrence) return;
  const nextRow = pendingRows.value[rowIndex + 1];
  if (nextRow && getRootTask(nextRow.task).id === root.id) return;
  return root;
}

function isTaskGroupStart(rowIndex: number) {
  const row = pendingRows.value[rowIndex];
  if (!row || row.depth !== 0) return false;
  const nextRow = pendingRows.value[rowIndex + 1];
  return Boolean(nextRow && getRootTask(nextRow.task).id === row.task.id) || Boolean(childPlaceholderParent(rowIndex));
}

function getSubtreeIds(taskId: string) {
  return taskSubtreeIds(props.tasks, taskId);
}

function isTodayMember(task: Task) {
  return mode.value !== "today" || Boolean(task.todayMember) || task.version <= 0;
}

function canToggleTask(task: Task) {
  return isTodayMember(task) && !hasChildren(task.id);
}

function taskToggleTitle(task: Task) {
  if (hasChildren(task.id)) return "父事项由子事项汇总，不能直接完成";
  if (!isTodayMember(task)) return "上下文父事项不能直接完成";
  if (task.recurrence) return task.status === "done" ? "恢复今天这一轮" : "完成今天这一轮";
  return task.status === "done" ? "恢复为进行中" : "标记为完成";
}

function isTaskCompleted(task: Task) {
  return task.status === "done";
}

function syncEditableTask(task: Task) {
  const source = props.tasks.find((item) => item.id === task.id);
  if (!source || source === task) return;
  source.title = task.title;
  source.planDate = task.planDate;
  source.estimate = task.estimate;
  source.project = task.project;
}

function prepareSubtreeDrag(taskId: string) {
  treeSortable.prepareSubtree(taskId);
}

function canIndent(task: Task) {
  const index = pendingRows.value.findIndex((row) => row.task.id === task.id);
  if (index <= 0) return false;
  const depth = getDepth(task);
  return pendingRows.value.slice(0, index).reverse().some((row) => row.depth === depth && row.task.parentId === task.parentId);
}

function canOutdent(task: Task) {
  return Boolean(task.parentId);
}

async function changeIndent(task: Task, direction: "indent" | "outdent") {
  const previousDepth = getDepth(task);
  selectTask(task);
  if (direction === "indent" && canIndent(task)) emit("indent", task.id);
  if (direction === "outdent" && canOutdent(task)) emit("outdent", task.id);
  await nextTick();
  if (getDepth(task) === previousDepth) return;
  levelChangedId.value = task.id;
  if (levelChangedTimer) window.clearTimeout(levelChangedTimer);
  levelChangedTimer = window.setTimeout(() => { levelChangedId.value = ""; }, 520);
}

function handleTitleTab(event: KeyboardEvent, task: Task) {
  event.preventDefault();
  changeIndent(task, event.shiftKey ? "outdent" : "indent");
}

function selectTask(task: Task) {
  emit("select", task.id);
}

async function addTask() {
  const existingIds = new Set(props.tasks.map((task) => task.id));
  emit("add");
  await nextTick();
  const taskId = props.tasks.find((task) => !existingIds.has(task.id) && !task.parentId)?.id
    ?? (!existingIds.has(props.selectedId) ? props.selectedId : "");
  if (!taskId) return;
  temporaryTaskId.value = taskId;
  await focusTaskInput(taskId);
}

async function addChild(task: Task) {
  const existingIds = new Set(props.tasks.map((item) => item.id));
  emit("addChild", task.id);
  await nextTick();
  const taskId = props.tasks.find((item) => !existingIds.has(item.id) && item.parentId === task.id)?.id
    ?? (!existingIds.has(props.selectedId) ? props.selectedId : "");
  if (!taskId) return;
  temporaryTaskId.value = taskId;
  await focusTaskInput(taskId);
}

async function addRelative(task: Task, placement: "before" | "after") {
  const existingIds = new Set(props.tasks.map((item) => item.id));
  emit("addRelative", task.id, placement);
  await nextTick();
  const taskId = props.tasks.find((item) => !existingIds.has(item.id))?.id
    ?? (!existingIds.has(props.selectedId) ? props.selectedId : "");
  if (!taskId) return;
  temporaryTaskId.value = taskId;
  await focusTaskInput(taskId);
}

async function duplicateTask(task: Task) {
  emit("duplicate", task.id);
  await focusSelectedInput();
}

async function focusSelectedInput() {
  await focusTaskInput(props.selectedId);
}

async function focusTaskInput(taskId: string) {
  await nextTick();
  const input = listRef.value?.querySelector<HTMLInputElement>(`[data-task-input="${taskId}"]`);
  input?.focus();
}

function finishTemporaryTask(task: Task) {
  if (temporaryTaskId.value !== task.id) return;
  temporaryTaskId.value = "";
  if (!task.title.trim()) emit("delete", task.id);
}

function finishTaskEdit(task: Task) {
  if (!task.title.trim()) {
    finishTemporaryTask(task);
    return;
  }
  syncEditableTask(task);
  emit("save", task.id);
}

function openNativePicker(taskId: string) {
  const input = listRef.value?.querySelector<HTMLInputElement>(`[data-date-input="${taskId}"]`);
  if (!input) return;
  input.focus();
  input.showPicker?.();
  if (!input.showPicker) input.click();
}

async function openEstimateEditor(taskId: string) {
  editingEstimateId.value = taskId;
  await nextTick();
  const input = listRef.value?.querySelector<HTMLInputElement>(`[data-estimate-input="${taskId}"]`);
  input?.focus();
  input?.select();
}

function closeEstimateEditor() {
  editingEstimateId.value = "";
}

function finishEstimateEditor(task: Task) {
  if (editingEstimateId.value !== task.id) return;
  task.estimate = normalizeEstimate(task.estimate);
  syncEditableTask(task);
  closeEstimateEditor();
  emit("save", task.id);
}

function saveEditableTask(task: Task) {
  syncEditableTask(task);
  emit("save", task.id);
}

async function openTodayEstimateEditor(task: Task) {
  editingTodayEstimateId.value = task.id;
  todayEstimateDraftValue.value = todayEstimateDraft(task.todayEstimate, task.parentId);
  await nextTick();
  const input = listRef.value?.querySelector<HTMLInputElement>(`[data-today-estimate-input="${task.id}"]`);
  input?.focus();
  input?.select();
}

function closeTodayEstimateEditor() {
  editingTodayEstimateId.value = "";
  todayEstimateDraftValue.value = undefined;
}

function finishTodayEstimateEditor(task: Task) {
  if (editingTodayEstimateId.value !== task.id) return;
  const estimate = normalizeEstimate(todayEstimateDraftValue.value);
  closeTodayEstimateEditor();
  emit("saveTodayEstimate", task.id, estimate);
}

function openNoteEditor(task: Task) {
  closeContextMenu();
  noteTaskId.value = task.id;
  noteDraft.value = task.note ?? "";
  noteDialogOpen.value = true;
}

function saveNote() {
  if (!noteTaskId.value) return;
  emit("saveNote", noteTaskId.value, noteDraft.value);
  noteDialogOpen.value = false;
}

function contextTask() {
  return props.tasks.find((task) => task.id === contextTaskId.value);
}

function openContextMenu(event: MouseEvent, task: Task) {
  const target = event.target as HTMLElement;
  if (target.closest("input, textarea")) return;
  event.preventDefault();
  event.stopPropagation();
  const position = contextMenuPosition(event.clientX, event.clientY, 190, 250, window.innerWidth, window.innerHeight);
  contextTaskId.value = task.id;
  contextMenuLeft.value = position.left;
  contextMenuTop.value = position.top;
  contextMenuOpen.value = true;
}

function closeContextMenu() {
  contextMenuOpen.value = false;
  contextTaskId.value = "";
}

function runContextCommand(command: "before" | "after" | "child" | "note" | "duplicate" | "delete") {
  const task = contextTask();
  closeContextMenu();
  if (!task) return;
  if (command === "before" || command === "after") void addRelative(task, command);
  if (command === "child") void addChild(task);
  if (command === "note") openNoteEditor(task);
  if (command === "duplicate") void duplicateTask(task);
  if (command === "delete") void confirmDelete(task);
}

function closeContextMenuForPointer(event: PointerEvent) {
  if (!(event.target as HTMLElement).closest(".plan-task-context-menu")) {
    closeContextMenu();
    closeTodayImportContextMenu();
  }
}

function closeContextMenuForKey(event: KeyboardEvent) {
  if (event.key === "Escape") {
    closeContextMenu();
    closeTodayImportContextMenu();
  }
}

function normalizeEstimate(value: number | undefined) {
  const minutes = Number(value);
  return Number.isFinite(minutes) && minutes > 0 ? Math.round(minutes) : undefined;
}

function openQuickPlan() {
  const activeTasks = props.tasks.filter((task) => isActivePlanTask({ status: task.status, recurring: Boolean(task.recurrence) }));
  quickPlanFullBaseline.value = createQuickPlanBaseline(activeTasks);
  quickPlanFullSourceLines.value = createQuickPlanDraft(activeTasks);
  const editableTasks = mode.value === "today"
    ? displayedTasks.value.filter((task) => task.status === "open")
    : activeTasks;
  quickPlanBaseline.value = createQuickPlanBaseline(editableTasks);
  quickPlanSourceLines.value = createQuickPlanDraft(editableTasks);
  quickPlanLines.value = quickPlanSourceLines.value.map((line) => ({ ...line }));
  quickPlanText.value = serializeQuickPlanLines(quickPlanLines.value);
  quickPlanPasteErrors.value = [];
  quickPlanMode.value = "edit";
  quickPlanOpen.value = true;
}

function updateQuickPlanText(text: string) {
  quickPlanText.value = text;
  const result = reconcileQuickPlanText(quickPlanSourceLines.value, text, `edit:${++quickPlanTextVersion}`);
  quickPlanPasteErrors.value = result.errors;
  if (!result.errors.length) quickPlanLines.value = result.lines;
}

function handleQuickPlanTextareaKey(event: KeyboardEvent) {
  if (event.key !== "Tab") return;
  event.preventDefault();
  event.stopPropagation();
  const view = quickPlanEditorRef.value?.getEditorView?.();
  if (!view) return;
  const start = view.state.selection.main.from;
  const end = view.state.selection.main.to;
  const lineStart = quickPlanText.value.lastIndexOf("\n", start - 1) + 1;
  let nextText = quickPlanText.value;
  let nextCursor = start;
  let changeTo = lineStart;
  let inserted = "";
  if (event.shiftKey) {
    const prefix = nextText.slice(lineStart, start);
    const removeLength = prefix.startsWith("\t") ? 1 : prefix.startsWith("  ") ? 2 : 0;
    if (removeLength) {
      changeTo = lineStart + removeLength;
      nextText = nextText.slice(0, lineStart) + nextText.slice(lineStart + removeLength);
      nextCursor = Math.max(lineStart, start - removeLength);
    }
  } else {
    inserted = "  ";
    nextText = nextText.slice(0, lineStart) + inserted + nextText.slice(lineStart);
    nextCursor = start + 2;
  }
  if (nextText === quickPlanText.value) return;
  const delta = inserted.length - (changeTo - lineStart);
  view.dispatch({
    changes: { from: lineStart, to: changeTo, insert: inserted },
    selection: { anchor: nextCursor, head: Math.max(nextCursor, end + delta) },
  });
}

function formatQuickPlanItem(depth: 0 | 1, title: string) {
  return `${depth ? "  " : ""}- ${title}`;
}

function openQuickPlanSummary() {
  if (quickPlanSummary.value.errors.length) return;
  if (!quickPlanSummary.value.dirty) {
    quickPlanOpen.value = false;
    return;
  }
  quickPlanMode.value = "summary";
}

function closeQuickPlanSummary() {
  quickPlanMode.value = "edit";
}

async function requestCloseQuickPlan(done: () => void) {
  if (!quickPlanSummary.value.dirty || quickPlanConfirmingClose.value) { done(); return; }
  quickPlanConfirmingClose.value = true;
  try {
    await ElMessageBox.confirm(`${mode.value === "today" ? "今日计划" : "全部计划"}还有未保存修改，确定放弃吗？`, "放弃修改", { confirmButtonText: "放弃", cancelButtonText: "继续编辑", type: "warning" });
    done();
  } catch {} finally {
    quickPlanConfirmingClose.value = false;
  }
}

async function saveQuickPlan() {
  if (quickPlanSaving.value || quickPlanSummary.value.errors.length) return;
  quickPlanSaving.value = true;
  try {
    let request: QuickPlanBatchRequest;
    if (mode.value === "today") {
      const mergedDraft = mergeScopedQuickPlanDraft(quickPlanFullSourceLines.value, quickPlanFullBaseline.value, quickPlanBaseline.value, quickPlanSummary.value);
      const fullSummary = analyzeQuickPlanDraft(quickPlanFullBaseline.value, mergedDraft);
      if (fullSummary.errors.length) throw new Error(fullSummary.errors[0].message);
      request = buildQuickPlanBatchRequest(props.subjectId, quickPlanFullBaseline.value, fullSummary);
      request.todayPlanChange = {
        workDate: props.todayDate,
        includeLineKeys: quickPlanSummary.value.added.map((item) => item.lineKey),
        excludeItems: quickPlanSummary.value.deleted.flatMap((item) => {
          if (!item.taskId || !todayPlannedIds.value.has(item.taskId)) return [];
          const task = props.displayTasks?.find((candidate) => candidate.id === item.taskId);
          return [{ taskId: item.taskId, expectedVersion: task?.dayPlanVersion }];
        }),
      };
    } else {
      request = buildQuickPlanBatchRequest(props.subjectId, quickPlanBaseline.value, quickPlanSummary.value);
    }
    await props.savePlanBatch(request);
    quickPlanOpen.value = false;
    ElMessage.success("计划已保存");
  } catch (error) {
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "计划保存失败，请重新加载后重试");
  } finally {
    quickPlanSaving.value = false;
  }
}

async function openActualAdjustment() {
  try {
    const snapshot = await props.loadActualSnapshot();
    actualAdjustmentSnapshot.value = snapshot;
    actualAdjustmentDrafts.value = snapshot.tasks.filter((task) => task.isLeaf).map((task) => ({ taskId: task.id, baselineMinutes: task.directMinutes, targetMinutes: task.directMinutes, text: String(task.directMinutes), expectedTaskVersion: task.version, adjustmentFingerprint: task.adjustmentFingerprint }));
    actualAdjustmentNewTasks.value = [];
    actualAdjustmentErrors.value = {};
    actualAdjustmentNote.value = "";
    actualAdjustmentClientRequestId.value = crypto.randomUUID();
    actualAdjustmentBatchId.value = crypto.randomUUID();
    actualAdjustmentSubjectId.value = props.subjectId;
    actualAdjustmentOpen.value = true;
  } catch (error) {
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "实际时间加载失败");
  }
}

function actualDraft(taskId: string) { return actualAdjustmentDrafts.value.find((item) => item.taskId === taskId); }
function updateActualDraft(taskId: string, text: string) {
  const draft = actualDraft(taskId); if (!draft) return;
  draft.text = text; const parsed = parseActualMinutes(text);
  if (!parsed.ok) { actualAdjustmentErrors.value = { ...actualAdjustmentErrors.value, [taskId]: parsed.message }; return; }
  draft.targetMinutes = parsed.minutes; const errors = { ...actualAdjustmentErrors.value }; delete errors[taskId]; actualAdjustmentErrors.value = errors;
}
function addActualNewTask() { actualAdjustmentNewTasks.value.push({ clientTaskId: crypto.randomUUID(), title: "", targetMinutes: 0, text: "0" }); }
function removeActualNewTask(clientTaskId: string) { actualAdjustmentNewTasks.value = actualAdjustmentNewTasks.value.filter((item) => item.clientTaskId !== clientTaskId); const errors = { ...actualAdjustmentErrors.value }; delete errors[clientTaskId]; actualAdjustmentErrors.value = errors; }
function updateActualNewTaskTitle(draft: ActualAdjustmentNewTaskDraft, title: string) {
  draft.title = title;
  if (title.trim() && actualAdjustmentErrors.value[draft.clientTaskId] === "事项名称不能为空") {
    const errors = { ...actualAdjustmentErrors.value };
    delete errors[draft.clientTaskId];
    actualAdjustmentErrors.value = errors;
  }
}
function updateActualNewTaskMinutes(draft: ActualAdjustmentNewTaskDraft, text: string) { draft.text = text; const parsed = parseActualMinutes(text); if (!parsed.ok) { actualAdjustmentErrors.value = { ...actualAdjustmentErrors.value, [draft.clientTaskId]: parsed.message }; return; } draft.targetMinutes = parsed.minutes; const errors = { ...actualAdjustmentErrors.value }; delete errors[draft.clientTaskId]; actualAdjustmentErrors.value = errors; }
function setActualNewTaskParent(draft: ActualAdjustmentNewTaskDraft, parentId: string) { draft.parentId = parentId || undefined; draft.expectedParentVersion = actualAdjustmentParents.value.find((task) => task.id === parentId)?.version; }
function actualParentTarget(parentId: string) { return parentTargetTotal(parentId, actualAdjustmentSnapshot.value?.tasks ?? [], actualAdjustmentDrafts.value, actualAdjustmentNewTasks.value); }

async function closeActualAdjustment() {
  if (!actualAdjustmentDirty.value) { actualAdjustmentOpen.value = false; return; }
  try { await ElMessageBox.confirm("调整今日实际还有未保存修改，确定放弃吗？", "放弃修改", { confirmButtonText: "放弃", cancelButtonText: "继续编辑", type: "warning" }); actualAdjustmentOpen.value = false; } catch {}
}

async function saveActualAdjustment() {
  if (actualAdjustmentSaving.value || !actualAdjustmentSnapshot.value) return;
  if (actualAdjustmentSubjectId.value !== props.subjectId) {
    ElMessage.warning("计划主体已经切换，请关闭面板后重新打开");
    return;
  }
  for (const draft of actualAdjustmentDrafts.value) updateActualDraft(draft.taskId, draft.text);
  for (const draft of actualAdjustmentNewTasks.value) {
    updateActualNewTaskMinutes(draft, draft.text);
    const errors = { ...actualAdjustmentErrors.value };
    if (!draft.title.trim()) errors[draft.clientTaskId] = "事项名称不能为空";
    else if (errors[draft.clientTaskId] === "事项名称不能为空") delete errors[draft.clientTaskId];
    actualAdjustmentErrors.value = errors;
  }
  if (Object.keys(actualAdjustmentErrors.value).length) return;
  const summary = actualAdjustmentSummaryValue.value;
  if (!summary.changes.length && !actualAdjustmentNewTasks.value.length) { actualAdjustmentOpen.value = false; return; }
  const targetMinutesByTask = new Map(actualAdjustmentDrafts.value.map((draft) => [draft.taskId, draft.targetMinutes]));
  try {
    const latestSnapshot = await props.loadActualSnapshot();
    const latestTaskById = new Map(latestSnapshot.tasks.filter((task) => task.isLeaf).map((task) => [task.id, task]));
    for (const draft of actualAdjustmentDrafts.value) {
      const latestTask = latestTaskById.get(draft.taskId);
      if (!latestTask) continue;
      draft.baselineMinutes = latestTask.directMinutes;
      draft.expectedTaskVersion = latestTask.version;
      draft.adjustmentFingerprint = latestTask.adjustmentFingerprint;
      draft.targetMinutes = targetMinutesByTask.get(draft.taskId) ?? draft.targetMinutes;
      draft.text = String(draft.targetMinutes);
    }
    actualAdjustmentSnapshot.value = latestSnapshot;
  } catch (error) {
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "无法刷新最新实际时间，请稍后重试");
    return;
  }
  const latestSummary = actualAdjustmentSummaryValue.value;
  if (!latestSummary.changes.length && !actualAdjustmentNewTasks.value.length) { actualAdjustmentOpen.value = false; return; }
  const escapeHtml = (value: string) => value.replace(/[&<>"']/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[character] ?? character);
  const taskById = new Map((actualAdjustmentSnapshot.value.tasks ?? []).map((task) => [task.id, task]));
  const existingRows = latestSummary.changes.map((item) => {
    const task = taskById.get(item.taskId);
    const delta = item.deltaMinutes >= 0 ? `+${item.deltaMinutes}` : String(item.deltaMinutes);
    return `<li><strong>实际调整</strong> · ${escapeHtml(task?.title ?? "未知事项")}：${item.baselineMinutes} → ${item.targetMinutes} 分钟（${delta}）</li>`;
  });
  const newRows = actualAdjustmentNewTasks.value.map((draft) => {
    const parent = draft.parentId ? taskById.get(draft.parentId) : undefined;
    const position = parent ? `${escapeHtml(parent.title)} 下` : "顶层事项";
    const adjustment = draft.targetMinutes > 0 ? `初始调整 +${draft.targetMinutes} 分钟` : "仅新增事项，无实际调整";
    return `<li><strong>新增事项</strong> · ${escapeHtml(draft.title.trim())}（${position}）：目标 ${draft.targetMinutes} 分钟，${adjustment}</li>`;
  });
  const message = `<p>新增 ${latestSummary.newTaskCount} 个事项，保存 ${latestSummary.adjustmentCount} 条实际调整；增加 ${latestSummary.addedMinutes} 分钟，减少 ${latestSummary.reducedMinutes} 分钟，净变化 ${latestSummary.netMinutes >= 0 ? "+" : ""}${latestSummary.netMinutes} 分钟。</p><ul class="actual-adjustment-confirm-list">${[...newRows, ...existingRows].join("")}</ul>`;
  try { await ElMessageBox.confirm(message, "确认今日实际变化", { confirmButtonText: "保存", cancelButtonText: "继续调整", type: "info", dangerouslyUseHTMLString: true }); } catch { return; }
  actualAdjustmentSaving.value = true;
  try {
    await props.saveActualBatch({ subjectId: actualAdjustmentSubjectId.value, workDate: actualAdjustmentSnapshot.value.workDate, clientRequestId: actualAdjustmentClientRequestId.value, batchId: actualAdjustmentBatchId.value, note: actualAdjustmentNote.value.trim() || undefined, items: latestSummary.changes, newTasks: actualAdjustmentNewTasks.value.map((draft) => ({ clientTaskId: draft.clientTaskId, title: draft.title.trim(), parentId: draft.parentId, expectedParentVersion: draft.expectedParentVersion, targetMinutes: draft.targetMinutes })) });
    actualAdjustmentOpen.value = false; ElMessage.success("今日实际已调整");
  } catch (error) { ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "今日实际保存失败，草稿已保留"); }
  finally { actualAdjustmentSaving.value = false; }
}

function hasChildren(taskId: string) {
  return props.tasks.some((task) => task.parentId === taskId);
}

function openRecurrenceEditor(task: Task) {
  if (hasChildren(task.id)) return;
  recurrenceTaskId.value = task.id;
  recurrenceFrequency.value = task.recurrence?.frequency ?? "none";
  recurrenceStart.value = task.recurrence?.effectiveStart ?? props.todayDate;
  recurrenceWeekdays.value = task.recurrence?.weekdaysMask
    ? Array.from({ length: 7 }, (_, index) => index).filter((index) => Boolean(task.recurrence!.weekdaysMask! & (1 << index)))
    : [];
  recurrenceDialogOpen.value = true;
}

function saveRecurrence() {
  const task = props.tasks.find((item) => item.id === recurrenceTaskId.value);
  if (!task) return;
  if (recurrenceFrequency.value === "none") {
    emit("setRecurrence", task.id, undefined);
  } else {
    const weekdaysMask = recurrenceFrequency.value === "weekly"
      ? recurrenceWeekdays.value.reduce((mask, index) => mask | (1 << index), 0)
      : undefined;
    if (recurrenceFrequency.value === "weekly" && !weekdaysMask) {
      ElMessage.warning("每周重复至少选择一天");
      return;
    }
    emit("setRecurrence", task.id, {
      frequency: recurrenceFrequency.value,
      weekdaysMask,
      effectiveStart: recurrenceStart.value,
    });
  }
  recurrenceDialogOpen.value = false;
}

async function confirmDelete(task: Task) {
  const childCount = props.tasks.filter((item) => item.parentId === task.id).length;
  const suffix = childCount ? `，并同时删除 ${childCount} 个子项` : "";
  try {
    await ElMessageBox.confirm(`确定删除“${task.title || "未命名事项"}”${suffix}？`, "删除事项", {
      confirmButtonText: "删除",
      cancelButtonText: "取消",
      type: "warning",
      customClass: "work-confirm-dialog",
      confirmButtonClass: "work-confirm-danger"
    });
    emit("delete", task.id);
  } catch {}
}

function openTodayImport() {
  const memberIds = new Set(todayPlannedIds.value);
  todayImportSelection.value = activeImportTasks.value
    .filter((task) => task.selectable && memberIds.has(task.id))
    .map((task) => task.id);
  todayImportEstimates.value = Object.fromEntries(activeImportTasks.value.map((task) => [task.id, task.todayEstimate ? String(task.todayEstimate) : ""]));
  todayImportTaskTitles.value = Object.fromEntries(activeImportTasks.value.map((task) => [task.id, task.title]));
  todayImportErrors.value = {};
  todayImportTitleErrors.value = {};
  todayImportTitleSavingId.value = "";
  todayImportCreateOpen.value = false;
  todayImportCreateParentId.value = "";
  todayImportCreateTitle.value = "";
  todayImportCreateError.value = "";
  todayImportQuery.value = "";
  todayImportOpen.value = true;
}

function updateTodayImportTaskTitle(taskId: string, title: string) {
  todayImportTaskTitles.value = { ...todayImportTaskTitles.value, [taskId]: title };
  if (title.trim()) {
    const errors = { ...todayImportTitleErrors.value };
    delete errors[taskId];
    todayImportTitleErrors.value = errors;
  }
}

function resetTodayImportTaskTitle(task: Task) {
  updateTodayImportTaskTitle(task.id, task.title);
}

async function saveTodayImportTaskTitle(task: Task) {
  if (todayImportTitleSavingId.value === task.id) return;
  const title = (todayImportTaskTitles.value[task.id] ?? task.title).trim();
  if (!title) {
    todayImportTitleErrors.value = { ...todayImportTitleErrors.value, [task.id]: "事项名称不能为空" };
    return;
  }
  if (title === task.title) return;
  const source = props.tasks.find((candidate) => candidate.id === task.id);
  if (!source) return;
  const previousTitle = source.title;
  source.title = title;
  todayImportTitleSavingId.value = task.id;
  try {
    const savedId = await props.saveTask(task.id);
    if (!savedId) throw new Error("事项名称保存失败");
    updateTodayImportTaskTitle(savedId, title);
  } catch (error) {
    source.title = previousTitle;
    updateTodayImportTaskTitle(task.id, previousTitle);
    todayImportTitleErrors.value = {
      ...todayImportTitleErrors.value,
      [task.id]: typeof error === "string" ? error : error instanceof Error ? error.message : "事项名称保存失败",
    };
  } finally {
    todayImportTitleSavingId.value = "";
  }
}

async function openTodayImportCreate(parentId = "") {
  const parent = parentId ? activeImportTasks.value.find((task) => task.id === parentId) : undefined;
  if (parent?.recurrence) {
    ElMessage.warning("重复事项新增子项前必须先关闭重复");
    return;
  }
  todayImportCreateParentId.value = parentId;
  todayImportCreateTargetId.value = "";
  todayImportCreateTitle.value = "";
  todayImportCreateError.value = "";
  todayImportCreateOpen.value = true;
  todayImportQuery.value = "";
  await nextTick();
  todayImportCreateInputRef.value?.focus();
}

async function openTodayImportRelativeCreate(task: Task, placement: "before" | "after") {
  closeTodayImportContextMenu();
  todayImportCreateParentId.value = task.parentId ?? "";
  todayImportCreateTargetId.value = task.id;
  todayImportCreatePlacement.value = placement;
  todayImportCreateTitle.value = "";
  todayImportCreateError.value = "";
  todayImportCreateOpen.value = true;
  todayImportQuery.value = "";
  await nextTick();
  todayImportCreateInputRef.value?.focus();
}

function closeTodayImportCreate() {
  if (todayImportCreateSaving.value) return;
  todayImportCreateOpen.value = false;
  todayImportCreateParentId.value = "";
  todayImportCreateTargetId.value = "";
  todayImportCreateTitle.value = "";
  todayImportCreateError.value = "";
}

async function createTodayImportTask() {
  if (todayImportCreateSaving.value) return;
  const title = todayImportCreateTitle.value.trim();
  if (!title) { todayImportCreateError.value = "事项名称不能为空"; return; }
  todayImportCreateSaving.value = true;
  todayImportCreateError.value = "";
  const parentId = todayImportCreateParentId.value;
  try {
    const taskId = todayImportCreateTargetId.value
      ? await props.createRelativeTask(todayImportCreateTargetId.value, todayImportCreatePlacement.value, title)
      : await props.createTask(title, parentId || undefined);
    todayImportSelection.value = [...new Set([...todayImportSelection.value.filter((id) => id !== parentId), taskId])];
    todayImportEstimates.value = { ...todayImportEstimates.value, [taskId]: "" };
    todayImportTaskTitles.value = { ...todayImportTaskTitles.value, [taskId]: title };
    todayImportCreateSaving.value = false;
    closeTodayImportCreate();
    ElMessage.success(parentId ? "子事项已新增并选中" : "事项已新增并选中");
  } catch (error) {
    todayImportCreateError.value = typeof error === "string" ? error : error instanceof Error ? error.message : "事项新增失败";
  } finally {
    todayImportCreateSaving.value = false;
  }
}

function todayImportContextTask() {
  return activeImportTasks.value.find((task) => task.id === todayImportContextTaskId.value);
}

function openTodayImportContextMenu(event: MouseEvent, task: Task) {
  event.preventDefault();
  event.stopPropagation();
  closeContextMenu();
  const position = contextMenuPosition(event.clientX, event.clientY, 190, 130, window.innerWidth, window.innerHeight);
  todayImportContextTaskId.value = task.id;
  todayImportContextMenuLeft.value = position.left;
  todayImportContextMenuTop.value = position.top;
  todayImportContextMenuOpen.value = true;
}

function closeTodayImportContextMenu() {
  todayImportContextMenuOpen.value = false;
  todayImportContextTaskId.value = "";
}

async function duplicateTodayImportRow(task: Task) {
  closeTodayImportContextMenu();
  try {
    const taskId = await props.duplicateSingleTask(task.id);
    if (!taskId) throw new Error("事项复制失败");
    todayImportSelection.value = [...new Set([...todayImportSelection.value, taskId])];
    todayImportEstimates.value = { ...todayImportEstimates.value, [taskId]: "" };
    todayImportTaskTitles.value = { ...todayImportTaskTitles.value, [taskId]: task.title };
    ElMessage.success("已复制事项");
  } catch (error) {
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "事项复制失败");
  }
}

function runTodayImportContextCommand(command: "before" | "after" | "duplicate") {
  const task = todayImportContextTask();
  if (!task) { closeTodayImportContextMenu(); return; }
  if (command === "duplicate") { void duplicateTodayImportRow(task); return; }
  void openTodayImportRelativeCreate(task, command);
}

function todayImportSubtreeIds(taskId: string) {
  return taskSubtreeIds(activeImportTasks.value, taskId);
}

function isTodayImportSelected(taskId: string) {
  return todayImportSelection.value.includes(taskId);
}

function updateTodayImportSelection(taskId: string, selected: boolean) {
  if (todayPlannedIds.value.has(taskId)) return;
  const next = new Set(todayImportSelection.value);
  if (selected) next.add(taskId);
  else next.delete(taskId);
  todayImportSelection.value = [...next];
}

function inputTodayImportEstimate(taskId: string, value: string) {
  todayImportEstimates.value = { ...todayImportEstimates.value, [taskId]: value };
  const parsed = parseQuickEstimateMinutes(value);
  const errors = { ...todayImportErrors.value };
  if (parsed.ok) {
    delete errors[taskId];
    if (parsed.minutes !== undefined && !isTodayImportSelected(taskId)) todayImportSelection.value = [...todayImportSelection.value, taskId];
  } else errors[taskId] = parsed.message;
  todayImportErrors.value = errors;
}

function setTodayImportEstimate(taskId: string, minutes?: number) {
  inputTodayImportEstimate(taskId, minutes === undefined ? "" : String(minutes));
}

function adjustTodayImportEstimate(taskId: string, delta: number) {
  const parsed = parseQuickEstimateMinutes(todayImportEstimates.value[taskId] ?? "");
  const minutes = parsed.ok ? parsed.minutes : undefined;
  setTodayImportEstimate(taskId, adjustQuickEstimateMinutes(minutes, delta));
}

function todayImportEstimateChanged(task: Task, minutes?: number) {
  return minutes !== task.todayEstimate;
}

async function saveTodayImport() {
  if (todayImportSaving.value) return;
  const errors: Record<string, string> = {};
  const parsedEstimates = new Map<string, number | undefined>();
  for (const taskId of todayImportSelection.value) {
    const parsed = parseQuickEstimateMinutes(todayImportEstimates.value[taskId] ?? "");
    if (!parsed.ok) errors[taskId] = parsed.message;
    else parsedEstimates.set(taskId, parsed.minutes);
  }
  todayImportErrors.value = errors;
  if (Object.keys(errors).length) return;
  const newlySelectedIds = todayImportSelection.value.filter((taskId) => !todayPlannedIds.value.has(taskId));
  const estimates = todayImportSelection.value.flatMap((taskId) => {
    const task = props.tasks.find((candidate) => candidate.id === taskId);
    const minutes = parsedEstimates.get(taskId);
    if (!task || !todayImportEstimateChanged(task, minutes)) return [];
    return [{ taskId, expectedTaskVersion: task.version, expectedDailyEstimateVersion: task.dailyEstimateVersion, estimateMinutes: minutes }];
  });
  if (!newlySelectedIds.length && !estimates.length) {
    todayImportOpen.value = false;
    return;
  }
  todayImportSaving.value = true;
  try {
    await props.saveTodayImport({ taskIds: newlySelectedIds, estimates });
    todayImportOpen.value = false;
    ElMessage.success(`已导入 ${newlySelectedIds.length} 项，更新 ${estimates.length} 项今日预计`);
  } catch (error) {
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "今日事项配置失败");
  } finally {
    todayImportSaving.value = false;
  }
}

const treeSortable = createTaskTreeSortable({
  container: () => rowsRef.value,
  rowSelector: ".pending-reminder-row",
  handleSelector: ".drag-handle",
  taskIdAttribute: "data-task-id",
  subtreeIds: getSubtreeIds,
  parentId: (taskId) => props.tasks.find((task) => task.id === taskId)?.parentId,
  onStart() {
    closeContextMenu();
    isSorting.value = true;
  },
  onIntent(taskId, intent) {
    emit("move", taskId, intent.parentId, intent.previousSiblingId, intent.nextSiblingId);
  },
  onInvalid() { ElMessage.warning("该落点会产生三级事项，请选择一级或同级位置"); },
  onSettled: rebuildSortableList,
});

const todayImportSortable = createTaskTreeSortable({
  container: () => todayImportRowsRef.value,
  rowSelector: ".today-import-row[data-today-import-task-id]",
  handleSelector: ".today-import-drag-handle",
  taskIdAttribute: "data-today-import-task-id",
  disabled: () => Boolean(todayImportQuery.value.trim()),
  subtreeIds: todayImportSubtreeIds,
  parentId: (taskId) => activeImportTasks.value.find((task) => task.id === taskId)?.parentId,
  onStart: () => { todayImportMoving.value = true; },
  async onIntent(taskId, intent) {
    try {
      await props.moveTask(taskId, intent.parentId, intent.previousSiblingId, intent.nextSiblingId);
    } catch (error) {
      ElMessage.warning(typeof error === "string" ? error : error instanceof Error ? error.message : "事项移动失败");
    }
  },
  onInvalid: () => { ElMessage.warning("该落点会产生三级事项，请选择一级或同级位置"); },
  async onSettled() {
    todayImportMoving.value = false;
    await nextTick();
    todayImportSortable.mount();
  },
});

function mountSortable() {
  treeSortable.mount();
}

async function openInlineActualAdjustment(task: Task, surface: "list" | "today-import") {
  if (hasChildren(task.id) || inlineActualLoading.value) return;
  inlineActualTaskId.value = task.id;
  inlineActualSurface.value = surface;
  inlineActualTask.value = undefined;
  inlineActualWorkDate.value = "";
  inlineActualTargetText.value = String(task.directMinutes);
  inlineActualNote.value = "";
  inlineActualError.value = "";
  inlineActualLoading.value = true;
  try {
    const snapshot = await props.loadActualSnapshot();
    if (inlineActualTaskId.value !== task.id || inlineActualSurface.value !== surface) return;
    const snapshotTask = snapshot.tasks.find((item) => item.id === task.id && item.isLeaf);
    if (!snapshotTask) throw new Error("事项今日用时已变化，请刷新后重试");
    inlineActualTask.value = snapshotTask;
    inlineActualWorkDate.value = snapshot.workDate;
    inlineActualTargetText.value = String(snapshotTask.directMinutes);
  } catch (error) {
    inlineActualError.value = typeof error === "string" ? error : error instanceof Error ? error.message : "今日用时加载失败";
  } finally {
    inlineActualLoading.value = false;
  }
}

function resetInlineActualAdjustment() {
  inlineActualTaskId.value = "";
  inlineActualSurface.value = "";
  inlineActualTask.value = undefined;
  inlineActualWorkDate.value = "";
  inlineActualTargetText.value = "";
  inlineActualNote.value = "";
  inlineActualError.value = "";
}

function inlineActualDirty() {
  return Boolean(inlineActualTask.value && inlineActualParsed.value.ok && inlineActualDelta.value !== 0);
}

async function setInlineActualAdjustmentVisible(task: Task, visible: boolean, surface: "list" | "today-import") {
  if (visible) {
    await openInlineActualAdjustment(task, surface);
    return;
  }
  if (inlineActualTaskId.value !== task.id || inlineActualSurface.value !== surface) return;
  if (inlineActualSaving.value || inlineActualLoading.value) return;
  if (inlineActualDirty()) {
    try {
      await ElMessageBox.confirm("今日实际还有未保存修改，确定放弃吗？", "放弃今日实际调整", { confirmButtonText: "放弃", cancelButtonText: "继续编辑", type: "warning" });
    } catch { return; }
  }
  resetInlineActualAdjustment();
}

async function saveInlineActualAdjustment() {
  const parsed = inlineActualParsed.value;
  const taskId = inlineActualTaskId.value;
  if (!taskId || !parsed.ok) {
    inlineActualError.value = parsed.ok ? "事项实际基线已失效，请刷新后重试" : parsed.message;
    return;
  }
  inlineActualSaving.value = true;
  inlineActualError.value = "";
  try {
    const snapshot = await props.loadActualSnapshot();
    const task = snapshot.tasks.find((item) => item.id === taskId && item.isLeaf);
    if (!task?.adjustmentFingerprint) throw new Error("事项实际基线已失效，请刷新后重试");
    if (parsed.minutes === task.directMinutes) {
      resetInlineActualAdjustment();
      return;
    }
    await props.saveActualBatch({
      subjectId: task.subjectId,
      workDate: snapshot.workDate,
      clientRequestId: crypto.randomUUID(),
      batchId: crypto.randomUUID(),
      note: inlineActualNote.value.trim() || undefined,
      items: [{
        taskId: task.id,
        expectedTaskVersion: task.version,
        baselineMinutes: task.directMinutes,
        targetMinutes: parsed.minutes,
        deltaMinutes: parsed.minutes - task.directMinutes,
        adjustmentFingerprint: task.adjustmentFingerprint,
      }],
      newTasks: [],
    });
    resetInlineActualAdjustment();
    ElMessage.success("今日实际已调整");
  } catch (error) {
    inlineActualError.value = typeof error === "string" ? error : error instanceof Error ? error.message : "今日实际调整失败";
  } finally {
    inlineActualSaving.value = false;
  }
}

function activeSegmentMinutes(taskId: string) {
  return props.runningTaskId === taskId ? Math.max(0, props.runningTaskMinutes ?? 0) : 0;
}

async function rebuildSortableList() {
  treeSortable.destroy();
  listRenderVersion.value += 1;
  await nextTick();
  isSorting.value = false;
  await nextTick();
  mountSortable();
}

onMounted(() => {
  mountSortable();
  document.addEventListener("pointerdown", closeContextMenuForPointer);
  document.addEventListener("keydown", closeContextMenuForKey);
  window.addEventListener("scroll", closeContextMenu, true);
  window.addEventListener("scroll", closeTodayImportContextMenu, true);
});

watch([todayImportOpen, todayImportRows, todayImportQuery], async ([open]) => {
  if (!open) { todayImportSortable.destroy(); closeTodayImportContextMenu(); return; }
  await nextTick();
  todayImportSortable.mount();
}, { flush: "post" });

onBeforeUnmount(() => {
  treeSortable.destroy();
  todayImportSortable.destroy();
  document.removeEventListener("pointerdown", closeContextMenuForPointer);
  document.removeEventListener("keydown", closeContextMenuForKey);
  window.removeEventListener("scroll", closeContextMenu, true);
  window.removeEventListener("scroll", closeTodayImportContextMenu, true);
  if (levelChangedTimer) window.clearTimeout(levelChangedTimer);
});
</script>

<template>
  <div ref="listRef" class="plan-reminder-editor" :class="{ 'today-plan-mode': mode === 'today', 'today-plan-loading': todayLoading }" :aria-busy="todayLoading">
    <div class="plan-editor-head">
      <div>
        <h2>{{ mode === 'today' ? subjectName + ' · 今日计划' : subjectName }}</h2>
        <span class="plan-editor-caption">
          {{ pendingRows.filter((row) => isTodayMember(row.task)).length }} 项待办 · 点击文字直接编辑
        </span>
      </div>
      <div v-if="mode === 'today'" class="plan-today-metrics" aria-label="今日计划时长统计">
        <span><small>今日预计</small><strong>{{ formatMinutes(todayEstimatedTotalMinutes) }}</strong></span>
        <span><small>今日实际</small><strong>{{ formatMinutes(todayActualTotalMinutes) }}</strong></span>
      </div>
      <div class="plan-head-actions">
        <button v-if="mode === 'today'" class="secondary-button quick-estimate-entry" type="button" title="选择今日事项并填写今日预计" @click="openTodayImport"><ListPlus :size="14" />配置今日事项</button>
        <button class="secondary-button quick-estimate-entry" type="button" :title="mode === 'today' ? '批量编辑今日计划' : '批量编辑全部进行中计划'" @click="openQuickPlan"><FilePenLine :size="14" />{{ mode === 'today' ? '编辑今日计划' : '编辑全部计划' }}</button>
        <button v-if="mode === 'active'" class="icon-button subtle" type="button" title="同步到 SeaTable" aria-label="同步到 SeaTable" @click="emit('syncSeaTable')"><DatabaseZap :size="15" /></button>
        <button v-if="mode === 'active'" class="icon-button subtle plan-import-button" type="button" title="从昨日 Obsidian 日报导入计划" aria-label="导入计划" @click="emit('importPlan')"><FileInput :size="15" /></button>
      </div>
    </div>

    <div v-if="mode === 'today' && error && !loading" class="today-plan-error" role="status">今日计划刷新失败，仍显示上一次成功结果：{{ error }}</div>
    <div class="visually-hidden" role="status" aria-live="polite">{{ todayLoading ? '正在加载今日计划' : '' }}</div>
    <div :key="listRenderVersion" ref="rowsRef" class="reminder-list" :class="{ 'is-sorting': isSorting }">
      <div v-if="todayLoading" class="today-plan-skeleton" aria-hidden="true">
        <div v-for="index in 4" :key="index" class="today-plan-skeleton-row" :class="{ child: index === 2 || index === 3 }">
          <i class="today-plan-skeleton-grip"></i>
          <i class="today-plan-skeleton-check"></i>
          <span><i class="today-plan-skeleton-title"></i><i class="today-plan-skeleton-meta"></i></span>
        </div>
      </div>
      <template v-for="(row, rowIndex) in visiblePendingRows" :key="row.task.id">
        <div
          class="reminder-row pending-reminder-row"
          :class="[
            {
              child: row.depth > 0,
              selected: row.task.id === selectedId,
              context: mode === 'today' && !isTodayMember(row.task),
              completed: isTaskCompleted(row.task),
              'level-changed': row.task.id === levelChangedId,
              'task-group-start': isTaskGroupStart(rowIndex),
              'task-group-single': row.depth === 0 && !isTaskGroupStart(rowIndex),
              'task-group-child': row.depth > 0
            }
          ]"
          :data-task-id="row.task.id"
          :data-depth="row.depth"
          :style="{ '--task-depth': String(row.depth) }"
          @click="selectTask(row.task)"
          @contextmenu="openContextMenu($event, row.task)"
        >
          <button class="drag-handle" :title="getSubtreeIds(row.task.id).size > 1 ? `拖动整组（共 ${getSubtreeIds(row.task.id).size} 项）` : '拖动排序'" @pointerdown="prepareSubtreeDrag(row.task.id)"><GripVertical :size="14" /></button>
          <button class="check-button" :class="{ completed: isTaskCompleted(row.task) }" :disabled="!canToggleTask(row.task)" :title="taskToggleTitle(row.task)" @click.stop="emit('toggle', row.task, $event)">
            <CheckCircle2 v-if="isTaskCompleted(row.task)" :size="18" />
            <Circle v-else :size="18" />
          </button>
          <div class="reminder-main">
            <input :data-task-input="row.task.id" v-model="row.task.title" :placeholder="row.depth ? '新子项' : '新待办'" @focus="selectTask(row.task)" @blur="finishTaskEdit(row.task)" @keydown.tab="handleTitleTab($event, row.task)" />
            <div class="reminder-meta">
              <span class="task-level-label"><CornerDownRight v-if="row.depth" :size="12" />{{ row.depth + 1 }}级</span>
              <span class="meta-picker" :class="{ filled: row.task.planDate }" title="设置预计开始日期" @click.stop="openNativePicker(row.task.id)">
                <input :data-date-input="row.task.id" v-model="row.task.planDate" aria-label="预计开始日期" tabindex="-1" type="date" @change="saveEditableTask(row.task)" />
                <CalendarDays :size="12" />{{ row.task.planDate || '未设置开始日期' }}
              </span>
              <span class="meta-picker estimate-picker" :class="{ filled: row.task.estimate }" :title="row.task.recurrence ? '设置每次预计用时' : '设置整体预计用时'" @click.stop="openEstimateEditor(row.task.id)">
                <TimerReset :size="12" />{{ row.task.estimate ? (row.task.recurrence ? '每次预计 ' : '整体预计 ') + formatMinutes(row.task.estimate) : (row.task.recurrence ? '未填每次预计' : '未填整体预计') }}
                <input
                  v-if="editingEstimateId === row.task.id"
                  :data-estimate-input="row.task.id"
                  v-model.number="row.task.estimate"
                  min="0"
                  placeholder="分钟"
                  type="number"
                  @blur="finishEstimateEditor(row.task)"
                  @click.stop
                  @keydown.enter.prevent="finishEstimateEditor(row.task)"
                  @keydown.esc.prevent="closeEstimateEditor"
                />
              </span>
              <span class="meta-picker estimate-picker today-estimate-picker" :class="{ filled: row.task.todayEstimate }" title="设置今日预计用时" @click.stop="openTodayEstimateEditor(row.task)">
                <TimerReset :size="12" />{{ row.task.todayEstimate ? '今日预计 ' + formatMinutes(row.task.todayEstimate) : '未填今日预计' }}
                <input
                  v-if="editingTodayEstimateId === row.task.id"
                  :data-today-estimate-input="row.task.id"
                  v-model.number="todayEstimateDraftValue"
                  min="0"
                  placeholder="分钟"
                  type="number"
                  @blur="finishTodayEstimateEditor(row.task)"
                  @click.stop
                  @keydown.enter.prevent="finishTodayEstimateEditor(row.task)"
                  @keydown.esc.prevent="closeTodayEstimateEditor"
                />
              </span>
              <span v-if="mode === 'today' && hasChildren(row.task.id)" class="meta-picker actual-time-picker summary" :class="{ filled: row.task.actual > 0 }" title="子事项今日用时合计">
                <Clock3 :size="12" />今日用时 {{ formatMinutes(row.task.actual) }} · 合计
              </span>
              <el-popover
                v-else-if="mode === 'today'"
                :visible="inlineActualSurface === 'list' && inlineActualTaskId === row.task.id"
                placement="bottom"
                :width="340"
                trigger="click"
                @update:visible="setInlineActualAdjustmentVisible(row.task, $event, 'list')"
              >
                <template #reference>
                  <button class="meta-picker actual-time-picker" :class="{ filled: row.task.directMinutes > 0, active: runningTaskId === row.task.id }" type="button" :aria-label="`调整${row.task.title}今日用时`" @click.stop>
                    <Clock3 :size="12" />今日用时 {{ formatMinutes(row.task.directMinutes) }}
                  </button>
                </template>
                <div v-if="inlineActualSurface === 'list' && inlineActualTaskId === row.task.id" class="actual-adjustment-inline">
                  <strong>{{ row.task.title }} · 今日实际</strong>
                  <div v-if="inlineActualLoading" class="actual-adjustment-inline-summary"><span>正在加载最新用时…</span></div>
                  <template v-else>
                    <div class="actual-adjustment-inline-summary">
                      <span>当前已确认 <b>{{ formatMinutes(inlineActualBaseline) }}</b></span>
                      <span v-if="activeSegmentMinutes(row.task.id)">当前本段 <b>{{ formatMinutes(activeSegmentMinutes(row.task.id)) }}</b>，尚未计入</span>
                    </div>
                    <label><span>目标分钟</span><input v-model="inlineActualTargetText" inputmode="numeric" aria-label="目标实际分钟" /></label>
                    <div class="actual-adjustment-inline-delta">本次变化 <strong>{{ inlineActualDelta >= 0 ? '+' : '' }}{{ inlineActualDelta }} 分钟</strong></div>
                    <label><span>备注（可选）</span><input v-model="inlineActualNote" placeholder="例如：按工作记录核对" /></label>
                    <small v-if="!inlineActualParsed.ok" class="error">{{ inlineActualParsed.message }}</small>
                    <small v-else-if="inlineActualError" class="error">{{ inlineActualError }}</small>
                    <div class="actual-adjustment-inline-actions">
                      <button class="secondary-button compact" type="button" :disabled="inlineActualSaving" @click="resetInlineActualAdjustment">取消</button>
                      <button class="primary-button compact" type="button" :disabled="inlineActualSaving || !inlineActualTask || !inlineActualParsed.ok || inlineActualDelta === 0" @click="saveInlineActualAdjustment">{{ inlineActualSaving ? '保存中...' : '确认调整' }}</button>
                    </div>
                  </template>
                </div>
              </el-popover>
              <el-tooltip
                :content="row.task.note?.trim() || '暂无备注，点击添加'"
                effect="dark"
                placement="top"
                :show-after="280"
              >
                <button class="meta-picker note-picker" :class="{ filled: row.task.note?.trim() }" type="button" @click.stop="openNoteEditor(row.task)">
                  <MessageSquareText :size="12" />{{ row.task.note?.trim() ? '有备注' : '备注' }}
                </button>
              </el-tooltip>
              <button
                v-if="!hasChildren(row.task.id)"
                class="meta-picker recurrence-picker"
                :class="{ filled: row.task.recurrence }"
                type="button"
                title="设置重复规则"
                @click.stop="openRecurrenceEditor(row.task)"
              >
                <Repeat2 :size="12" />{{ row.task.recurrence?.summary ?? '不重复' }}
              </button>
            </div>
          </div>
          <div class="reminder-inline-controls">
            <button
              v-if="isTodayMember(row.task) && row.task.status !== 'done' && row.task.selectable"
              class="icon-button subtle hover-row-action task-timer-action"
              :class="{ active: runningTaskId === row.task.id }"
              :title="runningTaskId === row.task.id ? '当前事项正在计时' : runningTaskId ? '结束当前计时并切换到此事项' : '开始计时'"
              @click.stop="emit('startTimer', row.task.id)"
            >
              <Radio v-if="runningTaskId === row.task.id" :size="14" />
              <Play v-else :size="14" fill="currentColor" />
            </button>
            <button class="icon-button subtle hover-row-action" :title="canOutdent(row.task) ? '减少缩进（Shift+Tab）' : '当前已经是一级'" :disabled="!canOutdent(row.task)" @click.stop="changeIndent(row.task, 'outdent')"><IndentDecrease :size="14" /></button>
            <button class="icon-button subtle hover-row-action" :title="canIndent(row.task) ? '增加缩进（Tab）' : '前面没有可作为上级的同级事项'" :disabled="!canIndent(row.task)" @click.stop="changeIndent(row.task, 'indent')"><IndentIncrease :size="14" /></button>
            <button class="icon-button subtle hover-row-action" :title="getSubtreeIds(row.task.id).size > 1 ? `复制整组（共 ${getSubtreeIds(row.task.id).size} 项）` : '复制事项'" @click.stop="duplicateTask(row.task)"><Copy :size="14" /></button>
            <button v-if="mode === 'active' && row.task.status === 'open' && row.task.selectable && !hasChildren(row.task.id) && !knownTodayMemberIds.has(row.task.id)" class="icon-button subtle hover-row-action add-today-row-action" title="加入今日" aria-label="加入今日" @click.stop="emit('addTodayTasks', [row.task.id])"><ListPlus :size="14" /></button>
            <button v-if="mode === 'today' && isTodayMember(row.task)" class="icon-button subtle hover-row-action" title="移出今日" @click.stop="emit('removeToday', row.task.id)"><ListMinus :size="14" /></button>
            <button class="icon-button subtle hover-row-action" title="删除" @click.stop="confirmDelete(row.task)"><Trash2 :size="14" /></button>
          </div>
        </div>
        <button
          v-if="childPlaceholderParent(rowIndex)"
          class="reminder-placeholder-row child-placeholder-row task-group-end"
          type="button"
          aria-label="新建子项"
          title="新建子项"
          @click="addChild(childPlaceholderParent(rowIndex)!)"
        >
          <span class="drag-handle-spacer" aria-hidden="true"></span>
          <span class="placeholder-plus" aria-hidden="true"><Plus :size="18" /></span>
          <span class="placeholder-input child-placeholder-input" aria-hidden="true"></span>
        </button>
      </template>
      <button v-if="!todayLoading" class="reminder-placeholder-row plan-placeholder-row" type="button" aria-label="新建待办" title="新建待办" @click="addTask">
        <span class="drag-handle-spacer" aria-hidden="true"></span>
        <span class="placeholder-plus" aria-hidden="true"><Plus :size="18" /></span>
        <span class="placeholder-input" aria-hidden="true"></span>
      </button>
    </div>

    <el-dialog v-model="todayImportOpen" class="recurrence-dialog today-import-dialog" modal-class="today-import-overlay" title="配置今日事项" width="min(820px, 95vw)" append-to-body align-center :close-on-click-modal="false">
      <section class="today-import-overview" aria-label="今日计划时长统计">
        <div class="today-import-overview-context">
          <span>今日计划总览</span>
          <strong>{{ subjectName }} · {{ todayDate }}</strong>
          <small>预计时长会跟随下方的事项选择和时间填写即时更新。</small>
        </div>
        <div class="today-import-overview-metrics">
          <div class="today-import-overview-metric actual"><span>今日实际</span><strong>{{ formatMinutes(todayActualTotalMinutes) }}</strong></div>
          <div class="today-import-overview-metric estimate"><span>今日预计</span><strong>{{ formatMinutes(todayImportTotalMinutes) }}</strong></div>
        </div>
      </section>
      <div class="today-import-toolbar">
        <div><strong>选择今日事项</strong><span>从全部进行中事项中选择；今日预计可留空，单位为分钟。</span></div>
        <input v-model="todayImportQuery" class="today-plan-search" placeholder="搜索事项或父级路径" />
      </div>
      <div class="today-import-table-tools">
        <button class="secondary-button compact" type="button" @click="openTodayImportCreate()"><Plus :size="14" />新增事项</button>
        <div class="today-import-summary">
          <span>已选择 <strong>{{ todayImportSelection.length }}</strong> 项</span>
          <span>新加入 <strong>{{ todayImportSelection.filter((id) => !todayPlannedIds.has(id)).length }}</strong> 项</span>
        </div>
      </div>
      <form v-if="todayImportCreateOpen" class="today-import-create-bar" @submit.prevent="createTodayImportTask">
        <div>
          <strong>{{ todayImportCreateParentId ? '新增子事项' : '新增一级事项' }}</strong>
          <small v-if="todayImportCreateParentId">添加到“{{ activeImportTasks.find((task) => task.id === todayImportCreateParentId)?.title }}”下</small>
          <small v-else>创建后会自动选中，可继续填写今日预计。</small>
        </div>
        <input ref="todayImportCreateInputRef" v-model="todayImportCreateTitle" placeholder="输入事项名称" aria-label="新增事项名称" @input="todayImportCreateError = ''" @keydown.esc.prevent="closeTodayImportCreate" />
        <button class="secondary-button compact" type="button" :disabled="todayImportCreateSaving" @click="closeTodayImportCreate">取消</button>
        <button class="primary-button compact" type="submit" :disabled="todayImportCreateSaving">{{ todayImportCreateSaving ? '新增中...' : '新增' }}</button>
        <small v-if="todayImportCreateError" class="today-import-create-error">{{ todayImportCreateError }}</small>
      </form>
      <div v-if="todayImportRows.length" class="today-import-list">
        <div class="today-import-list-head" aria-hidden="true"><span>事项</span><span>今日实际</span><span>今日预计</span><span>操作</span></div>
        <div ref="todayImportRowsRef" class="today-import-sortable-rows" :class="{ moving: todayImportMoving }">
        <div v-for="row in todayImportRows" :key="row.task.id" class="today-import-row" :data-today-import-task-id="row.task.id" :data-depth="row.depth" :class="{ group: !row.allocatable, child: row.depth > 0, selected: isTodayImportSelected(row.task.id) }" :style="{ '--task-depth': String(row.depth) }">
          <button class="today-import-drag-handle" type="button" :disabled="Boolean(todayImportQuery.trim())" :class="{ active: todayImportContextTaskId === row.task.id }" :title="todayImportQuery.trim() ? '清空搜索后调整顺序和层级' : '拖动排序；右键可复制事项或插入行'" :aria-label="`${row.task.title}拖动排序，右键打开行菜单`" @pointerdown="todayImportSortable.prepareSubtree(row.task.id)" @contextmenu="openTodayImportContextMenu($event, row.task)"><GripVertical :size="14" /></button>
          <template v-if="row.allocatable">
            <input class="today-import-check" type="checkbox" :checked="isTodayImportSelected(row.task.id)" :disabled="todayPlannedIds.has(row.task.id)" :aria-label="`${row.task.title}加入今日`" @change="updateTodayImportSelection(row.task.id, ($event.target as HTMLInputElement).checked)" />
          </template>
          <span v-else class="today-import-check-spacer" aria-hidden="true"></span>
          <div class="today-import-task">
            <div class="today-import-title-line">
              <CornerDownRight v-if="row.depth" :size="14" />
              <input
                :value="todayImportTaskTitles[row.task.id] ?? row.task.title"
                :aria-label="`编辑事项名称：${row.task.title}`"
                @input="updateTodayImportTaskTitle(row.task.id, ($event.target as HTMLInputElement).value)"
                @blur="saveTodayImportTaskTitle(row.task)"
                @keydown.enter.prevent="($event.target as HTMLInputElement).blur()"
                @keydown.esc.prevent="resetTodayImportTaskTitle(row.task); ($event.target as HTMLInputElement).blur()"
              />
            </div>
            <small v-if="!row.allocatable">父事项 · 仅用于层级识别</small>
            <small v-if="todayImportTitleSavingId === row.task.id" class="today-import-title-status">保存中...</small>
            <small v-else-if="todayImportTitleErrors[row.task.id]" class="today-import-title-error">{{ todayImportTitleErrors[row.task.id] }}</small>
          </div>
          <div class="today-import-actual">
            <el-popover v-if="row.allocatable" :visible="inlineActualSurface === 'today-import' && inlineActualTaskId === row.task.id" placement="bottom" :width="340" trigger="click" @update:visible="setInlineActualAdjustmentVisible(row.task, $event, 'today-import')">
              <template #reference><button type="button" :aria-label="`调整${row.task.title}今日实际`">{{ formatMinutes(row.task.directMinutes) }}</button></template>
              <div v-if="inlineActualSurface === 'today-import' && inlineActualTaskId === row.task.id" class="actual-adjustment-inline">
                <strong>{{ row.task.title }} · 今日实际</strong>
                <div v-if="inlineActualLoading" class="actual-adjustment-inline-summary"><span>正在加载最新用时…</span></div>
                <template v-else>
                  <div class="actual-adjustment-inline-summary"><span>当前已确认 <b>{{ formatMinutes(inlineActualBaseline) }}</b></span><span v-if="activeSegmentMinutes(row.task.id)">当前本段 <b>{{ formatMinutes(activeSegmentMinutes(row.task.id)) }}</b>，尚未计入</span></div>
                  <label><span>目标分钟</span><input v-model="inlineActualTargetText" inputmode="numeric" aria-label="目标实际分钟" /></label>
                  <div class="actual-adjustment-inline-delta">本次变化 <strong>{{ inlineActualDelta >= 0 ? '+' : '' }}{{ inlineActualDelta }} 分钟</strong></div>
                  <label><span>备注（可选）</span><input v-model="inlineActualNote" placeholder="例如：按工作记录核对" /></label>
                  <small v-if="!inlineActualParsed.ok" class="error">{{ inlineActualParsed.message }}</small>
                  <small v-else-if="inlineActualError" class="error">{{ inlineActualError }}</small>
                  <div class="actual-adjustment-inline-actions"><button class="secondary-button compact" type="button" :disabled="inlineActualSaving" @click="resetInlineActualAdjustment">取消</button><button class="primary-button compact" type="button" :disabled="inlineActualSaving || !inlineActualTask || !inlineActualParsed.ok || inlineActualDelta === 0" @click="saveInlineActualAdjustment">{{ inlineActualSaving ? '保存中...' : '确认调整' }}</button></div>
                </template>
              </div>
            </el-popover>
            <strong v-else>{{ formatMinutes(row.task.directMinutes) }}</strong>
          </div>
          <div v-if="row.allocatable" class="today-import-estimate">
            <div class="today-import-estimate-editor">
              <button type="button" title="减少 15 分钟" :aria-label="`${row.task.title}减少 15 分钟`" @click="adjustTodayImportEstimate(row.task.id, -15)"><Minus :size="14" /></button>
              <el-dropdown class="today-import-minute-dropdown" trigger="click" placement="bottom-start" popper-class="today-import-preset-popper" @command="(minutes: number) => setTodayImportEstimate(row.task.id, minutes)">
                <label>
                  <input :value="todayImportEstimates[row.task.id]" inputmode="numeric" placeholder="可留空" :aria-label="`${row.task.title}今日预计分钟，可点击选择常用时长`" @input="inputTodayImportEstimate(row.task.id, ($event.target as HTMLInputElement).value)" />
                  <span>分钟</span>
                </label>
                <template #dropdown>
                  <el-dropdown-menu>
                    <el-dropdown-item v-for="preset in todayImportEstimatePresets" :key="preset.minutes" :command="preset.minutes">{{ preset.label }}</el-dropdown-item>
                  </el-dropdown-menu>
                </template>
              </el-dropdown>
              <button type="button" title="增加 15 分钟" :aria-label="`${row.task.title}增加 15 分钟`" @click="adjustTodayImportEstimate(row.task.id, 15)"><Plus :size="14" /></button>
            </div>
          </div>
          <span v-else class="today-import-estimate-empty">—</span>
          <div class="today-import-actions"></div>
          <small v-if="todayImportErrors[row.task.id]" class="today-import-error">{{ todayImportErrors[row.task.id] }}</small>
        </div>
        </div>
      </div>
      <small v-if="todayImportQuery.trim()" class="today-import-drag-hint">搜索状态下不能拖动；清空搜索后可调整顺序和层级。</small>
      <div v-else-if="!todayImportRows.length" class="quick-estimate-empty">没有匹配的进行中事项。</div>
      <template #footer>
        <button class="secondary-button" type="button" :disabled="todayImportSaving || todayImportCreateSaving || Boolean(todayImportTitleSavingId)" @click="todayImportOpen = false">取消</button>
        <button class="primary-button" type="button" :disabled="todayImportSaving || todayImportCreateSaving || Boolean(todayImportTitleSavingId) || !todayImportSelection.length" @click="saveTodayImport">{{ todayImportSaving ? '保存中...' : `保存配置（${todayImportSelection.length}）` }}</button>
      </template>
    </el-dialog>

    <el-dialog v-model="recurrenceDialogOpen" class="recurrence-dialog" title="重复设置" width="min(430px, 92vw)" append-to-body destroy-on-close>
      <div class="recurrence-form">
        <el-segmented v-model="recurrenceFrequency" :options="[
          { label: '不重复', value: 'none' },
          { label: '每天', value: 'daily' },
          { label: '工作日', value: 'weekdays' },
          { label: '每周', value: 'weekly' }
        ]" />
        <label v-if="recurrenceFrequency !== 'none'" class="recurrence-field">
          <span>开始日期</span>
          <el-date-picker v-model="recurrenceStart" type="date" value-format="YYYY-MM-DD" :clearable="false" popper-class="recurrence-date-popper" />
        </label>
        <div v-if="recurrenceFrequency === 'weekly'" class="recurrence-field">
          <span>重复星期</span>
          <el-checkbox-group v-model="recurrenceWeekdays" class="recurrence-weekdays">
            <el-checkbox-button v-for="(label, index) in ['一', '二', '三', '四', '五', '六', '日']" :key="label" :value="index">{{ label }}</el-checkbox-button>
          </el-checkbox-group>
        </div>
      </div>
      <template #footer>
        <button class="secondary-button" type="button" @click="recurrenceDialogOpen = false">取消</button>
        <button class="primary-button" type="button" @click="saveRecurrence">保存</button>
      </template>
    </el-dialog>

    <el-dialog v-model="noteDialogOpen" class="recurrence-dialog task-note-dialog" title="事项备注" width="min(520px, 92vw)" append-to-body>
      <textarea v-model="noteDraft" rows="8" placeholder="记录事项背景、注意点或下一步……"></textarea>
      <template #footer>
        <button class="secondary-button" type="button" @click="noteDialogOpen = false">取消</button>
        <button class="primary-button" type="button" @click="saveNote">保存</button>
      </template>
    </el-dialog>

    <el-dialog
      v-model="quickPlanOpen"
      class="recurrence-dialog quick-plan-dialog"
      :title="mode === 'today' ? '编辑今日计划' : '编辑全部计划'"
      width="min(760px, 95vw)"
      append-to-body
      align-center
      :close-on-click-modal="false"
      :before-close="requestCloseQuickPlan"
    >
      <div v-if="quickPlanMode === 'edit'" class="quick-plan-workspace">
        <div class="quick-plan-help"><strong>{{ subjectName }}{{ mode === 'today' ? ' · 今日' : '' }}</strong><span>{{ mode === 'today' ? '这里只编辑今日事项；删除已有行会移出今日，不会删除事项本体。' : '这里编辑当前主体的全部进行中事项。' }} 每项必须以“- ”开头；一级写作“- 事项”，二级写作“&nbsp;&nbsp;- 子事项”。Tab / Shift+Tab 调整层级。</span></div>
        <MdEditor
          ref="quickPlanEditorRef"
          class="quick-plan-editor"
          editor-id="quick-plan-markdown-editor"
          :model-value="quickPlanText"
          :toolbars="[...quickPlanMarkdownToolbars]"
          :footers="['markdownTotal']"
          :preview="false"
          language="zh-CN"
          theme="dark"
          preview-theme="github"
          code-theme="github"
          placeholder="- 一级事项&#10;  - 二级事项"
          aria-label="计划 Markdown 编辑器"
          @update:model-value="updateQuickPlanText"
          @keydown.capture="handleQuickPlanTextareaKey"
        />
        <div v-if="quickPlanSummary.errors.length" class="quick-plan-errors">
          <strong>请先修正以下问题</strong>
          <span v-for="error in quickPlanSummary.errors" :key="`${error.lineIndex}:${error.message}`">第 {{ error.lineIndex + 1 }} 行：{{ error.message }}</span>
        </div>
      </div>
      <div v-else class="quick-plan-summary">
        <div class="quick-plan-summary-grid">
          <section><strong>新增 {{ quickPlanSummary.added.length }} 项</strong><span v-for="item in quickPlanSummary.added" :key="item.lineKey">{{ formatQuickPlanItem(item.depth, item.title) }}</span></section>
          <section><strong>修改标题 {{ quickPlanSummary.renamed.length }} 项</strong><span v-for="item in quickPlanSummary.renamed" :key="item.lineKey">{{ formatQuickPlanItem(item.depth, item.title) }}</span></section>
          <section><strong>调整顺序或层级 {{ quickPlanSummary.moved.length }} 项</strong><span v-for="item in quickPlanSummary.moved" :key="item.lineKey">{{ formatQuickPlanItem(item.depth, item.title) }}</span></section>
        </div>
        <section v-if="quickPlanSummary.deleted.length" class="quick-plan-delete-summary">
          <strong>{{ mode === 'today' ? '将移出今日' : '将删除' }} {{ quickPlanSummary.deleted.length }} 项</strong>
          <span v-for="item in quickPlanSummary.deleted" :key="item.lineKey" :class="{ child: item.depth }">{{ formatQuickPlanItem(item.depth, item.title) }}</span>
          <small>{{ mode === 'today' ? '这些已有事项将仅移出今日，不会删除事项本体。' : '父事项及其仍在删除范围内的子事项已全部列出，请确认后再保存。' }}</small>
        </section>
      </div>
      <template #footer>
        <template v-if="quickPlanMode === 'edit'">
          <button class="secondary-button" type="button" @click="requestCloseQuickPlan(() => { quickPlanOpen = false; })">取消</button>
          <button class="primary-button" type="button" :disabled="Boolean(quickPlanSummary.errors.length)" @click="openQuickPlanSummary">预览变化</button>
        </template>
        <template v-else>
          <button class="secondary-button" type="button" :disabled="quickPlanSaving" @click="closeQuickPlanSummary">返回编辑</button>
          <button class="primary-button" :class="{ danger: mode !== 'today' && quickPlanSummary.deleted.length }" type="button" :disabled="quickPlanSaving" @click="saveQuickPlan">{{ quickPlanSaving ? '保存中...' : quickPlanSummary.deleted.length ? (mode === 'today' ? '确认移出并保存' : '确认删除并保存') : '确认保存' }}</button>
        </template>
      </template>
    </el-dialog>

    <el-dialog
      v-model="actualAdjustmentOpen"
      class="recurrence-dialog quick-estimate-dialog actual-adjustment-dialog"
      modal-class="quick-estimate-overlay"
      title="调整今日实际"
      width="min(860px, 94vw)"
      append-to-body
      :close-on-click-modal="false"
      :before-close="(_done: () => void) => closeActualAdjustment()"
    >
      <div class="quick-estimate-toolbar actual-adjustment-toolbar">
        <div><strong>{{ subjectName }} · {{ actualAdjustmentSnapshot?.workDate }}</strong><span>输入今天最终应该是多少分钟；计时继续增长不会改变本次固定差额。</span></div>
        <button class="secondary-button" type="button" @click="addActualNewTask"><Plus :size="14" />新增事项</button>
      </div>
      <div class="quick-estimate-summary">
        <span>新增 <strong>{{ actualAdjustmentSummaryValue.newTaskCount }}</strong> 项</span><span>调整 <strong>{{ actualAdjustmentSummaryValue.adjustmentCount }}</strong> 项</span><span>增加 <strong>{{ actualAdjustmentSummaryValue.addedMinutes }}</strong> 分钟</span><span>减少 <strong>{{ actualAdjustmentSummaryValue.reducedMinutes }}</strong> 分钟</span><span>净变化 <strong>{{ actualAdjustmentSummaryValue.netMinutes >= 0 ? '+' : '' }}{{ actualAdjustmentSummaryValue.netMinutes }}</strong> 分钟</span>
      </div>
      <div class="actual-adjustment-list">
        <template v-for="task in actualAdjustmentSnapshot?.tasks ?? []" :key="task.id">
          <div v-if="!task.isLeaf" class="actual-adjustment-row group">
            <div><strong>{{ task.title }}</strong><small>当前合计 {{ formatMinutes(task.totalMinutes) }} · 目标合计 {{ formatMinutes(actualParentTarget(task.id)) }}</small></div>
          </div>
          <div v-else class="actual-adjustment-row" :class="{ child: task.parentId }">
            <div><CornerDownRight v-if="task.parentId" :size="14" /><strong>{{ task.title }}</strong><small>{{ task.timerState ? (task.timerState === 'running' ? '正在计时' : '计时已暂停') : task.status === 'done' ? '今天已完成' : '进行中' }} · 当前 {{ formatMinutes(task.directMinutes) }}</small></div>
            <label><input :value="actualDraft(task.id)?.text" inputmode="numeric" @input="updateActualDraft(task.id, ($event.target as HTMLInputElement).value)" /><span>分钟</span></label>
            <span class="actual-adjustment-delta">{{ (actualDraft(task.id)?.targetMinutes ?? task.directMinutes) - task.directMinutes >= 0 ? '+' : '' }}{{ (actualDraft(task.id)?.targetMinutes ?? task.directMinutes) - task.directMinutes }}</span>
            <small v-if="actualAdjustmentErrors[task.id]" class="error">{{ actualAdjustmentErrors[task.id] }}</small>
          </div>
        </template>
        <div v-for="draft in actualAdjustmentNewTasks" :key="draft.clientTaskId" class="actual-adjustment-row new-task">
          <div class="actual-new-task-fields"><input :value="draft.title" placeholder="新增事项名称" @input="updateActualNewTaskTitle(draft, ($event.target as HTMLInputElement).value)" /><select :value="draft.parentId ?? ''" @change="setActualNewTaskParent(draft, ($event.target as HTMLSelectElement).value)"><option value="">顶层事项</option><option v-for="parent in actualAdjustmentParents" :key="parent.id" :value="parent.id">{{ parent.title }} 下</option></select></div>
          <label><input :value="draft.text" inputmode="numeric" @input="updateActualNewTaskMinutes(draft, ($event.target as HTMLInputElement).value)" /><span>分钟</span></label>
          <button class="icon-button subtle" type="button" title="移除草稿" @click="removeActualNewTask(draft.clientTaskId)"><Trash2 :size="14" /></button>
          <small v-if="actualAdjustmentErrors[draft.clientTaskId]" class="error">{{ actualAdjustmentErrors[draft.clientTaskId] }}</small>
        </div>
      </div>
      <label class="actual-adjustment-note"><span>统一备注（可选）</span><input v-model="actualAdjustmentNote" placeholder="例如：按日报核对后修正" /></label>
      <template #footer><button class="secondary-button" type="button" :disabled="actualAdjustmentSaving" @click="closeActualAdjustment">取消</button><button class="primary-button" type="button" :disabled="actualAdjustmentSaving || !actualAdjustmentDirty" @click="saveActualAdjustment">{{ actualAdjustmentSaving ? '保存中...' : '查看摘要并保存' }}</button></template>
    </el-dialog>

    <Teleport to="body">
      <div v-if="todayImportContextMenuOpen" class="plan-task-context-menu task-row-context-menu" role="menu" :style="{ left: `${todayImportContextMenuLeft}px`, top: `${todayImportContextMenuTop}px` }" @pointerdown.stop>
        <button type="button" role="menuitem" @click="runTodayImportContextCommand('duplicate')">复制事项</button>
        <span class="plan-task-context-divider"></span>
        <button type="button" role="menuitem" @click="runTodayImportContextCommand('before')">在上方插入行</button>
        <button type="button" role="menuitem" @click="runTodayImportContextCommand('after')">在下方插入行</button>
      </div>
      <div v-if="contextMenuOpen" class="plan-task-context-menu" role="menu" :style="{ left: `${contextMenuLeft}px`, top: `${contextMenuTop}px` }" @pointerdown.stop>
        <button type="button" role="menuitem" @click="runContextCommand('duplicate')">复制事项</button>
        <span class="plan-task-context-divider"></span>
        <button type="button" role="menuitem" @click="runContextCommand('before')">在上方新增</button>
        <button type="button" role="menuitem" @click="runContextCommand('after')">在下方新增</button>
        <button type="button" role="menuitem" :disabled="Boolean(contextTask()?.parentId || contextTask()?.recurrence || contextTask()?.directMinutes)" @click="runContextCommand('child')">新建子事项</button>
        <span class="plan-task-context-divider"></span>
        <button type="button" role="menuitem" @click="runContextCommand('note')">编辑备注</button>
        <button class="danger" type="button" role="menuitem" @click="runContextCommand('delete')">删除</button>
      </div>
    </Teleport>
  </div>
</template>
