<script setup lang="ts">
import { computed, ref } from "vue";
import { Pause, Play, Repeat2, Square } from "lucide-vue-next";
import { ElMessage } from "element-plus";
import { useWorkdayStore } from "../store";

const emit = defineEmits<{ open: [] }>();
const store = useWorkdayStore();
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
      <button class="global-timer-main" type="button" title="打开事项列表并切换计时" @click="emit('open')">
        <span class="global-timer-indicator" aria-hidden="true"></span>
        <span class="global-timer-copy">
          <small>{{ store.runningEntry.state === 'paused' ? '已暂停' : '正在计时' }}</small>
          <strong>{{ store.timeEntryLabel(store.runningEntry) }}</strong>
        </span>
        <time>{{ clockText }}</time>
      </button>
      <div class="global-timer-actions">
        <button class="icon-button" type="button" title="切换事项" :disabled="busy" @click="emit('open')"><Repeat2 :size="14" /></button>
        <button v-if="store.runningEntry.state === 'running'" class="icon-button" type="button" title="暂停" :disabled="busy" @click="runTimerAction(store.pauseTimer, '暂停计时失败')"><Pause :size="14" fill="currentColor" /></button>
        <button v-else class="icon-button" type="button" title="继续" :disabled="busy" @click="runTimerAction(store.resumeTimer, '继续计时失败')"><Play :size="14" fill="currentColor" /></button>
        <button class="icon-button stop" type="button" title="结束计时" :disabled="busy" @click="runTimerAction(store.stopTimer, '结束计时失败')"><Square :size="12" fill="currentColor" /></button>
      </div>
    </template>

    <button v-else class="global-timer-start-button" type="button" :disabled="busy" @click="emit('open')"><Play :size="15" fill="currentColor" /><span>开始计时</span></button>
  </section>
</template>
