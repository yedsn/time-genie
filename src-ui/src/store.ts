import { computed, reactive, ref, watch } from "vue";
import { defineStore } from "pinia";
import { derivedUnassignedSeconds } from "./services/unassignedTimeDisplayCore";
import {
  createSubject as createSubjectRecord,
  createPositionedTask,
  createTask as createTaskRecord,
  createQuickTask as createQuickTaskRecord,
  changeTaskParent,
  createManualTimeEntry,
  createReport as createReportRecord,
  deleteTaskSubtree,
  discardUnassignedTime,
  duplicateTaskSubtree,
  getTodayWorkOverview,
  getWorkspaceCalendarContext,
  getTimerState,
  getUnassignedStateSnapshot,
  listCompletedTaskPage,
  listSubjects,
  listTasks,
  listTodayTasks,
  getTaskDayPlanSnapshot,
  setTaskDayPlan,
  setTaskDayPlanBatch,
  listTimeEntries,
  listTimeEntryHistory,
  moveTaskRecord,
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
  listReportTemplates,
  createReportTemplate,
  updateReportTemplate,
  duplicateReportTemplate,
  setDefaultReportTemplate,
  deleteReportTemplate,
  deleteActualAdjustment,
  reorderReportTemplates,
  getReportTemplatePlaceholders,
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
  setTaskDailyEstimateBatch,
  applyTaskPlanBatch,
  getActualAdjustmentSnapshot,
  applyActualAdjustmentBatch,
  setRecurringTaskEstimate,
  startTimerRecord,
  transitionTimerRecord,
  stopTimerRecord,
  notifyTimerStopConfirmation,
  updateTask as updateTaskRecord,
  updateTimeEntry,
  updateTimeEntryDisposition,
  type TimeEntryRecord,
  type TimeEntryHistoryDayRecord,
  type TimerStopResultRecord,
  type TimerTransitionNewTaskRequest,
  type TimerTransitionResultRecord,
  type TaskRecord,
  type TodayWorkOverviewRecord,
  type ReportRecordDto,
  type UnassignedStateRecord,
  type RecurrenceRuleRecord,
  type ActualAdjustmentBatchRequest,
  type ActualAdjustmentSnapshotRecord,
  type TaskDayPlanSnapshotRecord,
} from "./services/tauri";
import { TRANSIENT_MISSING_STATE_REFRESH_LIMIT, transientMissingStateResult } from "./services/transientStateGuard";
import { canSubmitUnassignedResolution } from "./services/unassignedResolutionGuard";
import { clampHistoryPage, collectTodayEntryTaskIds, findEntryInHistory, isTodayTimeEntry } from "./services/timeEntryHistoryCore";
import { completedTaskHistoryFallbackPage, isCurrentCompletedTaskHistoryRequest } from "./services/completedTaskHistoryCore";
import { canAcceptInlineTimerChild, inlineTimerParentUnavailableReason } from "./services/inlineTimerTaskCreationCore";
import type { QuickPlanBatchRequest } from "./services/quickPlanTextCore";
import { beginTodayPlanRequest, isCurrentTodayPlanRequest, resolveTodayPlanOutcome } from "./services/todayPlanRequestCore";

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
  dailyEstimateVersion?: number;
  dayPlanState?: "included" | "excluded";
  dayPlanVersion?: number;
  todayMember?: boolean;
  todayContext?: boolean;
  actual: number;
  directMinutes: number;
  status: TaskStatus;
  note?: string;
  pathLabel?: string;
  sortOrder: number;
  completedAt?: number;
  version: number;
  selectable: boolean;
  executionState: TaskRecord["executionState"];
  recurrence?: RecurrenceRuleRecord;
  occurrenceDate?: string;
  occurrenceOrigin?: "scheduled" | "manual";
  occurrenceVersion?: number;
  quickCreate?: boolean;
  createPreviousSiblingId?: string;
  createNextSiblingId?: string;
};

export type TimeAllocation = {
  id?: string;
  taskId: string;
  taskTitle?: string;
  subjectId?: string;
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
  workDate: string;
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
  timerChainId?: string;
  previousEntryId?: string;
  splitBoundaryAt?: number;
};

export type PlanTaskTab = "today" | "active" | "completed";

export type TimeEntryHistoryDay = Omit<TimeEntryHistoryDayRecord, "entries"> & {
  entries: TimeEntry[];
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
  templateId?: string;
  templateName?: string;
  templateArchived: boolean;
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
  const todayPlanTasks = reactive<Task[]>([]);
  const todayPlanLoading = ref(false);
  const todayPlanSubjectId = ref("");
  const todayPlanError = ref("");
  let todayPlanRequestSeq = 0;
  const statusColors = reactive<StatusColors>({ ...defaultStatusColors });
  const entries = reactive<TimeEntry[]>([]);
  const timeHistoryGroups = reactive<TimeEntryHistoryDay[]>([]);
  const timeHistoryPage = ref(1);
  const timeHistoryTotalDays = ref(0);
  const timeHistoryTotalPages = ref(0);
  const timeHistoryLoading = ref(false);
  const timeHistoryRevision = ref(0);
  const selectedTaskId = ref("");
  const selectedEntryId = ref("");
  const reports = ref<ReportRecord[]>([]);
  const todayOverview = ref<TodayWorkOverviewRecord | null>(null);
  const todayOverviewLoading = ref(false);
  const todayOverviewSubjectId = ref("");
  const executionSnapshots = reactive<Record<string, ActualAdjustmentSnapshotRecord>>({});
  const executionSnapshotLoading = reactive<Record<string, boolean>>({});
  const now = ref(Date.now());
  const workspaceTimezone = ref("Asia/Shanghai");
  const workspaceToday = ref("");
  const workspaceDayStartAt = ref(0);
  const workspaceDayEndAt = ref(0);
  const unassignedStartedAt = ref<number>();
  const unassignedElapsedBaseSeconds = ref(0);
  const unassignedElapsedSyncedAt = ref(0);
  const unassignedFirstStartedAt = ref<number>();
  const unassignedLastEndedAt = ref<number>();
  const unassignedDialogOpen = ref(false);
  const dismissedUnassignedSessionId = ref("");
  const unassignedWorkDate = ref("");
  const unassignedCurrentSession = ref<UnassignedStateRecord>();
  const timerStopConfirmationEntryId = ref("");
  const timerStopConfirmation = ref<{
    currentEntryId: string;
    slices: TimeEntry[];
    totalDurationSeconds: number;
    totalSettlementMinutes: number;
  }>();
  const unassignedSessionId = ref("");
  const unassignedVersion = ref(0);
  const unassignedThresholdSeconds = ref(5 * 60);
  let clock: number | undefined;
  let timeRefreshClock: number | undefined;
  let timeDataRequestSeq = 0;
  let timeDataMutationSeq = 0;
  let timeHistoryRequestSeq = 0;
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
    if (entry.kind === "break" || !isTodayTimeEntry(entry, workspaceToday.value)) return sum;
    return sum + Math.ceil(entryDurationSeconds(entry) / 60);
  }, 0));
  const allocatedMinutes = computed(() => entries.reduce((sum, entry) => (
    isTodayTimeEntry(entry, workspaceToday.value) ? sum + entry.allocated : sum
  ), 0));
  const pendingMinutes = computed(() => Math.max(0, todayMinutes.value - allocatedMinutes.value));
  const runningEntry = computed(() => entries.find((entry) => entry.state === "running" || entry.state === "paused"));
  const selectedEntry = computed(() => findTimeEntry(selectedEntryId.value));
  const timerStopConfirmationEntry = computed(() => {
    const fromChain = timerStopConfirmation.value?.slices.find((entry) => entry.id === timerStopConfirmationEntryId.value);
    return fromChain ?? entries.find((entry) => entry.id === timerStopConfirmationEntryId.value && entry.state === "ended");
  });
  const timerStopConfirmationSlices = computed(() => timerStopConfirmation.value?.slices ?? (timerStopConfirmationEntry.value ? [timerStopConfirmationEntry.value] : []));
  const doneCount = computed(() => tasks.filter((task) => task.status === "done").length);
  const todayCompletionTaskIds = computed(() => {
    const taskIds = new Set(tasks
      .filter((task) => task.version > 0 && (task.todayEstimate ?? 0) > 0)
      .map((task) => task.id));
    for (const taskId of collectTodayEntryTaskIds(entries, workspaceToday.value)) taskIds.add(taskId);
    return taskIds;
  });
  const todayCompletionTotalCount = computed(() => todayCompletionTaskIds.value.size);
  const todayCompletionDoneCount = computed(() => {
    const todayTaskById = new Map(todayTasks.map((task) => [task.id, task]));
    return [...todayCompletionTaskIds.value].filter((taskId) => {
      const task = todayTaskById.get(taskId) ?? tasks.find((item) => item.id === taskId);
      return task?.status === "done";
    }).length;
  });
  const todayCompletionPercent = computed(() => todayCompletionTotalCount.value
    ? Math.round(todayCompletionDoneCount.value / todayCompletionTotalCount.value * 100)
    : 0);
  const unassignedSeconds = computed(() => derivedUnassignedSeconds(
    unassignedElapsedBaseSeconds.value,
    unassignedElapsedSyncedAt.value,
    now.value,
  ));
  const visibleTasks = computed(() => {
    const childMap = new Map<string, Task[]>();
    const roots: Task[] = [];
    const taskIds = new Set(tasks.map((task) => task.id));
    for (const task of tasks) {
      if (task.parentId && taskIds.has(task.parentId)) {
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
  const planTaskTab = ref<PlanTaskTab>("today");
  const completedTasks = reactive<Task[]>([]);
  const completedTasksSubjectId = ref("");
  const completedTasksPage = ref(1);
  const completedTasksPageSize = 20;
  const completedTasksTotal = ref(0);
  const completedTasksTotalPages = ref(0);
  const completedTasksLoading = ref(false);
  const completedTasksError = ref("");
  const completedTasksStale = ref(true);
  let completedTasksRequestSeq = 0;
  watch(activePage, (page, previousPage) => {
    if (page === "plan" && previousPage !== "plan") planTaskTab.value = "today";
  });
  const todayOverviewSubject = computed(() => {
    if (!todayOverviewSubjectId.value) return undefined;
    return subjects.find((subject) => subject.id === todayOverviewSubjectId.value);
  });

  function startClock(trackUnassigned = true) {
    void initializeWorkspace(trackUnassigned);
    if (!clock) {
      clock = window.setInterval(() => {
        now.value = Date.now();
        if (workspaceDayEndAt.value && now.value >= workspaceDayEndAt.value) {
          void refreshWorkspaceCalendar().then(() => Promise.all([loadWorkspaceData(), loadTimeData(), loadUnassignedState()])).then(() => loadTodayOverview());
          void loadTodayPlan().catch(() => undefined);
        }
      }, 1_000);
    }
    if (!timeRefreshClock) {
      timeRefreshClock = window.setInterval(() => void loadTimeData().then(() => loadTodayOverview()), 5_000);
    }
  }

  function stopClock() {
    if (clock) window.clearInterval(clock);
    clock = undefined;
    if (timeRefreshClock) window.clearInterval(timeRefreshClock);
    timeRefreshClock = undefined;
  }

  async function initializeWorkspace(trackUnassigned: boolean) {
    await refreshWorkspaceCalendar();
    await Promise.all([loadWorkspaceData(), loadTimeData(), loadReports(), trackUnassigned ? loadUnassignedState() : Promise.resolve()]);
    await loadTodayOverview();
  }

  async function refreshWorkspaceCalendar() {
    const context = await getWorkspaceCalendarContext();
    workspaceTimezone.value = context.timezone;
    workspaceToday.value = context.currentWorkDate;
    workspaceDayStartAt.value = context.currentDayStartAt;
    workspaceDayEndAt.value = context.currentDayEndAt;
    return context;
  }

  async function ensureWorkspaceCalendar() {
    if (!workspaceToday.value || Date.now() >= workspaceDayEndAt.value) {
      await refreshWorkspaceCalendar();
    }
    return workspaceToday.value;
  }

  async function loadWorkspaceData() {
    const requestSeq = ++workspaceRequestSeq;
    workspaceLoading.value = true;
    try {
      const todayDate = await ensureWorkspaceCalendar();
      const [subjectRecords, taskResult, todayTaskResult] = await Promise.all([
        listSubjects(),
        listTasks(undefined, undefined, todayDate),
        listTodayTasks(undefined, todayDate),
      ]);
      if (requestSeq !== workspaceRequestSeq) return true;
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
      const mergedTasks = [...loadedTasks];
      for (const draft of unsavedTasks.filter((task) => !loadedIds.has(task.id))) {
        const nextIndex = draft.createNextSiblingId
          ? mergedTasks.findIndex((task) => task.id === draft.createNextSiblingId)
          : -1;
        if (nextIndex >= 0) {
          mergedTasks.splice(nextIndex, 0, draft);
          continue;
        }
        const previousIndex = draft.createPreviousSiblingId
          ? findSubtreeEndIndex(mergedTasks, draft.createPreviousSiblingId)
          : -1;
        if (previousIndex >= 0) mergedTasks.splice(previousIndex, 0, draft);
        else mergedTasks.push(draft);
      }
      tasks.splice(0, tasks.length, ...mergedTasks);
      todayTasks.splice(0, todayTasks.length, ...todayTaskResult.tasks.map(toUiTask));
      const fallbackSubjectId = subjects[0]?.id ?? "";
      if (!subjects.some((subject) => subject.id === selectedSubjectId.value)) selectedSubjectId.value = fallbackSubjectId;
      if (todayOverviewSubjectId.value && !subjects.some((subject) => subject.id === todayOverviewSubjectId.value)) todayOverviewSubjectId.value = "";
      if (!tasks.some((task) => task.id === selectedTaskId.value)) {
        selectedTaskId.value = buildVisibleTaskList(tasks.filter((task) => task.subjectId === selectedSubjectId.value))[0]?.id ?? "";
      }
      workspaceLoaded.value = true;
      if (planTaskTab.value === "today" && selectedSubjectId.value) void loadTodayPlan(selectedSubjectId.value).catch(() => undefined);
      if (planTaskTab.value === "completed" && selectedSubjectId.value) {
        completedTasksStale.value = true;
        void loadCompletedTasks(completedTasksPage.value);
      } else {
        completedTasksStale.value = true;
      }
      return true;
    } catch (error) {
      console.error("加载工作台数据失败", error);
      return false;
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
      dailyEstimateVersion: task.dailyEstimateVersion,
      dayPlanState: task.dayPlanState,
      dayPlanVersion: task.dayPlanVersion,
      todayMember: task.todayMember,
      todayContext: task.todayContext,
      actual: task.totalMinutes,
      directMinutes: task.directMinutes,
      status: task.status,
      note: task.note,
      pathLabel: task.pathLabel,
      sortOrder: task.sortOrder,
      completedAt: task.completedAt,
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
      workDate: entry.workDate,
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
        taskTitle: allocation.taskTitle,
        subjectId: allocation.subjectId,
        minutes: allocation.minutes,
        note: allocation.note,
      })),
      note: entry.note ?? "",
      kind: entry.kind,
      sourceType: entry.sourceType,
      state: entry.state,
      version: entry.version,
      timerChainId: entry.timerChainId,
      previousEntryId: entry.previousEntryId,
      splitBoundaryAt: entry.splitBoundaryAt,
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
      const workDate = await ensureWorkspaceCalendar();
      const [result, activeEntry] = await Promise.all([listTimeEntries(workDate), getTimerState()]);
      if (requestSeq !== timeDataRequestSeq || mutationSeq !== timeDataMutationSeq) return true;
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
        if (!findTimeEntry(selectedEntryId.value)) selectedEntryId.value = entries[0]?.id ?? "";
      }
      return true;
    } catch (error) {
      console.error("加载计时数据失败", error);
      return false;
    }
  }

  async function loadTodayOverview(subjectId = todayOverviewSubjectId.value) {
    const requestSeq = todayOverviewRequestSeq + 1;
    todayOverviewRequestSeq = requestSeq;
    todayOverviewLoading.value = true;
    try {
      const todayDate = await ensureWorkspaceCalendar();
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
      templateId: report.templateId,
      templateName: report.templateName,
      templateArchived: report.templateArchived,
    };
  }

  async function loadUnassignedState(authoritative = false) {
    const requestSeq = ++unassignedStateRequestSeq;
    const mutationSeq = unassignedStateMutationSeq;
    try {
      const snapshot = await getUnassignedStateSnapshot();
      if (requestSeq !== unassignedStateRequestSeq || mutationSeq !== unassignedStateMutationSeq) return true;
      unassignedCurrentSession.value = snapshot.current;
      if (authoritative && !snapshot.current) {
        unassignedStateMissingRefreshes = TRANSIENT_MISSING_STATE_REFRESH_LIMIT;
      }
      applyUnassignedState(snapshot.current ?? null);
      return true;
    } catch (error) {
      console.error("加载未归属时间失败", error);
      return false;
    }
  }

  async function loadTimeHistory(page = timeHistoryPage.value) {
    const requestSeq = ++timeHistoryRequestSeq;
    timeHistoryLoading.value = true;
    try {
      const result = await listTimeEntryHistory(Math.max(1, Math.round(page)));
      if (requestSeq !== timeHistoryRequestSeq) return result;
      timeHistoryPage.value = result.page;
      timeHistoryTotalDays.value = result.totalDays;
      timeHistoryTotalPages.value = result.totalPages;
      timeHistoryRevision.value = result.revision;
      timeHistoryGroups.splice(0, timeHistoryGroups.length, ...result.groups.map((group) => ({
        ...group,
        entries: group.entries.map(toUiTimeEntry),
      })));
      if (selectedEntryId.value && !findTimeEntry(selectedEntryId.value)) selectedEntryId.value = "";
      return result;
    } catch (error) {
      console.error("加载时间记录历史失败", error);
      throw error;
    } finally {
      if (requestSeq === timeHistoryRequestSeq) timeHistoryLoading.value = false;
    }
  }

  async function selectTimeHistoryPage(page: number) {
    if (timeHistoryLoading.value) return;
    const target = clampHistoryPage(page, timeHistoryTotalPages.value);
    if (target === timeHistoryPage.value && timeHistoryGroups.length) return;
    await loadTimeHistory(target);
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
    const previousSessionId = unassignedSessionId.value;
    const sessionChanged = Boolean(previousSessionId && previousSessionId !== state.sessionId);
    if (sessionChanged) {
      unassignedDialogOpen.value = false;
      dismissedUnassignedSessionId.value = "";
    }
    unassignedSessionId.value = state.sessionId;
    unassignedWorkDate.value = state.workDate;
    unassignedVersion.value = state.version;
    unassignedThresholdSeconds.value = state.thresholdSeconds;
    unassignedFirstStartedAt.value = state.firstStartedAt;
    unassignedLastEndedAt.value = state.lastEndedAt;
    unassignedStartedAt.value = state.currentSegmentStartedAt ?? state.firstStartedAt;
    unassignedElapsedBaseSeconds.value = Math.max(0, state.elapsedSeconds);
    unassignedElapsedSyncedAt.value = syncedAt;
  }

  function replaceTask(record: TaskRecord) {
    const index = tasks.findIndex((task) => task.id === record.id);
    const next = toUiTask(record);
    if (index >= 0) tasks.splice(index, 1, next);
    else tasks.push(next);
    const todayIndex = todayTasks.findIndex((task) => task.id === record.id);
    if (todayIndex >= 0) todayTasks.splice(todayIndex, 1, toUiTask(record));
    return next;
  }

  function applyTaskRecord(task: Task, record: TaskRecord) {
    Object.assign(task, toUiTask(record));
  }

  function selectPage(page: string) {
    activePage.value = page;
    if (page === "plan") { planTaskTab.value = "today"; void loadTodayPlan().catch(() => undefined); }
  }

  function selectSubject(subjectId: string) {
    if (!subjects.some((subject) => subject.id === subjectId)) return;
    selectedSubjectId.value = subjectId;
    activePage.value = "plan";
    planTaskTab.value = "today";
    void loadTodayPlan(subjectId).catch(() => undefined);
    completedTasksRequestSeq += 1;
    completedTasks.splice(0);
    completedTasksSubjectId.value = "";
    completedTasksPage.value = 1;
    completedTasksTotal.value = 0;
    completedTasksTotalPages.value = 0;
    completedTasksError.value = "";
    completedTasksLoading.value = false;
    completedTasksStale.value = true;
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
    const totalSeconds = unassignedSeconds.value;
    if (unassignedDialogOpen.value) return false;
    if (totalSeconds < 1) return false;
    if (!force && totalSeconds <= unassignedThresholdSeconds.value) return false;
    if (!force && dismissedUnassignedSessionId.value === unassignedSessionId.value) return false;
    unassignedDialogOpen.value = true;
    return true;
  }

  async function loadCompletedTasks(page = completedTasksPage.value) {
    const subjectId = selectedSubjectId.value;
    if (!subjectId) return false;
    const targetPage = Math.max(1, Math.round(page));
    const sequence = ++completedTasksRequestSeq;
    const request = { subjectId, page: targetPage, sequence };
    completedTasksSubjectId.value = subjectId;
    completedTasksPage.value = targetPage;
    completedTasksLoading.value = true;
    completedTasksError.value = "";
    try {
      const result = await listCompletedTaskPage(subjectId, targetPage, completedTasksPageSize);
      if (!isCurrentCompletedTaskHistoryRequest(
        request,
        completedTasksSubjectId.value,
        completedTasksPage.value,
        completedTasksRequestSeq,
      )) return false;
      completedTasks.splice(0, completedTasks.length, ...result.tasks.map(toUiTask));
      completedTasksPage.value = result.page;
      completedTasksTotal.value = result.total;
      completedTasksTotalPages.value = result.totalPages;
      completedTasksStale.value = false;
      const fallbackPage = completedTaskHistoryFallbackPage(result.page, result.tasks.length);
      if (fallbackPage !== result.page) return await loadCompletedTasks(fallbackPage);
      return true;
    } catch (error) {
      if (sequence !== completedTasksRequestSeq) return false;
      completedTasks.splice(0);
      completedTasksError.value = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      if (sequence === completedTasksRequestSeq) completedTasksLoading.value = false;
    }
  }

  async function selectPlanTaskTab(tab: PlanTaskTab) {
    planTaskTab.value = tab;
    if (tab === "today") { await loadTodayPlan().catch(() => undefined); return; }
    if (tab === "active") return;
    const subjectChanged = completedTasksSubjectId.value !== selectedSubjectId.value;
    if (subjectChanged) completedTasksPage.value = 1;
    if (subjectChanged || completedTasksStale.value || !completedTasks.length) {
      await loadCompletedTasks(subjectChanged ? 1 : completedTasksPage.value);
    }
  }

  async function loadTodayPlan(subjectId = selectedSubjectId.value) {
    if (!subjectId) return undefined;
    const workDate = await ensureWorkspaceCalendar();
    const requestedSubjectId = subjectId;
    const token = beginTodayPlanRequest(todayPlanRequestSeq, requestedSubjectId, workDate);
    todayPlanRequestSeq = token.sequence;
    todayPlanSubjectId.value = requestedSubjectId;
    todayPlanLoading.value = true;
    todayPlanError.value = "";
    try {
      const snapshot: TaskDayPlanSnapshotRecord = await getTaskDayPlanSnapshot(requestedSubjectId, workDate);
      const stillCurrent = isCurrentTodayPlanRequest(token, todayPlanRequestSeq, workspaceToday.value);
      const outcome = resolveTodayPlanOutcome(
        { tasks: [...todayPlanTasks], subjectId: todayPlanSubjectId.value },
        stillCurrent ? { ok: true, token, tasks: snapshot.tasks.map(toUiTask) } : { ok: false },
      );
      if (stillCurrent) {
        todayPlanTasks.splice(0, todayPlanTasks.length, ...outcome.tasks);
        todayPlanSubjectId.value = requestedSubjectId;
      }
      return snapshot;
    } catch (error) {
      if (isCurrentTodayPlanRequest(token, todayPlanRequestSeq, workspaceToday.value)) {
        todayPlanError.value = error instanceof Error ? error.message : String(error);
      }
      throw error;
    } finally {
      if (isCurrentTodayPlanRequest(token, todayPlanRequestSeq, workspaceToday.value)) todayPlanLoading.value = false;
    }
  }

  async function reloadTodayPlanForSubject(subjectId = selectedSubjectId.value) {
    if (!subjectId) return undefined;
    return await loadTodayPlan(subjectId);
  }

  async function setTodayPlanMembership(taskId: string, state: "included" | "excluded") {
    const task = [...todayPlanTasks, ...tasks].find((item) => item.id === taskId);
    const workDate = await ensureWorkspaceCalendar();
    await setTaskDayPlan(taskId, workDate, state, task?.dayPlanVersion);
    await Promise.all([loadWorkspaceData(), loadTodayPlan(task?.subjectId ?? selectedSubjectId.value), loadTodayOverview()]);
  }

  async function addTasksToToday(taskIds: string[]) {
    if (!taskIds.length) return;
    const selectedTasks = taskIds.map((taskId) => [...todayPlanTasks, ...tasks].find((item) => item.id === taskId));
    const subjectId = selectedTasks.find(Boolean)?.subjectId ?? selectedSubjectId.value;
    if (!subjectId) return;
    if (selectedTasks.some((task) => task && task.subjectId !== subjectId)) throw new Error("只能批量加入同一主体的事项");
    const workDate = await ensureWorkspaceCalendar();
    await setTaskDayPlanBatch(subjectId, workDate, taskIds.map((taskId) => {
      const task = [...todayPlanTasks, ...tasks].find((item) => item.id === taskId);
      return { taskId, state: "included" as const, expectedVersion: task?.dayPlanVersion };
    }));
    await Promise.all([loadWorkspaceData(), loadTodayPlan(subjectId), loadTodayOverview()]);
  }

  async function selectCompletedTasksPage(page: number) {
    if (completedTasksLoading.value || page === completedTasksPage.value) return;
    await loadCompletedTasks(page);
  }

  function markCompletedTasksStale() {
    completedTasksStale.value = true;
    if (planTaskTab.value === "completed" && activePage.value === "plan") {
      void loadCompletedTasks(completedTasksPage.value);
    }
  }

  async function refreshCompletedTasksAfterMutation() {
    await loadCompletedTasks(completedTasksPage.value);
  }

  async function reopenCompletedTask(task: Task) {
    const result = await commitTaskToggle(task);
    if (!result) return;
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(), loadTodayOverview()]);
    await refreshCompletedTasksAfterMutation();
    return result;
  }

  async function saveCompletedTask(task: Task) {
    await updateTaskRecord({
      id: task.id,
      expectedVersion: task.version,
      title: task.title,
      plannedDate: task.planDate ?? null,
      estimateMinutes: task.estimate ?? null,
      note: task.note ?? null,
      projectName: task.project || null,
    });
    await Promise.all([loadWorkspaceData(), loadTimeData(), loadReports(), reloadTodayPlanForSubject(task.subjectId)]);
    await loadTodayOverview();
    await loadCompletedTasks(completedTasksPage.value);
    return task.id;
  }

  async function deleteCompletedTask(task: Task) {
    await deleteTaskSubtree(task.id, task.version);
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(task?.subjectId), loadTodayOverview()]);
    await refreshCompletedTasksAfterMutation();
  }

  function dismissUnassignedResolution() {
    dismissedUnassignedSessionId.value = unassignedSessionId.value;
    unassignedDialogOpen.value = false;
  }

  function resetUnassignedTracking() {
    markUnassignedStateChanged();
    unassignedDialogOpen.value = false;
    dismissedUnassignedSessionId.value = "";
    unassignedStartedAt.value = undefined;
    unassignedElapsedBaseSeconds.value = 0;
    unassignedElapsedSyncedAt.value = 0;
    unassignedFirstStartedAt.value = undefined;
    unassignedLastEndedAt.value = undefined;
    unassignedSessionId.value = "";
    unassignedWorkDate.value = "";
    unassignedVersion.value = 0;
    unassignedStateMissingRefreshes = 0;
  }

  async function commitTaskToggle(task: Task): Promise<TaskCompletionResult | undefined> {
    if (task.version <= 0) return;
    const direction = task.status === "done" ? "reopened" : "completed";
    const optimisticStatus: TaskStatus = direction === "completed" ? "done" : "open";
    const todayTask = todayTasks.find((item) => item.id === task.id);
    const todayDate = await ensureWorkspaceCalendar();
    const occurrenceDate = task.occurrenceDate
      ?? todayTask?.occurrenceDate
      ?? (task.recurrence ? todayDate : undefined);
    const occurrenceVersion = task.occurrenceVersion ?? todayTask?.occurrenceVersion;
    const optimisticTargets = occurrenceDate
      ? [task, ...(todayTask && todayTask !== task ? [todayTask] : [])]
      : [...tasks.filter((item) => item.id === task.id), ...todayTasks.filter((item) => item.id === task.id)];
    const previousStates = [...new Set(optimisticTargets)].map((item) => ({
      task: item,
      status: item.status,
      executionState: item.executionState,
    }));
    for (const item of optimisticTargets) {
      item.status = optimisticStatus;
      item.executionState = optimisticStatus === "done" ? "done" : item.actual > 0 ? "started" : "not_started";
    }
    try {
      const updated = await setTaskCompleted(
        task.id,
        task.version,
        direction === "completed",
        occurrenceDate,
        occurrenceVersion,
      );
      if (occurrenceDate) {
        for (const item of new Set(optimisticTargets)) applyTaskRecord(item, updated);
      } else {
        replaceTask(updated);
      }
      return {
        taskId: task.id,
        subjectId: task.subjectId,
        direction,
        completedCount: direction === "completed" && updated.status === "done" ? 1 : 0,
      };
    } catch (error) {
      for (const previous of previousStates) {
        previous.task.status = previous.status;
        previous.task.executionState = previous.executionState;
      }
      throw error;
    }
  }

  async function toggleTask(task: Task) {
    const result = await commitTaskToggle(task);
    if (!result) return;
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(task.subjectId), loadTodayOverview()]);
    return result;
  }

  async function setTaskRecurrence(
    taskId: string,
    recurrence?: { frequency: RecurrenceRuleRecord["frequency"]; weekdaysMask?: number; effectiveStart: string },
  ) {
    const task = tasks.find((item) => item.id === taskId);
    if (!task || task.version <= 0) return;
    const todayDate = await ensureWorkspaceCalendar();
    if (!recurrence) {
      if (!task.recurrence) return;
      await closeTaskRecurrence({
        taskId: task.id,
        taskExpectedVersion: task.version,
        effectiveEnd: todayDate,
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
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(task.subjectId), loadTodayOverview()]);
  }

  function addTask(title = "", subjectId = selectedSubjectId.value) {
    const subject = subjects.find((item) => item.id === subjectId) ?? selectedSubject.value;
    const task: Task = { id: createTaskId(), subjectId: subject?.id ?? defaultSubjectId, title, project: subject?.name ?? "默认", actual: 0, directMinutes: 0, status: "open", sortOrder: Number.MAX_SAFE_INTEGER, version: 0, selectable: true, executionState: "not_started" };
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
      directMinutes: 0,
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

  function addTaskRelative(targetId: string, placement: "before" | "after") {
    const ordered = buildVisibleTaskList(tasks);
    const target = ordered.find((task) => task.id === targetId);
    if (!target) return;
    const siblings = ordered.filter((task) => task.subjectId === target.subjectId && task.parentId === target.parentId);
    const siblingIndex = siblings.findIndex((task) => task.id === target.id);
    const insertIndex = placement === "before" ? siblingIndex : siblingIndex + 1;
    const previous = siblings[insertIndex - 1];
    const next = siblings[insertIndex];
    const task: Task = {
      id: createTaskId(),
      subjectId: target.subjectId,
      parentId: target.parentId,
      title: "",
      project: target.project,
      planDate: target.planDate,
      actual: 0,
      directMinutes: 0,
      status: "open",
      sortOrder: target.sortOrder,
      version: 0,
      selectable: true,
      executionState: "not_started",
      createPreviousSiblingId: previous?.id,
      createNextSiblingId: next?.id,
    };
    const targetIndex = tasks.findIndex((item) => item.id === target.id);
    const localInsertIndex = placement === "before" ? targetIndex : findSubtreeEndIndex(tasks, target.id);
    tasks.splice(Math.max(0, localInsertIndex), 0, task);
    selectedTaskId.value = task.id;
    return task.id;
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
    if (deletedTask?.version) await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(deletedTask.subjectId), loadTodayOverview()]);
  }

  async function saveTask(taskId: string): Promise<string | false> {
    const task = tasks.find((item) => item.id === taskId);
    if (!task || !task.title.trim()) return false;
    if (savingTaskIds.has(taskId)) return false;
    savingTaskIds.add(taskId);
    try {
      const todayDate = await ensureWorkspaceCalendar();
      if (task.version <= 0) {
        const create = task.quickCreate
          ? createQuickTaskRecord
          : task.createPreviousSiblingId || task.createNextSiblingId
            ? createPositionedTask
            : createTaskRecord;
        const created = await create({
          subjectId: task.subjectId,
          parentId: task.parentId,
          title: task.title,
          plannedDate: task.planDate,
          estimateMinutes: task.estimate,
          note: task.note,
          projectName: task.project,
          ...(task.quickCreate ? {} : {
            previousSiblingId: task.createPreviousSiblingId,
            nextSiblingId: task.createNextSiblingId,
          }),
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
        if (task.todayEstimate) await setTaskDailyEstimate(created.id, todayDate, task.todayEstimate);
        await Promise.all([loadWorkspaceData(), loadTimeData(), loadReports(), reloadTodayPlanForSubject(created.subjectId)]);
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
      await Promise.all([loadWorkspaceData(), loadTimeData(), loadReports(), reloadTodayPlanForSubject(task.subjectId)]);
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

  function addTaskSelectionDraft(title: string, subjectId: string, parentId?: string) {
    const normalizedTitle = title.trim();
    if (!normalizedTitle) throw new Error("事项名称不能为空");
    if (!subjects.some((subject) => subject.id === subjectId)) throw new Error("所选主体不存在");
    if (!parentId) return addQuickTask(normalizedTitle, subjectId);

    const parent = tasks.find((task) => task.id === parentId);
    if (!parent) throw new Error("所选一级事项不存在");
    const unavailableReason = inlineTimerParentUnavailableReason(parent, subjectId);
    if (!canAcceptInlineTimerChild(parent, subjectId)) throw new Error(unavailableReason ?? "所选一级事项不能新建子项");
    const taskId = addChildTask(parent.id, normalizedTitle);
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
    const taskIds = new Set(source.map((task) => task.id));
    for (const task of source) {
      if (task.parentId && taskIds.has(task.parentId)) {
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
    resetUnassignedTracking();
    unassignedCurrentSession.value = undefined;
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
    const result: TimerStopResultRecord = await stopTimerRecord(entry.id, entry.version, false);
    const stopped = replaceTimeEntry(result.currentEntry);
    timerStopConfirmation.value = {
      currentEntryId: stopped.id,
      slices: result.slices.map(toUiTimeEntry),
      totalDurationSeconds: result.totalDurationSeconds,
      totalSettlementMinutes: result.totalSettlementMinutes,
    };
    markUnassignedStateChanged();
    selectedEntryId.value = stopped.id;
    timerStopConfirmationEntryId.value = stopped.id;
    resetUnassignedTracking();
    await notifyTimerStopConfirmation(stopped.id);
    await loadWorkspaceData();
    await loadUnassignedState();
    await loadTodayOverview();
    await loadTimeHistory(timeHistoryPage.value);
    return stopped.id;
  }

  async function movePlanTask(taskId: string, parentId?: string, previousSiblingId?: string, nextSiblingId?: string) {
    const moving = tasks.find((task) => task.id === taskId);
    if (!moving || moving.version <= 0) return;
    const ordered = buildVisibleTaskList(tasks);
    const movingIds = getSubtreeIds(taskId);
    const movingBlock = ordered.filter((task) => movingIds.has(task.id));
    const remaining = ordered.filter((task) => !movingIds.has(task.id));
    moving.parentId = parentId;
    let insertIndex = remaining.length;
    if (nextSiblingId) insertIndex = remaining.findIndex((task) => task.id === nextSiblingId);
    else if (previousSiblingId) insertIndex = findSubtreeEndIndex(remaining, previousSiblingId);
    if (insertIndex < 0) insertIndex = remaining.length;
    tasks.splice(0, tasks.length, ...remaining.slice(0, insertIndex), ...movingBlock, ...remaining.slice(insertIndex));
    try {
      replaceTask(await moveTaskRecord({
        id: moving.id,
        expectedVersion: moving.version,
        parentId,
        previousSiblingId,
        nextSiblingId,
      }));
      await loadWorkspaceData();
    } catch (error) {
      await loadWorkspaceData();
      throw error;
    }
  }

  async function moveExecutionTask(
    taskId: string,
    expectedVersion: number,
    subjectId: string,
    parentId?: string,
    previousSiblingId?: string,
    nextSiblingId?: string,
  ) {
    const moved = await moveTaskRecord({
      id: taskId,
      expectedVersion,
      parentId,
      previousSiblingId,
      nextSiblingId,
    });
    const snapshot = executionSnapshots[subjectId];
    if (snapshot) {
      const movingIds = new Set([taskId]);
      let expanded = true;
      while (expanded) {
        expanded = false;
        for (const task of snapshot.tasks) {
          if (task.parentId && movingIds.has(task.parentId) && !movingIds.has(task.id)) {
            movingIds.add(task.id);
            expanded = true;
          }
        }
      }
      const movingBlock = snapshot.tasks
        .filter((task) => movingIds.has(task.id))
        .map((task) => task.id === taskId ? { ...task, parentId, sortOrder: moved.sortOrder, version: moved.version } : task);
      const remaining = snapshot.tasks.filter((task) => !movingIds.has(task.id));
      let insertIndex = remaining.length;
      if (nextSiblingId) {
        const nextIndex = remaining.findIndex((task) => task.id === nextSiblingId);
        if (nextIndex >= 0) insertIndex = nextIndex;
      } else if (previousSiblingId) {
        const previousIds = new Set([previousSiblingId]);
        let previousExpanded = true;
        while (previousExpanded) {
          previousExpanded = false;
          for (const task of remaining) {
            if (task.parentId && previousIds.has(task.parentId) && !previousIds.has(task.id)) {
              previousIds.add(task.id);
              previousExpanded = true;
            }
          }
        }
        const previousIndex = remaining.findIndex((task) => task.id === previousSiblingId);
        if (previousIndex >= 0) {
          insertIndex = previousIndex + 1;
          while (insertIndex < remaining.length && previousIds.has(remaining[insertIndex].id)) insertIndex += 1;
        }
      } else if (parentId) {
        const parentIndex = remaining.findIndex((task) => task.id === parentId);
        if (parentIndex >= 0) insertIndex = parentIndex + 1;
      }
      snapshot.tasks.splice(0, snapshot.tasks.length, ...remaining.slice(0, insertIndex), ...movingBlock, ...remaining.slice(insertIndex));
    }
    replaceTask(moved);
    const [, todaySnapshot, executionSnapshot] = await Promise.all([
      loadWorkspaceData(),
      loadTodayPlan(subjectId),
      loadExecutionSnapshot(subjectId),
    ]);
    return executionSnapshot ?? todaySnapshot;
  }

  async function saveTaskNote(taskId: string, note: string) {
    const task = tasks.find((item) => item.id === taskId);
    if (!task || task.version <= 0) return;
    const normalized = note.trim() || undefined;
    replaceTask(await updateTaskRecord({
      id: task.id,
      expectedVersion: task.version,
      note: normalized ?? null,
    }));
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(task.subjectId)]);
  }

  async function saveTaskTodayEstimate(taskId: string, estimateMinutes?: number) {
    const task = tasks.find((item) => item.id === taskId);
    if (!task || task.version <= 0) return;
    const todayDate = await ensureWorkspaceCalendar();
    if (task.recurrence && estimateMinutes) {
      replaceTask(await setRecurringTaskEstimate(task.id, task.version, todayDate, estimateMinutes));
    } else {
      await setTaskDailyEstimate(task.id, todayDate, estimateMinutes);
    }
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(task.subjectId), loadTodayOverview()]);
  }

  async function saveTaskEstimateBatch(items: Array<{
    taskId: string;
    expectedTaskVersion: number;
    expectedDailyEstimateVersion?: number;
    estimateMinutes?: number;
  }>) {
    if (!selectedSubjectId.value || !items.length) return;
    const todayDate = await ensureWorkspaceCalendar();
    await setTaskDailyEstimateBatch(selectedSubjectId.value, todayDate, items);
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(selectedSubjectId.value), loadTodayOverview()]);
  }

  async function saveTodayImport(request: {
    taskIds: string[];
    estimates: Array<{
      taskId: string;
      expectedTaskVersion: number;
      expectedDailyEstimateVersion?: number;
      estimateMinutes?: number;
    }>;
  }) {
    const estimateMemberIds = new Set(request.estimates.filter((item) => item.estimateMinutes !== undefined).map((item) => item.taskId));
    if (request.estimates.length) await saveTaskEstimateBatch(request.estimates);
    const membershipOnlyIds = request.taskIds.filter((taskId) => !estimateMemberIds.has(taskId));
    if (membershipOnlyIds.length) await addTasksToToday(membershipOnlyIds);
  }

  async function saveTaskPlanBatch(request: QuickPlanBatchRequest) {
    const result = await applyTaskPlanBatch(request);
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(selectedSubjectId.value), loadTodayOverview()]);
    return result;
  }

  async function loadActualAdjustmentSnapshot(subjectId = selectedSubjectId.value) {
    const workDate = await ensureWorkspaceCalendar();
    return await getActualAdjustmentSnapshot(subjectId, workDate);
  }

  async function loadExecutionSnapshot(subjectId = selectedSubjectId.value) {
    if (!subjectId) return undefined;
    const workDate = await ensureWorkspaceCalendar();
    executionSnapshotLoading[subjectId] = true;
    try {
      const snapshot = await getActualAdjustmentSnapshot(subjectId, workDate);
      executionSnapshots[subjectId] = snapshot;
      return snapshot;
    } finally {
      executionSnapshotLoading[subjectId] = false;
    }
  }

  async function saveExecutionTaskTitle(taskId: string, expectedVersion: number, title: string, subjectId: string) {
    await updateTaskRecord({ id: taskId, expectedVersion, title });
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(subjectId), loadTodayOverview()]);
    return await loadExecutionSnapshot(subjectId);
  }

  async function saveExecutionTaskNote(taskId: string, expectedVersion: number, note: string, subjectId: string) {
    await updateTaskRecord({ id: taskId, expectedVersion, note: note.trim() || null });
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(subjectId)]);
    return await loadExecutionSnapshot(subjectId);
  }

  async function saveExecutionTaskDetails(task: {
    id: string;
    subjectId: string;
    expectedVersion: number;
    title: string;
    plannedDate?: string;
    estimateMinutes?: number;
    note?: string;
    projectName?: string;
  }) {
    await updateTaskRecord({
      id: task.id,
      expectedVersion: task.expectedVersion,
      title: task.title,
      plannedDate: task.plannedDate ?? null,
      estimateMinutes: task.estimateMinutes ?? null,
      note: task.note?.trim() || null,
      projectName: task.projectName?.trim() || null,
    });
    await Promise.all([loadWorkspaceData(), loadTimeData(), reloadTodayPlanForSubject(task.subjectId), loadTodayOverview()]);
    return await loadExecutionSnapshot(task.subjectId);
  }

  async function deleteExecutionTask(taskId: string, expectedVersion: number, subjectId: string) {
    await deleteTaskSubtree(taskId, expectedVersion);
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(subjectId), loadTodayOverview()]);
    return await loadExecutionSnapshot(subjectId);
  }

  async function saveActualAdjustmentBatch(request: ActualAdjustmentBatchRequest) {
    const result = await applyActualAdjustmentBatch(request);
    await Promise.all([loadWorkspaceData(), loadTimeData(), loadTodayOverview(), loadTimeHistory(timeHistoryPage.value)]);
    return result;
  }

  async function removeActualAdjustment(adjustmentId: string, expectedVersion: number) {
    await deleteActualAdjustment(adjustmentId, expectedVersion);
    await Promise.all([loadWorkspaceData(), loadTimeData(), loadTodayOverview(), loadTimeHistory(timeHistoryPage.value)]);
  }

  async function openTimerStopConfirmation(entryId: string) {
    timerStopConfirmation.value = undefined;
    timerStopConfirmationEntryId.value = entryId;
    selectedEntryId.value = entryId;
    await loadTimeData();
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(), loadTodayOverview()]);
  }

  function dismissTimerStopConfirmation() {
    timerStopConfirmationEntryId.value = "";
    timerStopConfirmation.value = undefined;
  }

  async function confirmTimerStopAllocation(completeTask = false) {
    const entry = timerStopConfirmationEntry.value;
    if (!entry) return false;
    if (!entry.defaultTask || timerStopConfirmationSlices.value.every((slice) => slice.minutes <= 0)) return false;
    const taskId = await persistTimerTask(entry.defaultTask);
    if (!taskId) throw new Error("分配事项保存失败");
    const task = tasks.find((candidate) => candidate.id === taskId);
    const completionCandidate = completeTask && task?.status !== "done" ? taskId : undefined;
    for (const slice of timerStopConfirmationSlices.value) {
      const allocations = slice.minutes > 0 ? [{
        taskId,
        minutes: slice.minutes,
        completeTask: completeTask && slice.id === entry.id,
        taskExpectedVersion: completeTask && slice.id === entry.id ? task?.version : undefined,
      }] : [];
      const updated = await replaceTimeAllocations(slice.id, slice.version, allocations);
      const chainSlice = timerStopConfirmation.value?.slices.find((item) => item.id === slice.id);
      if (chainSlice) Object.assign(chainSlice, toUiTimeEntry(updated));
      if (entries.some((item) => item.id === slice.id)) replaceTimeEntry(updated);
    }
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(task?.subjectId), loadTodayOverview()]);
    await loadTimeHistory(timeHistoryPage.value);
    const completedTaskIds = completionCandidate && tasks.find((item) => item.id === completionCandidate)?.status === "done"
      ? [completionCandidate]
      : [];
    timerStopConfirmationEntryId.value = "";
    timerStopConfirmation.value = undefined;
    return { completedCount: completedTaskIds.length, completedTaskIds };
  }

  async function resolveTimerStopDisposition(disposition: "break" | "discard") {
    if (!timerStopConfirmationSlices.value.length) return false;
    for (const slice of timerStopConfirmationSlices.value) {
      await updateTimeEntryDisposition({ entryId: slice.id, expectedVersion: slice.version, disposition });
    }
    await Promise.all([loadTimeData(), loadWorkspaceData()]);
    await loadTodayOverview();
    await loadTimeHistory(timeHistoryPage.value);
    timerStopConfirmationEntryId.value = "";
    timerStopConfirmation.value = undefined;
    return true;
  }

  async function transitionTimer(target: { taskId: string; reopenCompletedTaskVersion?: number } | { newTask: TimerTransitionNewTaskRequest }, note = ""): Promise<TimerTransitionResultRecord> {
    const current = runningEntry.value;
    markTimeDataChanged();
    const result = await transitionTimerRecord({
      currentEntryId: current?.id,
      currentEntryExpectedVersion: current?.version,
      taskId: "taskId" in target ? target.taskId : undefined,
      reopenCompletedTaskVersion: "taskId" in target ? target.reopenCompletedTaskVersion : undefined,
      newTask: "newTask" in target ? target.newTask : undefined,
      note: note.trim() || undefined,
    });
    resetUnassignedTracking();
    unassignedCurrentSession.value = undefined;
    if (result.previousStopResult) {
      for (const slice of result.previousStopResult.slices) replaceTimeEntry(slice);
    }
    replaceTimeEntry(result.activeEntry);
    selectedTaskId.value = result.activeEntry.defaultTaskId ?? result.createdTask?.id ?? "";
    selectedEntryId.value = result.activeEntry.id;
    await Promise.all([
      loadWorkspaceData(),
      loadUnassignedState(),
      loadTodayOverview(),
      loadTodayPlan(("taskId" in target ? tasks.find((task) => task.id === target.taskId)?.subjectId : target.newTask.subjectId) ?? selectedSubjectId.value),
      loadTimeHistory(timeHistoryPage.value),
    ]);
    return result;
  }

  async function allocate(entryId: string, allocations: TimeAllocation[]): Promise<BatchCompletionResult | undefined> {
    const entry = findTimeEntry(entryId);
    if (!entry) return;
    const normalized: Array<{ taskId: string; minutes: number; note?: string; completeTask: boolean; taskExpectedVersion?: number }> = [];
    for (const item of allocations) {
      const minutes = Math.max(0, Math.round(Number(item.minutes) || 0));
      if (!item.taskId || (minutes <= 0 && !item.completeTask)) continue;
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
    await Promise.all([loadWorkspaceData(), reloadTodayPlanForSubject(), loadTodayOverview()]);
    await loadTimeHistory(timeHistoryPage.value);
    const completedTaskIds = [...completionCandidates].filter((taskId) => tasks.find((task) => task.id === taskId)?.status === "done");
    return { completedCount: completedTaskIds.length, completedTaskIds };
  }

  async function addManualEntry(taskId: string | undefined, minutes: number, note = "", completeTask = false) {
    const todayDate = await ensureWorkspaceCalendar();
    const persistedTaskId = await persistTimerTask(taskId);
    if (taskId && !persistedTaskId) throw new Error("补录事项保存失败");
    const completionCandidate = completeTask && persistedTaskId && tasks.find((task) => task.id === persistedTaskId)?.status !== "done"
      ? persistedTaskId
      : undefined;
    const task = persistedTaskId ? tasks.find((candidate) => candidate.id === persistedTaskId) : undefined;
    const entry = toUiTimeEntry(await createManualTimeEntry({
      taskId: persistedTaskId,
      workDate: todayDate,
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
    await loadTimeHistory(1);
    const completedTaskIds = completionCandidate && tasks.find((item) => item.id === completionCandidate)?.status === "done"
      ? [completionCandidate]
      : [];
    return { entryId: entry.id, completedCount: completedTaskIds.length, completedTaskIds };
  }

  async function correctTimeEntry(entryId: string, startedAt: number, endedAt: number, note = "") {
    const entry = findTimeEntry(entryId);
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
    await loadTimeHistory(timeHistoryPage.value);
  }

  async function setTimeEntryDisposition(entryId: string, disposition: "break" | "discard") {
    const entry = findTimeEntry(entryId);
    if (!entry) return;
    markTimeDataChanged();
    const updatedRecord = await updateTimeEntryDisposition({
      entryId,
      expectedVersion: entry.version,
      disposition,
    });
    if (disposition === "discard") {
      const index = entries.findIndex((item) => item.id === entryId);
      if (index >= 0) entries.splice(index, 1);
      if (selectedEntryId.value === entryId) selectedEntryId.value = "";
    } else {
      replaceTimeEntry(updatedRecord);
    }
    await loadWorkspaceData();
    await loadTodayOverview();
    await loadTimeHistory(timeHistoryPage.value);
  }

  async function createReport(request: { type: ReportType; referenceDate: string; subjectId: string; taskIds: string[]; templateId?: string }) {
    const result = await createReportRecord({
      reportType: request.type,
      referenceDate: request.referenceDate,
      subjectId: request.subjectId,
      taskIds: request.taskIds,
      templateId: request.templateId,
    });
    const report = toUiReport(result);
    reports.value.unshift(report);
    return report;
  }

  async function updateReportScope(report: ReportRecord, request: { type: ReportType; referenceDate: string; subjectId: string; taskIds: string[]; templateId?: string }) {
    const updated = toUiReport(await updateReportScopeRecord({
      reportId: report.id,
      reportType: request.type,
      referenceDate: request.referenceDate,
      subjectId: request.subjectId,
      taskIds: request.taskIds,
      templateId: request.templateId,
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
    else if (next.workDate === workspaceToday.value || next.state !== "ended") entries.unshift(next);
    for (const group of timeHistoryGroups) {
      const historyIndex = group.entries.findIndex((entry) => entry.id === record.id);
      if (historyIndex >= 0) group.entries.splice(historyIndex, 1, next);
    }
    return next;
  }

  function findTimeEntry(entryId: string) {
    if (!entryId) return undefined;
    return entries.find((entry) => entry.id === entryId)
      ?? findEntryInHistory(timeHistoryGroups, entryId);
  }

  async function loadReportTemplates(type?: ReportType) {
    return await listReportTemplates(type);
  }

  function cloneTimeEntry(entry: TimeEntry): TimeEntry {
    return {
      ...entry,
      allocations: entry.allocations?.map((allocation) => ({ ...allocation })),
    };
  }

  async function resolveUnassignedTime(action: UnassignedTimeAction, allocations: TimeAllocation[] = []): Promise<BatchCompletionResult | false> {
    const seconds = unassignedSeconds.value;
    if (seconds < 1 || !unassignedSessionId.value) return false;
    const targetSessionId = unassignedSessionId.value;
    const targetVersion = unassignedVersion.value;

    const completionCandidates = new Set<string>();
    if (action === "work") {
      const mergedAllocations = new Map<string, { minutes: number; completeTask: boolean; taskExpectedVersion?: number }>();
      for (const allocation of allocations) {
        const taskId = await persistTimerTask(allocation.taskId);
        if (!taskId) continue;
        const allocationMinutes = Math.max(0, Number(allocation.minutes) || 0);
        if (!allocationMinutes && !allocation.completeTask) continue;
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
      if (!canSubmitUnassignedResolution(targetSessionId, targetVersion, unassignedSessionId.value, unassignedVersion.value)) {
        unassignedDialogOpen.value = false;
        return false;
      }
      await resolveUnassignedWork(targetSessionId, targetVersion, normalizedAllocations);
    } else if (action === "break") {
      if (!canSubmitUnassignedResolution(targetSessionId, targetVersion, unassignedSessionId.value, unassignedVersion.value)) {
        unassignedDialogOpen.value = false;
        return false;
      }
      await resolveUnassignedBreak(targetSessionId, targetVersion);
    } else {
      if (!canSubmitUnassignedResolution(targetSessionId, targetVersion, unassignedSessionId.value, unassignedVersion.value)) {
        unassignedDialogOpen.value = false;
        return false;
      }
      await discardUnassignedTime(targetSessionId, targetVersion);
    }

    unassignedDialogOpen.value = false;
    resetUnassignedTracking();
    await Promise.all([loadTimeData(), loadWorkspaceData(), loadUnassignedState()]);
    await loadTodayOverview();
    await loadTimeHistory(timeHistoryPage.value);
    const completedTaskIds = [...completionCandidates].filter((taskId) => tasks.find((task) => task.id === taskId)?.status === "done");
    return { completedCount: completedTaskIds.length, completedTaskIds };
  }

  return {
    activePage, subjects, selectedSubjectId, selectedSubject, selectedSubjectTasks, tasks, todayTasks, todayPlanTasks, todayPlanLoading, todayPlanSubjectId, todayPlanError, visibleTasks, statusColors, entries, timeHistoryGroups, timeHistoryPage, timeHistoryTotalDays, timeHistoryTotalPages, timeHistoryLoading, timeHistoryRevision, reports, todayOverview, todayOverviewLoading, todayOverviewSubjectId, todayOverviewSubject, selectedTaskId, selectedEntryId, selectedEntry, runningEntry, timerStopConfirmationEntryId, timerStopConfirmationEntry, timerStopConfirmationSlices, timerStopConfirmation,
    planTaskTab, completedTasks, completedTasksSubjectId, completedTasksPage, completedTasksPageSize, completedTasksTotal, completedTasksTotalPages, completedTasksLoading, completedTasksError, completedTasksStale,
    workspaceTimezone, workspaceToday, workspaceDayStartAt, workspaceDayEndAt, refreshWorkspaceCalendar,
    now, todayMinutes, allocatedMinutes, pendingMinutes, doneCount, todayCompletionDoneCount, todayCompletionTotalCount, todayCompletionPercent, unassignedSeconds, unassignedStartedAt,
    unassignedFirstStartedAt, unassignedLastEndedAt, unassignedDialogOpen, unassignedWorkDate, unassignedSessionId, unassignedVersion,
    unassignedCurrentSession,
    workspaceLoaded, workspaceLoading, executionSnapshots, executionSnapshotLoading, loadExecutionSnapshot, saveExecutionTaskTitle, saveExecutionTaskNote, saveExecutionTaskDetails, deleteExecutionTask, loadWorkspaceData, loadTodayPlan, setTodayPlanMembership, addTasksToToday, loadCompletedTasks, selectPlanTaskTab, selectCompletedTasksPage, markCompletedTasksStale, reopenCompletedTask, saveCompletedTask, deleteCompletedTask, loadTimeData, loadTimeHistory, selectTimeHistoryPage, loadUnassignedState, loadReports, loadTodayOverview, selectTodayOverviewSubject, entryDurationSeconds, taskPathLabel, taskDisplayLabel, timeEntryLabel, startClock, stopClock, selectPage, selectSubject, addSubject, renameSubject, commitTaskToggle, toggleTask, setTaskRecurrence, addTask, addQuickTask, addTaskSelectionDraft, addChildTask, addTaskRelative, saveTask, saveTaskNote, saveTaskTodayEstimate, saveTaskEstimateBatch, saveTodayImport, saveTaskPlanBatch, loadActualAdjustmentSnapshot, saveActualAdjustmentBatch, removeActualAdjustment, duplicateTask, deleteTask, moveTask, reorderTask, movePlanTask,
    indentTask, outdentTask, moveExecutionTask, startTimer, transitionTimer, pauseTimer, resumeTimer, stopTimer, openTimerStopConfirmation, dismissTimerStopConfirmation, confirmTimerStopAllocation, resolveTimerStopDisposition, allocate, addManualEntry, correctTimeEntry, setTimeEntryDisposition, createReport, updateReportScope, saveReportContent, regenerateReport, deleteReport, getReportTemplate, saveReportTemplate, loadReportTemplates, createReportTemplate, updateReportTemplate, duplicateReportTemplate, setDefaultReportTemplate, deleteReportTemplate, reorderReportTemplates, getReportTemplatePlaceholders, publicReportText, previewObsidianReport, writeObsidianReport,
    beginUnassignedTracking, pauseUnassignedTracking, promptUnassignedResolution, dismissUnassignedResolution, resolveUnassignedTime
  };
});
