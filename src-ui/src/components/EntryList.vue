<script setup lang="ts">
import { computed } from "vue";
import { CircleDot, Pause, Play, Square } from "lucide-vue-next";
import { useWorkdayStore, type TimeEntry } from "../store";

defineProps<{ formatMinutes: (minutes: number) => string; selectable?: boolean }>();
const store = useWorkdayStore();
const entries = computed(() => [...store.entries].reverse());

function entryMinutes(entry: TimeEntry) {
  return Math.ceil(store.entryDurationSeconds(entry) / 60);
}
</script>

<template>
  <div class="entry-list">
    <button v-for="entry in entries" :key="entry.id" class="entry-row" :class="{ selected: entry.id === store.selectedEntryId }" @click="store.selectedEntryId = entry.id">
      <span class="entry-icon" :class="entry.state"><CircleDot v-if="entry.state === 'ended'" :size="16" /><Pause v-else-if="entry.state === 'paused'" :size="15" /><Play v-else :size="15" fill="currentColor" /></span>
      <span class="entry-copy"><strong>{{ store.timeEntryLabel(entry) }}</strong><small>{{ entry.note || (entry.state === 'running' ? '未结束的当前时间片' : '今日工作记录') }}</small></span>
      <span class="entry-minutes">{{ formatMinutes(entryMinutes(entry)) }}</span>
      <span class="entry-allocation">已分配 {{ entry.allocated }}min</span>
      <Square v-if="entry.state === 'running'" class="entry-live" :size="13" fill="currentColor" />
    </button>
  </div>
</template>
