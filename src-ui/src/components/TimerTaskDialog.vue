<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from "vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { AlertCircle, Check, CheckCircle2, CornerDownRight, ListTodo, LoaderCircle, MessageSquareText, MoreHorizontal, Play, Plus, Search, Trash2 } from "lucide-vue-next";
import { useWorkdayStore, type Task } from "../store";
import {
  beginTimerDialogSave,
  createTimerDialogDraft,
  resolveTimerDialogSubject,
  settleTimerDialogSave,
  timerDialogAction,
  timerDialogDraftDirty,
  timerDialogRows,
  updateTimerDialogDraft,
  type TimerDialogDraft,
  type TimerDialogDraftField,
} from "../services/timerTaskDialogCore";

const props = defineProps<{ modelValue: boolean; contextSubjectId?: string; timerNote?: string }>();
const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  goPlan: [subjectId: string];
  timerStarted: [];
}>();

const store = useWorkdayStore();
const subjectId = ref("");
const query = ref("");
const searchRef = ref<HTMLInputElement>();
const returnFocusElement = ref<HTMLElement>();
const drafts = reactive<Record<string, TimerDialogDraft>>({});
const saveChains = new Map<string, Promise<boolean>>();
const timerBusyTaskId = ref("");
const recentTaskId = ref("");
const noteTaskId = ref("");
const moreTaskId = ref("");
const closing = ref(false);
const newTask = reactive({ open: false, parentId: "", title: "", todayEstimate: "", saving: false, error: "" });
const moreDrafts = reactive<Record<string, { estimate: string; planDate: string; recurrence: "none" | "daily" | "weekdays" | "weekly"; recurrenceStart: string; recurrenceWeekdays: number[] }>>({});
const weekdayLabels = ["一", "二", "三", "四", "五", "六", "日"];

const activeTask = computed(() => store.tasks.find((task) => task.id === store.runningEntry?.defaultTask));
const rows = computed(() => timerDialogRows(store.tasks.map((task) => ({
  ...task,
  recurring: Boolean(task.recurrence),
  totalMinutes: task.actual,
})), subjectId.value, query.value));
const currentSubject = computed(() => store.subjects.find((subject) => subject.id === subjectId.value));

watch(() => props.modelValue, async (open) => {
  if (!open) return;
  returnFocusElement.value = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  subjectId.value = resolveTimerDialogSubject({
    subjects: store.subjects,
    contextSubjectId: props.contextSubjectId,
    selectedSubjectId: store.selectedSubjectId,
    activeTaskSubjectId: activeTask.value?.subjectId,
  });
  query.value = "";
  for (const taskId of Object.keys(drafts)) delete drafts[taskId];
  for (const taskId of Object.keys(moreDrafts)) delete moreDrafts[taskId];
  noteTaskId.value = "";
  moreTaskId.value = "";
  resetNewTask();
});

watch(() => store.subjects.map((subject) => subject.id).join("|"), () => {
  if (!store.subjects.some((subject) => subject.id === subjectId.value)) {
    subjectId.value = resolveTimerDialogSubject({ subjects: store.subjects, selectedSubjectId: store.selectedSubjectId });
  }
});

function draftFor(task: Task) {
  const existing = drafts[task.id];
  if (existing) return existing;
  const draft = createTimerDialogDraft(task);
  drafts[task.id] = draft;
  return draft;
}

function actionFor(task: Task, hasChildren: boolean) {
  return timerDialogAction({
    taskId: task.id,
    taskTitle: draftFor(task).title || task.title || "未命名事项",
    hasChildren,
    activeTaskId: store.runningEntry?.defaultTask,
    activeState: store.runningEntry?.state,
  });
}

function updateDraft(task: Task, field: TimerDialogDraftField, value: string) {
  updateTimerDialogDraft(draftFor(task), field, value);
}

function parseTodayEstimate(text: string) {
  if (!text.trim()) return undefined;
  const value = Number(text);
  if (!Number.isFinite(value) || value < 0 || value > 100_000) throw new Error("今日预计请输入 0 到 100000 分钟");
  return Math.round(value);
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

async function saveField(task: Task, field: TimerDialogDraftField) {
  const draft = draftFor(task);
  if (!timerDialogDraftDirty(draft, field)) return !draft.errors[field];
  const token = beginTimerDialogSave(draft, field);
  const value = draft[field];
  return enqueueSave(task.id, async () => {
    let mutatedStoreTitle = false;
    try {
      if (field === "title") {
        const title = value.trim();
        if (!title) throw new Error("事项名称不能为空");
        const target = store.tasks.find((item) => item.id === task.id);
        if (!target) throw new Error("事项已不存在");
        target.title = title;
        mutatedStoreTitle = true;
        const savedId = await store.saveTask(target.id);
        if (!savedId) throw new Error("事项名称保存失败");
        settleTimerDialogSave(draft, token, { ok: true, savedValue: title });
      } else if (field === "todayEstimate") {
        const minutes = parseTodayEstimate(value);
        await store.saveTaskTodayEstimate(task.id, minutes);
        settleTimerDialogSave(draft, token, { ok: true, savedValue: minutes === undefined ? "" : String(minutes) });
      } else {
        await store.saveTaskNote(task.id, value);
        settleTimerDialogSave(draft, token, { ok: true, savedValue: value });
      }
      return true;
    } catch (error) {
      settleTimerDialogSave(draft, token, { ok: false, message: errorText(error, "保存失败") });
      if (field === "title" && mutatedStoreTitle) await store.loadWorkspaceData();
      if (timerBusyTaskId.value === task.id) await focusTaskControl(task.id, field);
      return false;
    }
  });
}

async function flushTask(task: Task) {
  for (const field of ["title", "todayEstimate", "note"] as const) {
    while (timerDialogDraftDirty(draftFor(task), field)) {
      if (!await saveField(task, field)) return false;
    }
  }
  const pending = saveChains.get(task.id);
  return pending ? await pending : true;
}

async function startOrSwitch(task: Task, hasChildren: boolean) {
  const action = actionFor(task, hasChildren);
  if (!action.actionable || timerBusyTaskId.value) return;
  timerBusyTaskId.value = task.id;
  let focusActionAfterFailure = false;
  try {
    if (!await flushTask(task)) return;
    const result = await store.transitionTimer({ taskId: task.id }, props.timerNote ?? "");
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
  const dirtyTasks = store.tasks.filter((task) => task.subjectId === subjectId.value).filter((task) => {
    const draft = drafts[task.id];
    return draft && (["title", "todayEstimate", "note"] as const).some((field) => timerDialogDraftDirty(draft, field));
  });
  for (const task of dirtyTasks) if (!await flushTask(task)) return;
  subjectId.value = nextSubjectId;
  query.value = "";
  resetNewTask();
}

function openNewTask(parentId = "") {
  newTask.open = true;
  newTask.parentId = parentId;
  newTask.title = "";
  newTask.todayEstimate = "";
  newTask.error = "";
}

function resetNewTask() {
  newTask.open = false;
  newTask.parentId = "";
  newTask.title = "";
  newTask.todayEstimate = "";
  newTask.saving = false;
  newTask.error = "";
}

async function createNewTask(startAfterCreate = false) {
  const title = newTask.title.trim();
  if (!title) { newTask.error = "事项名称不能为空"; return; }
  newTask.saving = true;
  newTask.error = "";
  try {
    const tempId = store.addTaskSelectionDraft(title, subjectId.value, newTask.parentId || undefined);
    if (!tempId) throw new Error("新事项创建失败");
    const tempTask = store.tasks.find((task) => task.id === tempId);
    if (!tempTask) throw new Error("新事项创建失败");
    tempTask.todayEstimate = parseTodayEstimate(newTask.todayEstimate);
    const taskId = await store.saveTask(tempId);
    if (!taskId) throw new Error("新事项保存失败");
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

async function saveMore(task: Task) {
  const draft = moreDraftFor(task);
  try {
    const estimate = draft.estimate.trim() ? Number(draft.estimate) : undefined;
    if (estimate !== undefined && (!Number.isFinite(estimate) || estimate < 0)) throw new Error("整体预计需为非负分钟");
    const target = store.tasks.find((item) => item.id === task.id);
    if (!target) throw new Error("事项已不存在");
    target.estimate = estimate === undefined ? undefined : Math.round(estimate);
    target.planDate = draft.planDate || undefined;
    await store.saveTask(target.id);
    if (draft.recurrence === "none") {
      if (target.recurrence) await store.setTaskRecurrence(target.id, undefined);
    } else {
      const weekdaysMask = draft.recurrence === "weekly"
        ? draft.recurrenceWeekdays.reduce((mask, weekday) => mask | (1 << weekday), 0)
        : undefined;
      if (draft.recurrence === "weekly" && !weekdaysMask) throw new Error("每周重复至少选择一天");
      await store.setTaskRecurrence(target.id, { frequency: draft.recurrence, weekdaysMask, effectiveStart: draft.recurrenceStart || store.workspaceToday });
    }
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
  let current = store.tasks.find((item) => item.id === activeId);
  while (current?.parentId) {
    if (current.parentId === task.id) return true;
    current = store.tasks.find((item) => item.id === current?.parentId);
  }
  return false;
}

async function completeTask(task: Task) {
  if (containsActiveTask(task)) { ElMessage.warning("请先结束或切换当前计时"); return; }
  try {
    await store.commitTaskToggle(task);
    await Promise.all([store.loadWorkspaceData(), store.loadTodayOverview()]);
    moreTaskId.value = "";
  } catch (error) { ElMessage.error(errorText(error, "完成事项失败")); }
}

async function deleteTask(task: Task) {
  if (containsActiveTask(task)) { ElMessage.warning("请先结束或切换当前计时"); return; }
  try {
    await ElMessageBox.confirm(`确定删除“${task.title || "未命名事项"}”及其子项吗？`, "删除事项", { confirmButtonText: "删除", cancelButtonText: "取消", type: "warning", customClass: "work-confirm-dialog", confirmButtonClass: "work-confirm-danger" });
    await store.deleteTask(task.id);
    moreTaskId.value = "";
  } catch {}
}

async function flushAllDrafts() {
  const dirtyTasks = store.tasks.filter((task) => {
    const draft = drafts[task.id];
    return draft && (["title", "todayEstimate", "note"] as const).some((field) => timerDialogDraftDirty(draft, field));
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
    if (newTask.open && (newTask.title.trim() || newTask.todayEstimate.trim())) {
      try {
        await ElMessageBox.confirm("新增事项还没有保存，确定放弃吗？", "放弃新增事项", { confirmButtonText: "放弃", cancelButtonText: "继续编辑", type: "warning" });
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
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    class="timer-task-dialog"
    modal-class="timer-task-dialog-overlay"
    title="选择计时事项"
    width="min(960px, 96vw)"
    append-to-body
    destroy-on-close
    :before-close="beforeClose"
    @opened="focusInitialControl"
    @closed="restoreTriggerFocus"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <div class="timer-task-dialog-toolbar">
      <div class="timer-task-subject-tabs" role="tablist" aria-label="计时事项主体">
        <button v-for="subject in store.subjects" :key="subject.id" type="button" role="tab" :aria-selected="subjectId === subject.id" :class="{ active: subjectId === subject.id }" @click="selectSubject(subject.id)">{{ subject.name }}</button>
      </div>
      <label class="timer-task-search"><Search :size="15" /><input ref="searchRef" v-model="query" placeholder="搜索当前主体事项" aria-label="搜索计时事项" /></label>
      <button class="secondary-button compact" type="button" @click="openNewTask()"><Plus :size="14" />新增事项</button>
    </div>

    <div class="timer-task-dialog-summary">
      <span><strong>{{ currentSubject?.name ?? '当前主体' }}</strong></span>
      <span>{{ rows.filter((row) => !row.hasChildren).length }} 个可计时事项</span>
      <span>名称和今日预计会自动保存</span>
    </div>

    <div class="timer-task-list" aria-live="polite" :aria-busy="store.workspaceLoading">
      <div class="timer-task-list-head" aria-hidden="true"><span>事项</span><span>今日预计</span><span>已确认实际</span><span>操作</span></div>
      <div v-if="store.workspaceLoading" class="timer-task-empty timer-task-loading" role="status"><LoaderCircle :size="22" class="spin" /><strong>正在加载事项…</strong><span>列表准备好后即可编辑或开始计时。</span></div>

      <div v-else-if="newTask.open" class="timer-task-row new-task" :class="{ child: newTask.parentId }">
        <div class="timer-task-name-cell"><CornerDownRight v-if="newTask.parentId" :size="14" /><input v-model="newTask.title" placeholder="新事项名称" aria-label="新事项名称" @keydown.enter="createNewTask(false)" /></div>
        <label class="timer-task-estimate-cell"><input v-model="newTask.todayEstimate" inputmode="numeric" aria-label="新事项今日预计分钟" /><span>分钟</span></label>
        <span class="timer-task-actual-cell">新事项</span>
        <div class="timer-task-actions"><button class="secondary-button compact" type="button" :disabled="newTask.saving" @click="resetNewTask">取消</button><button class="secondary-button compact" type="button" :disabled="newTask.saving" @click="createNewTask(false)">保存</button><button class="primary-button compact" type="button" :disabled="newTask.saving" @click="createNewTask(true)"><LoaderCircle v-if="newTask.saving" :size="13" class="spin" /><Play v-else :size="13" fill="currentColor" />开始</button></div>
        <small v-if="newTask.error" class="timer-task-row-error">{{ newTask.error }}</small>
      </div>

      <template v-if="!store.workspaceLoading && rows.length">
        <div v-for="row in rows" :key="row.task.id" class="timer-task-row" :data-timer-task-id="row.task.id" :class="[{ parent: row.hasChildren, child: row.depth > 0, recent: recentTaskId === row.task.id, active: store.runningEntry?.defaultTask === row.task.id }, `state-${actionFor(row.task, row.hasChildren).mode}`]">
          <div class="timer-task-name-cell" :style="{ '--task-depth': String(row.depth) }">
            <CornerDownRight v-if="row.depth" :size="14" />
            <input data-timer-task-field="title" :value="draftFor(row.task).title" :aria-label="`事项名称：${row.task.title}`" @input="updateDraft(row.task, 'title', ($event.target as HTMLInputElement).value)" @blur="saveField(row.task, 'title')" @keydown.enter.prevent="($event.target as HTMLInputElement).blur()" />
            <small v-if="row.hasChildren">{{ row.task.parentId ? '子组' : '父事项' }} · {{ formatMinutes(row.totalTodayEstimate) }} 预计</small>
            <small v-else-if="draftFor(row.task).saving.title">保存中...</small>
            <small v-else-if="draftFor(row.task).errors.title" class="error">{{ draftFor(row.task).errors.title }}</small>
            <small v-else-if="draftFor(row.task).saved.title">已保存</small>
          </div>

          <label class="timer-task-estimate-cell" :class="{ summary: row.hasChildren }">
            <template v-if="row.hasChildren"><strong>{{ formatMinutes(row.totalTodayEstimate) }}</strong><span>汇总</span></template>
            <template v-else><input data-timer-task-field="todayEstimate" :value="draftFor(row.task).todayEstimate" inputmode="numeric" :aria-label="`${row.task.title}今日预计分钟`" @input="updateDraft(row.task, 'todayEstimate', ($event.target as HTMLInputElement).value)" @blur="saveField(row.task, 'todayEstimate')" @keydown.enter.prevent="($event.target as HTMLInputElement).blur()" /><span>分钟</span></template>
            <small v-if="!row.hasChildren && draftFor(row.task).saving.todayEstimate">保存中</small>
            <small v-else-if="!row.hasChildren && draftFor(row.task).errors.todayEstimate" class="error">{{ draftFor(row.task).errors.todayEstimate }}</small>
            <small v-else-if="!row.hasChildren && draftFor(row.task).saved.todayEstimate">已保存</small>
          </label>

          <span class="timer-task-actual-cell">{{ row.hasChildren ? '汇总 ' : '' }}{{ formatMinutes(row.confirmedMinutes) }}</span>

          <div class="timer-task-actions">
            <el-popover :visible="noteTaskId === row.task.id" placement="bottom" :width="300" trigger="click" @update:visible="noteTaskId = $event ? row.task.id : ''">
              <template #reference><button class="icon-button subtle" type="button" :class="{ filled: Boolean(draftFor(row.task).note.trim()) }" :title="draftFor(row.task).note.trim() ? '编辑备注' : '添加备注'" :aria-label="`${row.task.title}备注`"><MessageSquareText :size="14" /></button></template>
              <div class="timer-task-note-popover"><strong>{{ row.task.title }} · 备注</strong><textarea data-timer-task-field="note" :value="draftFor(row.task).note" rows="4" @input="updateDraft(row.task, 'note', ($event.target as HTMLTextAreaElement).value)"></textarea><small v-if="draftFor(row.task).errors.note" class="error">{{ draftFor(row.task).errors.note }}</small><div><button class="secondary-button compact" type="button" @click="noteTaskId = ''">关闭</button><button class="primary-button compact" type="button" :disabled="Boolean(draftFor(row.task).saving.note)" @click="saveField(row.task, 'note').then((ok) => { if (ok) noteTaskId = '' })"><Check :size="13" />保存</button></div></div>
            </el-popover>

            <button v-if="row.hasChildren" class="secondary-button compact timer-task-add-child" type="button" @click="openNewTask(row.task.id)"><Plus :size="13" />子项</button>
            <button v-else class="timer-task-primary-action" :class="actionFor(row.task, false).mode" type="button" :disabled="!actionFor(row.task, false).actionable || Boolean(timerBusyTaskId)" :aria-label="actionFor(row.task, false).ariaLabel" @click="startOrSwitch(row.task, false)"><LoaderCircle v-if="timerBusyTaskId === row.task.id" :size="13" class="spin" /><Play v-else-if="actionFor(row.task, false).actionable" :size="13" fill="currentColor" />{{ actionFor(row.task, false).label }}</button>

            <el-popover :visible="moreTaskId === row.task.id" placement="bottom-end" :width="330" trigger="click" @update:visible="moreTaskId = $event ? row.task.id : ''">
              <template #reference><button class="icon-button subtle" type="button" :aria-label="`${row.task.title}更多操作`" title="更多操作"><MoreHorizontal :size="15" /></button></template>
              <div class="timer-task-more-popover"><strong>{{ row.task.title }}</strong><label><span>整体预计</span><div><input v-model="moreDraftFor(row.task).estimate" inputmode="numeric" /><small>分钟</small></div></label><label><span>预计开始</span><input v-model="moreDraftFor(row.task).planDate" type="date" /></label><label v-if="!row.hasChildren"><span>重复设置</span><select v-model="moreDraftFor(row.task).recurrence"><option value="none">不重复</option><option value="daily">每天</option><option value="weekdays">工作日</option><option value="weekly">每周</option></select></label><label v-if="!row.hasChildren && moreDraftFor(row.task).recurrence !== 'none'"><span>生效日期</span><input v-model="moreDraftFor(row.task).recurrenceStart" type="date" /></label><div v-if="!row.hasChildren && moreDraftFor(row.task).recurrence === 'weekly'" class="timer-task-weekdays"><span>重复星期</span><div><label v-for="(label, index) in weekdayLabels" :key="label"><input v-model="moreDraftFor(row.task).recurrenceWeekdays" type="checkbox" :value="index" />{{ label }}</label></div></div><div class="timer-task-more-actions"><button class="secondary-button compact" type="button" @click="completeTask(row.task)"><CheckCircle2 :size="13" />完成</button><button class="danger-button compact" type="button" @click="deleteTask(row.task)"><Trash2 :size="13" />删除</button><span></span><button class="primary-button compact" type="button" @click="saveMore(row.task)">保存设置</button></div></div>
            </el-popover>
          </div>
        </div>
      </template>

      <div v-else-if="!store.workspaceLoading && !newTask.open" class="timer-task-empty"><AlertCircle :size="22" /><strong>{{ query ? '没有匹配事项' : '当前主体暂无进行中事项' }}</strong><span>{{ query ? '换个关键词，或清空搜索查看全部事项。' : '可以直接在这里新建事项并开始计时。' }}</span><button v-if="!query" class="primary-button compact" type="button" @click="openNewTask()"><Plus :size="14" />新增事项</button></div>
    </div>

    <template #footer><div class="timer-task-dialog-footer"><button class="secondary-button compact" type="button" @click="goPlan"><ListTodo :size="14" />前往待办页排序或调整层级</button><span></span><button class="secondary-button" type="button" :disabled="closing" @click="closeDialog">关闭</button></div></template>
  </el-dialog>
</template>
