<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Check, ClockAlert, Coffee, Eraser, Minus, Plus, Trash2 } from "lucide-vue-next";
import { ElMessage } from "element-plus";
import { useWorkdayStore } from "../store";
import TaskSelectControl from "./TaskSelectControl.vue";
import { playCompletionFeedback } from "../services/completionFeedback";
import { remainingOpenTaskCount } from "../services/completionFeedbackCore";
import { clampPartialAllocationMinutes, partialAllocationMaximum } from "../services/allocationMath";

type TaskSelectControlInstance = { commit: () => string | undefined };
type AllocationDraft = { id: string; taskId: string; minutes: number; completeTask: boolean };

const store = useWorkdayStore();
const allocations = reactive<AllocationDraft[]>([]);
const taskControls = new Map<string, TaskSelectControlInstance>();
const saving = ref(false);

const liveSeconds = computed(() => store.unassignedSeconds);
const requiredMinutes = computed(() => Math.max(1, Math.ceil(liveSeconds.value / 60)));
const allocatedMinutes = computed(() => allocations.reduce((sum, allocation) => sum + Math.max(0, Number(allocation.minutes) || 0), 0));
const remainingMinutes = computed(() => requiredMinutes.value - allocatedMinutes.value);
const clockText = computed(() => {
  const seconds = liveSeconds.value;
  const hours = Math.floor(seconds / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  const rest = seconds % 60;
  return [hours, minutes, rest].map((value) => String(value).padStart(2, "0")).join(":");
});
const periodLabel = computed(() => {
  const startedAt = store.unassignedFirstStartedAt ?? store.unassignedStartedAt;
  if (!startedAt) return "刚刚开始";
  const endLabel = store.unassignedStartedAt ? "现在" : (store.unassignedLastEndedAt ? formatTime(store.unassignedLastEndedAt) : "现在");
  return `${formatTime(startedAt)} - ${endLabel}`;
});

function currentOpenTaskCount() {
  if (store.activePage === "plan") return store.selectedSubjectTasks.filter((task) => task.status !== "done").length;
  if (store.activePage === "today") return store.todayTasks.filter((task) => task.status !== "done").length;
  return undefined;
}

watch(() => store.unassignedDialogOpen, (open) => {
  if (!open) return;
  allocations.splice(0, allocations.length, {
    id: createAllocationId(),
    taskId: defaultTaskId(),
    minutes: requiredMinutes.value,
    completeTask: false,
  });
}, { immediate: true });

watch(requiredMinutes, (current, previous) => {
  if (!store.unassignedDialogOpen || !allocations.length || current <= previous) return;
  if (allocatedMinutes.value < previous) return;
  allocations[allocations.length - 1].minutes += current - previous;
});

function formatTime(timestamp: number) {
  return new Intl.DateTimeFormat("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false, timeZone: store.workspaceTimezone }).format(timestamp);
}

function dateLabel(date: string) {
  if (date === store.workspaceToday) return "今天";
  const today = new Date(`${store.workspaceToday}T00:00:00Z`);
  const target = new Date(`${date}T00:00:00Z`);
  const days = Math.round((today.getTime() - target.getTime()) / 86_400_000);
  return days === 1 ? "昨天" : date;
}

function createAllocationId() {
  return `unassigned-allocation-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function defaultTaskId() {
  const selected = store.tasks.find((task) => task.id === store.selectedTaskId);
  if (selected && !store.tasks.some((task) => task.parentId === selected.id)) return selected.id;
  return store.visibleTasks.find((task) => !store.tasks.some((child) => child.parentId === task.id))?.id ?? "";
}

function setTaskControl(id: string, component: unknown) {
  if (component && typeof component === "object" && "commit" in component) {
    taskControls.set(id, component as TaskSelectControlInstance);
  } else {
    taskControls.delete(id);
  }
}

function addAllocation() {
  allocations.push({ id: createAllocationId(), taskId: defaultTaskId(), minutes: 0, completeTask: false });
}

function removeAllocation(id: string) {
  if (allocations.length === 1) return;
  const index = allocations.findIndex((allocation) => allocation.id === id);
  if (index < 0) return;
  allocations.splice(index, 1);
}

function allocationInputMaximum(index: number) {
  return partialAllocationMaximum(allocations.map((item) => item.minutes), index, requiredMinutes.value);
}

function updateAllocationMinutes(index: number, event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  const nextMinutes = clampPartialAllocationMinutes(
    allocations.map((item) => item.minutes),
    index,
    input.value,
    requiredMinutes.value,
  );
  nextMinutes.forEach((minutes, currentIndex) => allocations[currentIndex].minutes = minutes);
  input.value = String(nextMinutes[index]);
}

function clearAllocations() {
  allocations.forEach((allocation) => allocation.minutes = 0);
  ElMessage.info("已清空草稿用时，点击分配给事项后保存");
}

async function assignTime(event?: MouseEvent) {
  for (const allocation of allocations) {
    const committedTaskId = taskControls.get(allocation.id)?.commit();
    if (committedTaskId) allocation.taskId = committedTaskId;
  }
  const positiveAllocations = allocations
    .map((allocation) => ({
      taskId: allocation.taskId,
      minutes: Math.max(0, Number(allocation.minutes) || 0),
      completeTask: allocation.completeTask,
    }))
    .filter((allocation) => allocation.minutes > 0);
  if (!positiveAllocations.length) {
    ElMessage.warning("请至少分配 1 分钟到事项");
    return;
  }
  if (positiveAllocations.some((allocation) => !allocation.taskId || !store.tasks.some((task) => task.id === allocation.taskId))) {
    ElMessage.warning("请为每一行选择一个具体事项");
    return;
  }
  if (remainingMinutes.value < 0) {
    ElMessage.warning(`分配时间超出 ${Math.abs(remainingMinutes.value)} 分钟`);
    return;
  }
  saving.value = true;
  const openBefore = currentOpenTaskCount();
  try {
    const saved = await store.resolveUnassignedTime("work", positiveAllocations);
    if (!saved) {
      ElMessage.warning("未归属时间已变化，请刷新后重试");
      return;
    }
    ElMessage.success("未归属时间已分配到事项");
    if (saved.completedCount) {
      const openAfter = remainingOpenTaskCount(openBefore, saved.completedCount) ?? currentOpenTaskCount();
      playCompletionFeedback({ completedCount: saved.completedCount, openBefore, openAfter });
    }
  } catch (error) {
    await store.loadUnassignedState();
    ElMessage.error(errorText(error, "未归属时间分配失败"));
  } finally {
    saving.value = false;
  }
}

async function markAsBreak() {
  saving.value = true;
  try {
    await store.resolveUnassignedTime("break");
    ElMessage.success("已记录为休息时间");
  } catch (error) {
    await store.loadUnassignedState();
    ElMessage.error(errorText(error, "休息时间保存失败"));
  } finally {
    saving.value = false;
  }
}

async function discardTime() {
  saving.value = true;
  try {
    await store.resolveUnassignedTime("discard");
    ElMessage.info("已清除这段无效时间");
  } catch (error) {
    await store.loadUnassignedState();
    ElMessage.error(errorText(error, "无效时间处理失败"));
  } finally {
    saving.value = false;
  }
}

function errorText(error: unknown, fallback: string) {
  if (typeof error === "string" && error.trim()) return error;
  return error instanceof Error ? error.message : fallback;
}
</script>

<template>
  <el-dialog
    v-model="store.unassignedDialogOpen"
    class="unassigned-time-dialog"
    width="var(--unassigned-dialog-width)"
    append-to-body
    :show-close="false"
    :close-on-click-modal="false"
    :close-on-press-escape="false"
    :close-on-hash-change="false"
  >
    <div class="unassigned-dialog-head">
      <span class="unassigned-dialog-icon"><ClockAlert :size="20" /></span>
      <div>
        <h2>处理{{ dateLabel(store.unassignedWorkDate) }}的未归属时间</h2>
        <p>每个日期独立处理，历史时间不会并入今天。</p>
      </div>
    </div>

    <div v-if="store.unassignedHistoricalPending.length" class="unassigned-session-tabs">
      <button
        v-if="store.unassignedCurrentSession"
        type="button"
        :class="{ active: store.unassignedCurrentSession.sessionId === store.unassignedSessionId }"
        @click="store.selectUnassignedSession(store.unassignedCurrentSession.sessionId)"
      >
        今天 · {{ Math.ceil(store.unassignedCurrentSession.elapsedSeconds / 60) }} 分钟
      </button>
      <button
        v-for="session in store.unassignedHistoricalPending"
        :key="session.sessionId"
        type="button"
        :class="{ active: session.sessionId === store.unassignedSessionId }"
        @click="store.selectUnassignedSession(session.sessionId)"
      >
        {{ dateLabel(session.workDate) }} · {{ Math.ceil(session.elapsedSeconds / 60) }} 分钟
      </button>
    </div>

    <div class="unassigned-live-period">
      <span>{{ periodLabel }}</span>
      <time>{{ clockText }}</time>
      <small>{{ store.unassignedStartedAt ? '仍在累计' : '等待处理' }}</small>
    </div>

    <section class="unassigned-allocation-editor">
      <div class="unassigned-allocation-head">
        <span>分配到事项</span>
        <strong :class="{ invalid: remainingMinutes < 0 }">
          {{ remainingMinutes === 0 ? `已分配 ${requiredMinutes} 分钟` : remainingMinutes > 0 ? `剩余 ${remainingMinutes} 分钟` : `超出 ${Math.abs(remainingMinutes)} 分钟` }}
        </strong>
      </div>
      <div v-for="(allocation, allocationIndex) in allocations" :key="allocation.id" class="unassigned-allocation-row">
        <TaskSelectControl
          :ref="(component) => setTaskControl(allocation.id, component)"
          v-model="allocation.taskId"
          variant="compact"
          placeholder="选择或输入临时事项"
          aria-label="未归属时间对应事项"
        />
        <label><input :value="allocation.minutes" min="0" :max="allocationInputMaximum(allocationIndex)" type="number" @input="updateAllocationMinutes(allocationIndex, $event)" /><span>分钟</span></label>
        <label class="allocation-complete-toggle" title="保存工时时同时完成这个事项">
          <input v-model="allocation.completeTask" type="checkbox" />
          <span>同时完成</span>
        </label>
        <button class="icon-button subtle" type="button" title="删除此事项" :disabled="allocations.length === 1" @click="removeAllocation(allocation.id)"><Minus :size="14" /></button>
      </div>
      <button class="unassigned-add-allocation" type="button" @click="addAllocation"><Plus :size="14" />添加事项</button>
    </section>

    <div class="unassigned-dialog-actions">
      <div class="unassigned-secondary-actions">
        <button class="secondary-button" type="button" :disabled="saving" @click="markAsBreak"><Coffee :size="14" />休息时间</button>
        <button class="secondary-button discard" type="button" :disabled="saving" @click="discardTime"><Trash2 :size="14" />无效时间</button>
      </div>
      <div class="unassigned-primary-actions">
        <button class="secondary-button" type="button" :disabled="saving || allocatedMinutes <= 0" @click="clearAllocations"><Eraser :size="14" />清空用时</button>
        <button class="primary-button" type="button" :disabled="saving" @click="assignTime($event)"><Check :size="15" />分配给事项</button>
      </div>
    </div>
  </el-dialog>
</template>
