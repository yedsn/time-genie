<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { MdEditor } from "md-editor-v3";
import "md-editor-v3/lib/style.css";
import Sortable, { MultiDrag } from "sortablejs";
import { CalendarDays, CheckCircle2, Circle, Clock3, Copy, CornerDownRight, DatabaseZap, FileInput, FilePenLine, GripVertical, IndentDecrease, IndentIncrease, MessageSquareText, Minus, Play, Plus, Radio, Repeat2, TimerReset, Trash2 } from "lucide-vue-next";
import type { Task } from "../store";
import { isActivePlanTask } from "../services/completedTaskHistoryCore";
import { contextMenuPosition, planTaskMoveIntent, todayEstimateDraft } from "../services/planTaskInteractionCore";
import { getSubjectWorkSchedule, type SubjectWorkScheduleDay } from "../services/tauri";
import { adjustQuickEstimateMinutes, allocatedMinutes, capacitySummary, changedEstimateDrafts, formatQuickEstimateHours, parseQuickDuration, parseQuickEstimateMinutes, quickEstimateRows, scheduleCapacity, weekdayIndexForDate, type QuickEstimateDraft } from "../services/quickPlanEstimateCore";
import { analyzeQuickPlanDraft, buildQuickPlanBatchRequest, createQuickPlanBaseline, createQuickPlanDraft, reconcileQuickPlanText, serializeQuickPlanLines, type QuickPlanBaselineTask, type QuickPlanBatchRequest, type QuickPlanDraftLine } from "../services/quickPlanTextCore";

const props = defineProps<{
  tasks: Task[];
  subjectId: string;
  subjectName: string;
  selectedId: string;
  runningTaskId?: string;
  formatMinutes: (minutes: number) => string;
  todayDate: string;
  timezone: string;
  saveEstimateBatch: (items: Array<{ taskId: string; expectedTaskVersion: number; expectedDailyEstimateVersion?: number; estimateMinutes?: number }>) => Promise<void>;
  savePlanBatch: (request: QuickPlanBatchRequest) => Promise<{ createdCount: number; updatedCount: number; deletedCount: number }>;
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
  setRecurrence: [taskId: string, recurrence?: { frequency: "daily" | "weekdays" | "weekly"; weekdaysMask?: number; effectiveStart: string }];
}>();

const listRef = ref<HTMLElement | null>(null);
const rowsRef = ref<HTMLElement | null>(null);
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
const dragTargetDepth = ref<0 | 1>(0);
const dragTargetInvalid = ref(false);
const quickEstimateOpen = ref(false);
const quickEstimateSaving = ref(false);
const quickEstimateDrafts = ref<QuickEstimateDraft[]>([]);
const quickEstimateErrors = ref<Record<string, string>>({});
const quickEstimateCapacityText = ref("");
const quickEstimateCapacityMinutes = ref(0);
const quickEstimateDoneFocus = ref(false);
const quickEstimateConfirmingClose = ref(false);
const quickPlanOpen = ref(false);
const quickPlanSaving = ref(false);
const quickPlanMode = ref<"edit" | "summary">("edit");
const quickPlanBaseline = ref<QuickPlanBaselineTask[]>([]);
const quickPlanSourceLines = ref<QuickPlanDraftLine[]>([]);
const quickPlanLines = ref<QuickPlanDraftLine[]>([]);
const quickPlanText = ref("");
const quickPlanConfirmingClose = ref(false);
const quickPlanPasteErrors = ref<Array<{ lineIndex: number; message: string }>>([]);
const quickPlanEditorRef = ref<{ getEditorView?: () => { state: { selection: { main: { from: number; to: number } } }; dispatch: (transaction: unknown) => void } } | undefined>(undefined);
let quickPlanTextVersion = 0;
let sortable: Sortable | undefined;
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

const sortableWithPlugins = Sortable as typeof Sortable & { treeMultiDragMounted?: boolean };
if (!sortableWithPlugins.treeMultiDragMounted) {
  Sortable.mount(new MultiDrag());
  sortableWithPlugins.treeMultiDragMounted = true;
}

const flatRows = computed(() => props.tasks.map((task) => ({ task, depth: getDepth(task) })));
const pendingRows = computed(() => flatRows.value.filter((row) => isActivePlanTask({
  status: row.task.status,
  recurring: Boolean(row.task.recurrence),
})));
const quickRows = computed(() => quickEstimateRows(props.tasks));
const quickAllocatableRows = computed(() => quickRows.value.filter((row) => row.allocatable));
const quickAllocatedMinutes = computed(() => allocatedMinutes(quickEstimateDrafts.value));
const quickCapacitySummary = computed(() => capacitySummary(quickAllocatedMinutes.value, quickEstimateCapacityMinutes.value));
const quickEstimateDirty = computed(() => changedEstimateDrafts(quickEstimateDrafts.value).length > 0);
const quickPlanSummary = computed(() => {
  const summary = analyzeQuickPlanDraft(quickPlanBaseline.value, quickPlanLines.value);
  return { ...summary, errors: [...quickPlanPasteErrors.value, ...summary.errors] };
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

function getSubtreeIds(taskId: string) {
  const ids = new Set([taskId]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const task of props.tasks) {
      if (task.parentId && ids.has(task.parentId) && !ids.has(task.id)) {
        ids.add(task.id);
        changed = true;
      }
    }
  }
  return ids;
}

function prepareSubtreeDrag(taskId: string) {
  if (!rowsRef.value) return;
  const movingIds = getSubtreeIds(taskId);
  const rows = Array.from(rowsRef.value.querySelectorAll<HTMLElement>(".pending-reminder-row"));
  for (const row of rows) Sortable.utils.deselect(row);
  for (const row of rows) {
    if (movingIds.has(row.dataset.taskId ?? "")) Sortable.utils.select(row);
  }
}

function clearSubtreeDrag(rows: HTMLElement[]) {
  for (const row of rows) Sortable.utils.deselect(row);
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
  closeEstimateEditor();
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
  if (!(event.target as HTMLElement).closest(".plan-task-context-menu")) closeContextMenu();
}

function closeContextMenuForKey(event: KeyboardEvent) {
  if (event.key === "Escape") closeContextMenu();
}

function normalizeEstimate(value: number | undefined) {
  const minutes = Number(value);
  return Number.isFinite(minutes) && minutes > 0 ? Math.round(minutes) : undefined;
}

function formatQuickDraft(minutes?: number) {
  return String(minutes ?? 0);
}

function openQuickPlan() {
  const activeTasks = pendingRows.value.map((row) => row.task);
  quickPlanBaseline.value = createQuickPlanBaseline(activeTasks);
  quickPlanSourceLines.value = createQuickPlanDraft(activeTasks);
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
    await ElMessageBox.confirm("快速编辑计划还有未保存修改，确定放弃吗？", "放弃修改", { confirmButtonText: "放弃", cancelButtonText: "继续编辑", type: "warning" });
    done();
  } catch {} finally {
    quickPlanConfirmingClose.value = false;
  }
}

async function saveQuickPlan() {
  if (quickPlanSaving.value || quickPlanSummary.value.errors.length) return;
  quickPlanSaving.value = true;
  try {
    await props.savePlanBatch(buildQuickPlanBatchRequest(props.subjectId, quickPlanBaseline.value, quickPlanSummary.value));
    quickPlanOpen.value = false;
    ElMessage.success("计划已保存");
  } catch (error) {
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "计划保存失败，请重新加载后重试");
  } finally {
    quickPlanSaving.value = false;
  }
}

async function openQuickEstimate() {
  const rows = quickAllocatableRows.value;
  quickEstimateDrafts.value = rows.map(({ task }) => ({
    taskId: task.id,
    originalMinutes: task.todayEstimate,
    minutes: task.todayEstimate,
    text: formatQuickDraft(task.todayEstimate),
    expectedTaskVersion: task.version,
    expectedDailyEstimateVersion: task.dailyEstimateVersion,
  }));
  quickEstimateErrors.value = {};
  quickEstimateDoneFocus.value = false;
  try {
    const schedule: SubjectWorkScheduleDay[] = await getSubjectWorkSchedule(props.subjectId);
    const day = schedule.find((item) => item.weekday === weekdayIndexForDate(props.todayDate, props.timezone));
    quickEstimateCapacityMinutes.value = day ? scheduleCapacity(day.segments, day.enabled) : 0;
  } catch {
    quickEstimateCapacityMinutes.value = 360;
  }
  quickEstimateCapacityText.value = `${quickEstimateCapacityMinutes.value}m`;
  quickEstimateOpen.value = true;
  await focusQuickEstimate(0);
}

function quickDraft(taskId: string) {
  return quickEstimateDrafts.value.find((draft) => draft.taskId === taskId);
}

function updateQuickDraft(taskId: string, text: string, moveAfter = false) {
  const draft = quickDraft(taskId);
  if (!draft) return false;
  const parsed = parseQuickEstimateMinutes(text);
  if (!parsed.ok) {
    quickEstimateErrors.value = { ...quickEstimateErrors.value, [taskId]: parsed.message };
    return false;
  }
  draft.text = formatQuickDraft(parsed.minutes);
  draft.minutes = parsed.minutes;
  const nextErrors = { ...quickEstimateErrors.value };
  delete nextErrors[taskId];
  quickEstimateErrors.value = nextErrors;
  if (moveAfter) moveQuickEstimateFocus(taskId, 1);
  return true;
}

function inputQuickDraft(taskId: string, text: string) {
  const draft = quickDraft(taskId);
  if (!draft) return;
  draft.text = text;
  const parsed = parseQuickEstimateMinutes(text);
  if (!parsed.ok) {
    quickEstimateErrors.value = { ...quickEstimateErrors.value, [taskId]: parsed.message };
    return;
  }
  draft.minutes = parsed.minutes;
  const nextErrors = { ...quickEstimateErrors.value };
  delete nextErrors[taskId];
  quickEstimateErrors.value = nextErrors;
}

function setQuickMinutes(taskId: string, minutes?: number) {
  const draft = quickDraft(taskId);
  if (!draft) return;
  draft.minutes = minutes;
  draft.text = formatQuickDraft(minutes);
  const nextErrors = { ...quickEstimateErrors.value };
  delete nextErrors[taskId];
  quickEstimateErrors.value = nextErrors;
}

function adjustQuickMinutes(taskId: string, delta: number) {
  const draft = quickDraft(taskId);
  if (!draft) return;
  setQuickMinutes(taskId, adjustQuickEstimateMinutes(draft.minutes, delta));
}

function quickDraftHours(taskId: string) {
  const draft = quickDraft(taskId);
  if (!draft) return "0 小时";
  const parsed = parseQuickEstimateMinutes(draft.text);
  return parsed.ok ? formatQuickEstimateHours(parsed.minutes) : "—";
}

function applyQuickSuggestion(task: Task) {
  if (task.estimate) setQuickMinutes(task.id, task.estimate);
}

async function focusQuickEstimate(index: number) {
  const row = quickAllocatableRows.value[index];
  if (!row) return;
  await nextTick();
  const input = document.querySelector<HTMLInputElement>(`[data-quick-estimate-input="${row.task.id}"]`);
  input?.focus();
  input?.select();
}

function moveQuickEstimateFocus(taskId: string, offset: number) {
  const index = quickAllocatableRows.value.findIndex((row) => row.task.id === taskId);
  const target = index + offset;
  if (target >= 0 && target < quickAllocatableRows.value.length) {
    void focusQuickEstimate(target);
  } else if (offset > 0) {
    quickEstimateDoneFocus.value = true;
    void nextTick(() => document.querySelector<HTMLButtonElement>("[data-quick-estimate-submit]")?.focus());
  }
}

function handleQuickKey(event: KeyboardEvent, taskId: string) {
  const target = event.currentTarget as HTMLInputElement;
  if (event.key === "Enter") {
    event.preventDefault();
    updateQuickDraft(taskId, target.value, true);
  }
  if (event.key === "ArrowDown") {
    event.preventDefault();
    if (updateQuickDraft(taskId, target.value)) moveQuickEstimateFocus(taskId, 1);
  }
  if (event.key === "ArrowUp") {
    event.preventDefault();
    if (updateQuickDraft(taskId, target.value)) moveQuickEstimateFocus(taskId, -1);
  }
}

function updateQuickCapacity() {
  const parsed = parseQuickDuration(quickEstimateCapacityText.value);
  if (parsed.ok && parsed.minutes !== undefined) quickEstimateCapacityMinutes.value = parsed.minutes;
}

async function requestCloseQuickEstimate(done: () => void) {
  if (!quickEstimateDirty.value || quickEstimateConfirmingClose.value) {
    done();
    return;
  }
  quickEstimateConfirmingClose.value = true;
  try {
    await ElMessageBox.confirm("分配今日预计还有未保存修改，确定放弃吗？", "放弃修改", {
      confirmButtonText: "放弃",
      cancelButtonText: "继续编辑",
      type: "warning",
    });
    done();
  } catch {} finally {
    quickEstimateConfirmingClose.value = false;
  }
}

async function saveQuickEstimate() {
  if (quickEstimateSaving.value) return;
  for (const draft of quickEstimateDrafts.value) {
    if (!updateQuickDraft(draft.taskId, draft.text)) {
      await focusQuickEstimate(quickAllocatableRows.value.findIndex((row) => row.task.id === draft.taskId));
      return;
    }
  }
  const changes = changedEstimateDrafts(quickEstimateDrafts.value);
  if (!changes.length) {
    quickEstimateOpen.value = false;
    return;
  }
  const recurringChanges = changes.filter((draft) => props.tasks.find((task) => task.id === draft.taskId)?.recurrence);
  if (recurringChanges.some((draft) => draft.minutes === undefined && props.tasks.find((task) => task.id === draft.taskId)?.estimate !== undefined)) {
    ElMessage.warning("有每次预计的重复事项不能在这里清空，请使用单项编辑");
    return;
  }
  if (recurringChanges.length) {
    try {
      await ElMessageBox.confirm("重复事项的修改会同时更新后续每次预计，是否继续？", "更新重复预计", {
        confirmButtonText: "继续保存",
        cancelButtonText: "取消",
        type: "info",
      });
    } catch { return; }
  }
  quickEstimateSaving.value = true;
  try {
    await props.saveEstimateBatch(changes.map((draft) => ({
      taskId: draft.taskId,
      expectedTaskVersion: draft.expectedTaskVersion,
      expectedDailyEstimateVersion: draft.expectedDailyEstimateVersion,
      estimateMinutes: draft.minutes,
    })));
    quickEstimateOpen.value = false;
    ElMessage.success(`已保存 ${changes.length} 项今日预计`);
  } catch (error) {
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "今日预计保存失败");
  } finally {
    quickEstimateSaving.value = false;
  }
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

function mountSortable() {
  if (!rowsRef.value) return;
  sortable?.destroy();
  sortable = new Sortable(rowsRef.value, {
    animation: 140,
    draggable: ".pending-reminder-row",
    handle: ".drag-handle",
    forceFallback: true,
    fallbackOnBody: true,
    multiDrag: true,
    selectedClass: "sortable-tree-selected",
    avoidImplicitDeselect: true,
    ghostClass: "sortable-ghost",
    chosenClass: "sortable-chosen",
    dragClass: "sortable-drag",
    onStart() {
      closeContextMenu();
      isSorting.value = true;
    },
    onMove(event, originalEvent) {
      const related = event.related as HTMLElement;
      const relatedDepth = Number(related?.dataset.depth ?? 0);
      const pointerX = originalEvent instanceof MouseEvent ? originalEvent.clientX : 0;
      const listLeft = rowsRef.value?.getBoundingClientRect().left ?? 0;
      dragTargetDepth.value = relatedDepth > 0 || pointerX > listLeft + 92 ? 1 : 0;
      const taskId = (event.dragged as HTMLElement).dataset.taskId ?? "";
      dragTargetInvalid.value = dragTargetDepth.value === 1 && getSubtreeIds(taskId).size > 1;
      rowsRef.value?.setAttribute("data-drop-depth", String(dragTargetDepth.value));
      rowsRef.value?.classList.toggle("drop-invalid", dragTargetInvalid.value);
      return !dragTargetInvalid.value;
    },
    async onEnd(event) {
      const taskId = (event.item as HTMLElement).dataset.taskId;
      const rows = Array.from(rowsRef.value?.querySelectorAll<HTMLElement>(".pending-reminder-row") ?? []);
      if (!taskId || event.oldIndex === undefined || event.newIndex === undefined) {
        clearSubtreeDrag(rows);
        await rebuildSortableList();
        return;
      }
      const movingIds = getSubtreeIds(taskId);
      const orderedRows = rows.map((row) => {
        const id = row.dataset.taskId ?? "";
        return { id, parentId: props.tasks.find((task) => task.id === id)?.parentId };
      });
      const intent = planTaskMoveIntent(orderedRows, taskId, movingIds, dragTargetDepth.value, movingIds.size > 1);
      if (intent) emit("move", taskId, intent.parentId, intent.previousSiblingId, intent.nextSiblingId);
      else ElMessage.warning("该落点会产生三级事项，请选择一级或同级位置");
      clearSubtreeDrag(rows);
      rowsRef.value?.removeAttribute("data-drop-depth");
      rowsRef.value?.classList.remove("drop-invalid");
      await rebuildSortableList();
    }
  });
}

async function rebuildSortableList() {
  sortable?.destroy();
  sortable = undefined;
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
});

onBeforeUnmount(() => {
  sortable?.destroy();
  sortable = undefined;
  document.removeEventListener("pointerdown", closeContextMenuForPointer);
  document.removeEventListener("keydown", closeContextMenuForKey);
  window.removeEventListener("scroll", closeContextMenu, true);
  if (levelChangedTimer) window.clearTimeout(levelChangedTimer);
});
</script>

<template>
  <div ref="listRef" class="plan-reminder-editor">
    <div class="plan-editor-head">
      <div>
        <h2>{{ subjectName }}</h2>
        <span>{{ pendingRows.length }} 项待办 · 点击文字直接编辑</span>
      </div>
      <div class="plan-head-actions">
        <button class="secondary-button quick-estimate-entry" type="button" title="连续分配今日预计" @click="openQuickEstimate"><Clock3 :size="14" />分配今日预计</button>
        <button class="secondary-button quick-estimate-entry" type="button" title="在一个编辑区调整整个计划" @click="openQuickPlan"><FilePenLine :size="14" />快速编辑计划</button>
        <button class="icon-button subtle" type="button" title="同步到 SeaTable" aria-label="同步到 SeaTable" @click="emit('syncSeaTable')"><DatabaseZap :size="15" /></button>
        <button class="icon-button subtle plan-import-button" type="button" title="从昨日 Obsidian 日报导入计划" aria-label="导入计划" @click="emit('importPlan')"><FileInput :size="15" /></button>
      </div>
    </div>

    <div :key="listRenderVersion" ref="rowsRef" class="reminder-list" :class="{ 'is-sorting': isSorting }">
      <template v-for="(row, rowIndex) in pendingRows" :key="row.task.id">
        <div
          class="reminder-row pending-reminder-row"
          :class="[
            {
              child: row.depth > 0,
              selected: row.task.id === selectedId,
              'level-changed': row.task.id === levelChangedId,
              'task-group-start': row.depth === 0,
              'task-group-child': row.depth > 0
            }
          ]"
          :data-task-id="row.task.id"
          :data-depth="row.depth"
          :style="{ '--child-indent': `${row.depth * 24}px` }"
          @click="selectTask(row.task)"
          @contextmenu="openContextMenu($event, row.task)"
        >
          <button class="drag-handle" :title="getSubtreeIds(row.task.id).size > 1 ? `拖动整组（共 ${getSubtreeIds(row.task.id).size} 项）` : '拖动排序'" @pointerdown="prepareSubtreeDrag(row.task.id)"><GripVertical :size="14" /></button>
          <button class="check-button" :class="{ completed: row.task.status === 'done' }" :title="row.task.recurrence ? (row.task.status === 'done' ? '恢复今天这一轮' : '完成今天这一轮') : '标记为完成'" @click.stop="emit('toggle', row.task, $event)">
            <CheckCircle2 v-if="row.task.status === 'done'" :size="18" />
            <Circle v-else :size="18" />
          </button>
          <div class="reminder-main">
            <input :data-task-input="row.task.id" v-model="row.task.title" :placeholder="row.depth ? '新子项' : '新待办'" @focus="selectTask(row.task)" @blur="finishTaskEdit(row.task)" @keydown.tab="handleTitleTab($event, row.task)" />
            <div class="reminder-meta">
              <span class="task-level-label"><CornerDownRight v-if="row.depth" :size="12" />{{ row.depth + 1 }}级</span>
              <span class="meta-picker" :class="{ filled: row.task.planDate }" title="设置预计开始日期" @click.stop="openNativePicker(row.task.id)">
                <input :data-date-input="row.task.id" v-model="row.task.planDate" aria-label="预计开始日期" tabindex="-1" type="date" @change="emit('save', row.task.id)" />
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
      <button class="reminder-placeholder-row plan-placeholder-row" type="button" aria-label="新建待办" title="新建待办" @click="addTask">
        <span class="drag-handle-spacer" aria-hidden="true"></span>
        <span class="placeholder-plus" aria-hidden="true"><Plus :size="18" /></span>
        <span class="placeholder-input" aria-hidden="true"></span>
      </button>
    </div>

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
      title="快速编辑计划"
      width="min(760px, 95vw)"
      append-to-body
      align-center
      :close-on-click-modal="false"
      :before-close="requestCloseQuickPlan"
    >
      <div v-if="quickPlanMode === 'edit'" class="quick-plan-workspace">
        <div class="quick-plan-help"><strong>{{ subjectName }}</strong><span>每项必须以“- ”开头；一级写作“- 事项”，二级写作“&nbsp;&nbsp;- 子事项”。Tab / Shift+Tab 调整层级。</span></div>
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
          <strong>将删除 {{ quickPlanSummary.deleted.length }} 项</strong>
          <span v-for="item in quickPlanSummary.deleted" :key="item.lineKey" :class="{ child: item.depth }">{{ formatQuickPlanItem(item.depth, item.title) }}</span>
          <small>父事项及其仍在删除范围内的子事项已全部列出，请确认后再保存。</small>
        </section>
      </div>
      <template #footer>
        <template v-if="quickPlanMode === 'edit'">
          <button class="secondary-button" type="button" @click="requestCloseQuickPlan(() => { quickPlanOpen = false; })">取消</button>
          <button class="primary-button" type="button" :disabled="Boolean(quickPlanSummary.errors.length)" @click="openQuickPlanSummary">预览变化</button>
        </template>
        <template v-else>
          <button class="secondary-button" type="button" :disabled="quickPlanSaving" @click="closeQuickPlanSummary">返回编辑</button>
          <button class="primary-button" :class="{ danger: quickPlanSummary.deleted.length }" type="button" :disabled="quickPlanSaving" @click="saveQuickPlan">{{ quickPlanSaving ? '保存中...' : quickPlanSummary.deleted.length ? '确认删除并保存' : '确认保存' }}</button>
        </template>
      </template>
    </el-dialog>

    <el-dialog
      v-model="quickEstimateOpen"
      class="recurrence-dialog quick-estimate-dialog"
      title="分配今日预计"
      width="min(820px, 95vw)"
      append-to-body
      align-center
      modal-class="quick-estimate-overlay"
      :close-on-click-modal="false"
      :before-close="requestCloseQuickEstimate"
    >
      <div class="quick-estimate-toolbar">
        <div>
          <strong>{{ todayDate }}</strong>
          <span>直接输入分钟数，或用左右按钮每次调整 15 分钟</span>
        </div>
        <label>当日容量<input v-model="quickEstimateCapacityText" @change="updateQuickCapacity" @blur="updateQuickCapacity" /></label>
      </div>
      <div class="quick-estimate-summary" :class="{ over: quickCapacitySummary.overMinutes > 0 }">
        <span>已分配 <strong>{{ formatMinutes(quickCapacitySummary.allocatedMinutes) }}</strong></span>
        <span>容量 {{ formatMinutes(quickCapacitySummary.capacityMinutes) }}</span>
        <span v-if="quickCapacitySummary.overMinutes">超出 <strong>{{ formatMinutes(quickCapacitySummary.overMinutes) }}</strong></span>
        <span v-else>剩余 <strong>{{ formatMinutes(quickCapacitySummary.remainingMinutes) }}</strong></span>
      </div>
      <div v-if="quickRows.length" class="quick-estimate-list">
        <div v-for="row in quickRows" :key="row.task.id" class="quick-estimate-row" :class="{ group: !row.allocatable, child: row.task.parentId }">
          <div class="quick-estimate-task">
            <CornerDownRight v-if="row.task.parentId" :size="14" />
            <strong>{{ row.task.title || '未命名事项' }}</strong>
            <small v-if="!row.allocatable">作为分组，不参与合计<span v-if="row.task.todayEstimate"> · 已有 {{ formatMinutes(row.task.todayEstimate) }} 未修改</span></small>
            <small v-else-if="row.task.recurrence">重复事项 · 保存后同步更新每次预计</small>
          </div>
          <template v-if="row.allocatable">
            <button v-if="row.suggestion" class="quick-estimate-suggestion" type="button" @click="applyQuickSuggestion(row.task)">采用 {{ formatMinutes(row.suggestion) }}</button>
            <div class="quick-estimate-input-wrap">
              <div class="quick-estimate-editor">
                <button type="button" title="减少 15 分钟" aria-label="减少 15 分钟" @click="adjustQuickMinutes(row.task.id, -15)"><Minus :size="14" /></button>
                <label>
                  <input
                    :data-quick-estimate-input="row.task.id"
                    :value="quickDraft(row.task.id)?.text"
                    inputmode="numeric"
                    placeholder="分钟"
                    aria-label="预计分钟数"
                    @input="inputQuickDraft(row.task.id, ($event.target as HTMLInputElement).value)"
                    @blur="updateQuickDraft(row.task.id, ($event.target as HTMLInputElement).value)"
                    @keydown="handleQuickKey($event, row.task.id)"
                  />
                  <span>分钟</span>
                </label>
                <button type="button" title="增加 15 分钟" aria-label="增加 15 分钟" @click="adjustQuickMinutes(row.task.id, 15)"><Plus :size="14" /></button>
                <span class="quick-estimate-hours">{{ quickDraftHours(row.task.id) }}</span>
              </div>
              <small v-if="quickEstimateErrors[row.task.id]" class="error">{{ quickEstimateErrors[row.task.id] }}</small>
            </div>
            <button class="quick-estimate-clear" type="button" @click="setQuickMinutes(row.task.id)">清空</button>
          </template>
        </div>
      </div>
      <div v-else class="quick-estimate-empty">当前主体没有可分配的进行中事项。</div>
      <template #footer>
        <button class="secondary-button" type="button" :disabled="quickEstimateSaving" @click="requestCloseQuickEstimate(() => { quickEstimateOpen = false; })">取消</button>
        <button data-quick-estimate-submit class="primary-button" :class="{ attention: quickEstimateDoneFocus }" type="button" :disabled="quickEstimateSaving || !quickAllocatableRows.length" @click="saveQuickEstimate">{{ quickEstimateSaving ? '保存中...' : '完成分配' }}</button>
      </template>
    </el-dialog>

    <Teleport to="body">
      <div v-if="contextMenuOpen" class="plan-task-context-menu" role="menu" :style="{ left: `${contextMenuLeft}px`, top: `${contextMenuTop}px` }" @pointerdown.stop>
        <button type="button" role="menuitem" @click="runContextCommand('before')">在上方新增</button>
        <button type="button" role="menuitem" @click="runContextCommand('after')">在下方新增</button>
        <button type="button" role="menuitem" :disabled="Boolean(contextTask()?.parentId || contextTask()?.recurrence || contextTask()?.directMinutes)" @click="runContextCommand('child')">新建子事项</button>
        <span class="plan-task-context-divider"></span>
        <button type="button" role="menuitem" @click="runContextCommand('note')">编辑备注</button>
        <button type="button" role="menuitem" @click="runContextCommand('duplicate')">复制</button>
        <button class="danger" type="button" role="menuitem" @click="runContextCommand('delete')">删除</button>
      </div>
    </Teleport>
  </div>
</template>
