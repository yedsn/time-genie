<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Pause, Play, Repeat2, Square, X } from "lucide-vue-next";
import { ElMessage } from "element-plus";
import { useWorkdayStore } from "../store";
import TaskSelectControl, { type TaskSelectIntent } from "./TaskSelectControl.vue";

const emit = defineEmits<{ open: [] }>();
const store = useWorkdayStore();
const selectedTaskId = ref(store.selectedTaskId);
const busy = ref(false);
const switching = ref(false);

watch(() => store.runningEntry?.id, () => {
  selectedTaskId.value = store.runningEntry?.defaultTask ?? store.selectedTaskId;
  if (!store.runningEntry) switching.value = false;
});

const elapsedSeconds = computed(() => {
  const entry = store.runningEntry;
  return entry ? store.entryDurationSeconds(entry) : 0;
});

const clockText = computed(() => {
  const seconds = elapsedSeconds.value;
  const hours = Math.floor(seconds / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  const rest = seconds % 60;
  return [hours, minutes, rest].map((value) => String(value).padStart(2, "0")).join(":");
});

async function submitTimerIntent(intent: TaskSelectIntent) {
  busy.value = true;
  try {
    const result = await store.transitionTimer(intent);
    switching.value = false;
    if (result.previousStopResult && !result.noOp) ElMessage.success("已切换事项，上一段计时可稍后确认归属");
  } catch (error) {
    ElMessage.error(errorText(error, store.runningEntry ? "切换计时失败" : "开始计时失败"));
  } finally {
    busy.value = false;
  }
}

async function runTimerAction(action: () => Promise<unknown>, fallback: string) {
  busy.value = true;
  try {
    await action();
  } catch (error) {
    ElMessage.error(errorText(error, fallback));
  } finally {
    busy.value = false;
  }
}

function errorText(error: unknown, fallback: string) {
  if (typeof error === "string" && error.trim()) return error;
  return error instanceof Error ? error.message : fallback;
}
</script>

<template>
  <section class="global-timer-bar topbar-quick-timer" :class="store.runningEntry?.state ?? 'idle'">
    <template v-if="store.runningEntry && !switching">
      <button class="global-timer-main" type="button" title="选择或新建事项并切换计时" @click="switching = true">
        <span class="global-timer-indicator" aria-hidden="true"></span>
        <span class="global-timer-copy">
          <small>{{ store.runningEntry.state === 'paused' ? '已暂停' : '正在计时' }}</small>
          <strong>{{ store.timeEntryLabel(store.runningEntry) }}</strong>
        </span>
        <time>{{ clockText }}</time>
      </button>
      <div class="global-timer-actions">
        <button class="icon-button" type="button" title="切换事项" :disabled="busy" @click="switching = true"><Repeat2 :size="14" /></button>
        <button v-if="store.runningEntry.state === 'running'" class="icon-button" type="button" title="暂停" :disabled="busy" @click="runTimerAction(store.pauseTimer, '暂停计时失败')"><Pause :size="14" fill="currentColor" /></button>
        <button v-else class="icon-button" type="button" title="继续" :disabled="busy" @click="runTimerAction(store.resumeTimer, '继续计时失败')"><Play :size="14" fill="currentColor" /></button>
        <button class="icon-button stop" type="button" title="结束计时" :disabled="busy" @click="runTimerAction(store.stopTimer, '结束计时失败')"><Square :size="12" fill="currentColor" /></button>
      </div>
    </template>

    <template v-else>
      <TaskSelectControl
        v-model="selectedTaskId"
        variant="compact"
        intent-mode
        :busy="busy"
        :placeholder="store.runningEntry ? '选择或输入新事项并切换' : '选择或输入新事项'"
        :aria-label="store.runningEntry ? '切换计时事项' : '快速计时事项'"
        @submit="submitTimerIntent"
      />
      <button v-if="store.runningEntry" class="quick-timer-start timer-switch-cancel" type="button" title="取消切换" :disabled="busy" @click="switching = false"><X :size="15" /></button>
      <button v-else class="quick-timer-start" type="button" title="请先选择事项" disabled><Play :size="15" fill="currentColor" /></button>
    </template>
  </section>
</template>
