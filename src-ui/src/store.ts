import { computed, reactive, ref } from "vue";
import { defineStore } from "pinia";
import {
  createSubject as createSubjectRecord,
  createTask as createTaskRecord,
  createQuickTask as createQuickTaskRecord,
  changeTaskParent,
  createManualTimeEntry,
  createReport as createReportRecord,
  deleteTaskSubtree,
  discardUnassignedTime,
  duplicateTaskSubtree,
  getTodayWorkOverview,
  getTimerState,
  getUnassignedState,
  listSubjects,
  listTasks,
  listTodayTasks,
  listTimeEntries,
  listReports,
  pauseTimerRecord,
  replaceTimeAllocations,
  regenerateReport as regenerateReportRecord,
  saveReportContent as saveReportContentRecord,
  updateReportScope as updateReportScopeRecord,
  deleteReport as deleteReportRecord,
  getReportTemplate as getReportTemplateRecord,
  getPublicReportText,
  saveReportTemplate as saveReportTemplateRecord,
  previewObsidianReportWrite,
  executeObsidianReportWrite,
  resolveUnassignedBreak,
  resolveUnassignedWork,
  renameSubject as renameSubjectRecord,
  reorderTaskSubtree,
  resumeTimerRecord,
  setTaskCompleted,
  saveTaskRecurrence,
  closeTaskRecurrence,
  setTaskDailyEstimate,
  startTimerRecord,
  stopTimerRecord,
  notifyTimerStopConfirmation,
  updateTask as updateTaskRecord,
  updateTimeEntry,
  updateTimeEntryDisposition,
  type TimeEntryRecord,
  type TaskRecord,
  type TodayWorkOverviewRecord,
  type ReportRecordDto,
  type UnassignedStateRecord,
  type RecurrenceRuleRecord,
} from "./services/tauri";
import { transientMissingStateResult } from "./services/transientStateGuard";

export type TaskStatus = "open" | "done";

export type StatusColors = Record<TaskStatus, string>;

export type Subject = {
  id: string;
  name: string;
  version: number;
  openTaskCount: number;
};

export type Task = {
  id: string;
  subjectId: string;
  parentId?: string;
  title: string;
  project: string;
  planDate?: string;
  estimate?: number;
  todayEstimate?: number;
  actual: number;
  status: TaskStatus;
  note?: string;
  sortOrder: number;
  version: number;
  selectable: boolean;
  executionState: TaskRecord["executionState"];
  recurrence?: RecurrenceRuleRecord;
  occurrenceDate?: string;
  occurrenceOrigin?: "scheduled" | "manual";
  occurrenceVersion?: number;
  quickCreate?: boolean;
};

export type TimeAllocation = {
  id?: string;
  taskId: string;
  minutes: number;
  note?: string;
  completeTask?: boolean;
  taskExpectedVersion?: number;
};

export type TaskCompletionResult = {
  taskId: string;
  subjectId: string;
  direction: "completed" | "reopened";
  completedCount: number;
};

export type BatchCompletionResult = {
  completedCount: number;
  completedTaskIds: string[];
};

export type TimeEntry = {
  id: string;
  label: string;
  startedAt: number;
  endedAt?: number;
  minutes: number;
  durationSeconds: number;
  syncedAt: number;
  allocated: number;
  defaultTask?: string;
  allocations?: TimeAllocation[];
  note: string;
  kind?: "work" | "break";
  sourceType: string;
  state: "running" | "paused" | "ended";
  version: number;
};

export type UnassignedTimeAction = "work" | "break" | "discard";

export type ReportType = "daily" | "weekly" | "monthly";

export type ReportRecord = {
  id: string;
  type: ReportType;
  subjectId: string;
  subjectName: string;
  period: string;
  referenceDate: string;
  taskIds: string[];
  markdown: string;
  contentSource: "generated" | "edited";
  generatedCount: number;
  updatedAt: number;
  version: number;
};

const defaultSubjectId = "subject-default";
const initialSubjects: Subject[] = [
  { id: defaultSubjectId, name: "默认", version: 1, openTaskCount: 0 }
];

const initialTasks: Task[] = [];

const defaultStatusColors: StatusColors = {
  open: "#91a098",
  done: "#34c584"
};

export const useWorkdayStore = defineStore("workday", () => {
  const activePage = ref("today");
  const subjects = reactive<Subject[]>(initialSubjects.map((subject) => ({ ...subject })));
  const selectedSubjectId = ref(defaultSubjectId);
  const tasks = reactive<Task[]>(initialTasks);
  const todayTasks = reactive<Task[]>([]);
  const statusColors = reactive<StatusColors>({ ...defaultStatusColors });
  const entries = reactive<TimeEntry[]>([]);
  const selectedTaskId = ref("");
  const selectedEntryId = ref("");
  const reports = ref<ReportRecord[]>([]);
  const todayOverview = ref<TodayWorkOverviewRecord | null>(null);
  const todayOverviewLoading = ref(false);
  const todayOverviewSubjectId = ref("");
  const now = ref(Date.now());
  const unassignedStartedAt = ref<number>();
  const unassignedAccumulatedSeconds = ref(0);
  const unassignedFirstStartedAt = ref<number>();
  const unassignedLastEndedAt = ref<number>();
  const unassignedDialogOpen = ref(false);
  const timerStopConfirmationEntryId = ref("");
  const unassignedSessionId = ref("");
  const unassignedVersion = ref(0);
  const unassignedThresholdSeconds = ref(5 * 60);
  let clock: number | undefined;
  let timeRefreshClock: number | undefined;
  let timeDataRequestSeq = 0;
  let timeDataMutationSeq = 0;
  let unassignedStateRequestSeq = 0;
  let unassignedStateMutationSeq = 0;
  let timerStateMissingRefreshes = 0;
  let unassignedStateMissingRefreshes = 0;
  let todayOverviewRequestSeq = 0;
  let workspaceRequestSeq = 0;
  const savingTaskIds = new Set<string>();
  const workspaceLoaded = ref(false);
  const workspaceLoading = ref(false);
  const todayMinutes = computed(() => entries.reduce((sum, entry) => {
    if (entry.kind === "break") return sum;
    return sum + Math.ceil(entryDurationSeconds(entry) / 60);
  }, 0));
  const allocatedMinutes = computed(() => entries.reduce((sum, entry) => sum + entry.allocated, 0));
  const pendingMinutes = computed(() => Math.max(0, todayMinutes.value - allocatedMinutes.value));
  const runningEntry = computed(() => entries.find((entry) => entry.state === "running" || entry.state === "paused"));
  const selectedEntry = computed(() => entries.find((entry) => entry.id === selectedEntryId.value));
  const timerStopConfirmationEntry = computed(() => entries.find((entry) => entry.id === timerStopConfirmationEntryId.value && entry.state === "ended"));
  const doneCount = computed(() => tasks.filter((task) => task.status === "done").length);
  const unassignedSeconds = computed(() => {
    const liveSeconds = unassignedStartedAt.value
      ? Math.max(0, Math.floor((now.value - unassignedStartedAt.value) / 1_000))
      : 0;
    return unassignedAccumulatedSeconds.value + liveSeconds;
  });
  const visibleTasks = computed(() => {
    const childMap = new Map<string, Task[]>();
    const roots: Task[] = [];
    for (const task of tasks) {
      if (task.parentId) {
        const children = childMap.get(task.parentId) ?? [];
        children.push(task);
        childMap.set(task.parentId, children);
      } else {
        roots.push(task);
      }
    }
    const flatten = (task: Task): Task[] => [task, ...(childMap.get(task.id) ?? []).flatMap(flatten)];
    return roots.flatMap(flatten);
  });
  const selectedSubject = computed(() => subjects.find((subject) => subject.id === selectedSubjectId.value) ?? subjects[0]);
  const selectedSubjectTasks = computed(() => buildVisibleTaskList(tasks.filter((task) => task.subjectId === selectedSubjectId.value)));
  const todayOverviewSubject = computed(() => {
    if (!todayOverviewSubjectId.value) return undefined;
    return subjects.find((subject) => subject.id === todayOverviewSubjectId.value);
  });

  function startClock(trackUnassigned = true) {
    void initializeWorkspace(trackUnassigned);
    if (!clock) {
      clock = window.setInterval(() => {
        now.value = Date.now();
      }, 1_000);
    }
    if (!timeRefreshClock) {
      timeRefreshClock = window.setInterval(() => void Promise.all([loadTimeData(), loadUnassignedState()]).then(() => loadTodayOverview()), 5_000);
    }
  }

  function stopClock() {
    if (clock) window.clearInterval(clock);
    clock = undefined;
    if (timeRefreshClock) window.clearInterval(timeRefreshClock);
    timeRefreshClock = undefined;
  }

  async function initializeWorkspace(trackUnassigned: boolean) {
    await Promise.all([loadWorkspaceData(), loadTimeData(), loadReports(), trackUnassigned ? loadUnassignedState() : Promise.resolve()]);
    await loadTodayOverview();
  }

  async function loadWorkspaceData() {
    const requestSeq = ++workspaceRequestSeq;
    workspaceLoading.value = true;
    try {
      const todayDate = formatLocalDate(new Date());
      const [subjectRecords, taskResult, todayTaskResult] = await Promise.all([
        listSubjects(),
        listTasks(),
        listTodayTasks(undefined, todayDate),
      ]);
      if (requestSeq !== workspaceRequestSeq) return;
      subjects.splice(0, subjects.length, ...subjectRecords.map((subject) => ({
        id: subject.id,
        name: subject.name,
        version: subject.version,
        openTaskCount: subject.openTaskCount,
      })));
      // Keep unsaved draft rows while background refreshes replace persisted data.
      const unsavedTasks = tasks.filter((task) => task.version <= 0);
      const loadedTasks = taskResult.tasks.map(toUiTask);
      const loadedIds = new Set(loadedTasks.map((task) => task.id));
      tasks.splice(0, tasks.length, ...loadedTasks, ...unsavedTasks.filter((task) => !loadedIds.has(task.id)));
      todayTasks.splice(0, todayTasks.length, ...todayTaskResult.tasks.map(toUiTask));
      const fallbackSubjectId = subjects[0]?.id ?? "";
      if (!subjects.some((subject) => subject.id === selectedSubjectId.value)) selectedSubjectId.value = fallbackSubjectId;
      if (todayOverviewSubjectId.value && !subjects.some((subject) => subject.id === todayOverviewSubjectId.value)) todayOverviewSubjectId.value = fallbackSubjectId;
      if (!todayOverviewSubjectId.value && selectedSubjectId.value) todayOverviewSubjectId.value = selectedSubjectId.value;
      if (!tasks.some((task) => task.id === selectedTaskId.value)) {
        selectedTaskId.value = buildVisibleTaskList(tasks.filter((task) => task.subjectId === selectedSubjectId.value))[0]?.id ?? "";
      }
      workspaceLoaded.value = true;
    } catch (error) {
      console.error("加载工作台数据失败", error);
    } finally {
      if (requestSeq === workspaceRequestSeq) workspaceLoading.value = false;
    }
  }

  function toUiTask(task: TaskRecord): Task {
    return {
      id: task.id,
      subjectId: task.subjectId,
      parentId: task.parentId,
      title: task.title,
      project: task.projectName ?? "",
      planDate: task.plannedDate,
      estimate: task.estimateMinutes,
      todayEstimate: task.todayEstimateMinutes,
      actual: task.totalMinutes,
      status: task.status,
      note: task.note,
      sortOrder: task.sortOrder,
      version: task.version,
      selectable: task.selectable,
      executionState: task.executionState,
      recurrence: task.recurrence,
      occurrenceDate: task.occurrenceDate,
      occurrenceOrigin: task.occurrenceOrigin,
      occurrenceVersion: task.occurrenceVersion,
      quickCreate: false,
    };
  }

  function toUiTimeEntry(entry: TimeEntryRecord): TimeEntry {
    const existing = entries.find((item) => item.id === entry.id);
    const syncedAt = Date.now();
    const durationSeconds = existing?.state === "running" && entry.state === "running"
      ? Math.max(entry.durationSeconds, entryDurationSecondsAt(existing, syncedAt))
      : entry.durationSeconds;
    return {
      id: entry.id,
      label: entry.label,
      startedAt: entry.startedAt,
      endedAt: entry.endedAt,
      minutes: entry.settlementMinutes,
      durationSeconds,
      syncedAt,
      allocated: entry.allocatedMinutes,
      defaultTask: entry.defaultTaskId,
      allocations: entry.allocations.map((allocation) => ({
        id: allocation.id,
        taskId: allocation.taskId,
        minutes: allocation.minutes,
        note: allocation.note,
      })),
      note: entry.note ?? "",
      kind: entry.kind,
      sourceType: entry.sourceType,
      state: entry.state,
      version: entry.version,
    };
  }

  function entryDurationSeconds(entry: TimeEntry) {
    return entryDurationSecondsAt(entry, now.value);
  }

  function entryDurationSecondsAt(entry: TimeEntry, timestamp: number) {
    const liveSeconds = entry.state === "running"
      ? Math.max(0, Math.floor((timestamp - entry.syncedAt) / 1_000))
      : 0;
    return entry.durationSeconds + liveSeconds;
  }

  function taskPathLabel(task: Task) {
    const path = [task.title];
    const visited = new Set([task.id]);
    let parentId = task.parentId;
    while (parentId && !visited.has(parentId)) {
      visited.add(parentId);
      const parent = tasks.find((item) => item.id === parentId);
      if (!parent) break;
      path.unshift(parent.title);
      parentId = parent.parentId;
    }
    return path.join(" / ");
  }

  function taskDisplayLabel(taskId?: string, fallback = "未关联事项") {
    if (!taskId) return fallback;
    const task = tasks.find((item) => item.id === taskId);
    return task ? taskPathLabel(task) : fallback;
  }

  function timeEntryLabel(entry: TimeEntry) {
    return taskDisplayLabel(entry.defaultTask, entry.label);
  }

  async function loadTimeData() {
    const requestSeq = ++timeDataRequestSeq;
    const mutationSeq = timeDataMutationSeq;
    try {
      const previousActiveEntry = runningEntry.value ? cloneTimeEntry(runningEntry.value) : undefined;
      const workDate = formatLocalDate(new Date());
      const [result, activeEntry] = await Promise.all([listTimeEntries(workDate), getTimerState()]);
      if (requestSeq !== timeDataRequestSeq || mutationSeq !== timeDataMutationSeq) return;
      const records = result.entries.slice();
      if (activeEntry) {
        const activeIndex = records.findIndex((entry) => entry.id === activeEntry.id);
        if (activeIndex >= 0) records.splice(activeIndex, 1, activeEntry);
        else records.unshift(activeEntry);
      }
      const nextEntries = records.map(toUiTimeEntry);
      const previousActiveStillPresent = previousActiveEntry
        ? nextEntries.some((entry) => entry.id === previousActiveEntry.id)
        : false;
      const activeStateStillPresent = Boolean(activeEntry) || previousActiveStillPresent;
      if (!activeEntry && previousActiveEntry && !previousActiveStillPresent) {
        const guard = transientMissingStateResult(true, false, timerStateMissingRefreshes);
        timerStateMissingRefreshes = guard.missingRefreshes;
        if (guard.preservePrevious) {
          nextEntries.unshift(previousActiveEntry);
        }
      } else {
        timerStateMissingRefreshes = transientMissingStateResult(Boolean(previousActiveEntry), activeStateStillPresent, timerStateMissingRefreshes).missingRefreshes;
      }
      entries.splice(0, entries.length, ...nextEntries);
      if (timerStopConfirmationEntryId.value && !entries.some((entry) => entry.id === timerStopConfirmationEntryId.value)) {
        timerStopConfirmationEntryId.value = "";
      }
      if (!entries.some((entry) => entry.id === selectedEntryId.value)) {
        selectedEntryId.value = entries[0]?.id ?? "";
      }
    } catch (error) {
      console.error("加载计时数据失败", error);
    }
  }

  async function loadTodayOverview(subjectId = todayOverviewSubjectId.value) {
    const requestSeq = todayOverviewRequestSeq + 1;
    todayOverviewRequestSeq = requestSeq;
    todayOverviewLoading.value = true;
    try {
      const todayDate = formatLocalDate(new Date());
      const scopedSubjectId = subjectId && subjects.some((subject) => subject.id === subjectId) ? subjectId : "";
      todayOverviewSubjectId.value = scopedSubjectId;
      const result = await getTodayWorkOverview({
        subjectId: scopedSubjectId || undefined,
        todayDate,
        historyDays: 30,
        trendDays: 14,
      });
      if (requestSeq === todayOverviewRequestSeq) todayOverview.value = result;
    } catch (error) {
      console.error("加载今日概览失败", error);
    } finally {
      if (requestSeq === todayOverviewRequestSeq) todayOverviewLoading.value = false;
    }
  }

  async function selectTodayOverviewSubject(subjectId: string) {
    todayOverviewSubjectId.value = subjectId;
    await loadTodayOverview(subjectId);
  }

  async function loadReports() {
    try {
      const result = await listReports();
      reports.value = result.reports.map(toUiReport);
    } catch (error) {
      console.error("加载报告失败", error);
    }
  }

  function toUiReport(report: ReportRecordDto): ReportRecord {
    return {
      id: report.id,
      type: report.reportType,
      subjectId: report.subjectId,
      subjectName: report.subjectName,
      period: report.period,
      referenceDate: report.referenceDate,
      taskIds: report.taskIds,
      markdown: report.markdown,
      contentSource: report.contentSource,
      generatedCount: report.generatedCount,
      updatedAt: report.updatedAt,
      version: report.version,
    };
  }

  async function loadUnassignedState() {
    const requestSeq = ++unassignedStateRequestSeq;
    const mutationSeq = unassignedStateMutationSeq;
    try {
      const state = await getUnassignedState();
      if (requestSeq !== unassignedStateRequestSeq || mutationSeq !== unassignedStateMutationSeq) return;
      applyUnassignedState(state);
    } catch (error) {
      console.error("加载未归属时间失败", error);
    }
  }

  function markTimeDataChanged() {
    timeDataMutationSeq += 1;
    timerStateMissingRefreshes = 0;
  }

  function markUnassignedStateChanged() {
    unassignedStateMutationSeq += 1;
    unassignedStateMissingRefreshes = 0;
  }

  function applyUnassignedState(state: UnassignedStateRecord | null) {
    const syncedAt = Date.now();
    if (!state) {
      if (unassignedSessionId.value) {
        const guard = transientMissingStateResult(true, false, unassignedStateMissingRefreshes);
        unassignedStateMissingRefreshes = guard.missingRefreshes;
        if (guard.preservePrevious) {
          now.value = syncedAt;
          return;
        }
      }
      resetUnassignedTracking();
      return;
    }
    unassignedStateMissingRefreshes = 0;
    const previousSeconds = unassignedSessionId.value === state.sessionId ? unassignedSeconds.value : 0;
    const liveSeconds = state.currentSegmentStartedAt
      ? Math.max(0, Math.floor((syncedAt - state.currentSegmentStartedAt) / 1_000))
      : 0;
    const elapsedSeconds = Math.max(state.elapsedSeconds, previousSeconds);
    unassignedSessionId.value = state.sessionId;
    unassignedVersion.value = state.version;
    unassignedThresholdSeconds.value = state.thresholdSeconds;
    unassignedFirstStartedAt.value = state.firstStartedAt;
    unassignedLastEndedAt.value = state.lastEndedAt;
    unassignedStartedAt.value = state.currentSegmentStartedAt;
    unassignedAccumulatedSeconds.value = Math.max(0, elapsedSeconds - liveSeconds);
  }

  function replaceTask(record: TaskRecord) {
    const index = tasks.findIndex((task) => task.id === record.id);
    const next = toUiTask(record);
    if (index >= 0) tasks.splice(index, 1, next);
    else tasks.push(next);
    return next;
  }

  function selectPage(page: string) {
    activePage.value = page;
  }

  function selectSubject(subjectId: string) {
    if (!subjects.some((subject) => subject.id === subjectId)) return;
    selectedSubjectId.value = subjectId;
    activePage.value = "plan";
    const subjectTasks = buildVisibleTaskList(tasks.filter((task) => task.subjectId === subjectId));
    if (!subjectTasks.some((task) => task.id === selectedTaskId.value)) selectedTaskId.value = subjectTasks[0]?.id ?? "";
  }

  async function addSubject(name: string) {
    const normalizedName = name.trim();
    if (!normalizedName) return;
    const result = await createSubjectRecord(normalizedName);
    const existingIndex = subjects.findIndex((subject) => subject.id === result.subject.id);
    const subject = { id: result.subject.id, name: result.subject.name, version: result.subject.version, openTaskCount: result.subject.openTaskCount };
    if (existingIndex >= 0) subjects.splice(existingIndex, 1, subject);
    else subjects.push(subject);
    selectSubject(subject.id);
    return subject.id;
  }

  async function renameSubject(subjectId: string, name: string) {
    const subject = subjects.find((item) => item.id === subjectId);
    const normalizedName = name.trim();
    if (!subject || !normalizedName) return false;
    if (subjects.some((item) => item.id !== subjectId && item.name === normalizedName)) return false;
    const updated = await renameSubjectRecord(subjectId, normalizedName, subject.version);
    subject.name = updated.name;
    subject.version = updated.version;
    subject.openTaskCount = updated.openTaskCount;
    await loadReports();
    await loadTodayOverview();
    return true;
  }

  function beginUnassignedTracking() {
    void loadUnassignedState();
  }

  function pauseUnassignedTracking() {
    void loadUnassignedState();
  }

  function promptUnassignedResolution(force = false) {
    const checkedAt = Date.now();
    now.value = checkedAt;
    const liveSeconds = unassignedStartedAt.value
      ? Math.max(0, Math.floor((checkedAt - unassignedStartedAt.value) / 1_000))
      : 0;
    const totalSeconds = unassignedAccumulatedSeconds.value + liveSeconds;
    if (unassignedDialogOpen.value || totalSeconds < 1) return false;
    if (!force && totalSeconds <= unassignedThresholdSeconds.value) return false;
    unassignedDialogOpen.value = true;
    return true;
  }

  function resetUnassignedTracking() {
    markUnassignedStateChanged();
    unassignedStartedAt.value = undefined;
    unassignedAccumulatedSeconds.value = 0;
    unassignedFirstStartedAt.value = undefined;
    unassignedLastEndedAt.value = undefined;
    unassignedSessionId.value = "";
    unassignedVersion.value = 0;
    unassignedStateMissingRefreshes = 0;
  }

  async function commitTaskToggle(task: Task): Promise<TaskCompletionResult | undefined> {
    if (task.version <= 0) return;
    const direction = task.status === "done" ? "reopened" : "completed";
    const todayTask = todayTasks.find((item) => item.id === task.id);
    const occurrenceDate = task.occurrenceDate
      ?? todayTask?.occurrenceDate
      ?? (task.recurrence ? formatLocalDate(new Date()) : undefined);
    const occurrenceVersion = task.occurrenceVersion ?? todayTask?.occurrenceVersion;
    const updated = await setTaskCompleted(
      task.id,
      task.version,
      direction === "completed",
      occurrenceDate,
      occurrenceVersion,
    );
    return {
      taskId: task.id,
      subjectId: task.subjectId,
      direction,
      completedCount: direction === "completed" && updated.status === "done" ? 1 : 0,
    };
  }

  async function toggleTask(task: Task) {
    const result = await commitTaskToggle(task);
    if (!result) return;
    await loadWorkspaceData();
    await loadTodayOverview();
    return result;
  }

  async function setTaskRecurrence(
    taskId: string,
    recurrence?: { frequency: RecurrenceRuleRecord["frequency"]; weekdaysMask?: number; effectiveStart: string },
  ) {
    const task = tasks.find((item) => item.id === taskId);
    if (!task || task.version <= 0) return;
    if (!recurrence) {
      if (!task.recurrence) return;
      await closeTaskRecurrence({
        taskId: task.id,
        taskExpectedVersion: task.version,
        effectiveEnd: formatLocalDate(new Date()),
        ruleExpectedVersion: task.recurrence.version,
      });
    } else {
      await saveTaskRecurrence({
        taskId: task.id,
        taskExpectedVersion: task.version,
        frequency: recurrence.frequency,
        weekdaysMask: recurrence.frequency === "weekly" ? recurrence.weekdaysMask : undefined,
        effectiveStart: recurrence.effectiveStart,
        ruleExpectedVersion: task.recurrence?.version,
      });
    }
    await loadWorkspaceData();
    await loadTodayOverview();
  }

  function addTask(title = "", subjectId = selectedSubjectId.value) {
    const subject = subjects.find((item) => item.id === subjectId) ?? selectedSubject.value;
    const task: Task = { id: createTaskId(), subjectId: subject?.id ?? defaultSubjectId, title, project: subject?.name ?? "默认", actual: 0, status: "open", sortOrder: Number.MAX_SAFE_INTEGER, version: 0, selectable: true, executionState: "not_started" };
    tasks.push(task);
    selectedTaskId.value = task.id;
    return task.id;
  }

  function addChildTask(parentId: string, title = "") {
    const parent = tasks.find((task) => task.id === parentId);
    if (!parent) return;
    if (parent.recurrence) throw new Error("重复事项新增子项前必须先关闭重复");
    const child: Task = {
      id: createTaskId(),
      subjectId: parent.subjectId,
      parentId,
      title,
      project: parent.project,
      planDate: parent.planDate,
      actual: 0,
      status: "open",
      sortOrder: Number.MAX_SAFE_INTEGER,
      version: 0,
      selectable: true,
      executionState: "not_started"
    };
    const parentIndex = tasks.findIndex((task) => task.id === parentId);
    let insertIndex = parentIndex + 1;
    const descendantIds = getDescendantIds(parentId);
    while (insertIndex < tasks.length && descendantIds.has(tasks[insertIndex].id)) insertIndex += 1;
    tasks.splice(insertIndex, 0, child);
    selectedTaskId.value = child.id;
    return child.id;
  }

  async function deleteTask(taskId: string) {
    const deletedTask = tasks.find((task) => task.id === taskId);
    if (deletedTask?.version) await deleteTaskSubtree(deletedTask.id, deletedTask.version);
    const idsToDelete = getSubtreeIds(taskId);
    for (let index = tasks.length - 1; index >= 0; index -= 1) {
      if (idsToDelete.has(tasks[index].id)) tasks.splice(index, 1);
    }
    if (idsToDelete.has(selectedTaskId.value)) {
      const subjectId = deletedTask?.subjectId ?? selectedSubjectId.value;
      selectedTaskId.value = buildVisibleTaskList(tasks.filter((task) => task.subjectId === subjectId))[0]?.id ?? "";
    }
    if (deletedTask?.version) await loadWorkspaceData();
    if (deletedTask?.version) await loadTodayOverview();
  }

  async function saveTask(taskId: string): Promise<string | false> {
    const task = tasks.find((item) => item.id === taskId);
    if (!task || !task.title.trim()) return false;
    if (savingTaskIds.has(taskId)) return false;
    savingTaskIds.add(taskId);
    try {
      if (task.version <= 0) {
        const create = task.quickCreate ? createQuickTaskRecord : createTaskRecord;
        const created = await create({
          subjectId: task.subjectId,
          parentId: task.parentId,
          title: task.title,
          plannedDate: task.planDate,
          estimateMinutes: task.estimate,
          note: task.note,
          projectName: task.project,
        });
        const oldId = task.id;
        let index = tasks.findIndex((item) => item.id === oldId);
        const existingCreatedIndex = tasks.findIndex((item) => item.id === created.id);
        if (existingCreatedIndex >= 0 && existingCreatedIndex !== index) {
          tasks.splice(existingCreatedIndex, 1);
          if (existingCreatedIndex < index) index -= 1;
        }
        if (index >= 0) tasks.splice(index, 1, toUiTask(created));
        else tasks.push(toUiTask(created));
        for (const child of tasks) if (child.parentId === oldId) child.parentId = created.id;
        if (selectedTaskId.value === oldId) selectedTaskId.value = created.id;
        await setTaskDailyEstimate(created.id, formatLocalDate(new Date()), task.todayEstimate);
        await Promise.all([loadWorkspaceData(), loadTimeData(), loadReports()]);
        await loadTodayOverview();
        return created.id;
      }
      await updateTaskRecord({
        id: task.id,
        expectedVersion: task.version,
        title: task.title,
        plannedDate: task.planDate ?? null,
        estimateMinutes: task.estimate ?? null,
        note: task.note ?? null,
        projectName: task.project || null,
      });
      await setTaskDailyEstimate(task.id, formatLocalDate(new Date()), task.todayEstimate);
      await Promise.all([loadWorkspaceData(), loadTimeData(), loadReports()]);
      await loadTodayOverview();
      return task.id;
    } finally {
      savingTaskIds.delete(taskId);
    }
  }

  function addQuickTask(title = "", subjectId = selectedSubjectId.value) {
    const taskId = addTask(title, subjectId);
    const task = tasks.find((item) => item.id === taskId);
    if (task) task.quickCreate = true;
    return taskId;
  }

  async function duplicateTask(taskId: string) {
    const ordered = buildVisibleTaskList(tasks);
    const source = ordered.find((task) => task.id === taskId);
    if (!source) return;
    if (source.version <= 0) return;
    const result = await duplicateTaskSubtree(source.id, source.version);
    await loadWorkspaceData();
    await loadTodayOverview();
    selectedTaskId.value = result.rootId;
    return result.rootId;
  }

  function moveTask(taskId: string, targetId: string, placement: "before" | "after" | "inside", parentId?: string) {
    if (taskId === targetId) return;
    const moving = tasks.find((task) => task.id === taskId);
    const target = tasks.find((task) => task.id === targetId);
    if (!moving || !target) return;
    const movingIds = getSubtreeIds(taskId);
    if (movingIds.has(targetId)) return;

    const ordered = buildVisibleTaskList(tasks);
    const movingBlock = ordered.filter((task) => movingIds.has(task.id));
    const remaining = ordered.filter((task) => !movingIds.has(task.id));
    const targetIndex = remaining.findIndex((task) => task.id === targetId);
    if (targetIndex < 0) return;

    moving.parentId = placement === "inside" ? targetId : parentId;
    const insertIndex = placement === "before" ? targetIndex : findSubtreeEndIndex(remaining, targetId);
    tasks.splice(0, tasks.length, ...remaining.slice(0, insertIndex), ...movingBlock, ...remaining.slice(insertIndex));
    selectedTaskId.value = taskId;
  }

  async function reorderTask(taskId: string, beforeId?: string, afterId?: string, parentId?: string) {
    if (!beforeId && !afterId) return;
    const ordered = buildVisibleTaskList(tasks);
    const moving = ordered.find((task) => task.id === taskId);
    if (!moving) return;
    const target = beforeId ? ordered.find((task) => task.id === beforeId) : ordered.find((task) => task.id === afterId);
    if (!target) return;
    const movingIds = getSubtreeIds(taskId);
    if (movingIds.has(target.id)) return;
    if (parentId && movingIds.has(parentId)) return;
    const nextParentId = moving.parentId;
    const siblings = ordered.filter((task) => task.parentId === nextParentId && task.id !== moving.id);
    const targetSiblingIndex = siblings.findIndex((task) => task.id === target.id);
    const insertionIndex = Math.max(0, beforeId ? targetSiblingIndex + 1 : targetSiblingIndex);
    const previous = siblings[insertionIndex - 1];
    const next = siblings[insertionIndex];
    const sortOrder = previous && next
      ? Math.floor((previous.sortOrder + next.sortOrder) / 2)
      : previous ? previous.sortOrder + 10 : next ? next.sortOrder - 10 : 10;
    moveTask(taskId, target.id, beforeId ? "after" : "before", nextParentId);
    replaceTask(await reorderTaskSubtree(taskId, moving.version, nextParentId, sortOrder));
  }

  async function indentTask(taskId: string) {
    const ordered = buildVisibleTaskList(tasks);
    const index = ordered.findIndex((task) => task.id === taskId);
    const moving = ordered[index];
    if (!moving || index <= 0) return;
    const currentDepth = getTaskDepth(moving, ordered);
    const newParent = ordered.slice(0, index).reverse().find((task) => getTaskDepth(task, ordered) === currentDepth && task.parentId === moving.parentId);
    if (!newParent) return;
    if (moving.version <= 0) return;
    replaceTask(await changeTaskParent(moving.id, moving.version, newParent.id));
    await loadWorkspaceData();
  }

  async function outdentTask(taskId: string) {
    const ordered = buildVisibleTaskList(tasks);
    const moving = ordered.find((task) => task.id === taskId);
    if (!moving?.parentId) return;
    const parent = ordered.find((task) => task.id === moving.parentId);
    if (!parent) return;
    if (moving.version <= 0) return;
    replaceTask(await changeTaskParent(moving.id, moving.version, parent.parentId));
    await loadWorkspaceData();
  }

  function getTaskDepth(task: Task, source: Task[]) {
    let depth = 0;
    let parentId = task.parentId;
    while (parentId && depth < 8) {
      const parent = source.find((item) => item.id === parentId);
      if (!parent) break;
      depth += 1;
      parentId = parent.parentId;
    }
    return depth;
  }

  function createTaskId() {
    return `task-${Date.now()}-${Math.random().toString(16).slice(2, 6)}`;
  }

  function getSubtreeIds(taskId: string) {
    const ids = new Set([taskId]);
    let changed = true;
    while (changed) {
      changed = false;
      for (const task of tasks) {
        if (task.parentId && ids.has(task.parentId) && !ids.has(task.id)) {
          ids.add(task.id);
          changed = true;
        }
      }
    }
    return ids;
  }

  function getDescendantIds(taskId: string) {
    const ids = getSubtreeIds(taskId);
    ids.delete(taskId);
    return ids;
  }

  function findSubtreeEndIndex(list: Task[], taskId: string) {
    const ids = new Set([taskId]);
    let changed = true;
    while (changed) {
      changed = false;
      for (const task of list) {
        if (task.parentId && ids.has(task.parentId) && !ids.has(task.id)) {
          ids.add(task.id);
          changed = true;
        }
      }
    }
    let endIndex = list.findIndex((task) => task.id === taskId) + 1;
    for (let index = endIndex; index < list.length; index += 1) {
      if (ids.has(list[index].id)) endIndex = index + 1;
    }
    return endIndex;
  }

  function buildVisibleTaskList(source: Task[]) {
    const childMap = new Map<string, Task[]>();
    const roots: Task[] = [];
    for (const task of source) {
      if (task.parentId) {
        const children = childMap.get(task.parentId) ?? [];
        children.push(task);
        childMap.set(task.parentId, children);
      } else {
        roots.push(task);
      }
    }
    const flatten = (task: Task): Task[] => [task, ...(childMap.get(task.id) ?? []).flatMap(flatten)];
    return roots.flatMap(flatten);
  }

  async function persistTimerTask(taskId?: string) {
    if (!taskId) return undefined;
    const task = tasks.find((item) => item.id === taskId);
    if (!task) return undefined;
    if (task.version > 0) return task.id;
    const savedId = await saveTask(task.id);
    return savedId || undefined;
  }

  async function startTimer(taskId = selectedTaskId.value, note = "") {
    if (runningEntry.value) return false;
    const persistedTaskId = await persistTimerTask(taskId);
    if (taskId && !persistedTaskId) throw new Error("计时事项保存失败");
    const entry = toUiTimeEntry(await startTimerRecord(persistedTaskId, note.trim() || undefined));
    markTimeDataChanged();
    markUnassignedStateChanged();
    entries.unshift(entry);
    selectedTaskId.value = persistedTaskId ?? "";
    selectedEntryId.value = entry.id;
    await loadWorkspaceData();
    await loadUnassignedState();
    await loadTodayOverview();
    return true;
  }

  async function pauseTimer() {
    const entry = runningEntry.value;
    if (!entry) return;
    markTimeDataChanged();
    replaceTimeEntry(await pauseTimerRecord(entry.id, entry.version));
    await loadWorkspaceData();
    await loadTodayOverview();
  }

  async function resumeTimer() {
    const entry = runningEntry.value;
    if (entry?.state === "paused") {
      markTimeDataChanged();
      replaceTimeEntry(await resumeTimerRecord(entry.id, entry.version));
      await loadWorkspaceData();
      await loadTodayOverview();
    }
  }

  async function stopTimer() {
    const entry = runningEntry.value;
    if (!entry) return;
    markTimeDataChanged();
    const stopped = replaceTimeEntry(await stopTimerRecord(entry.id, entry.version, false));
    markUnassignedStateChanged();
    selectedEntryId.value = stopped.id;
    timerStopConfirmationEntryId.value = stopped.id;
    await notifyTimerStopConfirmation(stopped.id);
    await loadWorkspaceData();
    await loadUnassignedState();
    await loadTodayOverview();
    return stopped.id;
  }

  async function openTimerStopConfirmation(entryId: string) {
    timerStopConfirmationEntryId.value = entryId;
    selectedEntryId.value = entryId;
    await loadTimeData();
    await loadWorkspaceData();
    await loadTodayOverview();
  }

  function dismissTimerStopConfirmation() {
    timerStopConfirmationEntryId.value = "";
  }

  function adjustTimerStopAllocation(entryId = timerStopConfirmationEntryId.value) {
    if (!entryId) return;
    selectedEntryId.value = entryId;
    activePage.value = "timer";
    timerStopConfirmationEntryId.value = "";
  }

  async function confirmTimerStopAllocation(completeTask = false) {
    const entry = timerStopConfirmationEntry.value;
    if (!entry) return false;
    if (!entry.defaultTask || entry.minutes <= 0) return false;
    const result = await allocate(entry.id, [{
      taskId: entry.defaultTask,
      minutes: entry.minutes,
      completeTask,
    }]);
    timerStopConfirmationEntryId.value = "";
    return result ?? { completedCount: 0, completedTaskIds: [] };
  }

  async function resolveTimerStopDisposition(disposition: "break" | "discard") {
    const entry = timerStopConfirmationEntry.value;
    if (!entry) return false;
    await setTimeEntryDisposition(entry.id, disposition);
    timerStopConfirmationEntryId.value = "";
    return true;
  }

  async function allocate(entryId: string, allocations: TimeAllocation[]): Promise<BatchCompletionResult | undefined> {
    const entry = entries.find((item) => item.id === entryId);
    if (!entry) return;
    const normalized: Array<{ taskId: string; minutes: number; note?: string; completeTask: boolean; taskExpectedVersion?: number }> = [];
    for (const item of allocations) {
      const minutes = Math.max(0, Math.round(Number(item.minutes) || 0));
      if (!item.taskId || minutes <= 0) continue;
      const taskId = await persistTimerTask(item.taskId);
      if (!taskId) throw new Error("分配事项保存失败");
      const task = tasks.find((candidate) => candidate.id === taskId);
      normalized.push({
        taskId,
        minutes,
        note: item.note,
        completeTask: Boolean(item.completeTask),
        taskExpectedVersion: item.completeTask ? task?.version : undefined,
      });
    }
    const completionCandidates = new Set(normalized
      .filter((allocation) => allocation.completeTask && tasks.find((task) => task.id === allocation.taskId)?.status !== "done")
      .map((allocation) => allocation.taskId));
    markTimeDataChanged();
    replaceTimeEntry(await replaceTimeAllocations(entry.id, entry.version, normalized));
    await loadWorkspaceData();
    await loadTodayOverview();
    const completedTaskIds = [...completionCandidates].filter((taskId) => tasks.find((task) => task.id === taskId)?.status === "done");
    return { completedCount: completedTaskIds.length, completedTaskIds };
  }

  async function addManualEntry(taskId: string | undefined, minutes: number, note = "", completeTask = false) {
    const persistedTaskId = await persistTimerTask(taskId);
    if (taskId && !persistedTaskId) throw new Error("补录事项保存失败");
    const completionCandidate = completeTask && persistedTaskId && tasks.find((task) => task.id === persistedTaskId)?.status !== "done"
      ? persistedTaskId
      : undefined;
    const task = persistedTaskId ? tasks.find((candidate) => candidate.id === persistedTaskId) : undefined;
    const entry = toUiTimeEntry(await createManualTimeEntry({
      taskId: persistedTaskId,
      workDate: formatLocalDate(new Date()),
      minutes: Math.round(minutes),
      note: note.trim() || undefined,
      completeTask,
      taskExpectedVersion: completeTask ? task?.version : undefined,
    }));
    markTimeDataChanged();
    entries.unshift(entry);
    selectedEntryId.value = entry.id;
    await loadWorkspaceData();
    await loadTodayOverview();
    const completedTaskIds = completionCandidate && tasks.find((item) => item.id === completionCandidate)?.status === "done"
      ? [completionCandidate]
      : [];
    return { entryId: entry.id, completedCount: completedTaskIds.length, completedTaskIds };
  }

  async function correctTimeEntry(entryId: string, startedAt: number, endedAt: number, note = "") {
    const entry = entries.find((item) => item.id === entryId);
    if (!entry) return;
    markTimeDataChanged();
    replaceTimeEntry(await updateTimeEntry({
      entryId,
      startedAt,
      endedAt,
      note: note.trim() || undefined,
      expectedVersion: entry.version,
    }));
    await loadWorkspaceData();
    await loadTodayOverview();
  }

  async function setTimeEntryDisposition(entryId: string, disposition: "break" | "discard") {
    const entry = entries.find((item) => item.id === entryId);
    if (!entry) return;
    markTimeDataChanged();
    const updated = toUiTimeEntry(await updateTimeEntryDisposition({
      entryId,
      expectedVersion: entry.version,
      disposition,
    }));
    if (disposition === "discard") {
      const index = entries.findIndex((item) => item.id === entryId);
      if (index >= 0) entries.splice(index, 1);
      if (selectedEntryId.value === entryId) selectedEntryId.value = entries[0]?.id ?? "";
    } else {
      const index = entries.findIndex((item) => item.id === updated.id);
      if (index >= 0) entries.splice(index, 1, updated);
      else entries.unshift(updated);
    }
    await loadWorkspaceData();
    await loadTodayOverview();
  }

  async function createReport(request: { type: ReportType; referenceDate: string; subjectId: string; taskIds: string[] }) {
    const result = await createReportRecord({
      reportType: request.type,
      referenceDate: request.referenceDate,
      subjectId: request.subjectId,
      taskIds: request.taskIds,
    });
    const report = toUiReport(result);
    reports.value.unshift(report);
    return report;
  }

  async function updateReportScope(report: ReportRecord, request: { type: ReportType; referenceDate: string; subjectId: string; taskIds: string[] }) {
    const updated = toUiReport(await updateReportScopeRecord({
      reportId: report.id,
      reportType: request.type,
      referenceDate: request.referenceDate,
      subjectId: request.subjectId,
      taskIds: request.taskIds,
      expectedVersion: report.version,
    }));
    const index = reports.value.findIndex((item) => item.id === updated.id);
    if (index >= 0) reports.value.splice(index, 1, updated);
    return updated;
  }

  async function saveReportContent(report: ReportRecord, markdown: string) {
    const updated = toUiReport(await saveReportContentRecord(report.id, markdown, report.version));
    const index = reports.value.findIndex((item) => item.id === updated.id);
    if (index >= 0) reports.value.splice(index, 1, updated);
    return updated;
  }

  async function regenerateReport(report: ReportRecord) {
    const updated = toUiReport(await regenerateReportRecord(report.id, report.version));
    const index = reports.value.findIndex((item) => item.id === updated.id);
    if (index >= 0) reports.value.splice(index, 1, updated);
    return updated;
  }

  async function deleteReport(report: ReportRecord) {
    await deleteReportRecord(report.id, report.version);
    const index = reports.value.findIndex((item) => item.id === report.id);
    if (index >= 0) reports.value.splice(index, 1);
  }

  async function getReportTemplate(type: ReportType, subjectId?: string) {
    return await getReportTemplateRecord(type, subjectId);
  }

  async function saveReportTemplate(request: {
    type: ReportType;
    subjectId?: string;
    content: string;
    expectedVersion?: number;
  }) {
    return await saveReportTemplateRecord({
      reportType: request.type,
      subjectId: request.subjectId,
      content: request.content,
      expectedVersion: request.expectedVersion,
    });
  }

  async function previewObsidianReport(report: ReportRecord) {
    return await previewObsidianReportWrite(report.id, report.version);
  }

  async function publicReportText(report: ReportRecord) {
    return await getPublicReportText(report.id, report.version);
  }

  async function writeObsidianReport(runId: string, strategy: "overwrite" | "append") {
    return await executeObsidianReportWrite(runId, strategy);
  }

  function replaceTimeEntry(record: TimeEntryRecord) {
    const next = toUiTimeEntry(record);
    if (next.state === "running" || next.state === "paused") timerStateMissingRefreshes = 0;
    const index = entries.findIndex((entry) => entry.id === record.id);
    if (index >= 0) entries.splice(index, 1, next);
    else entries.unshift(next);
    return next;
  }

  function cloneTimeEntry(entry: TimeEntry): TimeEntry {
    return {
      ...entry,
      allocations: entry.allocations?.map((allocation) => ({ ...allocation })),
    };
  }

  function formatLocalDate(date: Date) {
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, "0");
    const day = String(date.getDate()).padStart(2, "0");
    return `${year}-${month}-${day}`;
  }

  async function resolveUnassignedTime(action: UnassignedTimeAction, allocations: TimeAllocation[] = []): Promise<BatchCompletionResult | false> {
    const seconds = unassignedSeconds.value;
    if (seconds < 1 || !unassignedSessionId.value) return false;

    const completionCandidates = new Set<string>();
    if (action === "work") {
      const mergedAllocations = new Map<string, { minutes: number; completeTask: boolean; taskExpectedVersion?: number }>();
      for (const allocation of allocations) {
        const taskId = await persistTimerTask(allocation.taskId);
        if (!taskId) continue;
        const allocationMinutes = Math.max(0, Number(allocation.minutes) || 0);
        if (!allocationMinutes) continue;
        const task = tasks.find((candidate) => candidate.id === taskId);
        const current = mergedAllocations.get(taskId) ?? { minutes: 0, completeTask: false, taskExpectedVersion: task?.version };
        current.minutes += allocationMinutes;
        current.completeTask ||= Boolean(allocation.completeTask);
        current.taskExpectedVersion = task?.version;
        mergedAllocations.set(taskId, current);
      }
      const normalizedAllocations = [...mergedAllocations].map(([taskId, allocation]) => ({
        taskId,
        minutes: allocation.minutes,
        completeTask: allocation.completeTask,
        taskExpectedVersion: allocation.completeTask ? allocation.taskExpectedVersion : undefined,
      }));
      for (const allocation of normalizedAllocations) {
        if (allocation.completeTask && tasks.find((task) => task.id === allocation.taskId)?.status !== "done") completionCandidates.add(allocation.taskId);
      }
      await resolveUnassignedWork(unassignedSessionId.value, unassignedVersion.value, normalizedAllocations);
    } else if (action === "break") {
      await resolveUnassignedBreak(unassignedSessionId.value, unassignedVersion.value);
    } else {
      await discardUnassignedTime(unassignedSessionId.value, unassignedVersion.value);
    }

    unassignedDialogOpen.value = false;
    resetUnassignedTracking();
    await Promise.all([loadTimeData(), loadWorkspaceData(), loadUnassignedState()]);
    await loadTodayOverview();
    const completedTaskIds = [...completionCandidates].filter((taskId) => tasks.find((task) => task.id === taskId)?.status === "done");
    return { completedCount: completedTaskIds.length, completedTaskIds };
  }

  return {
    activePage, subjects, selectedSubjectId, selectedSubject, selectedSubjectTasks, tasks, todayTasks, visibleTasks, statusColors, entries, reports, todayOverview, todayOverviewLoading, todayOverviewSubjectId, todayOverviewSubject, selectedTaskId, selectedEntryId, selectedEntry, runningEntry, timerStopConfirmationEntryId, timerStopConfirmationEntry,
    now, todayMinutes, allocatedMinutes, pendingMinutes, doneCount, unassignedSeconds, unassignedStartedAt,
    unassignedFirstStartedAt, unassignedLastEndedAt, unassignedDialogOpen,
    workspaceLoaded, workspaceLoading, loadWorkspaceData, loadTimeData, loadUnassignedState, loadReports, loadTodayOverview, selectTodayOverviewSubject, entryDurationSeconds, taskPathLabel, taskDisplayLabel, timeEntryLabel, startClock, stopClock, selectPage, selectSubject, addSubject, renameSubject, commitTaskToggle, toggleTask, setTaskRecurrence, addTask, addQuickTask, addChildTask, saveTask, duplicateTask, deleteTask, moveTask, reorderTask,
    indentTask, outdentTask, startTimer, pauseTimer, resumeTimer, stopTimer, openTimerStopConfirmation, dismissTimerStopConfirmation, adjustTimerStopAllocation, confirmTimerStopAllocation, resolveTimerStopDisposition, allocate, addManualEntry, correctTimeEntry, setTimeEntryDisposition, createReport, updateReportScope, saveReportContent, regenerateReport, deleteReport, getReportTemplate, saveReportTemplate, publicReportText, previewObsidianReport, writeObsidianReport,
    beginUnassignedTracking, pauseUnassignedTracking, promptUnassignedResolution, resolveUnassignedTime
  };
});
