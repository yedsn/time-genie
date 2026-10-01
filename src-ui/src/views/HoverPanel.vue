<script setup lang="ts">
import { computed, ref } from "vue";
import { ArrowUpRight, Pause, Play, Square } from "lucide-vue-next";
import { ElMessage } from "element-plus";
import { useWorkdayStore } from "../store";
import { openMainOverview } from "../services/tauri";
import TaskSelectControl from "../components/TaskSelectControl.vue";

const store = useWorkdayStore();
const busy = ref(false);
const selectedTaskId = ref(store.selectedTaskId);
const entry = computed(() => store.runningEntry);
const elapsed = computed(() => {
  if (!entry.value) return "00:00";
  const minutes = Math.floor(store.entryDurationSeconds(entry.value) / 60);
  return `${String(Math.floor(minutes / 60)).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;
});

async function runTimerAction(action: () => Promise<unknown>, fallback: string) {
  busy.value = true;
  try {
    await action();
  } catch (error) {
    const message = typeof error === "string" ? error : error instanceof Error ? error.message : fallback;
    ElMessage.error(message);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="hover-panel-window">
    <div class="hover-panel">
      <header>
        <div class="eyebrow"><span class="live-dot"></span>今日工作台</div>
        <button class="icon-button" title="打开主窗口" @click="openMainOverview"><ArrowUpRight :size="16" /></button>
      </header>
      <div class="hover-timer">
        <div class="timer-state">{{ entry ? (entry.state === 'paused' ? '已暂停' : '正在记录') : '暂无运行中的计时' }}</div>
        <strong>{{ elapsed }}</strong>
        <div v-if="!entry" class="hover-task-select"><TaskSelectControl v-model="selectedTaskId" variant="compact" placeholder="事项（可选）" aria-label="悬浮窗计时事项" /></div>
        <div v-else class="hover-task">{{ store.timeEntryLabel(entry) }}</div>
      </div>
      <div class="hover-stats"><span>今日已记录 <b>{{ store.todayMinutes }} 分钟</b></span><span>待结算 <b class="accent-text">{{ store.pendingMinutes }} 分钟</b></span></div>
      <div class="hover-actions">
        <button v-if="!entry" class="primary-button" :disabled="busy" @click="runTimerAction(() => store.startTimer(selectedTaskId), '开始计时失败')"><Play :size="15" fill="currentColor" /> 开始计时</button>
        <button v-else-if="entry.state === 'running'" class="secondary-button" :disabled="busy" @click="runTimerAction(store.pauseTimer, '暂停计时失败')"><Pause :size="15" fill="currentColor" /> 暂停</button>
        <button v-else class="secondary-button" :disabled="busy" @click="runTimerAction(store.resumeTimer, '继续计时失败')"><Play :size="15" fill="currentColor" /> 继续</button>
        <button v-if="entry" class="danger-button" :disabled="busy" @click="runTimerAction(store.stopTimer, '结束计时失败')"><Square :size="14" fill="currentColor" /> 结束本段</button>
      </div>
    </div>
  </section>
</template>
