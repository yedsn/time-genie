<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { ElMessage } from "element-plus";
import { X } from "lucide-vue-next";
import { useWorkdayStore, type Task } from "../store";
import { canAcceptInlineTimerChild, inlineTimerParentUnavailableReason } from "../services/inlineTimerTaskCreationCore";
import { resolveTaskSelectionSubject, taskSelectionRows } from "../services/taskSelectionCore";

type TaskSuggestion = {
  value: string;
  label: string;
  pathLabel: string;
  stateLabel: string;
  stateClass: string;
  spentLabel: string;
  recurrenceLabel: string;
  selectable: boolean;
  isMatch: boolean;
  depth: number;
  taskId?: string;
  kind?: "task" | "create-root" | "choose-parent" | "create-child" | "parent-unavailable";
  parentId?: string;
  parentVersion?: number;
  unavailableReason?: string;
};

export type TaskSelectIntent =
  | { taskId: string }
  | { newTask: { subjectId: string; parentId?: string; expectedParentVersion?: number; title: string } };

type AutocompleteInstance = {
  getData: (query: string) => Promise<void>;
  highlight: (index: number) => void;
  close: () => void;
  blur: () => void;
  focus: () => void;
  activated: boolean | { value: boolean };
};

const props = withDefaults(defineProps<{
  modelValue: string;
  placeholder?: string;
  ariaLabel?: string;
  variant?: "default" | "focus" | "compact";
  intentMode?: boolean;
  busy?: boolean;
}>(), {
  placeholder: "选择或输入事项",
  ariaLabel: "选择事项",
  variant: "default",
  intentMode: false,
  busy: false,
});

const emit = defineEmits<{
  "update:modelValue": [value: string];
  submit: [intent: TaskSelectIntent];
}>();
const store = useWorkdayStore();
const inputText = ref("");
const dirty = ref(false);
const activeSubjectId = ref("");
const autocompleteRef = ref<AutocompleteInstance>();
const choosingParent = ref(false);
const pendingTitle = ref("");
let lastQuery = "";

const selectedTask = computed(() => store.tasks.find((task) => task.id === props.modelValue));
const selectedTaskPath = computed(() => selectedTask.value ? store.taskPathLabel(selectedTask.value) : "");
const activeRows = computed(() => taskSelectionRows(store.visibleTasks, activeSubjectId.value));
const inputClass = computed(() => [
  "task-autocomplete",
  props.variant === "focus" && "task-autocomplete--focus",
  props.variant === "compact" && "task-autocomplete--compact",
]);

function defaultSubjectId(task = selectedTask.value) {
  return resolveTaskSelectionSubject(store.subjects, task, store.selectedSubjectId);
}

watch([() => props.modelValue, selectedTaskPath], () => {
  if (selectedTask.value) activeSubjectId.value = defaultSubjectId(selectedTask.value);
  else if (!dirty.value) activeSubjectId.value = defaultSubjectId();
  if (!dirty.value) inputText.value = selectedTaskPath.value;
}, { immediate: true });

watch(() => store.subjects.map((subject) => subject.id).join("|"), () => {
  if (!store.subjects.some((subject) => subject.id === activeSubjectId.value)) activeSubjectId.value = defaultSubjectId();
}, { immediate: true });

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
      total += entry.allocations.filter((allocation) => allocation.taskId === taskId).reduce((sum, allocation) => sum + Math.max(0, allocation.minutes), 0);
      continue;
    }
    if (entry.defaultTask !== taskId) continue;
    total += Math.ceil(store.entryDurationSeconds(entry) / 60);
  }
  return total;
}

function taskSpentMinutes(task: Task, descendants: Task[]) {
  if (descendants.length) return descendants.reduce((sum, child) => sum + directTaskSpentMinutes(child.id), 0);
  return directTaskSpentMinutes(task.id);
}

function formatSpentMinutes(minutes: number) {
  if (minutes < 60) return String(minutes) + " 分钟";
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? String(hours) + "小时" + String(rest) + "分" : String(hours) + "小时";
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

function targetSubjectId() {
  return activeSubjectId.value || defaultSubjectId();
}

function fetchSuggestions(query: string, callback: (items: TaskSuggestion[]) => void) {
  lastQuery = query;
  if (choosingParent.value) {
    const subjectId = targetSubjectId();
    const parents = activeRows.value.filter((row) => row.depth === 0).map(({ task }) => {
      const unavailableReason = inlineTimerParentUnavailableReason(task, subjectId);
      return {
        value: "在“" + task.title + "”下新建",
        label: task.title,
        pathLabel: task.title,
        stateLabel: unavailableReason ?? "可选",
        stateClass: "not-started",
        spentLabel: "",
        recurrenceLabel: "",
        selectable: canAcceptInlineTimerChild(task, subjectId),
        isMatch: true,
        depth: 0,
        kind: unavailableReason ? "parent-unavailable" as const : "create-child" as const,
        parentId: task.id,
        parentVersion: task.version,
        unavailableReason,
      };
    });
    callback(parents.length ? parents : [{
      value: "暂无一级事项", label: "暂无一级事项", pathLabel: "当前主体还没有可作为父级的一级事项",
      stateLabel: "请先新建一级事项", stateClass: "not-started", spentLabel: "", recurrenceLabel: "",
      selectable: false, isMatch: true, depth: 0, kind: "parent-unavailable", unavailableReason: "当前主体还没有一级事项",
    }]);
    return;
  }

  const keyword = dirty.value ? query.trim().toLocaleLowerCase() : "";
  const suggestions: TaskSuggestion[] = activeRows.value
    .filter(({ task }) => !keyword || (task.title + " " + taskPathLabel(task)).toLocaleLowerCase().includes(keyword))
    .map(({ task, depth }) => {
      const descendants = getDescendantTasks(task);
      const spentMinutes = taskSpentMinutes(task, descendants);
      const state = taskState(task, spentMinutes, descendants);
      return {
        value: taskPathLabel(task), label: taskPathLabel(task), pathLabel: task.title,
        stateLabel: state.label, stateClass: state.className, spentLabel: "已用 " + formatSpentMinutes(spentMinutes),
        recurrenceLabel: task.recurrence?.summary ?? "", selectable: task.selectable !== false && descendants.length === 0,
        isMatch: Boolean(keyword), depth, taskId: task.id, kind: "task" as const,
      };
    });
  const title = query.trim();
  const exactSelectable = title && activeRows.value.some(({ task }) => {
    if (getDescendantTasks(task).length || task.selectable === false) return false;
    return task.title.trim() === title || taskPathLabel(task).trim() === title;
  });
  if (title && dirty.value && !exactSelectable) {
    suggestions.push(
      { value: "新建一级事项“" + title + "”", label: title, pathLabel: "新建为一级事项：" + title, stateLabel: "创建", stateClass: "not-started", spentLabel: "", recurrenceLabel: "", selectable: true, isMatch: true, depth: 0, kind: "create-root" },
      { value: "新建为二级事项“" + title + "”", label: title, pathLabel: "新建为二级事项：" + title, stateLabel: "选择一级事项", stateClass: "not-started", spentLabel: "", recurrenceLabel: "", selectable: true, isMatch: true, depth: 0, kind: "choose-parent" },
    );
  }
  if (!suggestions.length && !title) {
    suggestions.push({
      value: "当前主体暂无事项", label: "当前主体暂无事项", pathLabel: "当前主体暂无可选事项",
      stateLabel: "输入名称可新建", stateClass: "not-started", spentLabel: "", recurrenceLabel: "",
      selectable: false, isMatch: true, depth: 0, kind: "parent-unavailable", unavailableReason: "当前主体暂无可选事项",
    });
  }
  callback(suggestions);
}

function handleInput(value: string | number) {
  if (choosingParent.value) choosingParent.value = false;
  inputText.value = String(value);
  const selectedLabel = selectedTask.value ? taskPathLabel(selectedTask.value) : "";
  dirty.value = inputText.value.trim() !== selectedLabel.trim();
}

function clearSelectedTask() {
  inputText.value = "";
  dirty.value = false;
  choosingParent.value = false;
  pendingTitle.value = "";
  activeSubjectId.value = resolveTaskSelectionSubject(store.subjects, undefined, store.selectedSubjectId);
  emit("update:modelValue", "");
}

function openTaskSuggestions(event: MouseEvent) {
  const target = event.target as HTMLElement | null;
  if (target?.closest(".el-input__clear")) return;
  showTaskSuggestions();
}

function showTaskSuggestions() {
  if (autocompleteRef.value) {
    const activated = autocompleteRef.value.activated;
    if (typeof activated === "object" && activated) activated.value = true;
    else autocompleteRef.value.activated = true;
  }
  void autocompleteRef.value?.getData(inputText.value);
}

function closeTaskSuggestions() {
  autocompleteRef.value?.close();
  autocompleteRef.value?.blur();
}

function focusTaskInput() {
  autocompleteRef.value?.focus();
}

function switchSubject(subjectId: string) {
  if (props.busy || subjectId === activeSubjectId.value) return;
  activeSubjectId.value = subjectId;
  if (choosingParent.value) {
    choosingParent.value = false;
    inputText.value = pendingTitle.value;
  }
  window.setTimeout(async () => {
    focusTaskInput();
    showTaskSuggestions();
    await Promise.resolve();
    autocompleteRef.value?.highlight(0);
  });
}

function createDraft(title: string, parentId?: string) {
  try {
    const taskId = store.addTaskSelectionDraft(title, targetSubjectId(), parentId);
    if (!taskId) return;
    emit("update:modelValue", taskId);
    inputText.value = parentId ? (store.tasks.find((task) => task.id === parentId)?.title ?? "") + " / " + title : title;
    dirty.value = false;
    ElMessage.success("已新建事项“" + title + "”");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  }
}

function selectSuggestion(item: TaskSuggestion) {
  if (props.busy) return;
  if (item.kind === "create-root") {
    const title = item.label.trim();
    if (!title) return;
    if (props.intentMode) emit("submit", { newTask: { subjectId: targetSubjectId(), title } });
    else createDraft(title);
    return;
  }
  if (item.kind === "choose-parent") {
    pendingTitle.value = item.label.trim();
    choosingParent.value = true;
    dirty.value = true;
    window.setTimeout(() => {
      inputText.value = pendingTitle.value;
      focusTaskInput();
      showTaskSuggestions();
    }, 80);
    return;
  }
  if (item.kind === "create-child" && item.parentId) {
    const title = pendingTitle.value;
    choosingParent.value = false;
    inputText.value = title;
    if (props.intentMode) emit("submit", { newTask: { subjectId: targetSubjectId(), parentId: item.parentId, expectedParentVersion: item.parentVersion, title } });
    else createDraft(title, item.parentId);
    return;
  }
  if (!item.selectable) {
    inputText.value = lastQuery.trim();
    return;
  }
  dirty.value = false;
  inputText.value = item.label.trim();
  if (!item.taskId) return;
  emit("update:modelValue", item.taskId);
  if (props.intentMode) emit("submit", { taskId: item.taskId });
}

function commitText() {
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
  const existing = activeRows.value.map(({ task }) => task).find((task) => !getDescendantTasks(task).length && task.selectable !== false && (taskPathLabel(task).trim() === title || task.title.trim() === title));
  if (existing) {
    emit("update:modelValue", existing.id);
    dirty.value = false;
    return existing.id;
  }
  ElMessage.warning("请从列表选择事项，或明确选择新建为一级/二级事项");
  if (selectedTask.value) {
    inputText.value = taskPathLabel(selectedTask.value);
    dirty.value = false;
    return selectedTask.value.id;
  }
  return "";
}

function handleEscape() {
  if (!choosingParent.value) return;
  choosingParent.value = false;
  inputText.value = pendingTitle.value;
  window.setTimeout(showTaskSuggestions);
}

defineExpose({ commit: commitText, closeSuggestions: closeTaskSuggestions, focus: focusTaskInput });
</script>

<template>
  <div class="task-select-composite" :class="inputClass">
    <el-autocomplete ref="autocompleteRef" v-model="inputText" class="task-autocomplete-input" :fetch-suggestions="fetchSuggestions" :placeholder="placeholder" :aria-label="ariaLabel" popper-class="task-autocomplete-popper" placement="bottom-start" fit-input-width highlight-first-item :select-when-unmatched="false" clearable :clear-icon="X" @input="handleInput" @click="openTaskSuggestions" @clear="clearSelectedTask" @select="selectSuggestion" @keydown.enter.stop @keydown.esc.stop="handleEscape">
      <template #header>
        <div class="task-subject-tabs" role="tablist" aria-label="筛选事项主体">
          <button v-for="subject in store.subjects" :key="subject.id" type="button" role="tab" :aria-selected="activeSubjectId === subject.id" :class="{ active: activeSubjectId === subject.id }" @mousedown.prevent @click.stop="switchSubject(subject.id)">{{ subject.name }}</button>
        </div>
      </template>
      <template #default="{ item }">
        <span class="task-autocomplete-option" :class="{ 'is-parent': !item.selectable, 'is-match': item.isMatch, 'is-create': item.kind && item.kind !== 'task' }" :title="item.unavailableReason ?? item.label" :aria-disabled="!item.selectable">
          <span v-if="item.kind === 'task'" class="task-option-indent" :style="{ width: String(item.depth * 18) + 'px' }" aria-hidden="true"></span>
          <span class="task-option-name">{{ item.pathLabel }}</span>
          <span v-if="item.recurrenceLabel" class="task-option-repeat">{{ item.recurrenceLabel }}</span>
          <span class="task-option-spent">{{ item.spentLabel }}</span>
          <span class="task-option-state" :class="'is-' + item.stateClass">{{ item.stateLabel }}</span>
        </span>
      </template>
    </el-autocomplete>
  </div>
</template>
