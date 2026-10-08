<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import { AlertTriangle, CalendarDays, CheckCircle2, Clock3, Copy, Database, FileText, Home, LoaderCircle, Minus, Pencil, Plus, RefreshCw, Save, Settings, Square, UserRound, X } from "lucide-vue-next";
import { ElMessage, ElMessageBox } from "element-plus";
import { useWorkdayStore, type Task } from "../store";
import { checkAppUpdate, closeMainWindow, confirmObsidianPlan, configureCloud, downloadAndInstallUpdate, executeSeaTableTaskSync, executeStorageMigration, getCloudSession, getCloudSyncStatus, getSettings, getStorageMode, isMainWindowMaximized, listCloudDevices, listCloudSyncConflicts, listCloudSyncQueue, minimizeMainWindow, onAppUpdateEvent, onCloudSyncStateChanged, onCurrentWindowResized, onNavigatePage, onTrayCheckUpdate, previewObsidianPlan, previewSeaTableTaskSync, previewStorageMigration, registerCloudDevice, resetLocalCacheFromCloud, resolveAllCloudSyncConflicts, restartApp, resumeCloudSyncAfterReauth, retryFailedSeaTableSync, revokeCloudDevice, saveCloudSync, setIntegrationSecret, setLocalStorageMode, signInCloud, signOutAllCloudDevices, signOutCloud, testSeaTableConnection, toggleMainWindowMaximize, updateIntegrationConfig, updateSetting, type AppUpdateCheckResult, type AppUpdateEventPayload, type CloudDevice, type CloudSessionSnapshot, type CloudSyncConflict, type CloudSyncQueueItem, type PlanImportPreviewResult, type SeaTableSyncPreview, type SeaTableSyncResult, type SettingsSnapshot, type StorageMigrationPreview, type StorageModeSnapshot } from "../services/tauri";
import PlanListEditor from "../components/PlanListEditor.vue";
import GlobalTimerBar from "../components/GlobalTimerBar.vue";
import UnassignedTimeIndicator from "../components/UnassignedTimeIndicator.vue";
import UnassignedTimeDialog from "../components/UnassignedTimeDialog.vue";
import TimeTrackingWorkspace from "../components/TimeTrackingWorkspace.vue";
import TodayOverviewWorkspace from "../components/TodayOverviewWorkspace.vue";
import ReportWorkspace from "../components/ReportWorkspace.vue";
import AutomationHooksSettings from "../components/AutomationHooksSettings.vue";
import TimerStopConfirmationDialog from "../components/TimerStopConfirmationDialog.vue";
import appIconUrl from "../../../src-tauri/icons/icon.svg";
import { cloudSaveStatusLabel, cloudSyncPresentation } from "../services/cloudSyncPresentation";
import { cloudSessionLabel, cloudSessionNeedsPassword } from "../services/cloudSessionPresentation";
import { cloudDeviceActionDisabled, cloudDeviceActionLabel, cloudDeviceKind, runConfirmedCloudAction } from "../services/cloudDevicePresentation";
import { isCompletionFeedbackEnabled, playCompletionFeedback, setCompletionFeedbackEnabled, setCompletionFeedbackSoundEnabled } from "../services/completionFeedback";
import { remainingOpenTaskCount } from "../services/completionFeedbackCore";
import { APP_THEMES, applyAppTheme, isAppTheme, type AppTheme } from "../services/theme";

const store = useWorkdayStore();
const appVersion = __APP_VERSION__;
const pages = [
  { id: "today", label: "今日", icon: Home },
  { id: "plan", label: "待办", icon: CalendarDays },
  { id: "timer", label: "计时", icon: Clock3 },
  { id: "reports", label: "报告", icon: FileText },
  { id: "settings", label: "设置", icon: Settings }
];
const settingsNavGroups = [
  { label: "基础设置", items: [
    { id: "appearance", label: "外观与本机", icon: Settings }
  ] },
  { label: "数据与集成", items: [
    { id: "seatable", label: "SeaTable", icon: Database },
    { id: "storage", label: "数据存储", icon: Database }
  ] },
  { label: "行为与自动化", items: [
    { id: "feedback", label: "交互与托盘", icon: CheckCircle2 },
    { id: "hooks", label: "自动化钩子", icon: RefreshCw }
  ] },
  { label: "系统", items: [{ id: "update", label: "应用更新", icon: RefreshCw }] }
];

const subjectDialogOpen = ref(false);
const subjectNameDraft = ref("");
const editingSubjectId = ref("");
const subjectDialogTitle = computed(() => editingSubjectId.value ? "编辑主体名称" : "添加主体");
const windowMaximized = ref(false);
const settingsSnapshot = ref<SettingsSnapshot>();
const settingsLoaded = ref(false);
const settingsSaving = ref(false);
const updateChecking = ref(false);
const updateInstalling = ref(false);
const updateStatusText = ref("未检查更新");
const updateProgressText = ref("");
const cloudWorking = ref(false);
const cloudSession = ref<CloudSessionSnapshot>();
const cloudDevices = ref<CloudDevice[]>([]);
const storageState = ref<StorageModeSnapshot>();
const visibleCloudSaveState = ref<StorageModeSnapshot>();
const migrationPreview = ref<StorageMigrationPreview>();
const cloudSyncWorking = ref(false);
const cloudConflictDialogOpen = ref(false);
const cloudConflicts = ref<CloudSyncConflict[]>([]);
const cloudQueueDialogOpen = ref(false);
const cloudQueueLoading = ref(false);
const cloudQueueItems = ref<CloudSyncQueueItem[]>([]);
const resetLocalCacheDialogOpen = ref(false);
const resetLocalCacheWorking = ref(false);
const importDialogOpen = ref(false);
const importLoading = ref(false);
const importSaving = ref(false);
const importPreview = ref<PlanImportPreviewResult>();
const importItems = ref<Array<{ importItemId: string; selected: boolean; title: string; estimateMinutes?: number; parentId?: string; sourceText: string; parseStatus: "recognized" | "unrecognized" }>>([]);
const seaTableDialogOpen = ref(false);
const seaTableLoading = ref(false);
const seaTableExecuting = ref(false);
const seaTablePreview = ref<SeaTableSyncPreview>();
const seaTableResult = ref<SeaTableSyncResult>();
const settingsDraft = reactive({
  obsidianRootPath: "",
  obsidianDailyPathPattern: "工作日报/{date}.md",
  seatableServerUrl: "https://cloud.seatable.cn",
  seatableTaskTable: "事项计划",
  seatableTaskView: "默认视图",
  seatableReimbursementTable: "报销",
  seatableLocalIdField: "本地事项ID",
  subjectName: "默认",
  salaryHourlyRate: 0,
  reportDurationFormat: "minutes" as "minutes" | "hours",
  timezone: "Asia/Shanghai",
  appTheme: "forest" as AppTheme,
  completionFeedbackEnabled: true,
  completionFeedbackSoundEnabled: true,
  trayHoverEnabled: true,
  trayMenuSuppressHover: true,
  startMinimized: false,
  storageMode: "local" as "local" | "cloud",
  supabaseProjectUrl: "",
  supabaseAnonKey: "",
  supabaseEmail: "",
  supabasePassword: "",
  seatableToken: ""
});
const activeSettingsSection = ref("appearance");

function scrollToSettingsSection(sectionId: string) {
  const target = document.querySelector<HTMLElement>(`[data-settings-section="${sectionId}"]`);
  if (!target) return;
  activeSettingsSection.value = sectionId;
  target.scrollIntoView({ behavior: "smooth", block: "start" });
}

async function openSettingsTarget(selector: string) {
  store.activePage = "settings";
  activeSettingsSection.value = "storage";
  await nextTick();
  const target = document.querySelector<HTMLElement>(selector)
    ?? document.querySelector<HTMLElement>('[data-settings-section="storage"]');
  target?.scrollIntoView({ behavior: "smooth", block: "start" });
}

async function openCloudAccountSettings() {
  await openSettingsTarget(cloudNeedsReauth.value ? ".storage-login-section" : ".storage-account-section");
}

async function openCloudStorageSettings() {
  cloudQueueDialogOpen.value = false;
  await openSettingsTarget('[data-settings-section="storage"]');
}

function handleSettingsScroll(event: Event) {
  if (store.activePage !== "settings") return;
  const container = event.currentTarget as HTMLElement;
  const sections = Array.from(container.querySelectorAll<HTMLElement>("[data-settings-section]"));
  if (!sections.length) return;
  const marker = container.getBoundingClientRect().top + 28;
  const current = sections.reduce((closest, section) => Math.abs(section.getBoundingClientRect().top - marker) < Math.abs(closest.getBoundingClientRect().top - marker) ? section : closest);
  activeSettingsSection.value = current.dataset.settingsSection || activeSettingsSection.value;
}
let unlistenWindowResize: (() => void) | undefined;
let unlistenNavigatePage: (() => void) | undefined;
let unlistenCloudSyncState: (() => void) | undefined;
let unlistenTrayCheckUpdate: (() => void) | undefined;
let unlistenAppUpdateEvent: (() => void) | undefined;
let cloudSaveSettledTimer: number | undefined;
let cloudSavePendingSince = 0;
const completingTaskIds = new Set<string>();
const recentCompletedTaskId = ref("");
let recentCompletedTimer: number | undefined;

const cloudSyncView = computed(() => cloudSyncPresentation(storageState.value, formatSyncTime));
const cloudSyncLabel = computed(() => cloudSyncView.value.label);
const cloudSyncDetail = computed(() => cloudSyncView.value.detail);
const cloudSyncNeedsAttention = computed(() => cloudSyncView.value.needsAttention);
const cloudSyncAttentionLevel = computed(() => cloudSyncView.value.attentionLevel);
const cloudSaveView = computed(() => cloudSyncPresentation(visibleCloudSaveState.value, formatSyncTime));
const cloudSaveLabel = computed(() => cloudSaveStatusLabel(visibleCloudSaveState.value));
const cloudSaveNeedsAttention = computed(() => cloudSaveView.value.needsAttention);
const cloudSaveAttentionLevel = computed(() => cloudSaveView.value.attentionLevel);
const cloudSyncIndicatorWorking = computed(() => Boolean(visibleCloudSaveState.value?.saving));
const cloudAccountLabel = computed(() => {
  if (storageState.value?.mode !== "cloud") return "本地模式";
  return cloudSessionLabel(cloudSession.value);
});
const cloudNeedsReauth = computed(() => cloudSessionNeedsPassword(cloudSession.value));
const cloudAuthBlocked = computed(() => Boolean(storageState.value?.authBlocked));
const cloudDockSyncDisabled = computed(() => cloudSyncWorking.value);
const cloudDockRequiresStorageAttention = computed(() => (
  storageState.value?.mode !== "cloud"
  || !cloudSession.value?.signedIn
  || !storageState.value.online
  || Boolean(storageState.value.authBlocked)
  || Boolean(storageState.value.conflictCount)
  || Boolean(storageState.value.lastError)
));
const cloudDockSyncTitle = computed(() => {
  if (storageState.value?.mode !== "cloud") return "本地模式无需同步";
  if (!cloudSession.value?.signedIn) return "前往数据存储设置登录";
  if (storageState.value.conflictCount > 0) return "前往数据存储设置处理同步冲突";
  if (!storageState.value.online || storageState.value.authBlocked || storageState.value.lastError) return "前往数据存储设置处理同步异常";
  return "立即保存并同步到云端";
});
const cloudDockSyncLabel = computed(() => {
  if (storageState.value?.mode !== "cloud" || !cloudSession.value?.signedIn) return "设置";
  if (storageState.value.conflictCount) return "处理冲突";
  if (!storageState.value.online || storageState.value.authBlocked || storageState.value.lastError) return "处理异常";
  return "保存";
});

const CLOUD_SAVE_PENDING_MIN_VISIBLE_MS = 650;

function hasVisiblePendingSave(state: StorageModeSnapshot | undefined): boolean {
  return Boolean(state?.mode === "cloud" && state.pendingOperations > 0 && state.conflictCount === 0);
}

function updateVisibleCloudSaveState(state: StorageModeSnapshot | undefined) {
  if (cloudSaveSettledTimer) {
    window.clearTimeout(cloudSaveSettledTimer);
    cloudSaveSettledTimer = undefined;
  }

  if (!state) {
    visibleCloudSaveState.value = undefined;
    cloudSavePendingSince = 0;
    return;
  }

  if (hasVisiblePendingSave(state)) {
    if (!hasVisiblePendingSave(visibleCloudSaveState.value)) cloudSavePendingSince = Date.now();
    visibleCloudSaveState.value = state;
    return;
  }

  const shouldShowSettledImmediately = state.mode !== "cloud"
    || state.conflictCount > 0
    || Boolean(state.lastError)
    || state.syncState === "error"
    || !state.online;
  const elapsed = Date.now() - cloudSavePendingSince;
  const remaining = CLOUD_SAVE_PENDING_MIN_VISIBLE_MS - elapsed;
  if (!shouldShowSettledImmediately && hasVisiblePendingSave(visibleCloudSaveState.value) && remaining > 0) {
    cloudSaveSettledTimer = window.setTimeout(() => {
      visibleCloudSaveState.value = state;
      cloudSavePendingSince = 0;
      cloudSaveSettledTimer = undefined;
    }, remaining);
    return;
  }

  visibleCloudSaveState.value = state;
  cloudSavePendingSince = 0;
}

watch(storageState, updateVisibleCloudSaveState, { immediate: true });
const cloudBusinessEntities = new Set([
  "tasks",
  "task_daily_estimates",
  "task_recurrence_rules",
  "task_occurrences",
  "work_days",
  "time_entries",
  "unassigned_sessions",
  "reports",
]);
const cloudHasBusinessData = computed(() => {
  return Boolean(migrationPreview.value?.entities.some((item) => cloudBusinessEntities.has(item.entity) && (item.cloudCount ?? 0) > 0));
});

function formatSyncTime(value: number) {
  return new Date(value).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function cloudConflictTitle(conflict: CloudSyncConflict) {
  const payload = conflict.localPayload ?? {};
  if (conflict.entityType === "unassigned_session") {
    return `未归属时间 ${payload.work_date ?? ""}`.trim();
  }
  const title = payload.title ?? payload.name ?? payload.report_type ?? payload.key ?? payload.provider;
  if (conflict.entityType === "task_daily_estimate") return `按日预估 ${payload.work_date ?? conflict.entityId ?? ""}`.trim();
  if (conflict.entityType === "task_recurrence_rule") return `重复规则 ${payload.task_id ?? conflict.entityId ?? ""}`.trim();
  return typeof title === "string" && title ? title : `${cloudConflictEntityLabel(conflict.entityType)} ${conflict.entityId ?? ""}`.trim();
}

function cloudConflictEntityLabel(entityType: string) {
  if (entityType === "time_entry") return "时间记录";
  if (entityType === "task") return "普通事项";
  if (entityType === "task_occurrence") return "周期事项";
  if (entityType === "task_daily_estimate") return "按日预估";
  if (entityType === "task_recurrence_rule") return "重复规则";
  const labels: Record<string, string> = {
    subject: "主体",
    report: "报告",
    setting: "设置",
    integration_config: "集成配置",
    unassigned_session: "未归属时间",
    task_recurrence_rule: "周期规则",
  };
  return labels[entityType] ?? "同步数据";
}

function cloudConflictActionLabel(operationType: string) {
  const labels: Record<string, string> = {
    timer_start: "开始计时",
    timer_pause: "暂停计时",
    timer_resume: "继续计时",
    timer_stop: "结束计时",
    time_allocation_replace: "确认时间归属",
    task_set_completed_from_allocation: "归属时同时完成普通事项",
    task_occurrence_set_completed_from_allocation: "归属时同时完成周期事项",
    task_daily_estimate_set: "设置按日预估",
    task_daily_estimate_clear: "清空按日预估",
    task_recurrence_save: "保存重复规则",
    task_recurrence_close: "关闭重复规则",
    unassigned_session_create: "开始共享未归属计时",
    unassigned_session_pause: "兼容旧版未归属状态",
    unassigned_session_resume: "兼容旧版未归属状态",
    unassigned_session_awaiting_resolution: "兼容旧版未归属状态",
    unassigned_resolve_work: "分配未归属时间",
    unassigned_resolve_break: "记为休息",
    unassigned_discard: "丢弃未归属时间",
    unassigned_time_entry_create: "兼容旧版处理结果",
  };
  return labels[operationType] ?? operationType;
}

function cloudQueueTitle(item: CloudSyncQueueItem) {
  const payload = item.payload ?? {};
  const title = payload.title ?? payload.name ?? payload.label_snapshot ?? payload.report_type ?? payload.key ?? payload.provider;
  return typeof title === "string" && title ? title : `${cloudConflictEntityLabel(item.entityType)} ${item.entityId ?? ""}`.trim();
}

function cloudQueueStateLabel(state: string) {
  const labels: Record<string, string> = { pending: "等待同步", sending: "正在同步", failed: "同步失败", conflict: "存在冲突" };
  return labels[state] ?? state;
}

function formatQueueTime(value: number) {
  return new Date(value).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

const conflictFieldLabels: Record<string, string> = {
  version: "版本",
  title: "标题",
  name: "名称",
  status: "状态",
  state: "计时状态",
  work_date: "工作日期",
  estimate_minutes: "按日预估",
  deleted: "清空预估",
  action: "规则动作",
  frequency: "重复频率",
  weekdays_mask: "重复星期",
  effective_start: "生效日期",
  effective_end: "结束日期",
  resolution_type: "处理方式",
  resolved_at: "处理时间",
  first_started_at: "开始时间",
  predecessor_session_id: "前一段未归属时间",
  rules: "规则区间",
  occurrence_date: "轮次日期",
  label_snapshot: "计时事项",
  started_at: "开始时间",
  ended_at: "结束时间",
  duration_seconds: "累计用时",
  note: "备注",
  default_task_id: "默认事项",
  allocations: "时间归属",
};

const conflictFieldOrder = Object.keys(conflictFieldLabels);

function conflictPayload(conflict: CloudSyncConflict, source: "local" | "cloud") {
  return (source === "local" ? conflict.localPayload : conflict.cloudPayload) ?? null;
}

function conflictFields(conflict: CloudSyncConflict) {
  const local = conflict.localPayload ?? {};
  const cloud = conflict.cloudPayload ?? {};
  return conflictFieldOrder.filter((key) => key in local || key in cloud);
}

function conflictValue(payload: Record<string, unknown> | null | undefined, key: string) {
  if (!payload || !(key in payload) || payload[key] === null || payload[key] === undefined || payload[key] === "") return "无";
  const value = payload[key];
  if ((key === "started_at" || key === "ended_at") && typeof value === "number") return new Date(value).toLocaleString("zh-CN");
  if (key === "duration_seconds" && typeof value === "number") return `${Math.floor(value / 60)} 分 ${value % 60} 秒`;
  if (key === "allocations" && Array.isArray(value)) return value.length ? `${value.length} 项，共 ${value.reduce((sum, item) => sum + (typeof item === "object" && item && "minutes" in item && typeof item.minutes === "number" ? item.minutes : 0), 0)} 分钟` : "未归属";
  if (typeof value === "boolean") return value ? "是" : "否";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

function conflictFieldChanged(conflict: CloudSyncConflict, key: string) {
  return JSON.stringify(conflict.localPayload?.[key]) !== JSON.stringify(conflict.cloudPayload?.[key]);
}

function formattedConflictPayload(conflict: CloudSyncConflict, source: "local" | "cloud") {
  const payload = conflictPayload(conflict, source);
  return payload ? JSON.stringify(payload, null, 2) : "无";
}

function settingString(scope: "shared" | "device", key: string, fallback = "") {
  const value = (scope === "shared" ? settingsSnapshot.value?.shared : settingsSnapshot.value?.device)?.[key];
  return typeof value === "string" ? value : fallback;
}

function settingNumber(scope: "shared" | "device", key: string, fallback = 0) {
  const value = (scope === "shared" ? settingsSnapshot.value?.shared : settingsSnapshot.value?.device)?.[key];
  return typeof value === "number" ? value : fallback;
}

function settingBoolean(scope: "shared" | "device", key: string, fallback = false) {
  const value = (scope === "shared" ? settingsSnapshot.value?.shared : settingsSnapshot.value?.device)?.[key];
  return typeof value === "boolean" ? value : fallback;
}

async function loadSettings() {
  try {
    settingsSnapshot.value = await getSettings();
    settingsDraft.obsidianRootPath = settingString("device", "obsidian_root_path");
    settingsDraft.obsidianDailyPathPattern = settingString("device", "obsidian_daily_path_pattern", "工作日报/{date}.md");
    settingsDraft.seatableServerUrl = String(settingsSnapshot.value.integrations.seatable?.config.serverUrl ?? "https://cloud.seatable.cn");
    settingsDraft.seatableTaskTable = String(settingsSnapshot.value.integrations.seatable?.config.taskTable ?? "事项计划");
    settingsDraft.seatableTaskView = String(settingsSnapshot.value.integrations.seatable?.config.taskView ?? "默认视图");
    settingsDraft.seatableReimbursementTable = String(settingsSnapshot.value.integrations.seatable?.config.reimbursementTable ?? "报销");
    settingsDraft.seatableLocalIdField = String(settingsSnapshot.value.integrations.seatable?.config.localIdField ?? "本地事项ID");
    settingsDraft.subjectName = store.subjects.find((subject) => subject.id === settingString("shared", "default_subject_id"))?.name ?? "默认";
    settingsDraft.salaryHourlyRate = settingNumber("shared", "salary_hourly_rate");
    settingsDraft.reportDurationFormat = settingString("shared", "report_duration_format", "minutes") === "hours" ? "hours" : "minutes";
    settingsDraft.timezone = settingString("shared", "timezone", store.workspaceTimezone);
    const appTheme = settingString("device", "app_theme", "forest");
    settingsDraft.appTheme = isAppTheme(appTheme) ? appTheme : "forest";
    applyAppTheme(settingsDraft.appTheme, true);
    settingsDraft.completionFeedbackEnabled = settingBoolean("device", "completion_feedback_enabled", true);
    setCompletionFeedbackEnabled(settingsDraft.completionFeedbackEnabled);
    settingsDraft.completionFeedbackSoundEnabled = settingBoolean("device", "completion_feedback_sound_enabled", true);
    setCompletionFeedbackSoundEnabled(settingsDraft.completionFeedbackSoundEnabled);
    settingsDraft.trayHoverEnabled = settingBoolean("device", "tray_hover_enabled", true);
    settingsDraft.trayMenuSuppressHover = settingBoolean("device", "tray_menu_suppress_hover", true);
    settingsDraft.startMinimized = settingBoolean("device", "start_minimized");
    settingsDraft.storageMode = settingsSnapshot.value.storageMode;
    settingsDraft.supabaseProjectUrl = settingString("device", "supabase_project_url");
    settingsDraft.supabaseAnonKey = settingString("device", "supabase_anon_key");
    storageState.value = await getStorageMode();
    cloudSession.value = await getCloudSession();
    if (cloudSession.value.signedIn) cloudDevices.value = await listCloudDevices().catch(() => []);
    settingsLoaded.value = true;
  } catch {
    settingsLoaded.value = true;
  }
}

async function saveSettings() {
  if (settingsSaving.value) return;
  settingsSaving.value = true;
  try {
    await updateSetting("device", "obsidian_root_path", settingsDraft.obsidianRootPath);
    await updateSetting("device", "obsidian_daily_path_pattern", settingsDraft.obsidianDailyPathPattern);
    await updateSetting("device", "tray_hover_enabled", settingsDraft.trayHoverEnabled);
    await updateSetting("device", "tray_menu_suppress_hover", settingsDraft.trayMenuSuppressHover);
    await updateSetting("device", "start_minimized", settingsDraft.startMinimized);
    await updateSetting("device", "completion_feedback_enabled", settingsDraft.completionFeedbackEnabled);
    setCompletionFeedbackEnabled(settingsDraft.completionFeedbackEnabled);
    await updateSetting("device", "completion_feedback_sound_enabled", settingsDraft.completionFeedbackSoundEnabled);
    setCompletionFeedbackSoundEnabled(settingsDraft.completionFeedbackSoundEnabled);
    await updateSetting("device", "app_theme", settingsDraft.appTheme);
    applyAppTheme(settingsDraft.appTheme, true);
    if (settingsDraft.storageMode === "local") storageState.value = await setLocalStorageMode();
    await updateSetting("shared", "salary_hourly_rate", Number(settingsDraft.salaryHourlyRate) || 0);
    await updateSetting("shared", "report_duration_format", settingsDraft.reportDurationFormat);
    await updateSetting("shared", "timezone", settingsDraft.timezone);
    await store.refreshWorkspaceCalendar();
    const integration = settingsSnapshot.value?.integrations.seatable;
    settingsSnapshot.value = await updateIntegrationConfig("seatable", Boolean(settingsDraft.seatableServerUrl), {
      serverUrl: settingsDraft.seatableServerUrl,
      taskTable: settingsDraft.seatableTaskTable,
      taskView: settingsDraft.seatableTaskView,
      reimbursementTable: settingsDraft.seatableReimbursementTable,
      localIdField: settingsDraft.seatableLocalIdField,
      itemField: "事项",
      subjectField: "主体",
      projectField: "项目",
      solutionField: "方案",
      statusField: "状态",
      dateField: "日期",
      hoursField: "用时(h)",
      reimbursementPendingStatus: "待结算",
      reimbursementAmountField: "报销金额",
      reimbursementItemField: "报销项",
    }, integration?.version);
    if (settingsDraft.seatableToken.trim()) {
      settingsSnapshot.value = await setIntegrationSecret("seatable", settingsDraft.seatableToken.trim());
      settingsDraft.seatableToken = "";
    }
    ElMessage.success("设置已保存");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : "设置保存失败");
  } finally {
    settingsSaving.value = false;
  }
}

async function configureSupabase() {
  if (cloudWorking.value) return;
  cloudWorking.value = true;
  try {
    storageState.value = await configureCloud(settingsDraft.supabaseProjectUrl, settingsDraft.supabaseAnonKey);
    cloudSession.value = await getCloudSession();
    if (cloudSession.value.signedIn) cloudDevices.value = await listCloudDevices().catch(() => []);
    settingsSnapshot.value = await getSettings();
    ElMessage.success(cloudSession.value?.signedIn ? "Supabase 连接配置已保存" : "Supabase 连接配置已保存，请重新登录");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudWorking.value = false;
  }
}

async function loginSupabase() {
  if (cloudWorking.value) return;
  cloudWorking.value = true;
  try {
    await configureCloud(settingsDraft.supabaseProjectUrl, settingsDraft.supabaseAnonKey);
    cloudSession.value = await signInCloud(settingsDraft.supabaseEmail, settingsDraft.supabasePassword);
    await registerCloudDevice(settingString("device", "device_name", "当前设备"));
    cloudDevices.value = await listCloudDevices();
    storageState.value = await getStorageMode();
    settingsDraft.supabasePassword = "";
    migrationPreview.value = await previewStorageMigration();
    ElMessage.success("已登录 Supabase，可预览本地数据迁移");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudWorking.value = false;
  }
}

async function previewCloudMigration() {
  cloudWorking.value = true;
  try {
    migrationPreview.value = await previewStorageMigration();
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudWorking.value = false;
  }
}

async function migrateToCloud() {
  if (!migrationPreview.value?.canExecute || cloudWorking.value) return;
  try {
    await ElMessageBox.confirm("迁移完成后 Supabase 将作为最终权威数据源；本机修改会先保存到 SQLite 并进入同步队列，待同步或冲突处理完成前不会被云端快照覆盖。", "确认迁移到云端", {
      type: "warning", confirmButtonText: "确认迁移", cancelButtonText: "取消", closeOnClickModal: false, customClass: "work-confirm-dialog",
    });
  } catch { return; }
  cloudWorking.value = true;
  try {
    storageState.value = await executeStorageMigration();
    settingsDraft.storageMode = "cloud";
    settingsSnapshot.value = await getSettings();
    ElMessage.success("数据已迁移，当前使用 Supabase 云端模式");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudWorking.value = false;
  }
}

async function useExistingCloud() {
  if (cloudWorking.value) return;
  try {
    await ElMessageBox.confirm("当前设备会先确认没有待同步或冲突修改，再用云端数据重建本机缓存。未迁移的本地历史不会自动合并，请确认可以使用现有云端数据。", "使用现有云端数据", {
      type: "warning", confirmButtonText: "使用云端数据", cancelButtonText: "取消", closeOnClickModal: false, customClass: "work-confirm-dialog",
    });
  } catch { return; }
  cloudWorking.value = true;
  try {
    storageState.value = await executeStorageMigration("cloud_to_local_snapshot");
    settingsDraft.storageMode = "cloud";
    settingsSnapshot.value = await getSettings();
    migrationPreview.value = await previewStorageMigration("cloud_to_local_snapshot");
    await Promise.all([store.loadWorkspaceData(), store.loadTimeData(), store.loadReports(), store.loadUnassignedState()]);
    ElMessage.success("已加载现有云端数据");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudWorking.value = false;
  }
}

async function logoutSupabase() {
  cloudWorking.value = true;
  try {
    storageState.value = await signOutCloud();
    cloudSession.value = await getCloudSession();
    settingsDraft.storageMode = "local";
    migrationPreview.value = undefined;
    ElMessage.success("已退出云端账号并切回本地模式");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudWorking.value = false;
  }
}

async function wakeCloudSync(showFeedback: boolean) {
  if (storageState.value?.mode !== "cloud" || cloudSyncWorking.value) return;
  cloudSyncWorking.value = true;
  try {
    const result = await saveCloudSync();
    storageState.value = await getCloudSyncStatus();
    if (result.conflicts > 0 || storageState.value.conflictCount > 0) {
      if (showFeedback) ElMessage.warning("保存遇到版本冲突，请先处理冲突");
    } else if (result.pending > 0 || storageState.value.pendingOperations > 0) {
      const pending = Math.max(result.pending, storageState.value.pendingOperations);
      if (showFeedback) ElMessage.warning(`本机修改已保存，仍有 ${pending} 项等待同步`);
    } else if (showFeedback) {
      ElMessage.success(result.pushed > 0 ? `保存成功，已同步 ${result.pushed} 项修改` : "保存成功");
    }
  } catch (error) {
    if (showFeedback) ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudSyncWorking.value = false;
  }
}

async function refreshCloudSyncStatus() {
  storageState.value = await getCloudSyncStatus();
}

async function retryCloudSync() {
  if (storageState.value?.mode !== "cloud" || cloudSyncWorking.value) return;
  if (storageState.value.conflictCount > 0) {
    await openCloudConflicts();
    return;
  }
  try {
    await wakeCloudSync(true);
    if (storageState.value.conflictCount > 0) {
      await openCloudConflicts();
    }
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudSyncWorking.value = false;
  }
}

async function openCloudConflicts() {
  try {
    cloudConflicts.value = await listCloudSyncConflicts();
    cloudConflictDialogOpen.value = true;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  }
}

async function loadCloudSyncQueue() {
  cloudQueueLoading.value = true;
  try {
    [storageState.value, cloudQueueItems.value] = await Promise.all([getCloudSyncStatus(), listCloudSyncQueue()]);
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudQueueLoading.value = false;
  }
}

async function refreshOpenCloudSyncQueue() {
  if (!cloudQueueDialogOpen.value || cloudQueueLoading.value) return;
  try {
    cloudQueueItems.value = await listCloudSyncQueue();
  } catch {
    // 状态事件仍会继续刷新，弹窗保留上一次成功读取的本机队列。
  }
}

async function openCloudSyncQueue() {
  cloudQueueDialogOpen.value = true;
  await loadCloudSyncQueue();
}

async function syncAllCloudQueue() {
  if (cloudSyncWorking.value) return;
  try {
    await wakeCloudSync(true);
    cloudQueueItems.value = await listCloudSyncQueue();
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  }
}

async function openConflictsFromQueue() {
  cloudQueueDialogOpen.value = false;
  await openCloudConflicts();
}

async function handleCloudDockSync() {
  if (storageState.value?.conflictCount || storageState.value?.lastError || storageState.value?.authBlocked || !storageState.value?.online) {
    await openCloudSyncQueue();
    return;
  }
  if (storageState.value?.mode !== "cloud" || !cloudSession.value?.signedIn) {
    await openCloudAccountSettings();
    return;
  }
  await wakeCloudSync(true);
}

async function resolveAllConflicts(strategy: "use_cloud" | "keep_local") {
  if (cloudSyncWorking.value) return;
  const keepLocal = strategy === "keep_local";
  const conflictCount = cloudConflicts.value.length;
  try {
    await ElMessageBox.confirm(
      keepLocal
        ? `将对当前 ${conflictCount} 个冲突以及随后暴露的待同步冲突统一保留本地版本，并基于云端最新版本重新提交。`
        : `将对当前 ${conflictCount} 个冲突以及随后暴露的待同步冲突统一使用云端版本，本机对应修改会被放弃。`,
      keepLocal ? "全部保留本地" : "全部使用云端",
      { type: "warning", confirmButtonText: keepLocal ? "全部保留本地" : "全部使用云端", cancelButtonText: "取消", closeOnClickModal: false, customClass: "work-confirm-dialog" },
    );
  } catch { return; }
  cloudSyncWorking.value = true;
  try {
    storageState.value = await resolveAllCloudSyncConflicts(strategy);
    cloudConflicts.value = await listCloudSyncConflicts();
    await Promise.all([store.loadWorkspaceData(), store.loadTimeData(), store.loadReports(), store.loadUnassignedState()]);
    if (!cloudConflicts.value.length) cloudConflictDialogOpen.value = false;
    ElMessage.success(keepLocal ? "冲突已统一保留本地版本" : "冲突已统一使用云端版本");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudSyncWorking.value = false;
  }
}

async function checkSeaTableConnection() {
  seaTableLoading.value = true;
  try {
    const result = await testSeaTableConnection();
    const suffix = result.warnings.length ? `；${result.warnings.join("；")}` : "";
    ElMessage.success(`已连接 ${result.baseName || "SeaTable Base"}，事项表：${result.taskTable}${suffix}`);
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    seaTableLoading.value = false;
  }
}

function openResetLocalCacheDialog() {
  if (storageState.value?.mode !== "cloud") {
    ElMessage.warning("当前不是 Supabase 云端模式");
    return;
  }
  if (cloudWorking.value || cloudSyncWorking.value || resetLocalCacheWorking.value) {
    ElMessage.warning("云端操作正在执行，请稍后再试");
    return;
  }
  if (!cloudSession.value?.signedIn) {
    ElMessage.warning("Supabase 登录状态不可用，请先重新登录");
    return;
  }
  resetLocalCacheDialogOpen.value = true;
}

const resetLocalCacheDiscardWarning = computed(() => {
  const pending = storageState.value?.pendingOperations ?? 0;
  const conflicts = storageState.value?.conflictCount ?? 0;
  if (pending && conflicts) return `当前有 ${pending} 条待同步操作和 ${conflicts} 个冲突，将全部丢弃并以云端数据为准。`;
  if (pending) return `当前有 ${pending} 条待同步操作，将全部丢弃并以云端数据为准。`;
  if (conflicts) return `当前有 ${conflicts} 个同步冲突，将全部丢弃并以云端数据为准。`;
  return "本机尚未同步的修改将无法恢复，数据会以云端内容为准。";
});

async function confirmResetLocalCacheFromCloud() {
  if (resetLocalCacheWorking.value) return;
  resetLocalCacheWorking.value = true;
  cloudWorking.value = true;
  try {
    storageState.value = await resetLocalCacheFromCloud();
    migrationPreview.value = await previewStorageMigration("cloud_to_local_snapshot").catch(() => undefined);
    await Promise.all([store.loadWorkspaceData(), store.loadTimeData(), store.loadReports(), store.loadUnassignedState()]);
    resetLocalCacheDialogOpen.value = false;
    ElMessage.success("本地数据已重置，并重新加载云端数据");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    resetLocalCacheWorking.value = false;
    cloudWorking.value = false;
  }
}

async function logoutAllSupabase() {
  const confirmed = await runConfirmedCloudAction(
    () => ElMessageBox.confirm("将退出当前账号的所有设备，包括这台设备。所有设备都需要重新登录。", "退出所有设备", {
      type: "warning", confirmButtonText: "退出所有设备", cancelButtonText: "取消", closeOnClickModal: false, customClass: "work-confirm-dialog",
    }),
    async () => true,
  );
  if (!confirmed) return;
  cloudWorking.value = true;
  try {
    storageState.value = await signOutAllCloudDevices();
    cloudSession.value = await getCloudSession();
    cloudDevices.value = [];
    settingsDraft.storageMode = "local";
    ElMessage.success("已退出所有设备");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally { cloudWorking.value = false; }
}

async function revokeSupabaseDevice(device: CloudDevice) {
  const confirmed = await runConfirmedCloudAction(
    () => ElMessageBox.confirm(`撤销设备“${device.deviceName}”的云端授权？`, "撤销设备", {
      type: "warning", confirmButtonText: "确认撤销", cancelButtonText: "取消", closeOnClickModal: false, customClass: "work-confirm-dialog",
    }),
    async () => true,
  );
  if (!confirmed) return;
  cloudWorking.value = true;
  try {
    cloudDevices.value = await revokeCloudDevice(device.id);
    if (device.current) {
      cloudSession.value = await getCloudSession();
      settingsDraft.storageMode = "local";
    }
    ElMessage.success("设备授权已撤销");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally { cloudWorking.value = false; }
}

async function resumeBlockedCloudSync() {
  cloudWorking.value = true;
  try {
    storageState.value = await resumeCloudSyncAfterReauth();
    settingsDraft.storageMode = "cloud";
    ElMessage.success("已确认恢复待同步修改，可手动同步");
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally { cloudWorking.value = false; }
}

function updateProgressLabel(event: AppUpdateEventPayload) {
  if (event.stage === "download_progress" && event.contentLength && event.downloadedBytes !== undefined) {
    const percent = Math.min(100, Math.round((event.downloadedBytes / event.contentLength) * 100));
    updateProgressText.value = `下载中 ${percent}%`;
    return;
  }
  if (event.message) updateProgressText.value = event.message;
}

async function installCheckedUpdate(result: AppUpdateCheckResult) {
  const nextVersion = result.update?.version;
  if (!nextVersion) return;
  try {
    await ElMessageBox.confirm(
      `发现新版本 ${nextVersion}。当前版本：${result.currentVersion}。`,
      "发现新版本",
      { type: "info", confirmButtonText: "立即安装", cancelButtonText: "稍后", closeOnClickModal: false, customClass: "work-confirm-dialog" },
    );
  } catch { return; }

  updateInstalling.value = true;
  updateStatusText.value = `正在安装 ${nextVersion}`;
  updateProgressText.value = "准备下载更新";
  try {
    await downloadAndInstallUpdate();
    updateStatusText.value = `版本 ${nextVersion} 已安装`;
    updateProgressText.value = "重启应用后生效";
    try {
      await ElMessageBox.confirm(
        `版本 ${nextVersion} 已安装完成，是否现在重启应用？`,
        "更新已安装",
        { type: "success", confirmButtonText: "立即重启", cancelButtonText: "稍后", closeOnClickModal: false, customClass: "work-confirm-dialog" },
      );
      await restartApp();
    } catch { /* 用户选择稍后重启 */ }
  } catch (error) {
    updateStatusText.value = "更新安装失败";
    updateProgressText.value = "";
    ElMessageBox.alert(error instanceof Error ? error.message : String(error), "更新失败", {
      type: "error",
      confirmButtonText: "知道了",
      customClass: "work-confirm-dialog",
    });
  } finally {
    updateInstalling.value = false;
  }
}

async function handleCheckUpdate() {
  if (updateChecking.value || updateInstalling.value) return;
  store.activePage = "settings";
  updateChecking.value = true;
  updateStatusText.value = "正在检查更新";
  updateProgressText.value = "";
  try {
    const result = await checkAppUpdate();
    if (!result.available) {
      updateStatusText.value = "已是最新版本";
      ElMessageBox.alert(`当前版本 ${result.currentVersion} 已是最新版本。`, "已是最新版本", {
        type: "success",
        confirmButtonText: "知道了",
        customClass: "work-confirm-dialog",
      });
      return;
    }
    updateStatusText.value = `发现新版本 ${result.update?.version ?? ""}`.trim();
    await installCheckedUpdate(result);
  } catch (error) {
    updateStatusText.value = "检查更新失败";
    ElMessageBox.alert(error instanceof Error ? error.message : String(error), "检查更新失败", {
      type: "error",
      confirmButtonText: "知道了",
      customClass: "work-confirm-dialog",
    });
  } finally {
    updateChecking.value = false;
  }
}

async function toggleWindowMaximize() {
  windowMaximized.value = await toggleMainWindowMaximize();
}

function previewTheme(theme: AppTheme) {
  settingsDraft.appTheme = theme;
  applyAppTheme(theme);
}

onMounted(async () => {
  await loadSettings();
  windowMaximized.value = await isMainWindowMaximized();
  unlistenWindowResize = await onCurrentWindowResized((maximized) => {
    windowMaximized.value = maximized;
  });
  unlistenNavigatePage = await onNavigatePage((page) => {
    if (pages.some((item) => item.id === page)) store.activePage = page;
  });
  unlistenCloudSyncState = await onCloudSyncStateChanged((state) => {
    updateVisibleCloudSaveState(state);
    storageState.value = state;
    void refreshOpenCloudSyncQueue();
  });
  unlistenTrayCheckUpdate = await onTrayCheckUpdate(() => {
    void handleCheckUpdate();
  });
  unlistenAppUpdateEvent = await onAppUpdateEvent(updateProgressLabel);
});

onUnmounted(() => {
  unlistenWindowResize?.();
  unlistenNavigatePage?.();
  unlistenCloudSyncState?.();
  unlistenTrayCheckUpdate?.();
  unlistenAppUpdateEvent?.();
  if (recentCompletedTimer) window.clearTimeout(recentCompletedTimer);
  if (cloudSaveSettledTimer) window.clearTimeout(cloudSaveSettledTimer);
});

function openSubjectDialog() {
  editingSubjectId.value = "";
  subjectNameDraft.value = "";
  subjectDialogOpen.value = true;
}

function openSubjectEditor(subjectId: string) {
  const subject = store.subjects.find((item) => item.id === subjectId);
  if (!subject) return;
  editingSubjectId.value = subject.id;
  subjectNameDraft.value = subject.name;
  subjectDialogOpen.value = true;
}

async function saveSubject() {
  if (editingSubjectId.value) {
    if (!(await store.renameSubject(editingSubjectId.value, subjectNameDraft.value))) {
      ElMessage.warning("主体名称不能为空，也不能与现有主体重复");
      return;
    }
    subjectDialogOpen.value = false;
    ElMessage.success("主体名称已更新");
    return;
  }
  const subjectId = await store.addSubject(subjectNameDraft.value);
  if (!subjectId) {
    ElMessage.warning("请输入主体名称");
    return;
  }
  subjectDialogOpen.value = false;
  ElMessage.success("主体已添加");
}

function subjectTaskCount(subjectId: string) {
  return store.tasks.filter((task) => task.subjectId === subjectId && task.status !== "done").length;
}

function formatMinutes(minutes: number) {
  if (minutes < 60) return `${minutes}min`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? `${hours}h${rest}min` : `${hours}h`;
}

function formatLocalDate(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

async function openPlanImport() {
  if (!store.selectedSubjectId) return;
  importLoading.value = true;
  try {
    const target = new Date();
    const source = new Date(target);
    source.setDate(source.getDate() - 1);
    const preview = await previewObsidianPlan(formatLocalDate(source), formatLocalDate(target), store.selectedSubjectId);
    importPreview.value = preview;
    importItems.value = preview.items.map((item) => ({
      importItemId: item.id,
      selected: item.parseStatus === "recognized",
      title: item.title,
      estimateMinutes: item.estimateMinutes,
      parentId: item.parentId,
      sourceText: item.sourceText,
      parseStatus: item.parseStatus,
    }));
    importDialogOpen.value = true;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    importLoading.value = false;
  }
}

async function confirmPlanImport() {
  if (!importPreview.value || importSaving.value) return;
  importSaving.value = true;
  try {
    const result = await confirmObsidianPlan(
      importPreview.value.batchId,
      importItems.value.map(({ importItemId, selected, title, estimateMinutes }) => ({ importItemId, selected, title, estimateMinutes })),
      importPreview.value.version,
    );
    await store.loadWorkspaceData();
    importDialogOpen.value = false;
    ElMessage.success(result.alreadyConfirmed ? "该计划已导入" : `已导入 ${result.tasks.length} 条事项`);
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    importSaving.value = false;
  }
}

async function openSeaTableSync() {
  if (!store.selectedSubjectId || seaTableLoading.value) return;
  seaTableLoading.value = true;
  try {
    seaTablePreview.value = await previewSeaTableTaskSync({
      subjectId: store.selectedSubjectId,
      workDate: store.workspaceToday,
      taskIds: store.selectedSubjectTasks.map((task) => task.id),
    });
    seaTableResult.value = undefined;
    seaTableDialogOpen.value = true;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    seaTableLoading.value = false;
  }
}

async function confirmSeaTableSync() {
  if (!seaTablePreview.value || seaTableExecuting.value) return;
  if (seaTablePreview.value.conflictCount) {
    ElMessage.warning("存在可能重复的事项，请先在 SeaTable 中补充本地事项 ID 或清理重复记录");
    return;
  }
  try {
    await ElMessageBox.confirm(
      `将新增 ${seaTablePreview.value.createCount} 项、更新 ${seaTablePreview.value.updateCount} 项、跳过 ${seaTablePreview.value.skipCount} 项。`,
      "确认同步到 SeaTable",
      { type: "warning", confirmButtonText: "确认同步", cancelButtonText: "取消", customClass: "work-confirm-dialog", closeOnClickModal: false },
    );
  } catch {
    return;
  }
  seaTableExecuting.value = true;
  try {
    const result = await executeSeaTableTaskSync(seaTablePreview.value.runId);
    seaTableResult.value = result;
    if (result.failedCount) {
      ElMessage.warning(`部分同步失败：成功 ${result.successCount} 项，失败 ${result.failedCount} 项`);
    } else {
      seaTableDialogOpen.value = false;
      ElMessage.success(`同步完成：成功 ${result.successCount} 项，跳过 ${result.skippedCount} 项`);
    }
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    seaTableExecuting.value = false;
  }
}

async function retrySeaTableFailures() {
  if (!seaTableResult.value?.failedCount || seaTableExecuting.value) return;
  seaTableExecuting.value = true;
  try {
    const result = await retryFailedSeaTableSync(seaTableResult.value.runId);
    seaTableResult.value = result;
    if (result.failedCount) {
      ElMessage.warning(`仍有 ${result.failedCount} 项同步失败`);
    } else {
      seaTableDialogOpen.value = false;
      ElMessage.success(`失败事项已重试成功，共 ${result.successCount} 项`);
    }
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    seaTableExecuting.value = false;
  }
}

async function startTimerForTask(taskId: string) {
  if (store.runningEntry?.defaultTask === taskId) {
    store.activePage = "timer";
    return;
  }
  try {
    if (store.runningEntry) await store.stopTimer();
    await store.startTimer(taskId);
  } catch (error) {
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "开始计时失败");
  }
}

async function handleTaskRecurrence(
  taskId: string,
  recurrence?: { frequency: "daily" | "weekdays" | "weekly"; weekdaysMask?: number; effectiveStart: string },
) {
  try {
    await store.setTaskRecurrence(taskId, recurrence);
    ElMessage.success(recurrence ? "重复规则已保存" : "已关闭重复");
  } catch (error) {
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "重复规则保存失败");
  }
}

async function handleTaskSave(taskId: string) {
  try {
    await store.saveTask(taskId);
  } catch (error) {
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "事项保存失败");
  }
}

function currentOpenTaskCount(page = store.activePage) {
  if (page === "plan") return store.selectedSubjectTasks.filter((task) => task.status !== "done").length;
  if (page === "today") return store.todayTasks.filter((task) => task.status !== "done").length;
  return undefined;
}

async function handleTaskToggle(task: Task, event: MouseEvent) {
  if (completingTaskIds.has(task.id)) return;
  completingTaskIds.add(task.id);
  const sourcePage = store.activePage;
  const openBefore = currentOpenTaskCount(sourcePage);
  const row = event.currentTarget instanceof HTMLElement
    ? event.currentTarget.closest<HTMLElement>(".reminder-row, .el-table__row")
    : undefined;
  const completing = task.status !== "done";
  if (completing && isCompletionFeedbackEnabled()) row?.classList.add("task-completion-confirmed");
  try {
    const result = await store.commitTaskToggle(task);
    if (!result) return;
    if (result.direction === "completed" && isCompletionFeedbackEnabled()) {
      window.setTimeout(() => row?.classList.remove("task-completion-confirmed"), 380);
    }
    void Promise.all([store.loadWorkspaceData(), store.loadTodayOverview()]);
    if (result.completedCount > 0 && isCompletionFeedbackEnabled()) {
      recentCompletedTaskId.value = result.taskId;
      if (recentCompletedTimer) window.clearTimeout(recentCompletedTimer);
      recentCompletedTimer = window.setTimeout(() => recentCompletedTaskId.value = "", 720);
      const count = document.querySelector<HTMLElement>("[data-completed-count]");
      count?.classList.remove("completion-count-bump");
      void count?.offsetWidth;
      count?.classList.add("completion-count-bump");
      const openAfter = remainingOpenTaskCount(openBefore, result.completedCount)
        ?? currentOpenTaskCount(sourcePage);
      playCompletionFeedback({ completedCount: result.completedCount, openBefore, openAfter });
    }
  } catch (error) {
    row?.classList.remove("task-completion-confirmed");
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "事项状态更新失败");
  } finally {
    if (!isCompletionFeedbackEnabled()) row?.classList.remove("task-completion-confirmed");
    completingTaskIds.delete(task.id);
  }
}
</script>

<template>
  <main class="management-shell">
    <header class="window-titlebar" data-tauri-drag-region="deep">
      <div class="window-titlebar-drag-area" data-tauri-drag-region="deep">
        <strong class="window-titlebar-app-name" data-tauri-drag-region>时序</strong>
        <span class="window-titlebar-version" data-tauri-drag-region>v{{ appVersion }}</span>
      </div>
      <div class="window-controls" aria-label="窗口控制">
        <button type="button" title="最小化" aria-label="最小化窗口" @pointerdown.stop.prevent="minimizeMainWindow"><Minus :size="15" /></button>
        <button type="button" :title="windowMaximized ? '还原' : '最大化'" :aria-label="windowMaximized ? '还原窗口' : '最大化窗口'" @pointerdown.stop.prevent="toggleWindowMaximize">
          <Copy v-if="windowMaximized" :size="13" />
          <Square v-else :size="12" />
        </button>
        <button class="close" type="button" title="关闭到托盘" aria-label="关闭窗口" @pointerdown.stop.prevent="closeMainWindow"><X :size="16" /></button>
      </div>
    </header>

    <aside class="sidebar">
      <div class="brand-block">
        <img class="brand-mark" :src="appIconUrl" alt="" />
        <div><strong>时序</strong><span>计时 · 工时 · 日报</span></div>
      </div>
      <nav class="nav-list">
        <template v-for="page in pages" :key="page.id">
          <button :class="{ active: store.activePage === page.id }" @click="store.selectPage(page.id)">
            <component :is="page.icon" :size="17" />
            <span>{{ page.label }}</span>
            <i v-if="page.id === 'settings' && storageState?.mode === 'cloud' && cloudSyncNeedsAttention" :class="['nav-sync-indicator', { conflict: cloudSyncAttentionLevel === 'critical' }]" :title="cloudSyncLabel"></i>
          </button>
          <section v-if="page.id === 'plan'" class="subject-nav-list">
            <div
              v-for="subject in store.subjects"
              :key="subject.id"
              class="subject-nav-row"
              :class="{ active: store.activePage === 'plan' && store.selectedSubjectId === subject.id }"
            >
              <button class="subject-select-button" type="button" :title="subject.name" @click="store.selectSubject(subject.id)">
                <Folder :size="13" />
                <span>{{ subject.name }}</span>
                <small>{{ subjectTaskCount(subject.id) }}</small>
              </button>
              <button class="subject-edit-button" type="button" :title="`编辑${subject.name}`" @click="openSubjectEditor(subject.id)"><Pencil :size="12" /></button>
            </div>
            <button class="subject-add-button" type="button" title="添加主体" @click="openSubjectDialog"><Plus :size="13" /><span>添加主体</span></button>
          </section>
        </template>
      </nav>
      <section class="side-summary">
        <span>今日完成</span>
        <strong>{{ store.doneCount }} / {{ store.tasks.length }}</strong>
        <div><i :style="{ width: `${Math.round(store.doneCount / store.tasks.length * 100)}%` }"></i></div>
      </section>
    </aside>

    <section class="content-shell">
      <header class="topbar">
        <div class="topbar-title">
          <p class="eyebrow">2026-09-15 · Tuesday</p>
          <h1>{{ pages.find((page) => page.id === store.activePage)?.label }}工作台</h1>
        </div>
        <div class="topbar-tools">
          <UnassignedTimeIndicator />
          <GlobalTimerBar @open="store.activePage = 'timer'" />
        </div>
      </header>

      <div class="content-scroll" @scroll="handleSettingsScroll">
        <TodayOverviewWorkspace
          v-if="store.activePage === 'today'"
          @start-timer="startTimerForTask"
          @open-timer="store.activePage = 'timer'"
          @open-plan="store.activePage = 'plan'"
          @resolve-unassigned="store.promptUnassignedResolution(true)"
        />

      <section v-else-if="store.activePage === 'plan'" class="plan-edit-layout">
        <PlanListEditor
          :tasks="store.selectedSubjectTasks"
          :subject-name="store.selectedSubject?.name ?? '待办'"
          :selected-id="store.selectedTaskId"
          :recent-completed-id="recentCompletedTaskId"
          :running-task-id="store.runningEntry?.defaultTask"
          :format-minutes="formatMinutes"
          :today-date="store.workspaceToday"
          @select="store.selectedTaskId = $event"
          @add="store.addTask('', store.selectedSubjectId)"
          @add-child="store.addChildTask"
          @save="handleTaskSave"
          @import-plan="openPlanImport"
          @sync-sea-table="openSeaTableSync"
          @duplicate="store.duplicateTask"
          @delete="store.deleteTask"
          @toggle="handleTaskToggle"
          @indent="store.indentTask"
          @outdent="store.outdentTask"
          @start-timer="startTimerForTask"
          @reorder="store.reorderTask"
          @set-recurrence="handleTaskRecurrence"
        />
      </section>

      <TimeTrackingWorkspace v-else-if="store.activePage === 'timer'" />

      <ReportWorkspace v-else-if="store.activePage === 'reports'" />

      <section v-else class="settings-layout">
        <aside class="settings-page-nav" aria-label="设置页导航">
          <div class="settings-page-nav-head"><span class="settings-page-nav-mark"><Settings :size="16" /></span><div><strong>设置</strong><small>快速定位设置区域</small></div></div>
          <nav class="settings-page-nav-list">
            <section v-for="group in settingsNavGroups" :key="group.label" class="settings-page-nav-group">
              <div class="settings-page-nav-group-title">{{ group.label }}</div>
              <button v-for="item in group.items" :key="item.id" class="settings-page-nav-item" :class="{ active: activeSettingsSection === item.id }" type="button" @click="scrollToSettingsSection(item.id)"><component :is="item.icon" :size="14" /><span>{{ item.label }}</span></button>
            </section>
          </nav>
        </aside>
        <div class="settings-grid">
        <div id="settings-appearance" data-settings-section="appearance" class="work-panel settings-panel settings-appearance-panel">
          <div class="panel-title"><h2>外观主题</h2><span>当前设备独立设置</span></div>
          <div class="theme-picker" role="radiogroup" aria-label="应用主题">
            <button
              v-for="theme in APP_THEMES"
              :key="theme.id"
              class="theme-option"
              :class="{ active: settingsDraft.appTheme === theme.id }"
              type="button"
              role="radio"
              :aria-checked="settingsDraft.appTheme === theme.id"
              :disabled="!settingsLoaded"
              @click="previewTheme(theme.id)"
            >
              <span class="theme-swatch" :style="{ '--theme-bg': theme.swatches[0], '--theme-panel': theme.swatches[1], '--theme-accent': theme.swatches[2] }">
                <i></i><i></i><i></i>
              </span>
              <span class="theme-option-copy"><strong>{{ theme.name }}</strong><small>{{ theme.description }}</small></span>
              <span class="theme-selected-mark" aria-hidden="true"></span>
            </button>
          </div>
          <small class="settings-hint">选择后立即预览，点击“保存设置”后在下次启动时继续使用。</small>
        </div>

        <div id="settings-local" data-settings-section="local" class="work-panel settings-panel">
          <div class="panel-title"><h2>本机配置</h2><span>路径只保存在当前设备</span></div>
          <label>Obsidian 根目录<input v-model="settingsDraft.obsidianRootPath" :disabled="!settingsLoaded" placeholder="D:/Obsidian" /></label>
          <label>日报路径规则<input v-model="settingsDraft.obsidianDailyPathPattern" :disabled="!settingsLoaded" placeholder="工作日报/{date}.md" /></label>
          <label>默认主体<input v-model="settingsDraft.subjectName" disabled /></label>
          <label>工资时薪<input v-model.number="settingsDraft.salaryHourlyRate" :disabled="!settingsLoaded" min="0" type="number" /></label>
          <div class="settings-choice-field">
            <span>工作空间时区</span>
            <select v-model="settingsDraft.timezone" :disabled="!settingsLoaded">
              <option value="Asia/Shanghai">中国标准时间（Asia/Shanghai）</option>
              <option value="Asia/Tokyo">日本标准时间（Asia/Tokyo）</option>
              <option value="Europe/London">英国时间（Europe/London）</option>
              <option value="America/New_York">美国东部时间（America/New_York）</option>
              <option value="America/Los_Angeles">美国西部时间（America/Los_Angeles）</option>
            </select>
            <small class="settings-hint">工作空间时区决定“今天”和每天零点。修改后只影响后续日期边界，不会重写已有工时、轮次或报告。</small>
          </div>
          <div class="settings-choice-field">
            <span>报告时长格式</span>
            <div class="settings-segmented" role="radiogroup" aria-label="报告时长格式">
              <button type="button" role="radio" :aria-checked="settingsDraft.reportDurationFormat === 'minutes'" :class="{ active: settingsDraft.reportDurationFormat === 'minutes' }" :disabled="!settingsLoaded" @click="settingsDraft.reportDurationFormat = 'minutes'"><strong>分钟</strong><small>12min</small></button>
              <button type="button" role="radio" :aria-checked="settingsDraft.reportDurationFormat === 'hours'" :class="{ active: settingsDraft.reportDurationFormat === 'hours' }" :disabled="!settingsLoaded" @click="settingsDraft.reportDurationFormat = 'hours'"><strong>小时</strong><small>0.2h</small></button>
            </div>
            <small class="settings-hint">用于新生成或重新生成的报告，默认使用分钟。</small>
          </div>
        </div>

        <div id="settings-seatable" data-settings-section="seatable" class="work-panel settings-panel">
          <div class="panel-title"><h2>SeaTable</h2><span>{{ settingsSnapshot?.secrets.seatableTokenSet ? 'Token 已安全保存' : '尚未配置 Token' }}</span></div>
          <label>服务地址<input v-model="settingsDraft.seatableServerUrl" :disabled="!settingsLoaded" placeholder="https://cloud.seatable.cn" /></label>
          <label>事项表<input v-model="settingsDraft.seatableTaskTable" :disabled="!settingsLoaded" placeholder="事项计划" /></label>
          <label>事项视图<input v-model="settingsDraft.seatableTaskView" :disabled="!settingsLoaded" placeholder="默认视图" /></label>
          <label>本地事项 ID 字段<input v-model="settingsDraft.seatableLocalIdField" :disabled="!settingsLoaded" placeholder="本地事项ID" /></label>
          <label>报销表<input v-model="settingsDraft.seatableReimbursementTable" :disabled="!settingsLoaded" placeholder="报销" /></label>
          <label>Base API Token<input v-model="settingsDraft.seatableToken" :disabled="!settingsLoaded" type="password" :placeholder="settingsSnapshot?.secrets.seatableTokenSet ? '留空表示不修改' : '输入后保存到系统凭据库'" autocomplete="off" /></label>
          <button class="secondary-button" type="button" :disabled="seaTableLoading || !settingsSnapshot?.secrets.seatableTokenSet" @click="checkSeaTableConnection">{{ seaTableLoading ? '检测中...' : '检测连接与表结构' }}</button>
        </div>

        <div id="settings-storage" data-settings-section="storage" class="work-panel settings-panel settings-storage-panel">
          <div class="panel-title"><h2>数据存储</h2><span>{{ settingsDraft.storageMode === 'local' ? '本地 SQLite' : 'Supabase 云端' }}</span></div>
          <div class="storage-status-card">
            <div class="storage-status-main"><span :class="['storage-state-dot', storageState?.online ? 'online' : '', { warning: storageState?.conflictCount || storageState?.pendingOperations || storageState?.lastError }]"></span><div><strong>{{ storageState?.mode === 'cloud' ? '云端数据' : '本地数据' }}</strong><small>{{ storageState?.mode === 'cloud' ? cloudSyncLabel : '无需网络即可使用' }}</small></div></div>
            <div v-if="storageState?.mode === 'cloud'" class="storage-status-meta"><span>变更序号</span><strong>{{ storageState.lastChangeSeq }}</strong></div>
          </div>
          <p v-if="cloudSyncDetail" class="cloud-sync-detail">{{ cloudSyncDetail }}</p>
          <div v-if="storageState?.mode === 'cloud'" class="cloud-sync-actions storage-status-actions">
            <button v-if="cloudSyncNeedsAttention" class="secondary-button compact" type="button" :disabled="cloudSyncWorking" @click="refreshCloudSyncStatus">刷新状态</button>
            <button v-if="storageState.pendingOperations && !storageState.conflictCount" class="secondary-button compact" type="button" :disabled="cloudSyncWorking || !storageState.online" @click="retryCloudSync">重试同步</button>
            <button v-if="storageState.conflictCount" class="danger-button compact" type="button" :disabled="cloudSyncWorking" @click="openCloudConflicts">处理冲突</button>
          </div>

          <div class="storage-section storage-connection-section">
            <div class="storage-section-heading"><div><strong>连接配置</strong><small>用于连接当前 Supabase 项目</small></div><span>公开配置</span></div>
            <div class="storage-connection-grid">
              <label>Supabase Project URL<input v-model="settingsDraft.supabaseProjectUrl" :disabled="!settingsLoaded || cloudWorking" placeholder="https://example.supabase.co" /></label>
              <label>Supabase anon key<input v-model="settingsDraft.supabaseAnonKey" :disabled="!settingsLoaded || cloudWorking" type="password" placeholder="公开 anon key，不要填写 service_role" autocomplete="off" /></label>
            </div>
            <div class="settings-inline-actions"><button class="secondary-button" type="button" :disabled="cloudWorking" @click="configureSupabase">保存连接</button></div>
          </div>

          <p v-if="cloudSession?.status === 'offline_saved'" class="settings-help">账号已安全保存在本机，当前无法连接 Supabase；恢复网络后会自动续期，无需重新输入密码。</p>
          <div v-if="cloudAuthBlocked" class="cloud-sync-banner warning"><AlertTriangle :size="16" /><div><strong>待同步修改已暂停</strong><span>{{ cloudSession?.signedIn ? '重新登录后不会自动上传，请确认后恢复。' : '本地数据和待同步修改均已保留，重新登录后再确认是否恢复上传。' }}</span></div><button v-if="cloudSession?.signedIn" class="secondary-button compact" type="button" :disabled="cloudWorking" @click="resumeBlockedCloudSync">确认恢复</button></div>

          <div v-if="storageState?.mode === 'cloud'" class="storage-section local-cache-section">
            <div class="local-cache-reset-row">
              <div><strong>本地数据缓存</strong><small>本机显示异常时，可清空本机业务缓存后重新下载云端数据。</small></div>
              <button class="danger-button compact" type="button" :disabled="resetLocalCacheWorking" @click="openResetLocalCacheDialog"><LoaderCircle v-if="resetLocalCacheWorking" class="sync-spin" :size="13" />{{ resetLocalCacheWorking ? '重置中...' : '重置本地数据' }}</button>
            </div>
            <p v-if="storageState.pendingOperations || storageState.conflictCount" class="settings-help">重置时会丢弃待同步修改和冲突记录，并重新以云端数据建立本机缓存。</p>
          </div>

          <template v-if="cloudNeedsReauth">
            <div class="storage-section storage-login-section">
              <div class="storage-section-heading"><div><strong>登录 Supabase</strong><small>登录后会安全保存在本机，下次启动自动恢复</small></div><span>需要验证</span></div>
              <div class="storage-login-grid"><label>登录邮箱<input v-model="settingsDraft.supabaseEmail" :disabled="cloudWorking" type="email" autocomplete="username" placeholder="user@example.com" /></label><label>登录密码<input v-model="settingsDraft.supabasePassword" :disabled="cloudWorking" type="password" autocomplete="current-password" /></label></div>
              <div class="settings-inline-actions"><button class="primary-button" type="button" :disabled="cloudWorking || !settingsDraft.supabaseEmail || !settingsDraft.supabasePassword" @click="loginSupabase">登录 Supabase</button></div>
            </div>
          </template>
          <template v-else>
            <div class="storage-section storage-account-section">
              <div class="storage-section-heading"><div><strong>账号与设备</strong><small>管理当前账号的登录状态和授权设备</small></div><span>已登录</span></div>
              <div class="cloud-account-row"><div><span>当前账号</span><strong>{{ cloudSession?.email || cloudSession?.userId || '已保存账号' }}</strong></div><div class="settings-inline-actions cloud-account-actions"><button class="secondary-button compact" type="button" :disabled="cloudWorking" @click="logoutSupabase">退出当前设备</button><button class="danger-button compact" type="button" :disabled="cloudWorking" @click="logoutAllSupabase">退出所有设备</button></div></div>
              <div v-if="cloudDevices.length" class="device-session-list">
                <div v-for="device in cloudDevices" :key="device.id" class="cloud-account-row">
                  <div><span>{{ cloudDeviceKind(device) }} · {{ device.appVersion }}</span><strong>{{ device.deviceName }}</strong><small>最近在线 {{ new Date(device.lastSeenAt).toLocaleString() }}{{ device.revokedAt ? ' · 已撤销' : '' }}</small></div>
                  <button class="secondary-button compact" type="button" :disabled="cloudDeviceActionDisabled(device, cloudWorking)" @click="revokeSupabaseDevice(device)">{{ cloudDeviceActionLabel(device) }}</button>
                </div>
              </div>
            </div>
            <div class="storage-section storage-migration-section">
              <div class="storage-section-heading"><div><strong>迁移本地数据</strong><small>将本机 SQLite 数据安全迁移到云端</small></div><span>可选</span></div>
              <button v-if="!migrationPreview" class="secondary-button" type="button" :disabled="cloudWorking" @click="previewCloudMigration">预览本地数据迁移</button>
              <div v-else class="migration-preview-block">
                <div class="migration-count-grid"><span v-for="item in migrationPreview.entities" :key="item.entity"><strong>{{ item.localCount }}</strong><small>{{ item.entity }}</small></span></div>
                <p v-if="migrationPreview.conflicts.length">{{ migrationPreview.conflicts.join('；') }}</p>
                <div class="settings-inline-actions">
                  <button class="primary-button" type="button" :disabled="cloudWorking || !migrationPreview.canExecute || storageState?.mode === 'cloud'" @click="migrateToCloud">{{ storageState?.mode === 'cloud' ? '当前已使用云端' : '迁移并切换到云端' }}</button>
                  <button v-if="cloudHasBusinessData" class="secondary-button" type="button" :disabled="cloudWorking || storageState?.mode === 'cloud'" @click="useExistingCloud">使用现有云端数据</button>
                </div>
              </div>
            </div>
          </template>
        </div>

        <div id="settings-feedback" data-settings-section="feedback" class="work-panel settings-panel">
          <div class="panel-title"><h2>交互反馈</h2><span>当前设备独立设置</span></div>
          <label class="switch-line"><input v-model="settingsDraft.completionFeedbackEnabled" :disabled="!settingsLoaded" type="checkbox" @change="setCompletionFeedbackEnabled(settingsDraft.completionFeedbackEnabled)" />启用完成事项时的激励动画</label>
          <p class="settings-help">开启后显示彩带、完成提示和列表反馈；此选项不受 Windows 动画设置影响。</p>
          <label class="switch-line"><input v-model="settingsDraft.completionFeedbackSoundEnabled" :disabled="!settingsLoaded || !settingsDraft.completionFeedbackEnabled" type="checkbox" @change="setCompletionFeedbackSoundEnabled(settingsDraft.completionFeedbackSoundEnabled)" />播放完成事项时的激励音效</label>
          <p class="settings-help">使用本地成功提示音与视觉特效同步播放；关闭后仅保留视觉反馈。</p>
        </div>

        <div id="settings-tray" data-settings-section="tray" class="work-panel settings-panel">
          <div class="panel-title"><h2>托盘行为</h2><span>当前设备独立设置</span></div>
          <label class="switch-line"><input v-model="settingsDraft.trayHoverEnabled" :disabled="!settingsLoaded" type="checkbox" />鼠标停留后显示悬浮面板</label>
          <label class="switch-line"><input v-model="settingsDraft.trayMenuSuppressHover" :disabled="!settingsLoaded" type="checkbox" />右键菜单打开时抑制悬浮面板</label>
          <label class="switch-line"><input v-model="settingsDraft.startMinimized" :disabled="!settingsLoaded" type="checkbox" />开机后最小化启动</label>
          <button class="primary-button settings-save-button" type="button" :disabled="!settingsLoaded || settingsSaving" @click="saveSettings">{{ settingsSaving ? '保存中...' : '保存设置' }}</button>
        </div>

        <div id="settings-hooks" data-settings-section="hooks" class="work-panel settings-panel settings-hooks-panel">
          <AutomationHooksSettings />
        </div>

        <div id="settings-update" data-settings-section="update" class="work-panel settings-panel settings-update-panel">
          <div class="panel-title"><h2>应用更新</h2><span>v{{ appVersion }}</span></div>
          <div class="update-state-row">
            <div>
              <strong>{{ updateStatusText }}</strong>
              <small>{{ updateProgressText || '托盘右键菜单也可以检查更新' }}</small>
            </div>
            <button class="secondary-button compact" type="button" :disabled="updateChecking || updateInstalling" @click="handleCheckUpdate">{{ updateChecking ? '检查中...' : updateInstalling ? '安装中...' : '检查更新' }}</button>
          </div>
        </div>
        </div>
      </section>
      </div>
      <footer class="sync-status-dock" aria-label="保存状态">
        <div class="sync-status-main" :class="[cloudSyncAttentionLevel, { working: cloudSyncIndicatorWorking }]">
          <button class="sync-user-pill" type="button" :title="`${cloudAccountLabel}，点击管理账号`" @click="openCloudAccountSettings"><UserRound :size="14" /><strong>{{ cloudAccountLabel }}</strong></button>
          <button class="sync-state-pill" :class="[cloudSaveAttentionLevel, { synced: visibleCloudSaveState?.mode === 'cloud' && !cloudSaveNeedsAttention }]" type="button" :title="`${cloudSaveLabel}，点击查看待保存列表`" @click="openCloudSyncQueue">
            <LoaderCircle v-if="cloudSyncIndicatorWorking" class="sync-spin" :size="14" />
            <AlertTriangle v-else-if="cloudSaveNeedsAttention" :size="14" />
            <CheckCircle2 v-else :size="14" />
            <span>{{ cloudSaveLabel }}</span>
          </button>
        </div>
        <button class="sync-dock-button" :class="{ running: cloudSyncWorking, 'has-conflict': storageState?.conflictCount }" type="button" :disabled="cloudDockSyncDisabled" :title="cloudDockSyncTitle" @click="handleCloudDockSync">
          <LoaderCircle v-if="cloudSyncWorking" class="sync-spin" :size="14" />
          <AlertTriangle v-else-if="storageState?.conflictCount" :size="14" />
          <RefreshCw v-else-if="cloudDockRequiresStorageAttention" :size="14" />
          <Save v-else :size="14" />
          <span class="sync-dock-label">{{ cloudSyncWorking ? '保存中' : cloudDockSyncLabel }}</span>
        </button>
      </footer>
    </section>
    <UnassignedTimeDialog />
    <TimerStopConfirmationDialog />
    <el-dialog v-model="cloudQueueDialogOpen" class="cloud-sync-queue-dialog" title="同步队列" width="min(620px, 94vw)" append-to-body :close-on-click-modal="false">
      <div class="cloud-sync-queue-summary">
        <div><strong>{{ cloudQueueItems.length }}</strong><span>本机待处理</span></div>
        <div><strong>{{ storageState?.pendingOperations ?? 0 }}</strong><span>等待同步</span></div>
        <div :class="{ danger: storageState?.conflictCount }"><strong>{{ storageState?.conflictCount ?? 0 }}</strong><span>冲突</span></div>
      </div>
      <div v-if="cloudQueueLoading" class="cloud-sync-queue-empty"><LoaderCircle class="sync-spin" :size="22" /><span>正在读取本机同步队列</span></div>
      <div v-else-if="cloudQueueItems.length" class="cloud-sync-queue-list">
        <article v-for="item in cloudQueueItems" :key="item.operationId" :class="['cloud-sync-queue-item', item.state]">
          <span class="cloud-sync-queue-state">{{ cloudQueueStateLabel(item.state) }}</span>
          <div>
            <strong>{{ cloudQueueTitle(item) }}</strong>
            <span>{{ cloudConflictEntityLabel(item.entityType) }} · {{ cloudConflictActionLabel(item.operationType) }}<template v-if="item.coalescedCount > 1"> · 合并 {{ item.coalescedCount }} 次修改</template></span>
            <small>{{ formatQueueTime(item.createdAt) }}<template v-if="item.attemptCount"> · 已尝试 {{ item.attemptCount }} 次</template><template v-if="item.dependsOnOperationId"> · 等待前置内容</template></small>
            <p v-if="item.error">{{ item.error }}</p>
          </div>
        </article>
      </div>
      <div v-else class="cloud-sync-queue-empty"><CheckCircle2 :size="24" /><strong>当前没有待同步内容</strong><span>{{ storageState?.mode === 'cloud' ? '本机修改已全部处理。' : '当前使用本地存储模式。' }}</span></div>
      <p v-if="storageState?.lastError" class="cloud-sync-queue-error">{{ storageState.lastError }}</p>
      <template #footer>
        <button class="secondary-button" type="button" @click="cloudQueueDialogOpen = false">关闭</button>
        <button v-if="cloudDockRequiresStorageAttention && !storageState?.conflictCount" class="secondary-button" type="button" @click="openCloudStorageSettings">数据存储设置</button>
        <button v-if="storageState?.conflictCount" class="danger-button" type="button" :disabled="cloudSyncWorking" @click="openConflictsFromQueue">处理冲突</button>
        <button v-if="storageState?.pendingOperations" class="primary-button" type="button" :disabled="cloudSyncWorking || !storageState.online || !cloudSession?.signedIn || storageState.authBlocked || Boolean(storageState.conflictCount)" @click="syncAllCloudQueue"><LoaderCircle v-if="cloudSyncWorking" class="sync-spin" :size="14" />{{ cloudSyncWorking ? '同步中...' : '同步全部' }}</button>
      </template>
    </el-dialog>
    <el-dialog
      v-model="resetLocalCacheDialogOpen"
      class="reset-local-cache-dialog"
      title="重置本地数据"
      width="min(480px, 92vw)"
      append-to-body
      :show-close="!resetLocalCacheWorking"
      :close-on-click-modal="false"
      :close-on-press-escape="!resetLocalCacheWorking"
    >
      <div v-if="resetLocalCacheWorking" class="reset-local-cache-loading" role="status" aria-live="polite">
        <LoaderCircle class="sync-spin" :size="28" />
        <div><strong>正在重置本地数据</strong><span>正在清空本机业务缓存并重新下载云端数据，请不要关闭应用。</span></div>
      </div>
      <div v-else class="reset-local-cache-confirm">
        <span class="reset-local-cache-warning"><AlertTriangle :size="20" /></span>
        <div>
          <strong>确认清空本机缓存并重新下载？</strong>
          <p>事项、计时、未归属时间、报告和共享设置会以云端数据为准重新建立。</p>
          <small>登录状态、Supabase 连接配置和本机路径会保留。{{ resetLocalCacheDiscardWarning }}</small>
        </div>
      </div>
      <template #footer>
        <button class="secondary-button" type="button" :disabled="resetLocalCacheWorking" @click="resetLocalCacheDialogOpen = false">取消</button>
        <button class="danger-button" type="button" :disabled="resetLocalCacheWorking" @click="confirmResetLocalCacheFromCloud"><LoaderCircle v-if="resetLocalCacheWorking" class="sync-spin" :size="14" />{{ resetLocalCacheWorking ? '正在重置...' : '确认重置' }}</button>
      </template>
    </el-dialog>
    <el-dialog v-model="cloudConflictDialogOpen" class="cloud-conflict-dialog" title="处理云端同步冲突" width="min(700px, 94vw)" append-to-body :close-on-click-modal="false">
      <div class="cloud-conflict-list">
        <div v-for="conflict in cloudConflicts" :key="conflict.operationId" class="cloud-conflict-item">
          <div class="cloud-conflict-copy">
            <strong>{{ cloudConflictTitle(conflict) }}</strong>
            <span>{{ cloudConflictEntityLabel(conflict.entityType) }} · {{ cloudConflictActionLabel(conflict.operationType) }} · 本地基础版本 {{ conflict.baseVersion ?? '新建' }}</span>
            <small>{{ conflict.error || '云端内容已在其他设备变化' }}</small>
          </div>
          <div class="cloud-conflict-comparison">
            <div class="cloud-conflict-side local"><strong>本机内容</strong><span>将要上传的修改</span></div>
            <div class="cloud-conflict-side cloud"><strong>云端内容</strong><span>{{ conflict.cloudPayloadError ? '暂时无法读取' : conflict.cloudPayload ? '当前已保存版本' : '云端已不存在' }}</span></div>
            <template v-for="field in conflictFields(conflict)" :key="field">
              <div class="cloud-conflict-field-label">{{ conflictFieldLabels[field] }}</div>
              <div :class="['cloud-conflict-value', { changed: conflictFieldChanged(conflict, field) }]">{{ conflictValue(conflict.localPayload, field) }}</div>
              <div :class="['cloud-conflict-value', { changed: conflictFieldChanged(conflict, field) }]">{{ conflict.cloudPayloadError ? '读取失败' : conflictValue(conflict.cloudPayload, field) }}</div>
            </template>
          </div>
          <p v-if="conflict.cloudPayloadError" class="cloud-conflict-cloud-error">云端内容读取失败：{{ conflict.cloudPayloadError }}</p>
          <details class="cloud-conflict-raw">
            <summary>查看完整内容</summary>
            <div><strong>本机</strong><pre>{{ formattedConflictPayload(conflict, 'local') }}</pre></div>
            <div><strong>云端</strong><pre>{{ conflict.cloudPayloadError || formattedConflictPayload(conflict, 'cloud') }}</pre></div>
          </details>
        </div>
        <p v-if="!cloudConflicts.length" class="cloud-conflict-empty">当前没有需要处理的冲突。</p>
      </div>
      <template #footer>
        <button class="secondary-button" type="button" :disabled="cloudSyncWorking" @click="cloudConflictDialogOpen = false">稍后处理</button>
        <button v-if="cloudConflicts.length" class="secondary-button" type="button" :disabled="cloudSyncWorking" @click="resolveAllConflicts('use_cloud')">全部使用云端</button>
        <button v-if="cloudConflicts.length" class="primary-button" type="button" :disabled="cloudSyncWorking" @click="resolveAllConflicts('keep_local')"><LoaderCircle v-if="cloudSyncWorking" class="sync-spin" :size="14" />{{ cloudSyncWorking ? '处理中...' : '全部保留本地' }}</button>
      </template>
    </el-dialog>
    <el-dialog v-model="seaTableDialogOpen" class="seatable-sync-dialog" title="同步到 SeaTable" width="min(720px, 94vw)" append-to-body destroy-on-close>
      <div v-if="seaTablePreview" class="seatable-sync-preview">
        <div class="seatable-sync-summary">
          <span><strong>{{ seaTablePreview.createCount }}</strong>新增</span>
          <span><strong>{{ seaTablePreview.updateCount }}</strong>更新</span>
          <span><strong>{{ seaTablePreview.skipCount }}</strong>跳过</span>
          <span :class="{ warning: seaTablePreview.conflictCount }"><strong>{{ seaTablePreview.conflictCount }}</strong>可能重复</span>
        </div>
        <div class="seatable-sync-list">
          <div v-for="item in seaTablePreview.items" :key="item.itemId" :class="['seatable-sync-item', `action-${item.action}`]">
            <span>{{ item.action === 'create' ? '新增' : item.action === 'update' ? '更新' : item.action === 'skip' ? '跳过' : '冲突' }}</span>
            <div><strong>{{ item.title }}</strong><small>{{ item.reason }}</small></div>
          </div>
        </div>
        <p v-if="seaTableResult?.failedCount" class="seatable-sync-error">有 {{ seaTableResult.failedCount }} 项写入失败，可以只重试失败事项。</p>
      </div>
      <template #footer><button class="secondary-button" type="button" :disabled="seaTableExecuting" @click="seaTableDialogOpen = false">关闭</button><button v-if="seaTableResult?.failedCount" class="primary-button" type="button" :disabled="seaTableExecuting" @click="retrySeaTableFailures">{{ seaTableExecuting ? '重试中...' : '只重试失败项' }}</button><button v-else class="primary-button" type="button" :disabled="!seaTablePreview || seaTablePreview.conflictCount > 0 || seaTableExecuting" @click="confirmSeaTableSync">{{ seaTableExecuting ? '同步中...' : '确认同步' }}</button></template>
    </el-dialog>
    <el-dialog v-model="importDialogOpen" class="subject-create-dialog plan-import-dialog" title="导入昨日计划" width="min(640px, 92vw)" append-to-body>
      <div v-if="importPreview" class="plan-import-content">
        <div class="plan-import-source"><span>来源</span><strong>{{ importPreview.sourcePath }}</strong></div>
        <div v-if="importPreview.warnings.length" class="plan-import-warnings">
          <p v-for="warning in importPreview.warnings" :key="warning">{{ warning }}</p>
        </div>
        <div class="plan-import-items">
          <label v-for="item in importItems" :key="item.importItemId" class="plan-import-item" :class="{ unrecognized: item.parseStatus === 'unrecognized', child: item.parentId }">
            <input v-model="item.selected" type="checkbox" :disabled="item.parseStatus === 'unrecognized'" />
            <span class="plan-import-item-main">
              <input v-if="item.parseStatus === 'recognized'" v-model="item.title" />
              <code v-else>{{ item.sourceText }}</code>
              <small v-if="item.estimateMinutes">预计 {{ formatMinutes(item.estimateMinutes) }}</small>
              <small v-else-if="item.parseStatus === 'unrecognized'">未识别，保留原文但不导入</small>
            </span>
          </label>
          <p v-if="!importItems.length" class="plan-import-empty">没有可导入的明日计划，可直接在列表末尾创建待办。</p>
        </div>
      </div>
      <template #footer>
        <button class="secondary-button" type="button" @click="importDialogOpen = false">取消</button>
        <button class="primary-button" type="button" :disabled="importSaving || !importItems.some((item) => item.selected && item.title.trim())" @click="confirmPlanImport">{{ importSaving ? '导入中...' : '确认导入' }}</button>
      </template>
    </el-dialog>
    <el-dialog v-model="subjectDialogOpen" class="subject-create-dialog" :title="subjectDialogTitle" width="min(420px, 90vw)" append-to-body>
      <label class="subject-name-field"><span>主体名称</span><input v-model="subjectNameDraft" autofocus placeholder="例如：研发支持、个人事项" @keydown.enter.prevent="saveSubject" /></label>
      <template #footer>
        <button class="secondary-button" type="button" @click="subjectDialogOpen = false">取消</button>
        <button class="primary-button" type="button" @click="saveSubject">{{ editingSubjectId ? '保存名称' : '添加主体' }}</button>
      </template>
    </el-dialog>
  </main>
</template>
