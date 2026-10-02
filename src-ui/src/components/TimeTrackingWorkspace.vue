<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from "vue";
import { ElMessage } from "element-plus";
import { Check, ChevronDown, ChevronRight, CircleDot, Clock3, Coffee, Eraser, Pause, Play, Plus, RotateCcw, Split, Square, Trash2 } from "lucide-vue-next";
import { useWorkdayStore, type TimeEntry } from "../store";
import TaskSelectControl from "./TaskSelectControl.vue";
import { playCompletionFeedback } from "../services/completionFeedback";
import { clampPartialAllocationMinutes, partialAllocationMaximum } from "../services/allocationMath";

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

type TaskSelectControlInstance = { commit: () => string | undefined };

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

const orderedEntries = computed(() => [...store.entries].sort((a, b) => (b.endedAt ?? b.startedAt) - (a.endedAt ?? a.startedAt)));
const manualMinutesValid = computed(() => Number.isFinite(Number(manualMinutes.value)) && Number(manualMinutes.value) >= 1);

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

function allocationInputMaximum(entry: TimeEntry, index: number) {
  return partialAllocationMaximum(ensureDraft(entry).map((item) => item.minutes), index, entry.minutes);
}

function updateAllocationMinutes(entry: TimeEntry, index: number, event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  const drafts = ensureDraft(entry);
  const nextMinutes = clampPartialAllocationMinutes(
    drafts.map((item) => item.minutes),
    index,
    input.value,
    entry.minutes,
  );
  nextMinutes.forEach((minutes, currentIndex) => drafts[currentIndex].minutes = minutes);
  input.value = String(nextMinutes[index]);
}

async function saveAllocation(entry: TimeEntry) {
  for (const draft of ensureDraft(entry)) {
    const taskId = allocationTaskControls.get(draft.id)?.commit();
    if (taskId) draft.taskId = taskId;
  }
  const total = allocationTotal(entry);
  if (total > entry.minutes) {
    ElMessage.warning(`分配时间超出 ${total - entry.minutes} 分钟`);
    return;
  }
  allocationSavingId.value = entry.id;
  try {
    const result = await store.allocate(entry.id, ensureDraft(entry).map((draft) => ({
      taskId: draft.taskId,
      minutes: Math.max(0, Number(draft.minutes) || 0),
      completeTask: draft.completeTask,
    })));
    delete allocationDrafts[entry.id];
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
    expandedEntryId.value = "";
    ElMessage.success(disposition === "break" ? "已标记为休息时间" : "已标记为无效时间");
  } catch (error) {
    ElMessage.error(errorText(error, disposition === "break" ? "休息时间保存失败" : "无效时间保存失败"));
  } finally {
    allocationSavingId.value = "";
  }
}

async function startSelectedTimer() {
  const taskId = timerTaskControl.value?.commit() ?? store.selectedTaskId;
  timerBusy.value = true;
  try {
    await store.startTimer(taskId, timerNote.value);
    timerNote.value = "";
  } catch (error) {
    ElMessage.error(errorText(error, "开始计时失败"));
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
        <div v-if="!store.runningEntry" class="time-focus-selector">
          <span>计时事项</span>
          <TaskSelectControl
            ref="timerTaskControl"
            v-model="store.selectedTaskId"
            variant="focus"
            placeholder="选择或输入临时事项"
            aria-label="选择计时事项"
          />
          <input v-model="timerNote" class="time-focus-note" placeholder="备注（可选）" aria-label="计时备注" />
        </div>
        <div v-else class="time-focus-running-label">
          <span :class="store.runningEntry.state"></span>
          {{ store.runningEntry.state === 'paused' ? '已暂停' : '进行中' }}
        </div>
        <div class="time-focus-actions">
          <button v-if="!store.runningEntry" class="primary-button time-start-button" :disabled="timerBusy" @click="startSelectedTimer"><Play :size="16" fill="currentColor" />开始计时</button>
          <button v-else-if="store.runningEntry.state === 'running'" class="secondary-button" :disabled="timerBusy" @click="runTimerAction(store.pauseTimer, '暂停计时失败')"><Pause :size="15" fill="currentColor" />暂停</button>
          <button v-else class="primary-button" :disabled="timerBusy" @click="runTimerAction(store.resumeTimer, '继续计时失败')"><Play :size="15" fill="currentColor" />继续计时</button>
          <button v-if="store.runningEntry" class="danger-button" :disabled="timerBusy" @click="runTimerAction(store.stopTimer, '结束计时失败')"><Square :size="13" fill="currentColor" />结束计时</button>
        </div>
      </div>
    </div>

    <div class="time-history-head">
      <div>
        <h2>今日时间记录</h2>
        <span>{{ orderedEntries.length }} 段 · 待确认 {{ store.pendingMinutes }} 分钟</span>
      </div>
      <button class="secondary-button compact" type="button" @click="manualOpen = !manualOpen"><Plus :size="14" />手动补录</button>
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

    <div class="time-entry-list">
      <article
        v-for="entry in orderedEntries"
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
            <strong :class="{ invalid: allocationRemaining(entry) < 0 }">
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
            <label><input :value="allocation.minutes" min="0" :max="allocationInputMaximum(entry, allocationIndex)" type="number" @input="updateAllocationMinutes(entry, allocationIndex, $event)" /><span>分钟</span></label>
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
              <button class="primary-button compact" type="button" :disabled="allocationRemaining(entry) < 0 || allocationSavingId === entry.id" @click="saveAllocation(entry)"><Check :size="14" />确认归属</button>
            </div>
          </div>
        </div>
      </article>
    </div>
  </section>
</template>
