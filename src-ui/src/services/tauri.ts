import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type SettingsScope = "shared" | "device";
export type SettingsSnapshot = {
  storageMode: "local" | "cloud";
  workspaceId: string;
  deviceId: string;
  shared: Record<string, unknown>;
  device: Record<string, unknown>;
  integrations: Record<string, {
    enabled: boolean;
    config: Record<string, unknown>;
    hasSecret: boolean;
    version: number;
  }>;
  secrets: {
    seatableTokenSet: boolean;
    supabaseSessionSet: boolean;
  };
};

export type AppUpdateSummary = {
  version: string;
  currentVersion: string;
  notes?: string;
};

export type AppUpdateCheckResult = {
  available: boolean;
  currentVersion: string;
  update?: AppUpdateSummary;
};

export type AppUpdateEventPayload = {
  stage: "download_started" | "download_progress" | "download_finished" | "installed" | "failed" | string;
  downloadedBytes?: number;
  chunkLength?: number;
  contentLength?: number;
  message?: string;
};

export type AutomationHookEvent = "timer.started" | "timer.stopped" | "task.completed";
export type AutomationHookAction = "uri" | "process";

export type AutomationHookDraft = {
  name: string;
  eventType: AutomationHookEvent;
  actionType: AutomationHookAction;
  actionConfig: {
    uriTemplate?: string;
    program?: string;
    args?: string[];
    workingDirectory?: string;
  };
  timeoutSeconds: number;
  enabled: boolean;
};

export type AutomationHookRun = {
  id: string;
  hookId?: string;
  hookNameSnapshot: string;
  eventId: string;
  eventType: AutomationHookEvent;
  isTest: boolean;
  status: "queued" | "running" | "succeeded" | "failed" | "timed_out";
  exitCode?: number;
  durationMs?: number;
  stdoutTail?: string;
  stderrTail?: string;
  errorMessage?: string;
  startedAt?: number;
  finishedAt?: number;
  createdAt: number;
};

export type AutomationHookRule = AutomationHookDraft & {
  id: string;
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
  recentRun?: AutomationHookRun;
};

export type CloudSessionSnapshot = {
  signedIn: boolean;
  userId?: string;
  email?: string;
  expiresAt?: number;
};

export type StorageModeSnapshot = {
  mode: "local" | "cloud";
  workspaceId: string;
  deviceId: string;
  online: boolean;
  syncState: "synced" | "pending" | "error" | string;
  pendingOperations: number;
  conflictCount: number;
  lastChangeSeq: number;
};

export type StorageMigrationPreview = {
  direction: "local_to_cloud" | "cloud_to_local_snapshot";
  sourceWorkspaceId: string;
  targetWorkspaceId?: string;
  entities: Array<{ entity: string; localCount: number; cloudCount?: number }>;
  conflicts: string[];
  canExecute: boolean;
};

export type CloudSyncConflict = {
  operationId: string;
  operationType: string;
  entityType: string;
  entityId?: string;
  baseVersion?: number;
  localPayload: Record<string, unknown> | null;
  error?: string;
  attemptCount: number;
  createdAt: number;
  lastAttemptAt?: number;
};

export type SubjectRecord = {
  id: string;
  name: string;
  sortOrder: number;
  openTaskCount: number;
  version: number;
};

export type TaskRecord = {
  id: string;
  subjectId: string;
  parentId?: string;
  title: string;
  status: "open" | "done";
  plannedDate?: string;
  estimateMinutes?: number;
  todayEstimateMinutes?: number;
  note?: string;
  projectName?: string;
  solutionName?: string;
  sortOrder: number;
  depth: number;
  pathLabel: string;
  selectable: boolean;
  directMinutes: number;
  totalMinutes: number;
  executionState: "not_started" | "started" | "running" | "paused" | "done";
  recurrence?: RecurrenceRuleRecord;
  occurrenceDate?: string;
  occurrenceOrigin?: "scheduled" | "manual";
  occurrenceVersion?: number;
  version: number;
};

export type RecurrenceRuleRecord = {
  id: string;
  taskId: string;
  frequency: "daily" | "weekdays" | "weekly";
  weekdaysMask?: number;
  effectiveStart: string;
  effectiveEnd?: string;
  summary: string;
  version: number;
};

export type TaskListResult = {
  tasks: TaskRecord[];
  revision: number;
};

export type PlanImportPreviewItem = {
  id: string;
  parentId?: string;
  title: string;
  estimateMinutes?: number;
  sortOrder: number;
  sourceLineNo: number;
  sourceText: string;
  parseStatus: "recognized" | "unrecognized";
};

export type PlanImportPreviewResult = {
  batchId: string;
  sourcePath: string;
  sourceExists: boolean;
  sourceMtime?: number;
  sourceContentHash?: string;
  rawMarkdown: string;
  items: PlanImportPreviewItem[];
  warnings: string[];
  version: number;
};

export type TimeAllocationRecord = {
  id: string;
  taskId: string;
  taskTitle: string;
  minutes: number;
  note?: string;
  version: number;
};

export type TimeEntryRecord = {
  id: string;
  workDate: string;
  label: string;
  startedAt: number;
  endedAt?: number;
  durationSeconds: number;
  settlementMinutes: number;
  allocatedMinutes: number;
  defaultTaskId?: string;
  note?: string;
  kind: "work" | "break";
  sourceType: "timer" | "manual" | "unassigned" | string;
  state: "running" | "paused" | "ended";
  version: number;
  allocations: TimeAllocationRecord[];
};

export type TimeEntryListResult = {
  entries: TimeEntryRecord[];
  rawMinutes: number;
  allocatedMinutes: number;
  pendingMinutes: number;
  breakMinutes: number;
  revision: number;
};

export type TodayWorkOverviewScopeRecord = {
  subjectId?: string;
  subjectName: string;
  todayDate: string;
};

export type TodayWorkOverviewSummaryRecord = {
  actualMinutes: number;
  estimatedMinutes: number;
  completedCount: number;
  activeCount: number;
  notStartedCount: number;
  unassignedMinutes: number;
  completionRate: number;
};

export type TodayWorkOverviewDayRecord = {
  date: string;
  actualMinutes: number;
  estimatedMinutes: number;
  completedCount: number;
  totalCount: number;
  unassignedMinutes: number;
};

export type TodayWorkOverviewTimelineItemRecord = {
  id: string;
  itemType: "work" | "break" | "unassigned" | string;
  taskId?: string;
  taskTitle: string;
  subjectId?: string;
  subjectName?: string;
  startedAt: number;
  endedAt?: number;
  minutes: number;
  state?: "running" | "paused" | "ended" | "collecting" | "awaiting_resolution" | string;
};

export type TodayWorkOverviewTaskRecord = {
  taskId: string;
  title: string;
  pathLabel: string;
  subjectId: string;
  subjectName: string;
  status: "done" | "active" | "started" | "not_started" | string;
  actualMinutes: number;
  todayEstimateMinutes?: number;
  estimateMinutes?: number;
  lastEntryAt?: number;
  selectable: boolean;
};

export type TodayWorkOverviewRecord = {
  scope: TodayWorkOverviewScopeRecord;
  summary: TodayWorkOverviewSummaryRecord;
  days: TodayWorkOverviewDayRecord[];
  timeline: TodayWorkOverviewTimelineItemRecord[];
  tasks: TodayWorkOverviewTaskRecord[];
  revision: number;
};

export type UnassignedStateRecord = {
  sessionId: string;
  state: "collecting" | "awaiting_resolution";
  firstStartedAt: number;
  lastEndedAt?: number;
  currentSegmentStartedAt?: number;
  elapsedSeconds: number;
  requiredMinutes: number;
  thresholdSeconds: number;
  mustResolve: boolean;
  version: number;
};

export type UnassignedResolveResult = {
  sessionId: string;
  generatedEntryId?: string;
  resolutionType: "work" | "break" | "discard";
  elapsedSeconds: number;
  requiredMinutes: number;
};

export type ReportRecordDto = {
  id: string;
  reportType: "daily" | "weekly" | "monthly";
  subjectId: string;
  subjectName: string;
  periodStart: string;
  periodEnd: string;
  period: string;
  referenceDate: string;
  taskIds: string[];
  markdown: string;
  contentSource: "generated" | "edited";
  generatedCount: number;
  updatedAt: number;
  version: number;
};

export type ReportTemplateRecord = {
  id: string;
  reportType: "daily" | "weekly" | "monthly";
  subjectId?: string;
  content: string;
  isBuiltin: boolean;
  version: number;
};

export type ReportTaskSuggestion = {
  taskId: string;
  actualMinutes: number;
  dailyEstimateMinutes?: number;
};

export type ObsidianReportWritePreview = {
  runId: string;
  reportId: string;
  reportVersion: number;
  targetPath: string;
  fileExists: boolean;
  existingContent: string;
  existingContentHash?: string;
  existingFileMtime?: number;
  reportMarkdown: string;
  action: "create" | "update";
};

export type ObsidianReportWriteResult = {
  runId: string;
  reportId: string;
  targetPath: string;
  strategy: "overwrite" | "append";
  writtenAt: number;
};

export type SeaTableConnectionResult = {
  baseName: string;
  taskTable: string;
  reimbursementTable: string;
  reimbursementAvailable: boolean;
  warnings: string[];
};

export type SeaTableSyncPreviewItem = {
  itemId: string;
  taskId: string;
  title: string;
  action: "create" | "update" | "skip" | "conflict";
  reason: string;
  externalId?: string;
  row: Record<string, unknown>;
};

export type SeaTableSyncPreview = {
  runId: string;
  tableName: string;
  workDate: string;
  subjectName: string;
  items: SeaTableSyncPreviewItem[];
  createCount: number;
  updateCount: number;
  skipCount: number;
  conflictCount: number;
};

export type SeaTableSyncResult = {
  runId: string;
  state: "succeeded" | "partial" | "failed";
  successCount: number;
  failedCount: number;
  skippedCount: number;
};

export type ReimbursementResult = {
  subjectId: string;
  subjectName: string;
  periodStart: string;
  periodEnd: string;
  total: number;
  items: Array<{ description: string; amount: number }>;
};

export async function listSubjects(): Promise<SubjectRecord[]> {
  return await invoke<SubjectRecord[]>("subject_list");
}

export async function createSubject(name: string): Promise<{ subject: SubjectRecord; alreadyExists: boolean }> {
  return await invoke("subject_create", { request: { name } });
}

export async function renameSubject(subjectId: string, name: string, expectedVersion: number): Promise<SubjectRecord> {
  return await invoke<SubjectRecord>("subject_rename", { request: { subjectId, name, expectedVersion } });
}

export async function listTasks(subjectId?: string, plannedDate?: string): Promise<TaskListResult> {
  const dailyEstimateDate = formatLocalDate(new Date());
  return await invoke<TaskListResult>("task_list", {
    request: { subjectId, includeCompleted: true, plannedDate, todayDate: dailyEstimateDate, todayOnly: false, dailyEstimateDate, query: null },
  });
}

export async function listTodayTasks(subjectId?: string, todayDate?: string): Promise<TaskListResult> {
  return await invoke<TaskListResult>("task_list", {
    request: { subjectId, includeCompleted: true, plannedDate: null, todayDate, todayOnly: true, dailyEstimateDate: todayDate, query: null },
  });
}

export async function setTaskDailyEstimate(taskId: string, workDate: string, estimateMinutes?: number): Promise<void> {
  await invoke("task_daily_estimate_set", { request: { taskId, workDate, estimateMinutes } });
}

export async function createQuickTask(request: {
  subjectId: string;
  parentId?: string;
  title: string;
  plannedDate?: string;
  estimateMinutes?: number;
  note?: string;
  projectName?: string;
  solutionName?: string;
}): Promise<TaskRecord> {
  return await invoke<TaskRecord>("task_create_quick", { request });
}

export async function createTask(request: {
  subjectId: string;
  parentId?: string;
  title: string;
  plannedDate?: string;
  estimateMinutes?: number;
  note?: string;
  projectName?: string;
  solutionName?: string;
}): Promise<TaskRecord> {
  return await invoke<TaskRecord>("task_create", { request });
}

export async function updateTask(request: {
  id: string;
  expectedVersion: number;
  title?: string;
  parentId?: string | null;
  plannedDate?: string | null;
  estimateMinutes?: number | null;
  note?: string | null;
  projectName?: string | null;
  solutionName?: string | null;
}): Promise<TaskRecord> {
  return await invoke<TaskRecord>("task_update", { request });
}

export async function setTaskCompleted(
  id: string,
  expectedVersion: number,
  done: boolean,
  occurrenceDate?: string,
  occurrenceExpectedVersion?: number,
): Promise<TaskRecord> {
  return await invoke<TaskRecord>("task_set_completed", {
    request: { id, expectedVersion, done, occurrenceDate, occurrenceExpectedVersion },
  });
}

export async function saveTaskRecurrence(request: {
  taskId: string;
  taskExpectedVersion: number;
  frequency: RecurrenceRuleRecord["frequency"];
  weekdaysMask?: number;
  effectiveStart: string;
  ruleExpectedVersion?: number;
}): Promise<RecurrenceRuleRecord> {
  return await invoke("task_recurrence_save", { request });
}

export async function closeTaskRecurrence(request: {
  taskId: string;
  taskExpectedVersion: number;
  effectiveEnd: string;
  ruleExpectedVersion: number;
}): Promise<void> {
  await invoke("task_recurrence_close", { request });
}

function formatLocalDate(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export async function deleteTaskSubtree(id: string, expectedVersion: number): Promise<void> {
  await invoke("task_delete_subtree", { request: { id, expectedVersion } });
}

export async function reorderTaskSubtree(id: string, expectedVersion: number, parentId: string | undefined, sortOrder: number): Promise<TaskRecord> {
  return await invoke<TaskRecord>("task_reorder_subtree", { request: { id, expectedVersion, parentId, sortOrder } });
}

export async function changeTaskParent(id: string, expectedVersion: number, newParentId?: string, sortOrder?: number): Promise<TaskRecord> {
  return await invoke<TaskRecord>("task_change_parent", { request: { id, expectedVersion, newParentId, sortOrder } });
}

export async function duplicateTaskSubtree(id: string, expectedVersion: number): Promise<{ rootId: string; tasks: TaskRecord[] }> {
  return await invoke("task_duplicate_subtree", { request: { id, expectedVersion } });
}

export async function previewObsidianPlan(sourceDate: string, targetDate: string, subjectId: string): Promise<PlanImportPreviewResult> {
  return await invoke<PlanImportPreviewResult>("obsidian_plan_import_preview", {
    request: { sourceDate, targetDate, subjectId },
  });
}

export async function confirmObsidianPlan(
  batchId: string,
  items: Array<{ importItemId: string; selected: boolean; title: string; estimateMinutes?: number }>,
  expectedVersion: number,
): Promise<{ batchId: string; tasks: TaskRecord[]; alreadyConfirmed: boolean }> {
  return await invoke("obsidian_plan_import_confirm", { request: { batchId, items, expectedVersion } });
}

export async function getTimerState(): Promise<TimeEntryRecord | null> {
  if ((await getStorageMode()).mode === "cloud") {
    return normalizeCloudTimerRecord(await invoke<Record<string, unknown> | null>("cloud_timer_get_state"));
  }
  return await invoke<TimeEntryRecord | null>("timer_get_state");
}

export async function startTimerRecord(taskId?: string, note?: string): Promise<TimeEntryRecord> {
  if ((await getStorageMode()).mode === "cloud") {
    return normalizeCloudTimerRecord(await invoke<Record<string, unknown>>("cloud_timer_start", {
      request: { taskId, note, operationId: crypto.randomUUID() },
    }))!;
  }
  return await invoke<TimeEntryRecord>("timer_start", {
    request: { taskId, note, clientRequestId: crypto.randomUUID() },
  });
}

export async function pauseTimerRecord(entryId: string, expectedVersion: number): Promise<TimeEntryRecord> {
  if ((await getStorageMode()).mode === "cloud") {
    return normalizeCloudTimerRecord(await invoke<Record<string, unknown>>("cloud_timer_pause", {
      request: { entryId, expectedVersion, operationId: crypto.randomUUID() },
    }))!;
  }
  return await invoke<TimeEntryRecord>("timer_pause", { request: { entryId, expectedVersion } });
}

export async function resumeTimerRecord(entryId: string, expectedVersion: number): Promise<TimeEntryRecord> {
  if ((await getStorageMode()).mode === "cloud") {
    return normalizeCloudTimerRecord(await invoke<Record<string, unknown>>("cloud_timer_resume", {
      request: { entryId, expectedVersion, operationId: crypto.randomUUID() },
    }))!;
  }
  return await invoke<TimeEntryRecord>("timer_resume", {
    request: { entryId, expectedVersion, operationId: crypto.randomUUID() },
  });
}

export async function stopTimerRecord(
  entryId: string,
  expectedVersion: number,
  createDefaultAllocation = true,
): Promise<TimeEntryRecord> {
  if ((await getStorageMode()).mode === "cloud") {
    return normalizeCloudTimerRecord(await invoke<Record<string, unknown>>("cloud_timer_stop", {
      request: { entryId, expectedVersion, operationId: crypto.randomUUID() },
    }))!;
  }
  return await invoke<TimeEntryRecord>("timer_stop", {
    request: { entryId, expectedVersion, createDefaultAllocation },
  });
}

function normalizeCloudTimerRecord(value: Record<string, unknown> | null): TimeEntryRecord | null {
  if (!value) return null;
  const startedAt = typeof value.started_at === "string" ? Date.parse(value.started_at) : Number(value.started_at ?? 0);
  const endedAt = typeof value.ended_at === "string" ? Date.parse(value.ended_at) : Number(value.ended_at ?? 0);
  const durationSeconds = Number(value.duration_seconds ?? 0);
  return {
    id: String(value.id ?? ""),
    workDate: String(value.work_date ?? ""),
    label: String(value.label_snapshot ?? "未命名事项"),
    startedAt,
    endedAt: endedAt || undefined,
    durationSeconds,
    settlementMinutes: durationSeconds > 0 ? Math.max(1, Math.ceil(durationSeconds / 60)) : 0,
    allocatedMinutes: 0,
    defaultTaskId: value.default_task_id ? String(value.default_task_id) : undefined,
    note: value.note ? String(value.note) : undefined,
    kind: value.kind === "break" ? "break" : "work",
    sourceType: String(value.source_type ?? "timer"),
    state: value.state === "paused" ? "paused" : value.state === "ended" ? "ended" : "running",
    version: Number(value.version ?? 1),
    allocations: [],
  };
}

function normalizeCloudUnassignedState(value: Record<string, unknown> | null): UnassignedStateRecord | null {
  if (!value) return null;
  const timestamp = (candidate: unknown) => typeof candidate === "string" ? Date.parse(candidate) : Number(candidate ?? 0);
  return {
    sessionId: String(value.session_id ?? value.sessionId ?? ""),
    state: value.state === "awaiting_resolution" ? "awaiting_resolution" : "collecting",
    firstStartedAt: timestamp(value.first_started_at ?? value.firstStartedAt),
    lastEndedAt: value.last_ended_at || value.lastEndedAt ? timestamp(value.last_ended_at ?? value.lastEndedAt) : undefined,
    currentSegmentStartedAt: value.current_segment_started_at || value.currentSegmentStartedAt ? timestamp(value.current_segment_started_at ?? value.currentSegmentStartedAt) : undefined,
    elapsedSeconds: Number(value.elapsed_seconds ?? value.elapsedSeconds ?? value.duration_seconds ?? 0),
    requiredMinutes: Number(value.required_minutes ?? value.requiredMinutes ?? 0),
    thresholdSeconds: Number(value.threshold_seconds ?? value.thresholdSeconds ?? 300),
    mustResolve: Boolean(value.must_resolve ?? value.mustResolve ?? value.state === "awaiting_resolution"),
    version: Number(value.version ?? 1),
  };
}

export async function listTimeEntries(workDate: string, includeBreaks = true): Promise<TimeEntryListResult> {
  return await invoke<TimeEntryListResult>("time_entry_list", { request: { workDate, includeBreaks } });
}

export async function getTodayWorkOverview(request: {
  subjectId?: string;
  todayDate: string;
  historyDays?: number;
  trendDays?: number;
}): Promise<TodayWorkOverviewRecord> {
  return await invoke<TodayWorkOverviewRecord>("today_work_overview_get", { request });
}

export async function createManualTimeEntry(request: {
  taskId?: string;
  workDate: string;
  startedAt?: number;
  endedAt?: number;
  minutes?: number;
  note?: string;
}): Promise<TimeEntryRecord> {
  return await invoke<TimeEntryRecord>("time_entry_create_manual", {
    request: { ...request, clientRequestId: crypto.randomUUID() },
  });
}

export async function updateTimeEntry(request: {
  entryId: string;
  startedAt: number;
  endedAt: number;
  note?: string;
  expectedVersion: number;
}): Promise<TimeEntryRecord> {
  return await invoke<TimeEntryRecord>("time_entry_update", { request });
}

export async function replaceTimeAllocations(
  entryId: string,
  expectedVersion: number,
  allocations: Array<{ taskId: string; minutes: number; note?: string; completeTask?: boolean; taskExpectedVersion?: number }>,
): Promise<TimeEntryRecord> {
  return await invoke<TimeEntryRecord>("time_allocation_replace", {
    request: { entryId, expectedVersion, allocations },
  });
}

export async function getUnassignedState(): Promise<UnassignedStateRecord | null> {
  if ((await getStorageMode()).mode === "cloud") {
    return normalizeCloudUnassignedState(await invoke<Record<string, unknown> | null>("cloud_unassigned_get_state"));
  }
  return await invoke<UnassignedStateRecord | null>("unassigned_get_state");
}

export async function resolveUnassignedWork(
  sessionId: string,
  expectedVersion: number,
  allocations: Array<{ taskId: string; minutes: number; completeTask?: boolean; taskExpectedVersion?: number }>,
): Promise<UnassignedResolveResult> {
  if ((await getStorageMode()).mode === "cloud") {
    return await invoke("cloud_unassigned_resolve_work", { request: { sessionId, expectedVersion, allocations, operationId: crypto.randomUUID() } });
  }
  return await invoke<UnassignedResolveResult>("unassigned_resolve_work", {
    request: { sessionId, expectedVersion, allocations, operationId: crypto.randomUUID() },
  });
}

export async function resolveUnassignedBreak(
  sessionId: string,
  expectedVersion: number,
): Promise<UnassignedResolveResult> {
  if ((await getStorageMode()).mode === "cloud") {
    return await invoke("cloud_unassigned_resolve_break", { request: { sessionId, expectedVersion, operationId: crypto.randomUUID() } });
  }
  return await invoke<UnassignedResolveResult>("unassigned_resolve_break", {
    request: { sessionId, expectedVersion, operationId: crypto.randomUUID() },
  });
}

export async function discardUnassignedTime(
  sessionId: string,
  expectedVersion: number,
): Promise<UnassignedResolveResult> {
  if ((await getStorageMode()).mode === "cloud") {
    return await invoke("cloud_unassigned_discard", { request: { sessionId, expectedVersion, operationId: crypto.randomUUID() } });
  }
  return await invoke<UnassignedResolveResult>("unassigned_discard", {
    request: { sessionId, expectedVersion, operationId: crypto.randomUUID() },
  });
}

export async function listReports(request: {
  reportType?: "all" | "daily" | "weekly" | "monthly";
  subjectId?: string;
  query?: string;
} = {}): Promise<{ reports: ReportRecordDto[]; revision: number }> {
  return await invoke("report_list", { request: {
    reportType: request.reportType ?? "all",
    subjectId: request.subjectId,
    query: request.query,
  } });
}

export async function getReportTaskSuggestions(request: {
  reportType: "daily" | "weekly" | "monthly";
  referenceDate: string;
  subjectId: string;
}): Promise<ReportTaskSuggestion[]> {
  return await invoke("report_task_suggestions", { request });
}

export async function createReport(request: {
  reportType: "daily" | "weekly" | "monthly";
  referenceDate: string;
  subjectId: string;
  taskIds: string[];
}): Promise<ReportRecordDto> {
  return await invoke("report_create", { request: { ...request, clientRequestId: crypto.randomUUID() } });
}

export async function updateReportScope(request: {
  reportId: string;
  reportType: "daily" | "weekly" | "monthly";
  referenceDate: string;
  subjectId: string;
  taskIds: string[];
  expectedVersion: number;
}): Promise<ReportRecordDto> {
  return await invoke("report_update_scope", { request });
}

export async function saveReportContent(reportId: string, markdown: string, expectedVersion: number): Promise<ReportRecordDto> {
  return await invoke("report_save_content", { request: { reportId, markdown, expectedVersion } });
}

export async function regenerateReport(reportId: string, expectedVersion: number): Promise<ReportRecordDto> {
  return await invoke("report_regenerate", { request: { reportId, expectedVersion } });
}

export async function deleteReport(reportId: string, expectedVersion: number): Promise<void> {
  await invoke("report_delete", { request: { reportId, expectedVersion } });
}

export async function getPublicReportText(reportId: string, expectedVersion: number): Promise<string> {
  const result = await invoke<{ reportId: string; markdown: string }>("report_public_text", {
    request: { reportId, expectedVersion },
  });
  return result.markdown;
}

export async function getReportTemplate(reportType: "daily" | "weekly" | "monthly", subjectId?: string): Promise<ReportTemplateRecord> {
  return await invoke("report_template_get", { request: { reportType, subjectId } });
}

export async function saveReportTemplate(request: {
  reportType: "daily" | "weekly" | "monthly";
  subjectId?: string;
  content: string;
  expectedVersion?: number;
}): Promise<ReportTemplateRecord> {
  return await invoke("report_template_save", { request });
}

export async function previewObsidianReportWrite(
  reportId: string,
  expectedVersion: number,
): Promise<ObsidianReportWritePreview> {
  return await invoke("obsidian_report_write_preview", { request: { reportId, expectedVersion } });
}

export async function executeObsidianReportWrite(
  runId: string,
  strategy: "overwrite" | "append",
): Promise<ObsidianReportWriteResult> {
  return await invoke("obsidian_report_write_execute", { request: { runId, strategy } });
}

export async function getSettings(): Promise<SettingsSnapshot> {
  return await invoke<SettingsSnapshot>("settings_get");
}

export async function listAutomationHooks(): Promise<AutomationHookRule[]> {
  return await invoke("automation_hook_list");
}

export async function createAutomationHook(request: AutomationHookDraft): Promise<AutomationHookRule> {
  return await invoke("automation_hook_create", { request });
}

export async function updateAutomationHook(id: string, draft: AutomationHookDraft): Promise<AutomationHookRule> {
  return await invoke("automation_hook_update", { request: { id, ...draft } });
}

export async function setAutomationHookEnabled(id: string, enabled: boolean): Promise<AutomationHookRule> {
  return await invoke("automation_hook_set_enabled", { request: { id, enabled } });
}

export async function reorderAutomationHooks(hookIds: string[]): Promise<AutomationHookRule[]> {
  return await invoke("automation_hook_reorder", { request: { hookIds } });
}

export async function deleteAutomationHook(id: string): Promise<void> {
  await invoke("automation_hook_delete", { request: { id } });
}

export async function testAutomationHook(request: AutomationHookDraft): Promise<AutomationHookRun> {
  return await invoke("automation_hook_test", { request });
}

export async function listAutomationHookRuns(hookId?: string, limit = 50): Promise<AutomationHookRun[]> {
  return await invoke("automation_hook_run_list", { request: { hookId, limit } });
}

export async function updateSetting(scope: SettingsScope, key: string, value: unknown): Promise<SettingsSnapshot> {
  return await invoke<SettingsSnapshot>("settings_update", { request: { scope, key, value } });
}

export async function updateIntegrationConfig(
  provider: string,
  enabled: boolean,
  config: Record<string, unknown>,
  expectedVersion?: number,
): Promise<SettingsSnapshot> {
  return await invoke<SettingsSnapshot>("integration_config_update", {
    request: { provider, enabled, config, expectedVersion },
  });
}

export async function getAppVersion(): Promise<string> {
  return await invoke<string>("get_app_version");
}

export async function checkAppUpdate(): Promise<AppUpdateCheckResult> {
  return await invoke<AppUpdateCheckResult>("check_app_update");
}

export async function downloadAndInstallUpdate(): Promise<void> {
  await invoke("download_and_install_update");
}

export async function restartApp(): Promise<void> {
  await invoke("restart_app");
}

export async function setIntegrationSecret(provider: "seatable" | "supabase", value: string): Promise<SettingsSnapshot> {
  return await invoke<SettingsSnapshot>("integration_secret_set", { request: { provider, value } });
}

export async function clearIntegrationSecret(provider: "seatable" | "supabase"): Promise<SettingsSnapshot> {
  return await invoke<SettingsSnapshot>("integration_secret_clear", { provider });
}

export async function getStorageMode(): Promise<StorageModeSnapshot> {
  return await invoke("storage_mode_get");
}

export async function setLocalStorageMode(): Promise<StorageModeSnapshot> {
  return await invoke("storage_mode_set_local");
}

export async function configureCloud(projectUrl: string, anonKey: string): Promise<StorageModeSnapshot> {
  return await invoke("cloud_configure", { request: { projectUrl, anonKey } });
}

export async function getCloudSession(): Promise<CloudSessionSnapshot> {
  return await invoke("cloud_session_get");
}

export async function signInCloud(email: string, password: string): Promise<CloudSessionSnapshot> {
  return await invoke("cloud_sign_in_password", { request: { email, password } });
}

export async function signOutCloud(): Promise<StorageModeSnapshot> {
  return await invoke("cloud_sign_out");
}

export async function registerCloudDevice(deviceName: string): Promise<unknown> {
  return await invoke("cloud_device_register", {
    request: { deviceName, platform: "windows", appVersion: __APP_VERSION__ },
  });
}

export async function previewStorageMigration(direction: StorageMigrationPreview["direction"] = "local_to_cloud"): Promise<StorageMigrationPreview> {
  return await invoke("storage_migration_preview", { request: { direction } });
}

export async function executeStorageMigration(direction: StorageMigrationPreview["direction"] = "local_to_cloud"): Promise<StorageModeSnapshot> {
  return await invoke("storage_migration_execute", {
    request: { direction, confirmed: true },
  });
}

export async function getCloudSyncStatus(): Promise<StorageModeSnapshot> {
  return await invoke("cloud_sync_status");
}

export async function pushCloudSync(): Promise<{ pushed: number; pending: number; conflicts: number }> {
  return await invoke("cloud_sync_push");
}

export async function listCloudSyncConflicts(): Promise<CloudSyncConflict[]> {
  return await invoke("cloud_sync_conflicts");
}

export async function resolveCloudSyncConflict(
  operationId: string,
  strategy: "use_cloud" | "keep_local",
): Promise<StorageModeSnapshot> {
  return await invoke("cloud_sync_resolve_conflict", { request: { operationId, strategy } });
}

export async function testSeaTableConnection(): Promise<SeaTableConnectionResult> {
  return await invoke("seatable_connection_test");
}

export async function previewSeaTableTaskSync(request: {
  subjectId: string;
  workDate: string;
  taskIds: string[];
}): Promise<SeaTableSyncPreview> {
  return await invoke("seatable_task_sync_preview", { request });
}

export async function executeSeaTableTaskSync(runId: string): Promise<SeaTableSyncResult> {
  return await invoke("seatable_task_sync_execute", { request: { runId } });
}

export async function retryFailedSeaTableSync(runId: string): Promise<SeaTableSyncResult> {
  return await invoke("seatable_sync_retry_failed", { request: { runId } });
}

export async function querySeaTableReimbursements(request: {
  subjectId: string;
  periodStart: string;
  periodEnd: string;
}): Promise<ReimbursementResult> {
  return await invoke("seatable_reimbursements_query", { request });
}

export async function showWindow(label: string) {
  try { await invoke("window_show", { label }); } catch { await getCurrentWindow().show(); }
}

export async function hideWindow(label: string) {
  try { await invoke("window_hide", { label }); } catch { await getCurrentWindow().hide(); }
}

export async function openMainOverview() {
  try { await invoke("window_open_main_overview"); } catch { await getCurrentWindow().show(); }
}

export async function closeMainWindow() {
  try { await invoke("window_close_main"); } catch { await getCurrentWindow().hide(); }
}

export async function startMainWindowDragging() {
  try { await invoke("window_start_dragging_main"); } catch { await getCurrentWindow().startDragging(); }
}

export async function minimizeMainWindow() {
  try { await invoke("window_minimize_main"); } catch { await getCurrentWindow().minimize(); }
}

export async function toggleMainWindowMaximize() {
  try {
    return await invoke<boolean>("window_toggle_maximize_main");
  } catch {
    const window = getCurrentWindow();
    await window.toggleMaximize();
    return await window.isMaximized();
  }
}

export async function isMainWindowMaximized() {
  try { return await invoke<boolean>("window_is_main_maximized"); } catch {
    try { return await getCurrentWindow().isMaximized(); } catch { return false; }
  }
}

export async function onCurrentWindowResized(handler: (maximized: boolean) => void) {
  try {
    const window = getCurrentWindow();
    return await window.onResized(async () => handler(await isMainWindowMaximized()));
  } catch {
    return () => {};
  }
}

export async function hideHoverAfterKeyboardClose() {
  try { await invoke("window_hide_hover_after_keyboard_close"); } catch { await getCurrentWindow().hide(); }
}

export async function onCurrentWindowFocusChanged(handler: (focused: boolean) => void) {
  try {
    return await getCurrentWindow().onFocusChanged(({ payload }) => handler(payload));
  } catch {
    return () => {};
  }
}

export async function isCurrentWindowFocused() {
  try {
    return await getCurrentWindow().isFocused();
  } catch {
    return document.hasFocus();
  }
}

export async function onNavigatePage(handler: (page: string) => void) {
  try {
    return await listen<string>("navigate-page", ({ payload }) => handler(payload));
  } catch {
    return () => {};
  }
}

export async function onWorkDataChanged(handler: (payload: {
  revision: number;
  domains: string[];
  source: "realtime" | "poll" | string;
}) => void) {
  try {
    return await listen("work-data-changed", ({ payload }) => handler(payload as {
      revision: number;
      domains: string[];
      source: string;
    }));
  } catch {
    return () => {};
  }
}

export async function onCloudSyncStateChanged(handler: (payload: StorageModeSnapshot & { error?: string }) => void) {
  try {
    return await listen("cloud-sync-state-changed", ({ payload }) => {
      handler(payload as StorageModeSnapshot & { error?: string });
    });
  } catch {
    return () => {};
  }
}

export async function onTrayCheckUpdate(handler: () => void) {
  try {
    return await listen("tray-check-update", () => handler());
  } catch {
    return () => {};
  }
}

export async function onAppUpdateEvent(handler: (payload: AppUpdateEventPayload) => void) {
  try {
    return await listen("app-update-event", ({ payload }) => handler(payload as AppUpdateEventPayload));
  } catch {
    return () => {};
  }
}
