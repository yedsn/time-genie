<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Check, Coffee, ExternalLink, TimerReset, Trash2 } from "lucide-vue-next";
import { ElMessage } from "element-plus";
import { useWorkdayStore } from "../store";
import { playCompletionFeedback } from "../services/completionFeedback";

const store = useWorkdayStore();
const completeTask = ref(false);
const saving = ref(false);

const open = computed({
  get: () => Boolean(store.timerStopConfirmationEntry),
  set: (value) => {
    if (!value) store.dismissTimerStopConfirmation();
  },
});
const entry = computed(() => store.timerStopConfirmationEntry);
const defaultTask = computed(() => {
  const taskId = entry.value?.defaultTask;
  return store.tasks.find((task) => task.id === taskId && task.selectable && task.status !== "done");
});
const canUseDefaultTask = computed(() => Boolean(defaultTask.value && store.timerStopConfirmationSlices.some((slice) => slice.minutes > 0)));
const defaultTaskLabel = computed(() => store.taskDisplayLabel(entry.value?.defaultTask));

watch(() => store.timerStopConfirmationEntryId, () => {
  completeTask.value = false;
});

function formatMinutes(minutes = 0) {
  if (minutes < 60) return `${minutes} 分钟`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? `${hours} 小时 ${rest} 分钟` : `${hours} 小时`;
}

async function confirmDefaultAllocation() {
  if (!canUseDefaultTask.value) return;
  saving.value = true;
  try {
    const result = await store.confirmTimerStopAllocation(completeTask.value);
    ElMessage.success("本段归属已保存");
    if (result && result.completedCount) playCompletionFeedback({ completedCount: result.completedCount });
  } catch (error) {
    ElMessage.error(errorText(error, "归属保存失败"));
  } finally {
    saving.value = false;
  }
}

async function markDisposition(disposition: "break" | "discard") {
  saving.value = true;
  try {
    await store.resolveTimerStopDisposition(disposition);
    ElMessage.success(disposition === "break" ? "已标记为休息时间" : "已标记为无效时间");
  } catch (error) {
    ElMessage.error(errorText(error, disposition === "break" ? "休息时间保存失败" : "无效时间保存失败"));
  } finally {
    saving.value = false;
  }
}

function adjustAllocation() {
  if (!entry.value) return;
  store.adjustTimerStopAllocation(entry.value.id);
}

function errorText(error: unknown, fallback: string) {
  if (typeof error === "string" && error.trim()) return error;
  return error instanceof Error ? error.message : fallback;
}
</script>

<template>
  <el-dialog
    v-model="open"
    class="timer-stop-confirm-dialog"
    width="min(560px, 92vw)"
    append-to-body
    :show-close="false"
    :close-on-click-modal="false"
    :close-on-press-escape="false"
  >
    <div v-if="entry" class="timer-stop-confirm-content">
      <div class="timer-stop-confirm-head">
        <span class="timer-stop-confirm-icon"><TimerReset :size="20" /></span>
        <div>
          <h2>确认本次计时归属</h2>
          <p>跨日计时已按日期拆分保存，可一次确认整条计时。</p>
        </div>
      </div>

      <div class="timer-stop-confirm-summary">
        <span>本次用时</span>
        <strong>{{ formatMinutes(store.timerStopConfirmation?.totalSettlementMinutes ?? entry.minutes) }}</strong>
        <small>{{ store.timeEntryLabel(entry) }}</small>
      </div>

      <div v-if="store.timerStopConfirmationSlices.length > 1" class="timer-stop-chain-slices">
        <div v-for="slice in store.timerStopConfirmationSlices" :key="slice.id" class="timer-stop-chain-slice">
          <span>{{ slice.workDate }}</span>
          <strong>{{ formatMinutes(slice.minutes) }}</strong>
        </div>
      </div>

      <section class="timer-stop-default-allocation" :class="{ disabled: !canUseDefaultTask }">
        <div>
          <span>默认归属</span>
          <strong>{{ canUseDefaultTask ? defaultTaskLabel : '需要选择具体事项' }}</strong>
          <small>{{ canUseDefaultTask ? `按日期分别保存到该事项，共 ${formatMinutes(store.timerStopConfirmation?.totalSettlementMinutes ?? entry.minutes)}` : '当前计时没有有效默认事项，请进入计时页面调整' }}</small>
        </div>
        <label class="allocation-complete-toggle" title="保存工时时同时完成这个事项">
          <input v-model="completeTask" :disabled="!canUseDefaultTask || saving" type="checkbox" />
          <span>同时完成</span>
        </label>
      </section>

      <div class="timer-stop-confirm-actions">
        <div class="timer-stop-secondary-actions">
          <button class="secondary-button compact" type="button" :disabled="saving" @click="markDisposition('break')"><Coffee :size="14" />休息时间</button>
          <button class="secondary-button compact discard" type="button" :disabled="saving" @click="markDisposition('discard')"><Trash2 :size="14" />无效时间</button>
        </div>
        <div class="timer-stop-primary-actions">
          <button class="secondary-button compact" type="button" :disabled="saving" @click="adjustAllocation"><ExternalLink :size="14" />调整归属</button>
          <button class="primary-button compact" type="button" :disabled="saving || !canUseDefaultTask" @click="confirmDefaultAllocation"><Check :size="14" />按默认保存</button>
        </div>
      </div>
    </div>
  </el-dialog>
</template>
