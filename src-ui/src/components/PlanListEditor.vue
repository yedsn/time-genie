<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { ElMessage, ElMessageBox } from "element-plus";
import Sortable, { MultiDrag } from "sortablejs";
import { CalendarDays, CheckCircle2, ChevronDown, ChevronRight, Circle, Copy, CornerDownRight, DatabaseZap, FileInput, GripVertical, IndentDecrease, IndentIncrease, Play, Plus, Radio, Repeat2, TimerReset, Trash2 } from "lucide-vue-next";
import type { Task } from "../store";

const props = defineProps<{
  tasks: Task[];
  subjectName: string;
  selectedId: string;
  recentCompletedId?: string;
  runningTaskId?: string;
  formatMinutes: (minutes: number) => string;
  storageMode: "local" | "cloud";
}>();

const emit = defineEmits<{
  select: [id: string];
  save: [id: string];
  importPlan: [];
  syncSeaTable: [];
  add: [];
  addChild: [id: string];
  duplicate: [id: string];
  delete: [id: string];
  toggle: [task: Task, event: MouseEvent];
  indent: [id: string];
  outdent: [id: string];
  startTimer: [id: string];
  reorder: [taskId: string, beforeId?: string, afterId?: string, parentId?: string];
  setRecurrence: [taskId: string, recurrence?: { frequency: "daily" | "weekdays" | "weekly"; weekdaysMask?: number; effectiveStart: string }];
}>();

const listRef = ref<HTMLElement | null>(null);
const rowsRef = ref<HTMLElement | null>(null);
const editingEstimateId = ref("");
const editingTodayEstimateId = ref("");
const levelChangedId = ref("");
const completedExpanded = ref(true);
const isSorting = ref(false);
const temporaryTaskId = ref("");
const listRenderVersion = ref(0);
const recurrenceDialogOpen = ref(false);
const recurrenceTaskId = ref("");
const recurrenceFrequency = ref<"none" | "daily" | "weekdays" | "weekly">("none");
const recurrenceStart = ref(formatLocalDate(new Date()));
const recurrenceWeekdays = ref<number[]>([]);
let sortable: Sortable | undefined;
let levelChangedTimer: number | undefined;

const sortableWithPlugins = Sortable as typeof Sortable & { treeMultiDragMounted?: boolean };
if (!sortableWithPlugins.treeMultiDragMounted) {
  Sortable.mount(new MultiDrag());
  sortableWithPlugins.treeMultiDragMounted = true;
}

const flatRows = computed(() => props.tasks.map((task) => ({ task, depth: getDepth(task) })));
const pendingRows = computed(() => flatRows.value.filter((row) => row.task.status !== "done"));
const completedRows = computed(() => flatRows.value.filter((row) => row.task.status === "done"));

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

function getAncestorPath(task: Task) {
  const ancestors: Task[] = [];
  const visited = new Set<string>();
  let parentId = task.parentId;
  while (parentId && !visited.has(parentId)) {
    visited.add(parentId);
    const parent = props.tasks.find((item) => item.id === parentId);
    if (!parent) break;
    ancestors.unshift(parent);
    parentId = parent.parentId;
  }
  return ancestors;
}

function completedPathLabel(task: Task) {
  const ancestors = getAncestorPath(task);
  return ancestors.length
    ? ancestors.map((item) => item.title.trim() || "未命名事项").join(" / ")
    : "顶层事项";
}

function childPlaceholderParent(rowIndex: number) {
  const row = pendingRows.value[rowIndex];
  if (!row) return;
  const root = getRootTask(row.task);
  if (root.status === "done") return;
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

async function openTodayEstimateEditor(taskId: string) {
  editingTodayEstimateId.value = taskId;
  await nextTick();
  const input = listRef.value?.querySelector<HTMLInputElement>(`[data-today-estimate-input="${taskId}"]`);
  input?.focus();
  input?.select();
}

function closeTodayEstimateEditor() {
  editingTodayEstimateId.value = "";
}

function finishTodayEstimateEditor(task: Task) {
  if (editingTodayEstimateId.value !== task.id) return;
  task.todayEstimate = normalizeEstimate(task.todayEstimate);
  closeTodayEstimateEditor();
  emit("save", task.id);
}

function normalizeEstimate(value: number | undefined) {
  const minutes = Number(value);
  return Number.isFinite(minutes) && minutes > 0 ? Math.round(minutes) : undefined;
}

function hasChildren(taskId: string) {
  return props.tasks.some((task) => task.parentId === taskId);
}

function openRecurrenceEditor(task: Task) {
  if (props.storageMode === "cloud") {
    ElMessage.warning("重复事项暂仅支持本地模式");
    return;
  }
  if (hasChildren(task.id)) return;
  recurrenceTaskId.value = task.id;
  recurrenceFrequency.value = task.recurrence?.frequency ?? "none";
  recurrenceStart.value = task.recurrence?.effectiveStart ?? formatLocalDate(new Date());
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

function formatLocalDate(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
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
      isSorting.value = true;
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
      const movingIndexes = rows
        .map((row, index) => movingIds.has(row.dataset.taskId ?? "") ? index : -1)
        .filter((index) => index >= 0);
      const blockStart = Math.min(...movingIndexes);
      const blockEnd = Math.max(...movingIndexes);
      const beforeRow = rows.slice(0, blockStart).reverse().find((row) => !movingIds.has(row.dataset.taskId ?? ""));
      const afterRow = rows.slice(blockEnd + 1).find((row) => !movingIds.has(row.dataset.taskId ?? ""));
      const currentDepth = Number((event.item as HTMLElement).dataset.depth ?? 0);
      const parentRow = currentDepth > 0
        ? rows.slice(0, blockStart).reverse().find((row) => !movingIds.has(row.dataset.taskId ?? "") && Number(row.dataset.depth ?? 0) === currentDepth - 1)
        : undefined;
      const currentTask = props.tasks.find((task) => task.id === taskId);
      const parentId = currentDepth > 0 ? parentRow?.dataset.taskId ?? currentTask?.parentId : undefined;
      emit("reorder", taskId, beforeRow?.dataset.taskId, afterRow?.dataset.taskId, parentId);
      clearSubtreeDrag(rows);
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
});

onBeforeUnmount(() => {
  sortable?.destroy();
  sortable = undefined;
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
        >
          <button class="drag-handle" :title="getSubtreeIds(row.task.id).size > 1 ? `拖动整组（共 ${getSubtreeIds(row.task.id).size} 项）` : '拖动排序'" @pointerdown="prepareSubtreeDrag(row.task.id)"><GripVertical :size="14" /></button>
          <button class="check-button" :title="row.task.recurrence ? '完成今天这一轮' : '标记为完成'" @click.stop="emit('toggle', row.task, $event)">
            <Circle :size="18" />
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
              <span class="meta-picker estimate-picker today-estimate-picker" :class="{ filled: row.task.todayEstimate }" title="设置今日预计用时" @click.stop="openTodayEstimateEditor(row.task.id)">
                <TimerReset :size="12" />{{ row.task.todayEstimate ? '今日预计 ' + formatMinutes(row.task.todayEstimate) : '未填今日预计' }}
                <input
                  v-if="editingTodayEstimateId === row.task.id"
                  :data-today-estimate-input="row.task.id"
                  v-model.number="row.task.todayEstimate"
                  min="0"
                  placeholder="分钟"
                  type="number"
                  @blur="finishTodayEstimateEditor(row.task)"
                  @click.stop
                  @keydown.enter.prevent="finishTodayEstimateEditor(row.task)"
                  @keydown.esc.prevent="closeTodayEstimateEditor"
                />
              </span>
              <button
                v-if="!hasChildren(row.task.id)"
                class="meta-picker recurrence-picker"
                :class="{ filled: row.task.recurrence, disabled: storageMode === 'cloud' }"
                type="button"
                :title="storageMode === 'cloud' ? '重复事项暂仅支持本地模式' : '设置重复规则'"
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

    <section class="completed-reminders" :class="{ expanded: completedExpanded }">
      <button class="completed-reminders-toggle" type="button" @click="completedExpanded = !completedExpanded">
        <ChevronDown v-if="completedExpanded" :size="15" />
        <ChevronRight v-else :size="15" />
        <span>已完成</span>
        <strong data-completed-count>{{ completedRows.length }}</strong>
      </button>

      <div v-if="completedExpanded" class="completed-reminder-list">
        <div
          v-for="row in completedRows"
          :key="row.task.id"
          class="reminder-row completed"
          :class="{ selected: row.task.id === selectedId, 'completion-new': row.task.id === recentCompletedId }"
          :data-task-id="row.task.id"
          @click="selectTask(row.task)"
        >
          <span class="drag-handle-spacer" aria-hidden="true"></span>
          <button class="check-button" title="恢复为未完成" @click.stop="emit('toggle', row.task, $event)">
            <CheckCircle2 :size="18" />
          </button>
          <div class="reminder-main">
            <input :data-task-input="row.task.id" v-model="row.task.title" placeholder="已完成事项" @focus="selectTask(row.task)" @blur="finishTaskEdit(row.task)" />
            <div class="reminder-meta">
              <span class="completed-path" :title="completedPathLabel(row.task)">
                <CornerDownRight :size="12" />{{ completedPathLabel(row.task) }}
              </span>
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
              <span class="meta-picker estimate-picker today-estimate-picker" :class="{ filled: row.task.todayEstimate }" title="设置今日预计用时" @click.stop="openTodayEstimateEditor(row.task.id)">
                <TimerReset :size="12" />{{ row.task.todayEstimate ? '今日预计 ' + formatMinutes(row.task.todayEstimate) : '未填今日预计' }}
                <input
                  v-if="editingTodayEstimateId === row.task.id"
                  :data-today-estimate-input="row.task.id"
                  v-model.number="row.task.todayEstimate"
                  min="0"
                  placeholder="分钟"
                  type="number"
                  @blur="finishTodayEstimateEditor(row.task)"
                  @click.stop
                  @keydown.enter.prevent="finishTodayEstimateEditor(row.task)"
                  @keydown.esc.prevent="closeTodayEstimateEditor"
                />
              </span>
            </div>
          </div>
          <div class="reminder-inline-controls">
            <button
              class="icon-button subtle hover-row-action task-timer-action"
              :class="{ active: runningTaskId === row.task.id }"
              :title="runningTaskId === row.task.id ? '当前事项正在计时' : runningTaskId ? '结束当前计时并切换到此事项' : '继续执行并开始计时'"
              @click.stop="emit('startTimer', row.task.id)"
            >
              <Radio v-if="runningTaskId === row.task.id" :size="14" />
              <Play v-else :size="14" fill="currentColor" />
            </button>
            <button class="icon-button subtle hover-row-action" title="复制事项" @click.stop="duplicateTask(row.task)"><Copy :size="14" /></button>
            <button class="icon-button subtle hover-row-action" title="删除" @click.stop="confirmDelete(row.task)"><Trash2 :size="14" /></button>
          </div>
        </div>
        <p v-if="!completedRows.length" class="completed-reminders-empty">还没有已完成事项</p>
      </div>
    </section>

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
  </div>
</template>
