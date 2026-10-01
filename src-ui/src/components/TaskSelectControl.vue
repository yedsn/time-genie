<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { ElMessage } from "element-plus";
import { X } from "lucide-vue-next";
import { useWorkdayStore, type Task } from "../store";

type TaskSuggestion = {
  value: string;
  label: string;
  pathLabel: string;
  subjectLabel: string;
  stateLabel: string;
  stateClass: string;
  spentLabel: string;
  recurrenceLabel: string;
  selectable: boolean;
  isMatch: boolean;
  taskId?: string;
};

type AutocompleteInstance = {
  getData: (query: string) => Promise<void>;
};

const props = withDefaults(defineProps<{
  modelValue: string;
  placeholder?: string;
  ariaLabel?: string;
  variant?: "default" | "focus" | "compact";
}>(), {
  placeholder: "选择或输入临时事项",
  ariaLabel: "选择事项",
  variant: "default"
});

const emit = defineEmits<{ "update:modelValue": [value: string] }>();
const store = useWorkdayStore();
const allSubjectsValue = "__all_subjects__";
const inputText = ref("");
const dirty = ref(false);
const selectedSubjectId = ref(allSubjectsValue);
const autocompleteRef = ref<AutocompleteInstance>();
let lastQuery = "";
let blurTimer: number | undefined;

const selectedTask = computed(() => store.tasks.find((task) => task.id === props.modelValue));
const selectedTaskPath = computed(() => selectedTask.value ? store.taskPathLabel(selectedTask.value) : "");
const allSubjectTasks = computed(() => {
  const ordered = selectedSubjectId.value === allSubjectsValue
    ? store.visibleTasks
    : store.visibleTasks.filter((task) => task.subjectId === selectedSubjectId.value);
  return [
    ...ordered.filter((task) => task.status !== "done"),
    ...ordered.filter((task) => task.status === "done")
  ];
});
const inputClass = computed(() => [
  "task-autocomplete",
  props.variant === "focus" && "task-autocomplete--focus",
  props.variant === "compact" && "task-autocomplete--compact"
]);

watch([() => props.modelValue, selectedTaskPath], () => {
  if (!dirty.value) inputText.value = selectedTaskPath.value;
}, { immediate: true });

watch(selectedSubjectId, () => {
  if (selectedSubjectId.value === allSubjectsValue || selectedTask.value?.subjectId === selectedSubjectId.value) return;
  inputText.value = "";
  dirty.value = false;
  emit("update:modelValue", "");
});

function taskLabel(task: Task) {
  const subject = store.subjects.find((item) => item.id === task.subjectId);
  return [subject?.name ?? "默认", store.taskPathLabel(task)].join(" / ");
}

function taskSubjectLabel(task: Task) {
  return store.subjects.find((item) => item.id === task.subjectId)?.name ?? "默认";
}

function taskPathLabel(task: Task) {
  return store.taskPathLabel(task);
}

function getDescendantTasks(task: Task): Task[] {
  const children = store.tasks.filter((item) => item.parentId === task.id);
  return children.flatMap((child) => [child, ...getDescendantTasks(child)]);
}

function directTaskSpentMinutes(taskId: string) {
  let total = 0;
  for (const entry of store.entries) {
    if (entry.allocations?.length) {
      total += entry.allocations
        .filter((allocation) => allocation.taskId === taskId)
        .reduce((sum, allocation) => sum + Math.max(0, allocation.minutes), 0);
      continue;
    }
    if (entry.defaultTask !== taskId) continue;
    total += Math.ceil(store.entryDurationSeconds(entry) / 60);
  }
  return total;
}

function taskSpentMinutes(task: Task, descendants: Task[]) {
  if (descendants.length) {
    return descendants.reduce((sum, child) => sum + directTaskSpentMinutes(child.id), 0);
  }
  return directTaskSpentMinutes(task.id);
}

function formatSpentMinutes(minutes: number) {
  if (minutes < 60) return `${minutes} 分钟`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? `${hours}小时${rest}分` : `${hours}小时`;
}

function taskState(task: Task, spentMinutes: number, descendants: Task[]) {
  if (descendants.length) {
    const runningChild = descendants.find((child) => store.entries.some((entry) => entry.defaultTask === child.id && entry.state === "running"));
    const pausedChild = descendants.find((child) => store.entries.some((entry) => entry.defaultTask === child.id && entry.state === "paused"));
    if (runningChild) return { label: "进行中", className: "running" };
    if (pausedChild) return { label: "已暂停", className: "paused" };
    if (descendants.every((child) => child.status === "done")) return { label: "已完成", className: "done" };
    if (spentMinutes > 0) return { label: "已开始", className: "started" };
    return { label: "未开始", className: "not-started" };
  }
  const liveEntry = store.entries.find((entry) => entry.defaultTask === task.id && (entry.state === "running" || entry.state === "paused"));
  if (liveEntry?.state === "running") return { label: "进行中", className: "running" };
  if (liveEntry?.state === "paused") return { label: "已暂停", className: "paused" };
  if (task.status === "done") return { label: "已完成", className: "done" };
  if (spentMinutes > 0) return { label: "已开始", className: "started" };
  return { label: "未开始", className: "not-started" };
}

function fetchSuggestions(query: string, callback: (items: TaskSuggestion[]) => void) {
  lastQuery = query;
  const keyword = query.trim().toLocaleLowerCase();
  const suggestions = allSubjectTasks.value
    .map((task) => {
      const label = taskLabel(task);
      const descendants = getDescendantTasks(task);
      const spentMinutes = taskSpentMinutes(task, descendants);
      const state = taskState(task, spentMinutes, descendants);
      return {
        value: label,
        label,
        pathLabel: taskPathLabel(task),
        subjectLabel: taskSubjectLabel(task),
        stateLabel: state.label,
        stateClass: state.className,
        spentLabel: `已用 ${formatSpentMinutes(spentMinutes)}`,
        recurrenceLabel: task.recurrence?.summary ?? "",
        selectable: descendants.length === 0,
        isMatch: !keyword || label.toLocaleLowerCase().includes(keyword),
        taskId: task.id
      };
    })
  callback(suggestions);
}

function handleInput(value: string | number) {
  inputText.value = String(value);
  const selectedLabel = selectedTask.value ? taskPathLabel(selectedTask.value) : "";
  dirty.value = inputText.value.trim() !== selectedLabel.trim();
}

function clearSelectedTask() {
  if (blurTimer) window.clearTimeout(blurTimer);
  inputText.value = "";
  dirty.value = false;
  emit("update:modelValue", "");
}

function openTaskSuggestions(event: MouseEvent) {
  const target = event.target as HTMLElement | null;
  if (target?.closest(".el-input__clear")) return;
  void autocompleteRef.value?.getData(inputText.value);
}

function selectSuggestion(item: TaskSuggestion) {
  if (blurTimer) window.clearTimeout(blurTimer);
  dirty.value = false;
  if (!item.selectable) {
    inputText.value = lastQuery.trim();
    dirty.value = inputText.value.trim() !== (selectedTask.value ? taskPathLabel(selectedTask.value) : "").trim();
    return;
  }
  inputText.value = item.pathLabel.trim();
  if (item.taskId) {
    // Selecting another concrete item replaces the previous association immediately.
    emit("update:modelValue", item.taskId);
    return;
  }
  commitText();
}

function commitText() {
  if (blurTimer) window.clearTimeout(blurTimer);
  const title = inputText.value.trim();
  if (!title) {
    dirty.value = false;
    emit("update:modelValue", "");
    return "";
  }
  if (selectedTask.value && taskPathLabel(selectedTask.value).trim() === title) {
    dirty.value = false;
    return selectedTask.value.id;
  }
  const targetSubjectId = selectedSubjectId.value === allSubjectsValue ? store.selectedSubjectId : selectedSubjectId.value;
  const subjectMatches = selectedSubjectId.value === allSubjectsValue
    ? store.tasks
    : store.tasks.filter((task) => task.subjectId === selectedSubjectId.value);
  const existing = subjectMatches.find((task) => taskPathLabel(task).trim() === title)
    ?? subjectMatches.find((task) => task.title.trim() === title);
  if (existing && getDescendantTasks(existing).length) {
    inputText.value = selectedTask.value ? taskPathLabel(selectedTask.value) : "";
    dirty.value = false;
    return "";
  }
  if (existing) {
    emit("update:modelValue", existing.id);
    dirty.value = false;
    return existing.id;
  } else {
    const taskId = store.addQuickTask(title, targetSubjectId);
    emit("update:modelValue", taskId);
    ElMessage.success(`已创建临时事项“${title}”`);
    dirty.value = false;
    return taskId;
  }
}

function scheduleCommit() {
  blurTimer = window.setTimeout(() => {
    if (dirty.value) commitText();
  }, 120);
}

defineExpose({ commit: commitText });
</script>

<template>
  <div class="task-select-composite" :class="inputClass">
    <el-select v-model="selectedSubjectId" class="task-subject-select" aria-label="选择主体" popper-class="task-subject-popper">
      <el-option label="全部主体" :value="allSubjectsValue" />
      <el-option v-for="subject in store.subjects" :key="subject.id" :label="subject.name" :value="subject.id" />
    </el-select>
    <el-autocomplete
      ref="autocompleteRef"
      v-model="inputText"
      class="task-autocomplete-input"
      :fetch-suggestions="fetchSuggestions"
      :placeholder="placeholder"
      :aria-label="ariaLabel"
      popper-class="task-autocomplete-popper"
      fit-input-width
      highlight-first-item
      select-when-unmatched
      :trigger-on-focus="true"
      clearable
      :clear-icon="X"
      @input="handleInput"
      @click="openTaskSuggestions"
      @clear="clearSelectedTask"
      @select="selectSuggestion"
      @blur="scheduleCommit"
      @keydown.enter.stop
    >
      <template #default="{ item }">
        <span
          class="task-autocomplete-option"
          :class="{ 'is-parent': !item.selectable, 'is-match': item.isMatch }"
          :title="item.selectable ? item.label : '父事项仅汇总子事项用时，请选择具体子事项'"
          :aria-disabled="!item.selectable"
        >
          <span v-if="selectedSubjectId === allSubjectsValue" class="task-option-subject">{{ item.subjectLabel }}</span>
          <span class="task-option-name">{{ item.pathLabel }}</span>
          <span v-if="item.recurrenceLabel" class="task-option-repeat">{{ item.recurrenceLabel }}</span>
          <span class="task-option-spent">{{ item.spentLabel }}</span>
          <span class="task-option-state" :class="`is-${item.stateClass}`">{{ item.stateLabel }}</span>
        </span>
      </template>
    </el-autocomplete>
  </div>
</template>
