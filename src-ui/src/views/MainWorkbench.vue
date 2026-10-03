<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from "vue";
import { CalendarDays, Clock3, Copy, FileText, Folder, Home, Minus, Pencil, Plus, Settings, Square, X } from "lucide-vue-next";
import { ElMessage, ElMessageBox } from "element-plus";
import { useWorkdayStore, type Task } from "../store";
import { checkAppUpdate, closeMainWindow, confirmObsidianPlan, configureCloud, downloadAndInstallUpdate, executeSeaTableTaskSync, executeStorageMigration, getCloudSession, getCloudSyncStatus, getSettings, getStorageMode, isMainWindowMaximized, listCloudSyncConflicts, minimizeMainWindow, onAppUpdateEvent, onCloudSyncStateChanged, onCurrentWindowResized, onNavigatePage, onTrayCheckUpdate, previewObsidianPlan, previewSeaTableTaskSync, previewStorageMigration, pushCloudSync, registerCloudDevice, resolveCloudSyncConflict, restartApp, retryFailedSeaTableSync, setIntegrationSecret, setLocalStorageMode, signInCloud, signOutCloud, testSeaTableConnection, toggleMainWindowMaximize, updateIntegrationConfig, updateSetting, type AppUpdateCheckResult, type AppUpdateEventPayload, type CloudSessionSnapshot, type CloudSyncConflict, type PlanImportPreviewResult, type SeaTableSyncPreview, type SeaTableSyncResult, type SettingsSnapshot, type StorageMigrationPreview, type StorageModeSnapshot } from "../services/tauri";
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
const storageState = ref<StorageModeSnapshot>();
const migrationPreview = ref<StorageMigrationPreview>();
const cloudSyncWorking = ref(false);
const cloudConflictDialogOpen = ref(false);
const cloudConflicts = ref<CloudSyncConflict[]>([]);
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
let unlistenWindowResize: (() => void) | undefined;
let unlistenNavigatePage: (() => void) | undefined;
let unlistenCloudSyncState: (() => void) | undefined;
let unlistenTrayCheckUpdate: (() => void) | undefined;
let unlistenAppUpdateEvent: (() => void) | undefined;
const completingTaskIds = new Set<string>();
const recentCompletedTaskId = ref("");
let recentCompletedTimer: number | undefined;

const cloudSyncLabel = computed(() => {
  if (storageState.value?.mode !== "cloud") return "本地模式";
  if (!storageState.value.online) return "云端离线";
  if (storageState.value.lastError || storageState.value.syncState === "error") return "同步失败";
  if (storageState.value.conflictCount > 0) return `${storageState.value.conflictCount} 项冲突`;
  if (storageState.value.pendingOperations > 0) return `${storageState.value.pendingOperations} 项等待同步`;
  return "已同步";
});

const cloudSyncDetail = computed(() => {
  const state = storageState.value;
  if (!state || state.mode !== "cloud") return "";
  if (state.lastError) return `同步失败：${state.lastError}`;
  if (state.conflictCount > 0) return "本机修改已保存，需处理冲突后继续同步";
  if (state.pendingOperations > 0) return "本机修改已保存，等待同步到云端";
  if (state.lastSyncedAt) return `最近同步 ${formatSyncTime(state.lastSyncedAt)}`;
  return "本机缓存已准备好，尚无云端同步记录";
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
  const title = payload.title ?? payload.name ?? payload.report_type ?? payload.key ?? payload.provider;
  return typeof title === "string" && title ? title : `${conflict.entityType} ${conflict.entityId ?? ""}`.trim();
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
    settingsSnapshot.value = await getSettings();
    ElMessage.success("Supabase 连接配置已保存");
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
    await ElMessageBox.confirm("迁移完成后 Supabase 将成为权威数据源。本地 SQLite 只保留缓存和离线队列。", "确认迁移到云端", {
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
    await ElMessageBox.confirm("当前设备的 SQLite 将重建为云端缓存。未上传的本地数据不会合并，请确认本地内容可以被替换。", "使用现有云端数据", {
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

async function refreshCloudSyncStatus() {
  if (storageState.value?.mode !== "cloud" || cloudSyncWorking.value) return;
  cloudSyncWorking.value = true;
  try {
    storageState.value = await getCloudSyncStatus();
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudSyncWorking.value = false;
  }
}

async function retryCloudSync() {
  if (storageState.value?.mode !== "cloud" || cloudSyncWorking.value) return;
  cloudSyncWorking.value = true;
  try {
    const result = await pushCloudSync();
    storageState.value = await getCloudSyncStatus();
    if (result.conflicts > 0) {
      await openCloudConflicts();
      ElMessage.warning("同步遇到版本冲突，请选择要保留的版本");
    } else {
      await Promise.all([store.loadWorkspaceData(), store.loadTimeData(), store.loadReports(), store.loadUnassignedState()]);
      ElMessage.success(result.pushed > 0 ? `已同步 ${result.pushed} 项修改` : "当前没有待同步修改");
    }
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally {
    cloudSyncWorking.value = false;
  }
}

async function openCloudConflicts() {
  cloudConflicts.value = await listCloudSyncConflicts();
  cloudConflictDialogOpen.value = true;
}

async function resolveConflict(conflict: CloudSyncConflict, strategy: "use_cloud" | "keep_local") {
  if (cloudSyncWorking.value) return;
  const keepLocal = strategy === "keep_local";
  try {
    await ElMessageBox.confirm(
      keepLocal
        ? "本地内容将以云端最新版本为基础重新提交，并覆盖该实体当前的云端内容。"
        : "本地这次修改将被放弃，并从云端重新加载当前内容。",
      keepLocal ? "保留本地版本" : "使用云端版本",
      { type: "warning", confirmButtonText: keepLocal ? "保留本地" : "使用云端", cancelButtonText: "取消", closeOnClickModal: false, customClass: "work-confirm-dialog" },
    );
  } catch { return; }
  cloudSyncWorking.value = true;
  try {
    storageState.value = await resolveCloudSyncConflict(conflict.operationId, strategy);
    cloudConflicts.value = await listCloudSyncConflicts();
    await Promise.all([store.loadWorkspaceData(), store.loadTimeData(), store.loadReports(), store.loadUnassignedState()]);
    if (!cloudConflicts.value.length) cloudConflictDialogOpen.value = false;
    ElMessage.success(keepLocal ? "已保留并重新提交本地版本" : "已使用云端版本");
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
    storageState.value = state;
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
      workDate: formatLocalDate(new Date()),
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
  try {
    const result = await store.commitTaskToggle(task);
    if (!result) return;
    if (result.direction === "completed" && isCompletionFeedbackEnabled()) {
      row?.classList.add("task-completion-confirmed");
      await new Promise((resolve) => window.setTimeout(resolve, 330));
    }
    await store.loadWorkspaceData();
    await store.loadTodayOverview();
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
    ElMessage.error(typeof error === "string" ? error : error instanceof Error ? error.message : "事项状态更新失败");
  } finally {
    row?.classList.remove("task-completion-confirmed");
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
            <i v-if="page.id === 'settings' && storageState?.mode === 'cloud' && (storageState.pendingOperations || storageState.conflictCount || storageState.lastError)" :class="['nav-sync-indicator', { conflict: storageState.conflictCount || storageState.lastError }]" :title="cloudSyncLabel"></i>
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

      <div class="content-scroll">
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
          :storage-mode="storageState?.mode ?? 'local'"
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

      <section v-else class="settings-grid">
        <div class="work-panel settings-panel settings-appearance-panel">
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

        <div class="work-panel settings-panel">
          <div class="panel-title"><h2>本机配置</h2><span>路径只保存在当前设备</span></div>
          <label>Obsidian 根目录<input v-model="settingsDraft.obsidianRootPath" :disabled="!settingsLoaded" placeholder="D:/Obsidian" /></label>
          <label>日报路径规则<input v-model="settingsDraft.obsidianDailyPathPattern" :disabled="!settingsLoaded" placeholder="工作日报/{date}.md" /></label>
          <label>默认主体<input v-model="settingsDraft.subjectName" disabled /></label>
          <label>工资时薪<input v-model.number="settingsDraft.salaryHourlyRate" :disabled="!settingsLoaded" min="0" type="number" /></label>
          <div class="settings-choice-field">
            <span>报告时长格式</span>
            <div class="settings-segmented" role="radiogroup" aria-label="报告时长格式">
              <button type="button" role="radio" :aria-checked="settingsDraft.reportDurationFormat === 'minutes'" :class="{ active: settingsDraft.reportDurationFormat === 'minutes' }" :disabled="!settingsLoaded" @click="settingsDraft.reportDurationFormat = 'minutes'"><strong>分钟</strong><small>12min</small></button>
              <button type="button" role="radio" :aria-checked="settingsDraft.reportDurationFormat === 'hours'" :class="{ active: settingsDraft.reportDurationFormat === 'hours' }" :disabled="!settingsLoaded" @click="settingsDraft.reportDurationFormat = 'hours'"><strong>小时</strong><small>0.2h</small></button>
            </div>
            <small class="settings-hint">用于新生成或重新生成的报告，默认使用分钟。</small>
          </div>
        </div>

        <div class="work-panel settings-panel">
          <div class="panel-title"><h2>SeaTable</h2><span>{{ settingsSnapshot?.secrets.seatableTokenSet ? 'Token 已安全保存' : '尚未配置 Token' }}</span></div>
          <label>服务地址<input v-model="settingsDraft.seatableServerUrl" :disabled="!settingsLoaded" placeholder="https://cloud.seatable.cn" /></label>
          <label>事项表<input v-model="settingsDraft.seatableTaskTable" :disabled="!settingsLoaded" placeholder="事项计划" /></label>
          <label>事项视图<input v-model="settingsDraft.seatableTaskView" :disabled="!settingsLoaded" placeholder="默认视图" /></label>
          <label>本地事项 ID 字段<input v-model="settingsDraft.seatableLocalIdField" :disabled="!settingsLoaded" placeholder="本地事项ID" /></label>
          <label>报销表<input v-model="settingsDraft.seatableReimbursementTable" :disabled="!settingsLoaded" placeholder="报销" /></label>
          <label>Base API Token<input v-model="settingsDraft.seatableToken" :disabled="!settingsLoaded" type="password" :placeholder="settingsSnapshot?.secrets.seatableTokenSet ? '留空表示不修改' : '输入后保存到系统凭据库'" autocomplete="off" /></label>
          <button class="secondary-button" type="button" :disabled="seaTableLoading || !settingsSnapshot?.secrets.seatableTokenSet" @click="checkSeaTableConnection">{{ seaTableLoading ? '检测中...' : '检测连接与表结构' }}</button>
        </div>

        <div class="work-panel settings-panel">
          <div class="panel-title"><h2>数据存储</h2><span>{{ settingsDraft.storageMode === 'local' ? '本地 SQLite' : 'Supabase 云端' }}</span></div>
          <div class="storage-mode-row"><span :class="['storage-state-dot', storageState?.online ? 'online' : '', { warning: storageState?.conflictCount || storageState?.pendingOperations || storageState?.lastError }]"></span><strong>{{ storageState?.mode === 'cloud' ? '云端数据' : '本地数据' }}</strong><small>{{ storageState?.mode === 'cloud' ? `${cloudSyncLabel} · 变更序号 ${storageState.lastChangeSeq}` : '无需网络即可使用' }}</small></div>
          <p v-if="cloudSyncDetail" class="cloud-sync-detail">{{ cloudSyncDetail }}</p>
          <div v-if="storageState?.mode === 'cloud'" class="cloud-sync-actions">
            <button class="secondary-button compact" type="button" :disabled="cloudSyncWorking" @click="refreshCloudSyncStatus">刷新状态</button>
            <button v-if="storageState.pendingOperations" class="secondary-button compact" type="button" :disabled="cloudSyncWorking || !storageState.online" @click="retryCloudSync">重试同步</button>
            <button v-if="storageState.conflictCount" class="danger-button compact" type="button" :disabled="cloudSyncWorking" @click="openCloudConflicts">处理冲突</button>
          </div>
          <label>Supabase Project URL<input v-model="settingsDraft.supabaseProjectUrl" :disabled="!settingsLoaded || cloudWorking" placeholder="https://example.supabase.co" /></label>
          <label>Supabase anon key<input v-model="settingsDraft.supabaseAnonKey" :disabled="!settingsLoaded || cloudWorking" type="password" placeholder="公开 anon key，不要填写 service_role" autocomplete="off" /></label>
          <div class="settings-inline-actions"><button class="secondary-button" type="button" :disabled="cloudWorking" @click="configureSupabase">保存连接</button></div>
          <template v-if="!cloudSession?.signedIn">
            <label>登录邮箱<input v-model="settingsDraft.supabaseEmail" :disabled="cloudWorking" type="email" autocomplete="username" placeholder="user@example.com" /></label>
            <label>登录密码<input v-model="settingsDraft.supabasePassword" :disabled="cloudWorking" type="password" autocomplete="current-password" /></label>
            <button class="primary-button" type="button" :disabled="cloudWorking || !settingsDraft.supabaseEmail || !settingsDraft.supabasePassword" @click="loginSupabase">登录 Supabase</button>
          </template>
          <template v-else>
            <div class="cloud-account-row"><div><span>当前账号</span><strong>{{ cloudSession.email || cloudSession.userId }}</strong></div><button class="secondary-button compact" type="button" :disabled="cloudWorking" @click="logoutSupabase">退出</button></div>
            <button v-if="!migrationPreview" class="secondary-button" type="button" :disabled="cloudWorking" @click="previewCloudMigration">预览本地数据迁移</button>
            <div v-else class="migration-preview-block">
              <div class="migration-count-grid"><span v-for="item in migrationPreview.entities" :key="item.entity"><strong>{{ item.localCount }}</strong><small>{{ item.entity }}</small></span></div>
              <p v-if="migrationPreview.conflicts.length">{{ migrationPreview.conflicts.join('；') }}</p>
              <div class="settings-inline-actions">
                <button class="primary-button" type="button" :disabled="cloudWorking || !migrationPreview.canExecute || storageState?.mode === 'cloud'" @click="migrateToCloud">{{ storageState?.mode === 'cloud' ? '当前已使用云端' : '迁移并切换到云端' }}</button>
                <button v-if="migrationPreview.entities.some((item) => (item.cloudCount ?? 0) > 0)" class="secondary-button" type="button" :disabled="cloudWorking || storageState?.mode === 'cloud'" @click="useExistingCloud">使用现有云端数据</button>
              </div>
            </div>
          </template>
        </div>

        <div class="work-panel settings-panel">
          <div class="panel-title"><h2>交互反馈</h2><span>当前设备独立设置</span></div>
          <label class="switch-line"><input v-model="settingsDraft.completionFeedbackEnabled" :disabled="!settingsLoaded" type="checkbox" @change="setCompletionFeedbackEnabled(settingsDraft.completionFeedbackEnabled)" />启用完成事项时的激励动画</label>
          <p class="settings-help">开启后显示彩带、完成提示和列表反馈；此选项不受 Windows 动画设置影响。</p>
          <label class="switch-line"><input v-model="settingsDraft.completionFeedbackSoundEnabled" :disabled="!settingsLoaded || !settingsDraft.completionFeedbackEnabled" type="checkbox" @change="setCompletionFeedbackSoundEnabled(settingsDraft.completionFeedbackSoundEnabled)" />播放完成事项时的激励音效</label>
          <p class="settings-help">使用本地成功提示音与视觉特效同步播放；关闭后仅保留视觉反馈。</p>
        </div>

        <div class="work-panel settings-panel settings-hooks-panel">
          <AutomationHooksSettings />
        </div>

        <div class="work-panel settings-panel">
          <div class="panel-title"><h2>应用更新</h2><span>v{{ appVersion }}</span></div>
          <div class="update-state-row">
            <div>
              <strong>{{ updateStatusText }}</strong>
              <small>{{ updateProgressText || '托盘右键菜单也可以检查更新' }}</small>
            </div>
            <button class="secondary-button compact" type="button" :disabled="updateChecking || updateInstalling" @click="handleCheckUpdate">{{ updateChecking ? '检查中...' : updateInstalling ? '安装中...' : '检查更新' }}</button>
          </div>
        </div>

        <div class="work-panel settings-panel">
          <div class="panel-title"><h2>托盘行为</h2><span>当前设备独立设置</span></div>
          <label class="switch-line"><input v-model="settingsDraft.trayHoverEnabled" :disabled="!settingsLoaded" type="checkbox" />鼠标停留后显示悬浮面板</label>
          <label class="switch-line"><input v-model="settingsDraft.trayMenuSuppressHover" :disabled="!settingsLoaded" type="checkbox" />右键菜单打开时抑制悬浮面板</label>
          <label class="switch-line"><input v-model="settingsDraft.startMinimized" :disabled="!settingsLoaded" type="checkbox" />开机后最小化启动</label>
          <button class="primary-button settings-save-button" type="button" :disabled="!settingsLoaded || settingsSaving" @click="saveSettings">{{ settingsSaving ? '保存中...' : '保存设置' }}</button>
        </div>
        </section>
      </div>
    </section>
    <UnassignedTimeDialog />
    <TimerStopConfirmationDialog />
    <el-dialog v-model="cloudConflictDialogOpen" class="cloud-conflict-dialog" title="处理云端同步冲突" width="min(700px, 94vw)" append-to-body :close-on-click-modal="false">
      <div class="cloud-conflict-list">
        <div v-for="conflict in cloudConflicts" :key="conflict.operationId" class="cloud-conflict-item">
          <div class="cloud-conflict-copy">
            <strong>{{ cloudConflictTitle(conflict) }}</strong>
            <span>{{ conflict.operationType }} · 本地基础版本 {{ conflict.baseVersion ?? '新建' }}</span>
            <small>{{ conflict.error || '云端内容已在其他设备变化' }}</small>
          </div>
          <div class="cloud-conflict-actions">
            <button class="secondary-button compact" type="button" :disabled="cloudSyncWorking" @click="resolveConflict(conflict, 'use_cloud')">使用云端</button>
            <button class="primary-button compact" type="button" :disabled="cloudSyncWorking" @click="resolveConflict(conflict, 'keep_local')">保留本地</button>
          </div>
        </div>
        <p v-if="!cloudConflicts.length" class="cloud-conflict-empty">当前没有需要处理的冲突。</p>
      </div>
      <template #footer><button class="secondary-button" type="button" :disabled="cloudSyncWorking" @click="cloudConflictDialogOpen = false">稍后处理</button></template>
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
