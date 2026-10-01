<script setup lang="ts">
import { computed } from "vue";
import { ClockAlert } from "lucide-vue-next";
import { useWorkdayStore } from "../store";

const store = useWorkdayStore();

const clockText = computed(() => {
  const seconds = store.unassignedSeconds;
  const hours = Math.floor(seconds / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  const rest = seconds % 60;
  return hours
    ? [hours, minutes, rest].map((value) => String(value).padStart(2, "0")).join(":")
    : [minutes, rest].map((value) => String(value).padStart(2, "0")).join(":");
});

function openAllocation() {
  store.promptUnassignedResolution(true);
}
</script>

<template>
  <button
    v-if="!store.runningEntry && !store.unassignedDialogOpen"
    class="unassigned-time-indicator"
    type="button"
    title="处理未归属时间"
    @click="openAllocation"
  >
    <ClockAlert :size="14" />
    <span>未归属</span>
    <time>{{ clockText }}</time>
  </button>
</template>
