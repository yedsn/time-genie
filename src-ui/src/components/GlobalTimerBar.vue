<script setup lang="ts">
import { computed, ref } from "vue";
import { Pause, Play, Square } from "lucide-vue-next";
import { ElMessage } from "element-plus";
import { useWorkdayStore } from "../store";
import TaskSelectControl from "./TaskSelectControl.vue";

type TaskSelectControlInstance = { commit: () => string | undefined };

const emit = defineEmits<{ open: [] }>();
const store = useWorkdayStore();
const selectedTaskId = ref(store.selectedTaskId);
const taskControl = ref<TaskSelectControlInstance>();
const busy = ref(false);

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

async function startSelectedTimer() {
  const taskId = taskControl.value?.commit() ?? selectedTaskId.value;
  busy.value = true;
  try {
    await store.startTimer(taskId);
  } catch (error) {
    ElMessage.error(errorText(error, "开始计时失败"));
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
    <template v-if="store.runningEntry">
      <button class="global-timer-main" type="button" title="打开计时页面" @click="emit('open')">
        <span class="global-timer-indicator" aria-hidden="true"></span>
        <span class="global-timer-copy">
          <small>{{ store.runningEntry.state === 'paused' ? '已暂停' : '正在计时' }}</small>
          <strong>{{ store.timeEntryLabel(store.runningEntry) }}</strong>
        </span>
        <time>{{ clockText }}</time>
      </button>
      <div class="global-timer-actions">
        <button v-if="store.runningEntry.state === 'running'" class="icon-button" type="button" title="暂停" :disabled="busy" @click="runTimerAction(store.pauseTimer, '暂停计时失败')"><Pause :size="14" fill="currentColor" /></button>
        <button v-else class="icon-button" type="button" title="继续" :disabled="busy" @click="runTimerAction(store.resumeTimer, '继续计时失败')"><Play :size="14" fill="currentColor" /></button>
        <button class="icon-button stop" type="button" title="结束计时" :disabled="busy" @click="runTimerAction(store.stopTimer, '结束计时失败')"><Square :size="12" fill="currentColor" /></button>
      </div>
    </template>

    <template v-else>
      <TaskSelectControl
        ref="taskControl"
        v-model="selectedTaskId"
        variant="compact"
        placeholder="事项（可选）"
        aria-label="快速计时事项"
      />
      <button class="quick-timer-start" type="button" title="开始计时" :disabled="busy" @click="startSelectedTimer"><Play :size="15" fill="currentColor" /></button>
    </template>
  </section>
</template>
