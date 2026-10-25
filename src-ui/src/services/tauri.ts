import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { workDataChangeDomainsForMutation } from "./workDataRefresh";

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

export type WorkspaceCalendarContext = {
  timezone: string;
  currentWorkDate: string;
  currentDayStartAt: number;
  currentDayEndAt: number;
};

export type DataBackupExportResult = {
  path: string;
  createdAt: string;
  backupKind: "standard" | "stale_epoch_rescue";
  sourceMode: "local" | "cloud";
  cloudConsistent: boolean;
  entityCounts: Record<string, number>;
  excludedCategories: string[];
  checksum: string;
};

export type DataBackupPreview = {
  path: string;
  formatVersion: number;
  appVersion: string;
  createdAt: string;
  frozenAt: number;
  backupKind: "standard" | "stale_epoch_rescue";
  sourceMode: "local" | "cloud";
  cloudConsistent: boolean;
  pendingOperationCount: number;
  workspaceName: string;
  timezone: string;
  entityCounts: Record<string, number>;
  excludedCategories: string[];
  replacedEntityCounts: Record<string, number>;
  legacySyncDiagnostics?: LegacySyncDiagnostics;
};

export type LegacySyncDiagnostics = {
  dispositionReason: string;
  queueCounts: {
    pending: number;
    sending: number;
    failed: number;
    conflict: number;
    total: number;
  };
  operations: Array<{
    sequence: number;
    operationType: string;
    entityType: string;
    state: "pending" | "sending" | "failed" | "conflict";
    attemptCount: number;
    baseVersion?: number;
    payloadVersion?: number;
    workspaceEpoch: number;
    errorCode?: string;
    createdAt: number;
    lastAttemptAt?: number;
  }>;
  operationsTruncated: boolean;
  businessDataIncluded: boolean;
  rawPayloadExcluded: boolean;
  note: string;
};

export type DataBackupImportResult = {
  workspaceId: string;
  safetyBackupPath: string;
  importedEntityCounts: Record<string, number>;
};

export type DataBackupHistoryItem = {
  id: string;
  path: string;
  fileName: string;
  createdAt: string;
  backupKind: "standard" | "stale_epoch_rescue";
  sourceMode: "local" | "cloud";
  cloudConsistent: boolean;
  entityCount: number;
  sizeBytes: number;
  checksum: string;
  origin: "manual" | "safety";
  fileExists: boolean;
};

export async function exportDataBackup(path: string): Promise<DataBackupExportResult> {
  return await invoke("data_backup_export", { request: { path } });
}

export async function previewDataBackup(path: string): Promise<DataBackupPreview> {
  return await invoke("data_backup_preview", { request: { path } });
}

export async function importDataBackup(path: string): Promise<DataBackupImportResult> {
  return await invoke("data_backup_import", { request: { path, confirmed: true } });
}

export async function listDataBackupHistory(): Promise<DataBackupHistoryItem[]> {
  return await invoke("data_backup_history_list");
}

export async function openDataBackup(path: string, reveal = false): Promise<void> {
  await invoke("data_backup_open", { request: { path, reveal } });
}

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
  status: "authenticated" | "offline_saved" | "reauth_required";
  reason?: string;
  userId?: string;
  email?: string;
  expiresAt?: number;
};

export type StorageModeSnapshot = {
  mode: "local" | "cloud";
  storageProtocol: "local" | "direct_v2" | string;
  storageBackend: "local_sqlite" | "direct_supabase" | string;
  cloudAccessState: "local_writable" | "online_writable" | "offline_readonly" | "degraded_readonly" | "reauth_required" | string;
  cloudWritable: boolean;
  readOnlyReason?: string;
  workspaceId: string;
  deviceId: string;
  online: boolean;
  syncing?: boolean;
  saving?: boolean;
  lastChangeSeq: number;
  lastSyncedAt?: number;
  cacheSource?: "memory" | "persistent" | string;
  cacheFetchedAt?: number;
  lastError?: string;
  authBlocked: boolean;
  authBlockedReason?: string;
  workspaceEpoch: number;
  realtimeConnected?: boolean;
  realtimeError?: string;
};

export type WorkbenchBootstrapRecord = WorkspaceCalendarContext & {
  subjects: SubjectRecord[];
};

export type DirectCloudReleaseMetrics = {
  workspaceId: string;
  queryCount: number;
  queryErrorCount: number;
  queryLatencyTotalMs: number;
  queryLatencyMaxMs: number;
  averageQueryLatencyMs: number;
  mutationCount: number;
  mutationErrorCount: number;
  mutationLatencyTotalMs: number;
  mutationLatencyMaxMs: number;
  averageMutationLatencyMs: number;
  clientApplyLatencyTotalMs: number;
  clientApplyCount: number;
  averageClientApplyLatencyMs: number;
  refreshStartedCount: number;
  refreshReusedCount: number;
  refreshSuppressedCount: number;
  refreshFollowupCount: number;
  protocolRejectionCount: number;
  realtimeReconnectCount: number;
  copyFailureCount: number;
  lastErrorCode?: string;
  cloudExecutor: {
    maxConcurrentJobs: number;
    queuedCount: number;
    startedCount: number;
    completedCount: number;
    failedCount: number;
    queueWaitTotalMs: number;
  };
  httpClientCreationCount: number;
};

export type CloudDevice = {
  id: string;
  workspaceId: string;
  deviceName: string;
  platform: string;
  appVersion: string;
  lastSeenAt: string;
  authorizedAt: string;
  reauthorizedAt?: string;
  revokedAt?: string;
  current: boolean;
};

export type StorageProtocolUpgradePreflight = {
  workspaceId: string;
  deviceId: string;
  storageProtocol: string;
  localProtocol: string;
  workspaceEpoch: number;
  localWorkspaceEpoch: number;
  serverUpgradeAllowed: boolean;
  alreadyDirect: boolean;
  requiresLocalQueueDisposition: boolean;
  allowedQueueDispositions: string[];
  devices: CloudDevice[];
  queue: { pending: number; sending: number; failed: number; conflict: number; total: number };
  canUpgradeWithDrainedQueue: boolean;
  blockedReasons: string[];
  serverTime: string;
};

export type StorageProtocolUpgradeResult = {
  upgraded: boolean;
  alreadyDirect: boolean;
  workspaceEpoch: number;
  storageProtocol: "direct_v2";
  changeSeq?: number;
  safetySnapshotId?: string;
  pushedOperations: number;
  lastChangeSeq: number;
  refreshedDomains: string[];
};

export type StorageProtocolAbandonResult = {
  upgraded: boolean;
  alreadyDirect: boolean;
  workspaceEpoch: number;
  storageProtocol: "direct_v2";
  changeSeq?: number;
  safetySnapshotId?: string;
  quarantinedOperations: number;
  rescueBackupPath: string;
  rescueBackupChecksum: string;
  lastChangeSeq: number;
};

export type CloudOperationHistoryCursor = { occurredAt: string; id: string };
export type CloudOperationHistoryItem = {
  id: string;
  operationId: string;
  businessType: string;
  source: "user" | "automation" | "import" | "migration" | "restore" | "system";
  deviceId?: string;
  deviceName: string;
  summary: string;
  affectedEntityCount: number;
  occurredAt: string;
  expiresAt: string;
  metadata: Record<string, unknown>;
};
export type CloudOperationHistoryPage = { items: CloudOperationHistoryItem[]; nextCursor?: CloudOperationHistoryCursor };
export type CloudOperationHistoryDetailItem = {
  id: number; entityType: string; entityId: string; entityLabel: string; action: "create" | "update" | "delete" | "restore";
  before?: Record<string, unknown>; after?: Record<string, unknown>; changedFields: string[];
};
export type CloudOperationHistoryDetail = {
  id: string; operationId: string; businessType: string; source: string; deviceId?: string; deviceNameSnapshot: string; summary: string;
  affectedEntityCount: number; metadata: Record<string, unknown>; occurredAt: string; expiresAt: string; items: CloudOperationHistoryDetailItem[];
};
export type CloudOperationHistoryFilter = {
  limit?: number; cursor?: CloudOperationHistoryCursor; from?: string; to?: string; deviceId?: string; source?: string; businessType?: string; query?: string;
};

export async function listCloudOperationHistory(request: CloudOperationHistoryFilter = {}): Promise<CloudOperationHistoryPage> {
  return await invoke("cloud_operation_history_list", { request });
}

export async function getCloudOperationHistoryDetail(logId: string): Promise<CloudOperationHistoryDetail> {
  return await invoke("cloud_operation_history_detail", { request: { logId } });
}

export type CloudWorkspaceSnapshot = {
  id: string;
  snapshotType: "manual" | "automatic" | "pre_restore";
  name: string;
  note?: string;
  status: "creating" | "ready" | "corrupt" | "failed";
  formatVersion: number;
  baseChangeSeq: number;
  workspaceEpoch: number;
  entityCounts: Record<string, number>;
  groupHashes: Record<string, string>;
  contentHash?: string;
  sizeBytes: number;
  chunkCount: number;
  deviceName: string;
  protected: boolean;
  protectedAt?: string;
  expiresAt?: string;
  createdAt: string;
  completedAt?: string;
  failureReason?: string;
};

export type CloudWorkspaceSnapshotPreview = {
  snapshotId: string;
  restorable: boolean;
  errorCode?: string;
  entityCounts?: Record<string, number>;
  groupCounts?: Record<string, number>;
  groupHashes?: Record<string, string>;
  contentHash?: string;
  sizeBytes?: number;
  chunkCount?: number;
  baseChangeSeq?: number;
  workspaceEpoch?: number;
  createdAt?: string;
  completedAt?: string;
};

export async function createCloudWorkspaceSnapshot(request: {
  name: string;
  note?: string;
  snapshotType?: "manual" | "automatic" | "pre_restore";
  lazy?: boolean;
}): Promise<CloudWorkspaceSnapshot> {
  return await invoke("cloud_workspace_snapshot_create", { request });
}

export async function listCloudWorkspaceSnapshots(): Promise<CloudWorkspaceSnapshot[]> {
  return await invoke("cloud_workspace_snapshot_list");
}

export async function setCloudWorkspaceSnapshotProtected(snapshotId: string, protectedValue: boolean): Promise<CloudWorkspaceSnapshot> {
  return await invoke("cloud_workspace_snapshot_set_protected", { request: { snapshotId, protected: protectedValue } });
}

export async function deleteCloudWorkspaceSnapshot(snapshotId: string): Promise<{ deleted: boolean; snapshotId: string }> {
  return await invoke("cloud_workspace_snapshot_delete", { request: { snapshotId } });
}

export async function previewCloudWorkspaceSnapshot(snapshotId: string): Promise<CloudWorkspaceSnapshotPreview> {
  return await invoke("cloud_workspace_snapshot_preview", { request: { snapshotId } });
}

export type CloudWorkspaceSnapshotSettings = {
  automaticSnapshotEnabled: boolean;
  automaticSnapshotTime: string;
  automaticSnapshotTimezone: string;
  snapshotRetentionDays: number;
  nextAutomaticSnapshotAt: string | null;
  lastAutomaticSnapshotAt: string | null;
  schedulerActive: boolean;
};

export async function getCloudWorkspaceSnapshotSettings(): Promise<CloudWorkspaceSnapshotSettings> {
  return await invoke("cloud_workspace_snapshot_settings_get");
}

export async function updateCloudWorkspaceSnapshotSettings(automaticSnapshotEnabled: boolean, automaticSnapshotTime: string, snapshotRetentionDays: number): Promise<CloudWorkspaceSnapshotSettings> {
  return await invoke("cloud_workspace_snapshot_settings_update", { request: { automaticSnapshotEnabled, automaticSnapshotTime, snapshotRetentionDays } });
}

export type CloudWorkspaceRestorePrepareResult = {
  lockToken: string; targetSnapshotId: string; targetContentHash: string; preparedEpoch: number;
  safetySnapshotId: string | null; expiresAt: string; preview: CloudWorkspaceSnapshotPreview;
};
export type CloudWorkspaceRestoreResult = {
  restored: boolean; targetSnapshotId: string; safetySnapshotId: string | null; previousEpoch: number;
  workspaceEpoch: number; resetChangeSeq: number; localCacheRebuilt: boolean;
};
export async function prepareCloudWorkspaceRestore(snapshotId: string, createSafetySnapshot: boolean): Promise<CloudWorkspaceRestorePrepareResult> {
  return await invoke("cloud_workspace_restore_prepare", { request: { snapshotId, createSafetySnapshot } });
}
export async function commitCloudWorkspaceRestore(request: {
  lockToken: string; targetSnapshotId: string; targetContentHash: string; preparedEpoch: number;
}): Promise<CloudWorkspaceRestoreResult> {
  return await invoke("cloud_workspace_restore_commit", { request });
}
export async function cancelCloudWorkspaceRestore(lockToken: string): Promise<{ cancelled: boolean; safetySnapshotId?: string }> {
  return await invoke("cloud_workspace_restore_cancel", { request: { lockToken } });
}

export type StorageMigrationPreview = {
  direction: "local_to_cloud" | "cloud_to_local_snapshot";
  sourceWorkspaceId: string;
  targetWorkspaceId?: string;
  packageVersion: number;
  entities: Array<{ entity: string; localCount: number; cloudCount?: number }>;
  sourceEntityCount: number;
  targetEntityCount: number;
  targetIsEmpty: boolean;
  replaceEntityCount: number;
  safetySnapshotRequired: boolean;
  conflicts: string[];
  canExecute: boolean;
};

export type DataCopyStageResult = {
  packageId: string;
  status: "validated";
  entityCounts: Record<string, number>;
  contentHash: string;
  chunkCount: number;
};

export type DataCopyReplaceResult = {
  replaced: boolean;
  packageId: string;
  contentHash: string;
  safetySnapshotId: string;
  previousEpoch: number;
  workspaceEpoch: number;
  resetChangeSeq: number;
  entityCounts: Record<string, number>;
};

export type DataCopyExportManifest = {
  packageId: string;
  packageVersion: number;
  workspaceEpoch: number;
  changeSeq: number;
  entityCounts: Record<string, number>;
  contentHash: string;
  chunkCount: number;
  expiresAt: string;
};

export type DataCopyExportChunk = {
  packageId: string;
  chunkIndex: number;
  entityName: string;
  contentText: string;
  contentHash: string;
  rowCount: number;
};

export type DataCopyReplaceLocalResult = {
  replaced: boolean;
  packageId: string;
  contentHash: string;
  safetyBackupPath: string;
  workspaceId: string;
  entityCounts: Record<string, number>;
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
  dayInstanceDate?: string;
  dayInstanceKind?: "member" | "context";
  dayInstanceSourceTaskId?: string;
  title: string;
  status: "open" | "done";
  plannedDate?: string;
  estimateMinutes?: number;
  todayEstimateMinutes?: number;
  dailyEstimateVersion?: number;
  dayPlanState?: "included" | "excluded";
  dayPlanVersion?: number;
  todayMember?: boolean;
  todayContext?: boolean;
  note?: string;
  projectName?: string;
  solutionName?: string;
  sortOrder: number;
  completedAt?: number;
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

export type TaskDayPlanSnapshotRecord = {
  subjectId: string;
  workDate: string;
  tasks: TaskRecord[];
  revision: number;
};

export type TaskDayPlanBatchItem = {
  taskId: string;
  state: "included" | "excluded";
  expectedVersion?: number;
};

export type ActualTimeAdjustmentRecord = {
  id: string; subjectId: string; taskId: string; taskTitle: string; workDate: string;
  deltaMinutes: number; baselineMinutes: number; targetMinutes: number; note?: string;
  batchId: string; createdAt: number; version: number; deletedAt?: number;
};

export type ActualAdjustmentSnapshotTaskRecord = {
  id: string; subjectId: string; parentId?: string; title: string; note?: string; status: "open" | "done"; completedAt?: number; completedToday: boolean;
  dayInstanceDate?: string; dayInstanceKind?: "member" | "context"; dayInstanceSourceTaskId?: string;
  plannedDate?: string; estimateMinutes?: number; todayEstimateMinutes?: number; dailyEstimateVersion?: number; sortOrder: number;
  version: number; isLeaf: boolean; selectable: boolean; directMinutes: number; totalMinutes: number;
  adjustmentFingerprint: string; timerState?: "running" | "paused"; canAcceptChildren: boolean;
  dayPlanState?: "included" | "excluded"; dayPlanVersion?: number; todayMember?: boolean; todayContext?: boolean;
};

export type ActualAdjustmentSnapshotRecord = {
  subjectId: string; workDate: string; tasks: ActualAdjustmentSnapshotTaskRecord[]; adjustments: ActualTimeAdjustmentRecord[];
};

export type ActualAdjustmentBatchRequest = {
  subjectId: string; workDate: string; clientRequestId: string; batchId: string; note?: string;
  dayInstanceDate?: string;
  items: Array<{ taskId: string; expectedTaskVersion: number; baselineMinutes: number; targetMinutes: number; deltaMinutes: number; adjustmentFingerprint: string }>;
  newTasks: Array<{ clientTaskId: string; title: string; parentId?: string; expectedParentVersion?: number; targetMinutes: number }>;
};

export type ActualAdjustmentBatchResult = {
  batchId: string; createdAdjustmentCount: number; createdTaskCount: number;
  createdTasks: Array<{ clientTaskId: string; taskId: string }>; adjustments: ActualTimeAdjustmentRecord[];
};

export type CompletedTaskPageResult = {
  tasks: TaskRecord[];
  page: number;
  pageSize: number;
  total: number;
  totalPages: number;
};

export type TimeAllocationRecord = {
  id: string;
  taskId: string;
  taskTitle: string;
  subjectId?: string;
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
  timerChainId?: string;
  previousEntryId?: string;
  splitBoundaryAt?: number;
  allocations: TimeAllocationRecord[];
};

export type DailyReportImportTargetKind = "backlog" | "day_instance";

export type DailyReportImportPreviewItem = {
  id: string;
  parentId?: string;
  title: string;
  status: "done" | "open";
  estimateMinutes?: number;
  rawActualMinutes?: number;
  importActualMinutes?: number;
  sortOrder: number;
  sourceLineNo: number;
  sourceText: string;
  warning?: string;
  alreadyImported: boolean;
};

export type DailyReportImportPreviewResult = {
  batchId: string;
  subjectId: string;
  sourceDate: string;
  targetKind: DailyReportImportTargetKind;
  sourcePath: string;
  sourceMtime: number;
  sourceContentHash: string;
  items: DailyReportImportPreviewItem[];
  warnings: string[];
  version: number;
};

export type DailyReportImportConfirmResult = {
  batchId: string;
  targetKind: DailyReportImportTargetKind;
  sourceDate: string;
  tasks: TaskRecord[];
  createdCount: number;
  skippedCount: number;
  adjustmentCount: number;
  alreadyConfirmed: boolean;
};

export type TimerStopResultRecord = {
  currentEntry: TimeEntryRecord;
  slices: TimeEntryRecord[];
  totalDurationSeconds: number;
  totalSettlementMinutes: number;
};

export type TimerTransitionNewTaskRequest = {
  subjectId: string;
  parentId?: string;
  expectedParentVersion?: number;
  title: string;
  dayInstanceDate?: string;
};

export type TimerTransitionRequest = {
  clientRequestId?: string;
  currentEntryId?: string;
  currentEntryExpectedVersion?: number;
  taskId?: string;
  reopenCompletedTaskVersion?: number;
  newTask?: TimerTransitionNewTaskRequest;
  note?: string;
};

export type TimerTransitionResultRecord = {
  createdTask?: TaskRecord;
  previousStopResult?: TimerStopResultRecord;
  activeEntry: TimeEntryRecord;
  transitionAt: number;
  noOp: boolean;
};

export type TimeEntryListResult = {
  entries: TimeEntryRecord[];
  rawMinutes: number;
  allocatedMinutes: number;
  pendingMinutes: number;
  breakMinutes: number;
  revision: number;
};

export type TimeEntryHistoryDayRecord = {
  workDate: string;
  entryCount: number;
  rawMinutes: number;
  allocatedMinutes: number;
  pendingMinutes: number;
  breakMinutes: number;
  entries: TimeEntryRecord[];
  adjustmentMinutes: number;
  finalActualMinutes: number;
  adjustments: ActualTimeAdjustmentRecord[];
};

export type TimeEntryHistoryResult = {
  page: number;
  pageSize: number;
  totalDays: number;
  totalPages: number;
  groups: TimeEntryHistoryDayRecord[];
  revision: number;
};

export type TimeAllocationExportRequest = {
  startDate: string;
  endDate: string;
  includeSpecialTime: boolean;
};

export type TimeAllocationExportPreview = {
  rowCount: number;
  totalMinutes: number;
  defaultFileName: string;
};

export type TimeAllocationExportResult = {
  rowCount: number;
  totalMinutes: number;
  path: string;
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
  itemType: "work" | "break" | "unassigned" | "actual_adjustment" | string;
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
  workDate: string;
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

export type UnassignedStateSnapshotRecord = {
  current?: UnassignedStateRecord;
};

export type UnassignedResolveResult = {
  sessionId: string;
  generatedEntryId?: string;
  resolutionType: "work" | "break" | "discard";
  elapsedSeconds: number;
  requiredMinutes: number;
  nextSession?: UnassignedStateRecord;
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
  templateId?: string;
  templateName?: string;
  templateArchived: boolean;
  markdown: string;
  contentSource: "generated" | "edited";
  generatedCount: number;
  updatedAt: number;
  version: number;
};

export type ReportTemplateRecord = {
  id: string;
  reportType: "daily" | "weekly" | "monthly";
  name: string;
  description?: string;
  content: string;
  isDefault: boolean;
  isBuiltin: boolean;
  sortOrder: number;
  deletedAt?: number;
  version: number;
};

export type ReportListResultDto = {
  reports: ReportRecordDto[];
  page: number;
  pageSize: number;
  total: number;
  hasMore: boolean;
  typeCounts: {
    all: number;
    daily: number;
    weekly: number;
    monthly: number;
  };
  revision: number;
};

export type ReportTemplatePlaceholder = {
  token: string;
  label: string;
  description: string;
  example: string;
  reportTypes: Array<"daily" | "weekly" | "monthly">;
};

export type ReportTaskSuggestion = {
  taskId: string;
  parentId?: string;
  title: string;
  status: "open" | "done";
  estimateMinutes?: number;
  actualMinutes: number;
  dailyEstimateMinutes?: number;
  sortOrder: number;
  periodStatus: "completed" | "active" | "planned" | "context";
  defaultSelected: boolean;
  candidateRole: "selectable" | "context";
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

export type SubjectWorkScheduleSegment = { start: string; end: string };
export type SubjectWorkScheduleDay = {
  id: string;
  weekday: number;
  enabled: boolean;
  segments: SubjectWorkScheduleSegment[];
  version: number;
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
  const result = await invoke<{ subject: SubjectRecord; alreadyExists: boolean }>("subject_create", { request: { name } });
  if (!result.alreadyExists) await notifyWorkDataChanged(workDataChangeDomainsForMutation("subject"), "subject-create");
  return result;
}

export async function renameSubject(subjectId: string, name: string, expectedVersion: number): Promise<SubjectRecord> {
  const result = await invoke<SubjectRecord>("subject_rename", { request: { subjectId, name, expectedVersion } });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("subject"), "subject-rename");
  return result;
}

export async function getSubjectWorkSchedule(subjectId: string): Promise<SubjectWorkScheduleDay[]> {
  return await invoke("subject_work_schedule_get", { request: { subjectId } });
}

export async function saveSubjectWorkSchedule(subjectId: string, days: SubjectWorkScheduleDay[]): Promise<SubjectWorkScheduleDay[]> {
  return await invoke("subject_work_schedule_save", { request: { subjectId, days } });
}

export async function listTasks(subjectId?: string, plannedDate?: string, dailyEstimateDate?: string): Promise<TaskListResult> {
  return await invoke<TaskListResult>("task_list", {
    request: { subjectId, includeCompleted: false, plannedDate, todayDate: dailyEstimateDate, todayOnly: false, dailyEstimateDate, query: null },
  });
}

export async function listCompletedTaskPage(subjectId: string, page = 1, pageSize = 20): Promise<CompletedTaskPageResult> {
  return await invoke<CompletedTaskPageResult>("task_list_completed_page", {
    request: { subjectId, page, pageSize },
  });
}

export async function listTodayTasks(subjectId?: string, todayDate?: string): Promise<TaskListResult> {
  return await invoke<TaskListResult>("task_list", {
    request: { subjectId, includeCompleted: true, plannedDate: null, todayDate, todayOnly: true, dailyEstimateDate: todayDate, query: null },
  });
}

export async function setTaskDailyEstimate(taskId: string, workDate: string, estimateMinutes?: number, expectedDailyEstimateVersion?: number): Promise<{ taskId: string; workDate: string; estimateMinutes?: number; version?: number }> {
  const result = await invoke<{ taskId: string; workDate: string; estimateMinutes?: number; version?: number }>("task_daily_estimate_set", { request: { taskId, workDate, estimateMinutes }, expectedDailyEstimateVersion });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-daily-estimate");
  return result;
}

export async function createQuickTask(request: {
  subjectId: string;
  parentId?: string;
  dayInstanceDate?: string;
  title: string;
  plannedDate?: string;
  estimateMinutes?: number;
  note?: string;
  projectName?: string;
  solutionName?: string;
}): Promise<TaskRecord> {
  const result = await invoke<TaskRecord>("task_create_quick", { request });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-create-quick");
  return result;
}

export async function createTask(request: {
  subjectId: string;
  parentId?: string;
  dayInstanceDate?: string;
  title: string;
  plannedDate?: string;
  estimateMinutes?: number;
  note?: string;
  projectName?: string;
  solutionName?: string;
}): Promise<TaskRecord> {
  const result = await invoke<TaskRecord>("task_create", { request });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-create");
  return result;
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
  const result = await invoke<TaskRecord>("task_update", { request });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-update");
  return result;
}

export async function setTaskCompleted(
  id: string,
  expectedVersion: number,
  done: boolean,
  occurrenceDate?: string,
  occurrenceExpectedVersion?: number,
): Promise<TaskRecord> {
  const result = await invoke<TaskRecord>("task_set_completed", {
    request: { id, expectedVersion, done, occurrenceDate, occurrenceExpectedVersion },
  });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-completion");
  return result;
}

export async function saveTaskRecurrence(request: {
  taskId: string;
  taskExpectedVersion: number;
  frequency: RecurrenceRuleRecord["frequency"];
  weekdaysMask?: number;
  effectiveStart: string;
  ruleExpectedVersion?: number;
}): Promise<RecurrenceRuleRecord> {
  const result = await invoke<RecurrenceRuleRecord>("task_recurrence_save", { request });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-recurrence-save");
  return result;
}

export async function closeTaskRecurrence(request: {
  taskId: string;
  taskExpectedVersion: number;
  effectiveEnd: string;
  ruleExpectedVersion: number;
}): Promise<void> {
  await invoke("task_recurrence_close", { request });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-recurrence-close");
}

export async function deleteTaskSubtree(id: string, expectedVersion: number): Promise<void> {
  await invoke("task_delete_subtree", { request: { id, expectedVersion } });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-delete");
}

export async function reorderTaskSubtree(id: string, expectedVersion: number, parentId: string | undefined, sortOrder: number): Promise<TaskRecord> {
  const result = await invoke<TaskRecord>("task_reorder_subtree", { request: { id, expectedVersion, parentId, sortOrder } });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-reorder");
  return result;
}

export async function changeTaskParent(id: string, expectedVersion: number, newParentId?: string, sortOrder?: number): Promise<TaskRecord> {
  const result = await invoke<TaskRecord>("task_change_parent", { request: { id, expectedVersion, newParentId, sortOrder } });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-parent");
  return result;
}

export async function duplicateTaskSubtree(id: string, expectedVersion: number): Promise<{ rootId: string; tasks: TaskRecord[] }> {
  const result = await invoke<{ rootId: string; tasks: TaskRecord[] }>("task_duplicate_subtree", { request: { id, expectedVersion } });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-duplicate");
  return result;
}

export async function previewDailyReportImport(
  subjectId: string,
  sourceDate: string,
  targetKind: DailyReportImportTargetKind,
): Promise<DailyReportImportPreviewResult> {
  return await invoke<DailyReportImportPreviewResult>("daily_report_import_preview", {
    request: { subjectId, sourceDate, targetKind },
  });
}

export async function confirmDailyReportImport(request: {
  batchId: string;
  selectedItemIds: string[];
  expectedVersion: number;
  clientRequestId: string;
}): Promise<DailyReportImportConfirmResult> {
  const result = await invoke<DailyReportImportConfirmResult>("daily_report_import_confirm", { request });
  if (!result.alreadyConfirmed && result.createdCount) {
    await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "daily-report-task-import");
  }
  return result;
}

export async function updateTodayRecurringTask(request: {
  instanceId: string;
  instanceExpectedVersion: number;
  sourceExpectedVersion: number;
  title?: string;
  plannedDate?: string | null;
  estimateMinutes?: number | null;
  note?: string | null;
  projectName?: string | null;
}): Promise<{ instance: TaskRecord; source: TaskRecord }> {
  const result = await invoke<{ instance: TaskRecord; source: TaskRecord }>("task_today_recurring_update", { request });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-today-recurring-update");
  return result;
}

export async function getTaskDayPlanSnapshot(subjectId: string, workDate: string): Promise<TaskDayPlanSnapshotRecord> {
  return await invoke<TaskDayPlanSnapshotRecord>("task_day_plan_snapshot", { request: { subjectId, workDate } });
}

export async function setTaskDayPlan(taskId: string, workDate: string, state: "included" | "excluded", expectedVersion?: number): Promise<{ taskId: string; workDate: string; state: string; version: number }> {
  const result = await invoke<{ taskId: string; workDate: string; state: string; version: number }>("task_day_plan_set", {
    request: { taskId, workDate, state, expectedVersion },
  });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-day-plan");
  return result;
}

export async function setTaskDayPlanBatch(subjectId: string, workDate: string, items: TaskDayPlanBatchItem[]): Promise<{ changedCount: number }> {
  const result = await invoke<{ changedCount: number }>("task_day_plan_batch_set", { request: { subjectId, workDate, items } });
  if (result.changedCount) await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-day-plan-batch");
  return result;
}

export async function getActualAdjustmentSnapshot(subjectId: string, workDate: string, executionOnly?: boolean): Promise<ActualAdjustmentSnapshotRecord> {
  return await invoke("actual_adjustment_snapshot", { request: { subjectId, workDate, executionOnly } });
}

export async function applyActualAdjustmentBatch(request: ActualAdjustmentBatchRequest): Promise<ActualAdjustmentBatchResult> {
  const result = await invoke<ActualAdjustmentBatchResult>("actual_adjustment_batch_apply", { request });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("time"), "actual-adjustment-apply");
  return result;
}

export async function deleteActualAdjustment(adjustmentId: string, expectedVersion: number, clientRequestId = crypto.randomUUID()): Promise<ActualTimeAdjustmentRecord> {
  const result = await invoke<ActualTimeAdjustmentRecord>("actual_adjustment_delete", { request: { adjustmentId, expectedVersion, clientRequestId } });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("time"), "actual-adjustment-delete");
  return result;
}

export type TaskDailyEstimateBatchItem = {
  taskId: string;
  expectedTaskVersion: number;
  expectedDailyEstimateVersion?: number;
  estimateMinutes?: number;
};

export async function setTaskDailyEstimateBatch(
  subjectId: string,
  workDate: string,
  items: TaskDailyEstimateBatchItem[],
): Promise<{ changedCount: number }> {
  const result = await invoke<{ changedCount: number }>("task_daily_estimate_batch_set", {
    request: { subjectId, workDate, items },
  });
  if (result.changedCount) {
    await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-daily-estimate-batch");
  }
  return result;
}

export type TaskPlanBatchRequest = {
  subjectId: string;
  dayInstanceDate?: string;
  baselineTasks: Array<{ taskId: string; expectedVersion: number }>;
  finalItems: Array<{ lineKey: string; taskId?: string; title: string; parentLineKey?: string }>;
  confirmedDeleteTaskIds: string[];
  todayPlanChange?: {
    workDate: string;
    includeLineKeys: string[];
    excludeItems: Array<{ taskId: string; expectedVersion?: number }>;
  };
};

export type TaskPlanBatchResult = {
  createdCount: number;
  updatedCount: number;
  deletedCount: number;
  todayPlanChangedCount: number;
  createdItems: Array<{ lineKey: string; taskId: string }>;
};

export async function applyTaskPlanBatch(request: TaskPlanBatchRequest): Promise<TaskPlanBatchResult> {
  const result = await invoke<TaskPlanBatchResult>("task_plan_batch_apply", { request });
  if (result.createdCount || result.updatedCount || result.deletedCount || result.todayPlanChangedCount) {
    await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-plan-batch-apply");
  }
  return result;
}

export async function setRecurringTaskEstimate(taskId: string, expectedVersion: number, workDate: string, estimateMinutes: number): Promise<TaskRecord> {
  const result = await invoke<TaskRecord>("task_recurring_estimate_set", { request: { taskId, expectedVersion, workDate, estimateMinutes } });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-recurring-estimate");
  return result;
}

export async function moveTaskRecord(request: {
  id: string;
  expectedVersion: number;
  parentId?: string;
  previousSiblingId?: string;
  nextSiblingId?: string;
}): Promise<TaskRecord> {
  const result = await invoke<TaskRecord>("task_move", { request });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-move");
  return result;
}

export async function createPositionedTask(request: {
  subjectId: string;
  parentId?: string;
  dayInstanceDate?: string;
  title: string;
  plannedDate?: string;
  estimateMinutes?: number;
  note?: string;
  projectName?: string;
  solutionName?: string;
  previousSiblingId?: string;
  nextSiblingId?: string;
}): Promise<TaskRecord> {
  const result = await invoke<TaskRecord>("task_create_positioned", { request });
  await notifyWorkDataChanged(workDataChangeDomainsForMutation("task"), "task-create-positioned");
  return result;
}

export async function getTimerState(): Promise<TimeEntryRecord | null> {
  return await invoke<TimeEntryRecord | null>("timer_get_state");
}

export async function startTimerRecord(taskId?: string, note?: string): Promise<TimeEntryRecord> {
  const clientRequestId = crypto.randomUUID();
  return await invoke<TimeEntryRecord>("timer_start", {
    request: { taskId, note, clientRequestId },
  });
}

export async function transitionTimerRecord(request: TimerTransitionRequest): Promise<TimerTransitionResultRecord> {
  return await invoke<TimerTransitionResultRecord>("timer_transition", {
    request: { ...request, clientRequestId: request.clientRequestId ?? crypto.randomUUID() },
  });
}

export async function pauseTimerRecord(entryId: string, expectedVersion: number): Promise<TimeEntryRecord> {
  return await invoke<TimeEntryRecord>("timer_pause", { request: { entryId, expectedVersion } });
}

export async function resumeTimerRecord(entryId: string, expectedVersion: number): Promise<TimeEntryRecord> {
  const operationId = crypto.randomUUID();
  return await invoke<TimeEntryRecord>("timer_resume", {
    request: { entryId, expectedVersion, operationId },
  });
}

export async function stopTimerRecord(
  entryId: string,
  expectedVersion: number,
  createDefaultAllocation = false,
): Promise<TimerStopResultRecord> {
  return await invoke<TimerStopResultRecord>("timer_stop", {
    request: { entryId, expectedVersion, createDefaultAllocation },
  });
}

export async function listTimeEntries(workDate: string, includeBreaks = true): Promise<TimeEntryListResult> {
  return await invoke<TimeEntryListResult>("time_entry_list", { request: { workDate, includeBreaks } });
}

export async function listTimeEntryHistory(page: number): Promise<TimeEntryHistoryResult> {
  return await invoke<TimeEntryHistoryResult>("time_entry_history_list", {
    request: { page, includeBreaks: true },
  });
}

export async function previewTimeAllocationExport(
  request: TimeAllocationExportRequest,
): Promise<TimeAllocationExportPreview> {
  return await invoke<TimeAllocationExportPreview>("time_allocation_export_preview", { request });
}

export async function executeTimeAllocationExport(
  request: TimeAllocationExportRequest & { targetPath: string },
): Promise<TimeAllocationExportResult> {
  return await invoke<TimeAllocationExportResult>("time_allocation_export_execute", { request });
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
  completeTask?: boolean;
  taskExpectedVersion?: number;
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

export async function updateTimeEntryDisposition(request: {
  entryId: string;
  expectedVersion: number;
  disposition: "break" | "discard";
}): Promise<TimeEntryRecord> {
  return await invoke<TimeEntryRecord>("time_entry_update_disposition", { request });
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
  return await invoke<UnassignedStateRecord | null>("unassigned_get_state");
}

export async function getUnassignedStateSnapshot(): Promise<UnassignedStateSnapshotRecord> {
  return await invoke<UnassignedStateSnapshotRecord>("unassigned_get_state_snapshot");
}

export async function getUnassignedSession(sessionId: string): Promise<UnassignedStateRecord> {
  return await invoke<UnassignedStateRecord>("unassigned_get_session", { sessionId });
}

export async function resolveUnassignedWork(
  sessionId: string,
  expectedVersion: number,
  allocations: Array<{ taskId: string; minutes: number; completeTask?: boolean; taskExpectedVersion?: number }>,
): Promise<UnassignedResolveResult> {
  const operationId = crypto.randomUUID();
  return await invoke<UnassignedResolveResult>("unassigned_resolve_work", {
    request: { sessionId, expectedVersion, allocations, operationId },
  });
}

export async function resolveUnassignedBreak(
  sessionId: string,
  expectedVersion: number,
): Promise<UnassignedResolveResult> {
  const operationId = crypto.randomUUID();
  return await invoke<UnassignedResolveResult>("unassigned_resolve_break", {
    request: { sessionId, expectedVersion, operationId },
  });
}

export async function discardUnassignedTime(
  sessionId: string,
  expectedVersion: number,
): Promise<UnassignedResolveResult> {
  const operationId = crypto.randomUUID();
  return await invoke<UnassignedResolveResult>("unassigned_discard", {
    request: { sessionId, expectedVersion, operationId },
  });
}

export async function listReports(request: {
  reportType?: "all" | "daily" | "weekly" | "monthly";
  subjectId?: string;
  query?: string;
  page?: number;
  pageSize?: number;
} = {}): Promise<ReportListResultDto> {
  return await invoke<ReportListResultDto>("report_list", { request: {
    reportType: request.reportType ?? "all",
    subjectId: request.subjectId,
    query: request.query,
    page: request.page ?? 1,
    pageSize: request.pageSize ?? 20,
  } });
}

export async function getReportTaskSuggestions(request: {
  reportType: "daily" | "weekly" | "monthly";
  referenceDate: string;
  subjectId: string;
  includeTaskIds?: string[];
}): Promise<ReportTaskSuggestion[]> {
  return await invoke("report_task_suggestions", { request });
}

export async function getReportWorkPeriodPreview(
  subjectId: string,
  referenceDate: string,
): Promise<string> {
  return await invoke("report_work_period_preview", {
    request: { subjectId, referenceDate },
  });
}

export async function previewReport(request: {
  reportType: "daily" | "weekly" | "monthly";
  referenceDate: string;
  subjectId: string;
  templateId?: string;
}): Promise<string> {
  return await invoke("report_preview", { request });
}

export async function createReport(request: {
  reportType: "daily" | "weekly" | "monthly";
  referenceDate: string;
  subjectId: string;
  taskIds: string[];
  templateId?: string;
}): Promise<ReportRecordDto> {
  return await invoke("report_create", { request: { ...request, clientRequestId: crypto.randomUUID() } });
}

export async function updateReportScope(request: {
  reportId: string;
  reportType: "daily" | "weekly" | "monthly";
  referenceDate: string;
  subjectId: string;
  taskIds: string[];
  templateId?: string;
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

export async function listReportTemplates(reportType?: "daily" | "weekly" | "monthly"): Promise<ReportTemplateRecord[]> {
  return await invoke("report_template_list", { request: { reportType } });
}

export async function getReportTemplateById(templateId: string): Promise<ReportTemplateRecord> {
  return await invoke("report_template_get_by_id", { request: { templateId } });
}

export async function createReportTemplate(request: { reportType: "daily" | "weekly" | "monthly"; name: string; description?: string; content: string }): Promise<ReportTemplateRecord> {
  return await invoke("report_template_create", { request });
}

export async function updateReportTemplate(request: { templateId: string; name: string; description?: string; content: string; sortOrder: number; expectedVersion: number }): Promise<ReportTemplateRecord> {
  return await invoke("report_template_update", { request });
}

export async function duplicateReportTemplate(request: { templateId: string; name: string; description?: string }): Promise<ReportTemplateRecord> {
  return await invoke("report_template_duplicate", { request });
}

export async function setDefaultReportTemplate(templateId: string, expectedVersion: number): Promise<ReportTemplateRecord> {
  return await invoke("report_template_set_default", { request: { templateId, expectedVersion } });
}

export async function deleteReportTemplate(templateId: string, expectedVersion: number, replacementTemplateId?: string): Promise<void> {
  await invoke("report_template_delete", { request: { templateId, expectedVersion, replacementTemplateId } });
}

export async function reorderReportTemplates(reportType: "daily" | "weekly" | "monthly", templateIds: string[]): Promise<ReportTemplateRecord[]> {
  return await invoke("report_template_reorder", { request: { reportType, templateIds } });
}

export async function getReportTemplatePlaceholders(reportType: "daily" | "weekly" | "monthly"): Promise<ReportTemplatePlaceholder[]> {
  return await invoke("report_template_placeholders", { reportType });
}

export async function getWorkspaceCalendarContext(): Promise<WorkspaceCalendarContext> {
  return await invoke<WorkspaceCalendarContext>("work_calendar_get_context");
}

export async function getWorkbenchBootstrap(): Promise<WorkbenchBootstrapRecord> {
  return await invoke<WorkbenchBootstrapRecord>("workbench_bootstrap_get");
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

export async function wakeCloudConnection(): Promise<void> {
  await invoke("cloud_connection_wake");
}

export async function getDirectCloudReleaseMetrics(): Promise<DirectCloudReleaseMetrics> {
  return await invoke("direct_cloud_release_metrics_get");
}

export async function recordDirectCloudClientMetric(metric: "client_apply" | "started" | "reused" | "suppressed" | "followup", durationMs?: number) {
  await invoke("direct_cloud_client_metric_record", { metric, durationMs });
}

export async function setLocalStorageMode(): Promise<StorageModeSnapshot> {
  return await invoke("storage_mode_set_local");
}

export async function setCloudStorageMode(): Promise<StorageModeSnapshot> {
  return await invoke("storage_mode_set_cloud");
}

export async function clearCloudReadCache(): Promise<StorageModeSnapshot> {
  return await invoke("cloud_read_cache_clear");
}

export async function prepareDirectCloudForceRefresh(): Promise<StorageModeSnapshot> {
  return await invoke("direct_cloud_force_refresh_prepare");
}

export async function configureCloud(projectUrl: string, anonKey: string): Promise<StorageModeSnapshot> {
  return await invoke("cloud_configure", { request: { projectUrl, anonKey } });
}

export type CloudProjectProbeState =
  | "ready"
  | "unconfigured"
  | "schema_missing"
  | "schema_not_exposed"
  | "unreachable";

export interface CloudProjectProbe {
  state: CloudProjectProbeState;
  message: string;
}

export async function probeCloudProject(): Promise<CloudProjectProbe> {
  return await invoke("cloud_project_probe");
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

export async function signOutAllCloudDevices(): Promise<StorageModeSnapshot> {
  return await invoke("cloud_sign_out_all");
}

export async function registerCloudDevice(deviceName: string): Promise<unknown> {
  return await invoke("cloud_device_register", {
    request: { deviceName, platform: "windows", appVersion: __APP_VERSION__ },
  });
}

export async function previewStorageMigration(direction: StorageMigrationPreview["direction"] = "local_to_cloud"): Promise<StorageMigrationPreview> {
  return await invoke("storage_migration_preview", { request: { direction } });
}

export async function stageLocalDataCopyToCloud(): Promise<DataCopyStageResult> {
  return await invoke("storage_copy_stage_local_to_cloud");
}

export async function replaceCloudWithStagedLocalData(request: {
  packageId: string;
  contentHash: string;
  operationId: string;
  confirmed: boolean;
}): Promise<DataCopyReplaceResult> {
  return await invoke("storage_copy_replace_cloud", { request });
}

export async function beginCloudDataCopyExport(): Promise<DataCopyExportManifest> {
  return await invoke("storage_copy_export_cloud_begin");
}

export async function getCloudDataCopyExportChunk(packageId: string, chunkIndex: number): Promise<DataCopyExportChunk> {
  return await invoke("storage_copy_export_cloud_chunk", { request: { packageId, chunkIndex } });
}

export async function discardCloudDataCopyExport(packageId: string): Promise<{ packageId: string; discarded: boolean }> {
  return await invoke("storage_copy_export_cloud_discard", { request: { packageId } });
}

export async function replaceLocalWithCloudDataCopy(confirmed: boolean): Promise<DataCopyReplaceLocalResult> {
  return await invoke("storage_copy_replace_local", { request: { confirmed } });
}

export async function listCloudDevices(): Promise<CloudDevice[]> {
  return await invoke("cloud_device_list");
}

export async function revokeCloudDevice(deviceId: string): Promise<CloudDevice[]> {
  return await invoke("cloud_device_revoke", { request: { deviceId } });
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

export async function openMainTimerTaskSelection() {
  try { await invoke("window_open_main_timer_task_selection"); } catch { await getCurrentWindow().show(); }
}

export async function openMainUnassignedResolution() {
  try { await invoke("window_open_main_unassigned"); } catch { await getCurrentWindow().show(); }
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
  revision?: number;
  workspaceId?: string;
  changeSeq?: number;
  workspaceEpoch?: number;
  domains: string[];
  source: "realtime" | "poll" | string;
  timerStoppedEntryId?: string;
  sourceWindowLabel?: string;
  operationId?: string;
  correlationId?: string;
  coveredDomains?: string[];
  coveredQueryKeys?: string[];
  changedByDeviceId?: string;
}) => void) {
  try {
    return await listen("work-data-changed", ({ payload }) => handler(payload as {
      revision?: number;
      workspaceId?: string;
      changeSeq?: number;
      workspaceEpoch?: number;
      domains: string[];
      source: string;
      timerStoppedEntryId?: string;
      sourceWindowLabel?: string;
      operationId?: string;
      correlationId?: string;
      coveredDomains?: string[];
      coveredQueryKeys?: string[];
      changedByDeviceId?: string;
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

export type TimerFactFollowedEvent = {
  operationType: string;
  entityId: string;
  version: number;
};

export async function onTimerFactFollowed(handler: (payload: TimerFactFollowedEvent) => void) {
  try {
    return await listen<TimerFactFollowedEvent>("timer-fact-followed", ({ payload }) => handler(payload));
  } catch {
    return () => {};
  }
}

export async function onTrayHoverShown(handler: () => void) {
  try {
    return await listen("tray-hover-shown", () => handler());
  } catch {
    return () => {};
  }
}

export async function onOpenUnassignedResolution(handler: () => void) {
  try {
    return await listen("open-unassigned-resolution", () => handler());
  } catch {
    return () => {};
  }
}

export async function onOpenTimerTaskSelection(handler: () => void) {
  try {
    return await listen("open-timer-task-selection", () => handler());
  } catch {
    return () => {};
  }
}

export async function notifyWorkDataChanged(
  domains: string[],
  source = "frontend",
  options: { operationId?: string; correlationId?: string; coveredDomains?: string[]; coveredQueryKeys?: string[] } = {},
) {
  try {
    const sourceWindowLabel = getCurrentWindow().label;
    await emit("work-data-changed", {
      revision: 0,
      domains,
      source,
      sourceWindowLabel,
      correlationId: options.correlationId ?? options.operationId ?? crypto.randomUUID(),
      operationId: options.operationId,
      coveredDomains: options.coveredDomains ?? domains,
      coveredQueryKeys: options.coveredQueryKeys ?? [],
    });
  } catch {
    // The calling window already owns the successful mutation; synchronization is best-effort.
  }
}

export async function notifyTimerStopConfirmation(entryId: string, source = "frontend") {
  try {
    const sourceWindowLabel = getCurrentWindow().label;
    await emit("work-data-changed", {
      revision: 0,
      domains: ["tasks", "time", "unassigned"],
      source,
      timerStoppedEntryId: entryId,
      sourceWindowLabel,
      correlationId: crypto.randomUUID(),
      coveredDomains: ["tasks", "time", "unassigned"],
      coveredQueryKeys: [],
    });
  } catch {
    // The local store already has the stopped entry; cross-window refresh is best-effort.
  }
}

export async function onTrayCheckUpdate(handler: () => void) {
  try {
    return await listen("tray-check-update", () => handler());
  } catch {
    return () => {};
  }
}

export async function onShortcutsChanged(
  handler: (payload: { enabled: boolean; mainWindowShortcut: string }) => void,
) {
  try {
    return await listen("shortcuts-changed", ({ payload }) => {
      handler(payload as { enabled: boolean; mainWindowShortcut: string });
    });
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
