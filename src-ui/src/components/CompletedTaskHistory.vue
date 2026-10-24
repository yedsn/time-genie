<script setup lang="ts">
import { computed } from "vue";
import { CheckCircle2, ChevronLeft, ChevronRight, Clock3, CornerDownRight, LoaderCircle, RefreshCw, RotateCcw, Trash2 } from "lucide-vue-next";
import { ElMessageBox } from "element-plus";
import type { Task } from "../store";

const props = defineProps<{
  tasks: Task[];
  subjectName: string;
  page: number;
  total: number;
  totalPages: number;
  loading: boolean;
  error: string;
  formatMinutes: (minutes: number) => string;
}>();

const emit = defineEmits<{
  refresh: [];
  page: [page: number];
  reopen: [task: Task];
  save: [task: Task];
  delete: [task: Task];
}>();

const pageLabel = computed(() => props.totalPages ? `${props.page} / ${props.totalPages}` : "1 / 1");

function parentPath(task: Task) {
  const path = task.pathLabel?.trim();
  if (!path || path === task.title) return "顶层事项";
  const parts = path.split(" / ");
  parts.pop();
  return parts.join(" / ") || "顶层事项";
}

function formatCompletedAt(value?: number) {
  if (!value) return "完成时间未知";
  return new Date(value).toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function finishTitleEdit(event: Event, task: Task) {
  const input = event.currentTarget as HTMLInputElement;
  if (!task.title.trim()) {
    input.focus();
    return;
  }
  emit("save", task);
}

function blurTitle(event: Event) {
  (event.currentTarget as HTMLInputElement).blur();
}

async function confirmDelete(task: Task) {
  try {
    await ElMessageBox.confirm(`确定删除“${task.title || "未命名事项"}”？如果它还有子项，会一并删除。`, "删除已完成事项", {
      confirmButtonText: "删除",
      cancelButtonText: "取消",
      type: "warning",
      customClass: "work-confirm-dialog",
      confirmButtonClass: "work-confirm-danger",
    });
    emit("delete", task);
  } catch {}
}
</script>

<template>
  <section class="completed-history-panel">
    <header class="completed-history-head">
      <div>
        <h2>{{ subjectName }}</h2>
        <span>共 {{ total }} 项普通已完成事项 · 按完成时间倒序排列</span>
      </div>
      <button class="icon-button subtle" type="button" title="刷新已完成事项" :disabled="loading" @click="emit('refresh')">
        <RefreshCw :size="15" :class="{ spinning: loading }" />
      </button>
    </header>

    <div v-if="loading && !tasks.length" class="completed-history-state">
      <LoaderCircle :size="24" class="spinning" />
      <strong>正在加载已完成事项</strong>
      <span>只读取当前主体的这一页</span>
    </div>
    <div v-else-if="error" class="completed-history-state error">
      <RefreshCw :size="22" />
      <strong>已完成事项加载失败</strong>
      <span>{{ error }}</span>
      <button class="secondary-button" type="button" @click="emit('refresh')">重新加载</button>
    </div>
    <div v-else-if="!tasks.length" class="completed-history-state">
      <CheckCircle2 :size="26" />
      <strong>还没有已完成事项</strong>
      <span>普通事项完成后会出现在这里，重复事项的每日轮次不会进入历史。</span>
    </div>
    <div v-else class="completed-history-list">
      <article v-for="task in tasks" :key="task.id" class="completed-history-row">
        <button class="completed-history-reopen" type="button" title="恢复为进行中" @click="emit('reopen', task)">
          <RotateCcw :size="16" />
        </button>
        <div class="completed-history-main">
          <input v-model="task.title" aria-label="事项名称" @blur="finishTitleEdit($event, task)" @keydown.enter.prevent="blurTitle" />
          <div class="completed-history-meta">
            <span :title="parentPath(task)"><CornerDownRight :size="12" />{{ parentPath(task) }}</span>
            <span><CheckCircle2 :size="12" />{{ formatCompletedAt(task.completedAt) }}</span>
            <span><Clock3 :size="12" />累计 {{ formatMinutes(task.actual) }}</span>
          </div>
        </div>
        <button class="icon-button subtle completed-history-delete" type="button" title="删除" @click="confirmDelete(task)"><Trash2 :size="14" /></button>
      </article>
    </div>

    <footer v-if="totalPages > 1" class="completed-history-pagination">
      <span>每页 20 项</span>
      <div>
        <button class="icon-button subtle" type="button" title="上一页" :disabled="loading || page <= 1" @click="emit('page', page - 1)"><ChevronLeft :size="15" /></button>
        <strong>{{ pageLabel }}</strong>
        <button class="icon-button subtle" type="button" title="下一页" :disabled="loading || page >= totalPages" @click="emit('page', page + 1)"><ChevronRight :size="15" /></button>
      </div>
    </footer>
  </section>
</template>
