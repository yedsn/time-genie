<script setup lang="ts">
import { computed } from "vue";
import { CheckCircle2, Circle, CornerDownRight, MoreHorizontal, Plus, Trash2 } from "lucide-vue-next";
import type { Task, TaskStatus } from "../store";

const props = defineProps<{
  tasks: Task[];
  formatMinutes: (minutes: number) => string;
  statusLabel: (status: TaskStatus) => string;
  taskClass: (task: Task) => Array<string | Record<string, boolean>>;
  planScheduleLabel: (task: Task) => string;
}>();

const emit = defineEmits<{ select: [id: string]; delete: [id: string]; toggle: [task: Task, event: MouseEvent]; addChild: [id: string] }>();

const tableTasks = computed(() => props.tasks.filter(isTaskRow).map(toTableTask));

function isTaskRow(task: unknown): task is Task {
  if (!task || typeof task !== "object") return false;
  const value = task as Partial<Task>;
  return typeof value.id === "string" && typeof value.title === "string" && typeof value.status === "string";
}

function normalizeClassList(classes: Array<string | Record<string, boolean>>) {
  return classes.flatMap((item) => {
    if (typeof item === "string") return item;
    return Object.entries(item).filter(([, enabled]) => enabled).map(([name]) => name);
  });
}

function toTableTask(task: Task): Task {
  return { ...task };
}

function rowClassName({ row }: { row: Task }) {
  return [...normalizeClassList(props.taskClass(row)), row.parentId ? "child" : ""].filter(Boolean).join(" ");
}

function handleRowClick(row: Task) {
  emit("select", row.id);
}

function getRowKey(row: Task) {
  return row.id;
}

function getSourceTask(row: Task) {
  return props.tasks.find((task) => task.id === row.id) ?? row;
}

function isOverdue(row: Task) {
  if (row.status !== "open" || !row.planDate) return false;
  const now = new Date();
  const today = [now.getFullYear(), String(now.getMonth() + 1).padStart(2, "0"), String(now.getDate()).padStart(2, "0")].join("-");
  return row.planDate < today;
}
</script>

<template>
  <div class="task-table">
    <el-table
      :data="tableTasks"
      :row-key="getRowKey"
      table-layout="fixed"
      class="work-task-table"
      :row-class-name="rowClassName"
      @row-click="handleRowClick"
    >
      <el-table-column label="今日事项" fixed="left" width="280">
        <template #default="{ row }">
          <div class="task-fixed-name" :class="{ 'is-child': row.parentId }" :data-task-id="row.id">
            <button class="check-button" :title="row.status === 'done' ? '标记为计划中' : '标记为完成'" @click.stop="emit('toggle', getSourceTask(row), $event)">
              <CheckCircle2 v-if="row.status === 'done'" :size="18" />
              <Circle v-else :size="18" />
            </button>
            <div class="task-name">
              <strong><CornerDownRight v-if="row.parentId" :size="14" />{{ row.title }}</strong>
              <span>{{ row.parentId ? '子项' : row.project }}</span>
            </div>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="预计开始" width="190">
        <template #default="{ row }"><span class="task-schedule" :class="{ overdue: isOverdue(row) }">{{ isOverdue(row) ? '逾期 · ' : '' }}{{ planScheduleLabel(row) }}</span></template>
      </el-table-column>
      <el-table-column label="预计用时" width="110">
        <template #default="{ row }"><span class="task-time">{{ row.estimate ? formatMinutes(row.estimate) : '未填' }}</span></template>
      </el-table-column>
      <el-table-column label="今日预计 / 实际" width="160">
        <template #default="{ row }"><span class="task-time">{{ row.todayEstimate ? formatMinutes(row.todayEstimate) : '未填' }} <em>/</em> {{ formatMinutes(row.actual) }}</span></template>
      </el-table-column>
      <el-table-column label="状态" fixed="right" width="86" align="center">
        <template #default="{ row }"><span class="status-chip" :class="row.status">{{ statusLabel(row.status) }}</span></template>
      </el-table-column>
      <el-table-column label="操作" fixed="right" width="100" align="right">
        <template #default="{ row }">
          <div class="row-actions">
            <button v-if="!row.parentId && !row.recurrence" class="icon-button subtle" title="添加子项" @click.stop="emit('addChild', row.id)"><Plus :size="14" /></button>
            <button class="icon-button subtle" title="删除事项" @click.stop="emit('delete', row.id)"><Trash2 :size="14" /></button>
          </div>
        </template>
      </el-table-column>
      <template #empty><div class="empty-state"><MoreHorizontal :size="20" />暂无今日计划</div></template>
    </el-table>
  </div>
</template>
