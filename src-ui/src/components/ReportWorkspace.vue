<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { storeToRefs } from "pinia";
import { ElMessage, ElMessageBox } from "element-plus";
import { Braces, CalendarDays, CalendarRange, Check, ClipboardCopy, FilePenLine, FileText, Folder, HardDriveDownload, MoreHorizontal, Pencil, Plus, ReceiptText, RefreshCcw, Search, Sparkles, Trash2 } from "lucide-vue-next";
import { MdEditor, MdPreview } from "md-editor-v3";
import "md-editor-v3/lib/style.css";
import { useWorkdayStore, type ReportRecord, type ReportType, type Task } from "../store";
import { getReportTaskSuggestions, getSettings, querySeaTableReimbursements, type ObsidianReportWritePreview, type ReportTaskSuggestion, type ReportTemplateRecord } from "../services/tauri";

const store = useWorkdayStore();
const { reports } = storeToRefs(store);
const reportFilter = ref<"all" | ReportType>("all");
const reportQuery = ref("");
const selectedReportId = ref("");
const editorDraft = ref("");
const editorMode = ref<"edit" | "preview">("edit");
const reportDialogOpen = ref(false);
const dialogMode = ref<"create" | "scope">("create");
const draftType = ref<ReportType>("daily");
const draftSubjectId = ref(store.subjects[0]?.id ?? "");
const draftReferenceDate = ref(new Date(2026, 8, 15));
const draftTaskIds = ref<string[]>([]);
const draftTaskQuery = ref("");
const draftTaskSuggestions = ref<ReportTaskSuggestion[]>([]);
const draftTasksLoading = ref(false);
const reportDurationFormat = ref<"minutes" | "hours">("minutes");
let draftSuggestionRequest = 0;
const reportSaving = ref(false);
const templateDialogOpen = ref(false);
const templateLoading = ref(false);
const templateSaving = ref(false);
const templateDraft = ref("");
const templateRecord = ref<ReportTemplateRecord>();
const templateType = ref<ReportType>("daily");
const templateSubjectId = ref("");
const obsidianDialogOpen = ref(false);
const obsidianLoading = ref(false);
const obsidianWriting = ref(false);
const obsidianStrategy = ref<"overwrite" | "append">("overwrite");
const obsidianPreview = ref<ObsidianReportWritePreview>();

const templateVariables = [
  { token: "{{姓名}}", label: "姓名" },
  { token: "{{日期}}", label: "日期" },
  { token: "{{星期}}", label: "星期" },
  { token: "{{日期范围}}", label: "日期范围" },
  { token: "{{工作时段}}", label: "工作时段" },
  { token: "{{总工时}}", label: "总工时" },
  { token: "{{今日事项}}", label: "今日事项" },
  { token: "{{明日计划}}", label: "明日计划" },
  { token: "{{统计信息}}", label: "统计信息" },
  { token: "{{每日情况}}", label: "每日情况" },
  { token: "{{总结}}", label: "总结" },
] as const;

const toolbars = [
  "revoke", "next", "-", "bold", "italic", "strikeThrough", "title", "quote",
  "unorderedList", "orderedList", "task", "codeRow", "code", "link", "table", "=", "preview"
] as const;

const selectedReport = computed(() => reports.value.find((report) => report.id === selectedReportId.value));
const filteredReports = computed(() => {
  const query = reportQuery.value.trim().toLocaleLowerCase();
  return reports.value.filter((report) => {
    if (reportFilter.value !== "all" && report.type !== reportFilter.value) return false;
    if (!query) return true;
    return `${report.period} ${report.subjectName} ${reportTypeLabel(report.type)}`.toLocaleLowerCase().includes(query);
  });
});
const draftTasks = computed(() => store.tasks.filter((task) => task.subjectId === draftSubjectId.value));
const draftTaskMinutes = computed(() => new Map(draftTaskSuggestions.value.map((item) => [item.taskId, item.actualMinutes])));
const draftTaskDailyEstimates = computed(() => new Map(draftTaskSuggestions.value.map((item) => [item.taskId, item.dailyEstimateMinutes])));
const visibleDraftTasks = computed(() => {
  const query = draftTaskQuery.value.trim().toLocaleLowerCase();
  if (!query) return draftTasks.value;
  const matchingIds = new Set(draftTasks.value.filter((task) => task.title.toLocaleLowerCase().includes(query)).map((task) => task.id));
  for (const task of draftTasks.value) {
    if (!matchingIds.has(task.id)) continue;
    let parentId = task.parentId;
    while (parentId) {
      matchingIds.add(parentId);
      parentId = store.tasks.find((item) => item.id === parentId)?.parentId;
    }
  }
  return draftTasks.value.filter((task) => matchingIds.has(task.id));
});
const hasUnsavedChanges = computed(() => Boolean(selectedReport.value && selectedReport.value.markdown !== editorDraft.value));
const dialogTitle = computed(() => dialogMode.value === "create" ? "创建报告" : "调整报告范围");
const existingDraftReport = computed(() => {
  if (dialogMode.value !== "create") return undefined;
  const period = periodValue(draftType.value, draftReferenceDate.value);
  return reports.value.find((report) => report.type === draftType.value
    && report.subjectId === draftSubjectId.value
    && report.period === period);
});
const reportCounts = computed(() => ({
  all: reports.value.length,
  daily: reports.value.filter((report) => report.type === "daily").length,
  weekly: reports.value.filter((report) => report.type === "weekly").length,
  monthly: reports.value.filter((report) => report.type === "monthly").length
}));
const draftSummary = computed(() => {
  const selected = new Set(draftTaskIds.value);
  const tasks = draftTasks.value.filter((task) => selected.has(task.id));
  return {
    total: tasks.length,
    completed: tasks.filter((task) => task.status === "done").length,
    minutes: tasks.reduce((sum, task) => sum + draftMinutes(task.id), 0)
  };
});
const templatePreview = computed(() => {
  const report = selectedReport.value;
  const period = report?.period ?? periodValue(templateType.value, new Date());
  const replacements: Record<string, string> = {
    "{{姓名}}": "晓健",
    "{{日期}}": report?.referenceDate ?? toDateValue(new Date()),
    "{{星期}}": "周四",
    "{{日期范围}}": period,
    "{{工作时段}}": "10:00-12:00 13:30-17:30",
    "{{总工时}}": formatHours(360),
    "{{今日事项}}": `1. 🟢 已完成事项 <预计：${formatHours(60)} 实际：${formatHours(90)}>\n2. 🔴 进行中的事项 <预计：${formatHours(60)} 实际：${formatHours(45)}>`,
    "{{明日计划}}": "1. 明日重点事项\n   - 继续处理子事项",
    "{{统计信息}}": "- 累计用时：6h\n- 已完成事项：4 项",
    "{{每日情况}}": "- 2026-09-21：6h\n- 2026-09-22：缺少日报或结算数据",
    "{{总结}}": "- 在这里补充本期总结。",
  };
  return Object.entries(replacements).reduce((content, [token, value]) => content.split(token).join(value), templateDraft.value);
});

watch(selectedReport, (report) => {
  editorDraft.value = report?.markdown ?? "";
}, { immediate: true });

watch([draftSubjectId, draftType, draftReferenceDate], () => {
  if (!reportDialogOpen.value || dialogMode.value !== "create") return;
  void loadSuggestedTasks(true);
});

function reportTypeLabel(type: ReportType) {
  if (type === "daily") return "日报";
  if (type === "weekly") return "周报";
  return "月报";
}

function reportTypeClass(type: ReportType) {
  return `type-${type}`;
}

function formatUpdatedAt(timestamp: number) {
  return new Intl.DateTimeFormat("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false }).format(timestamp);
}

function toDateValue(value: Date) {
  const year = value.getFullYear();
  const month = String(value.getMonth() + 1).padStart(2, "0");
  const day = String(value.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function formatDateRange(start: Date, end: Date) {
  return `${toDateValue(start)} 至 ${toDateValue(end)}`;
}

function periodValue(type: ReportType, value: Date) {
  if (type === "daily") return toDateValue(value);
  if (type === "monthly") {
    const start = new Date(value.getFullYear(), value.getMonth(), 1);
    const end = new Date(value.getFullYear(), value.getMonth() + 1, 0);
    return formatDateRange(start, end);
  }
  const day = value.getDay() || 7;
  const start = new Date(value.getFullYear(), value.getMonth(), value.getDate() - day + 1);
  const end = new Date(start.getFullYear(), start.getMonth(), start.getDate() + 6);
  return formatDateRange(start, end);
}

function reportPeriodRange(report: ReportRecord) {
  const value = new Date(`${report.referenceDate}T00:00:00`);
  if (report.type === "daily") {
    const date = toDateValue(value);
    return { periodStart: date, periodEnd: date };
  }
  if (report.type === "monthly") {
    return {
      periodStart: toDateValue(new Date(value.getFullYear(), value.getMonth(), 1)),
      periodEnd: toDateValue(new Date(value.getFullYear(), value.getMonth() + 1, 0)),
    };
  }
  const day = value.getDay() || 7;
  const start = new Date(value.getFullYear(), value.getMonth(), value.getDate() - day + 1);
  return { periodStart: toDateValue(start), periodEnd: toDateValue(new Date(start.getFullYear(), start.getMonth(), start.getDate() + 6)) };
}

function taskMinutes(taskId: string) {
  return store.entries.reduce((total, entry) => {
    if (entry.allocations?.length) {
      return total + entry.allocations
        .filter((allocation) => allocation.taskId === taskId)
        .reduce((sum, allocation) => sum + allocation.minutes, 0);
    }
    if (entry.defaultTask !== taskId) return total;
    return total + Math.ceil(store.entryDurationSeconds(entry) / 60);
  }, 0);
}

function draftMinutes(taskId: string) {
  return draftTaskMinutes.value.get(taskId) ?? 0;
}

function draftDailyEstimate(taskId: string) {
  return draftTaskDailyEstimates.value.get(taskId);
}

function formatHours(minutes: number) {
  return reportDurationFormat.value === "minutes" ? `${minutes}min` : `${(minutes / 60).toFixed(1)}h`;
}

function taskDepth(task: Task) {
  let depth = 0;
  let parentId = task.parentId;
  while (parentId && depth < 8) {
    const parent = store.tasks.find((item) => item.id === parentId);
    if (!parent) break;
    depth += 1;
    parentId = parent.parentId;
  }
  return depth;
}

function reportTaskLines(taskIds: string[]) {
  const selectedIds = new Set(taskIds);
  const selectedTasks = store.visibleTasks.filter((task) => selectedIds.has(task.id));
  if (!selectedTasks.length) return ["- 暂无导入事项"];
  return selectedTasks.map((task) => {
    let depth = 0;
    let parentId = task.parentId;
    while (parentId && depth < 8) {
      if (selectedIds.has(parentId)) depth += 1;
      parentId = store.tasks.find((item) => item.id === parentId)?.parentId;
    }
    const minutes = taskMinutes(task.id);
    const status = task.status === "done" ? "[x]" : "[ ]";
    const metric = minutes > 0 ? ` \\<实际：${formatHours(minutes)}\\>` : "";
    return `${"  ".repeat(depth)}- ${status} ${task.title}${metric}`;
  });
}

function generateMarkdown(type: ReportType, subjectName: string, period: string, taskIds: string[]) {
  const totalMinutes = taskIds.reduce((sum, taskId) => sum + taskMinutes(taskId), 0);
  const completed = taskIds.filter((taskId) => store.tasks.find((task) => task.id === taskId)?.status === "done").length;
  const sectionTitle = type === "daily" ? "今日事项" : type === "weekly" ? "本周事项" : "本月事项";
  return [
    `# ${period}`,
    "",
    `> 主体：${subjectName} · 已完成 ${completed}/${taskIds.length} 项 · 累计用时 ${formatHours(totalMinutes)}`,
    "",
    `## ${sectionTitle}`,
    "",
    ...reportTaskLines(taskIds),
    "",
    "## 总结",
    "",
    "- 在这里补充本期总结。",
    ""
  ].join("\n");
}

function selectSuggestedTasks() {
  const suggested = new Set(draftTaskSuggestions.value.map((item) => item.taskId));
  draftTaskIds.value = draftTasks.value.filter((task) => suggested.has(task.id)).map((task) => task.id);
}

async function loadSuggestedTasks(applySelection = false) {
  const requestId = ++draftSuggestionRequest;
  if (!draftSubjectId.value) {
    draftTaskSuggestions.value = [];
    if (applySelection) draftTaskIds.value = [];
    return;
  }
  draftTasksLoading.value = true;
  try {
    const suggestions = await getReportTaskSuggestions({
      reportType: draftType.value,
      referenceDate: toDateValue(draftReferenceDate.value),
      subjectId: draftSubjectId.value,
    });
    if (requestId !== draftSuggestionRequest) return;
    draftTaskSuggestions.value = suggestions;
    if (applySelection) selectSuggestedTasks();
  } catch (error) {
    if (requestId !== draftSuggestionRequest) return;
    draftTaskSuggestions.value = [];
    if (applySelection) draftTaskIds.value = [];
    ElMessage.error(errorText(error, "加载报告事项失败"));
  } finally {
    if (requestId === draftSuggestionRequest) draftTasksLoading.value = false;
  }
}

async function openCreateDialog() {
  dialogMode.value = "create";
  draftType.value = "daily";
  draftSubjectId.value = store.selectedSubjectId || store.subjects[0]?.id || "";
  draftReferenceDate.value = new Date();
  draftTaskQuery.value = "";
  reportDialogOpen.value = true;
  await loadSuggestedTasks(true);
}

async function openScopeDialog(report: ReportRecord) {
  dialogMode.value = "scope";
  selectedReportId.value = report.id;
  draftType.value = report.type;
  draftSubjectId.value = report.subjectId;
  draftReferenceDate.value = new Date(`${report.referenceDate}T00:00:00`);
  draftTaskIds.value = [...report.taskIds];
  draftTaskQuery.value = "";
  reportDialogOpen.value = true;
  await loadSuggestedTasks(false);
}

function toggleAllDraftTasks() {
  draftTaskIds.value = draftTaskIds.value.length === draftTasks.value.length ? [] : draftTasks.value.map((task) => task.id);
}

function clearDraftTasks() {
  draftTaskIds.value = [];
}

async function saveReportDialog() {
  const subject = store.subjects.find((item) => item.id === draftSubjectId.value);
  if (!subject) {
    ElMessage.warning("请选择主体");
    return;
  }
  if (!draftTaskIds.value.length) {
    ElMessage.warning("请至少导入一个事项");
    return;
  }
  const referenceDate = toDateValue(draftReferenceDate.value);
  reportSaving.value = true;
  try {
    if (dialogMode.value === "create") {
      if (existingDraftReport.value) {
        try {
          await ElMessageBox.confirm(
            "当前主体和周期已有报告。继续后会按当前选择重新生成，并覆盖原 Markdown 内容。",
            `覆盖原${reportTypeLabel(draftType.value)}`,
            {
              type: "warning",
              confirmButtonText: "覆盖并生成",
              cancelButtonText: "取消",
              customClass: "work-confirm-dialog",
              confirmButtonClass: "work-confirm-danger",
              closeOnClickModal: false,
            },
          );
        } catch {
          return;
        }
        const scoped = await store.updateReportScope(existingDraftReport.value, {
          type: draftType.value,
          subjectId: subject.id,
          referenceDate,
          taskIds: [...draftTaskIds.value],
        });
        const updated = await store.regenerateReport(scoped);
        selectedReportId.value = updated.id;
        editorDraft.value = updated.markdown;
        reportFilter.value = "all";
        reportDialogOpen.value = false;
        editorMode.value = "edit";
        ElMessage.success(`${reportTypeLabel(updated.type)}已覆盖并重新生成`);
        return;
      }
      const report = await store.createReport({
        type: draftType.value,
        subjectId: subject.id,
        referenceDate,
        taskIds: [...draftTaskIds.value],
      });
      selectedReportId.value = report.id;
      reportFilter.value = "all";
      reportDialogOpen.value = false;
      editorMode.value = "edit";
      ElMessage.success(`${reportTypeLabel(report.type)}已创建并导入事项`);
      return;
    }
    const report = selectedReport.value;
    if (!report) return;
    const updated = await store.updateReportScope(report, {
      type: draftType.value,
      subjectId: subject.id,
      referenceDate,
      taskIds: [...draftTaskIds.value],
    });
    selectedReportId.value = updated.id;
    reportDialogOpen.value = false;
    ElMessage.success("报告范围已更新，可重新生成内容");
  } catch (error) {
    const existingId = existingReportId(error);
    if (existingId) {
      await store.loadReports();
      const existing = reports.value.find((report) => report.id === existingId);
      if (!existing) {
        ElMessage.error("已有报告加载失败，请刷新后重试");
        return;
      }
      const scoped = await store.updateReportScope(existing, {
        type: draftType.value,
        subjectId: subject.id,
        referenceDate,
        taskIds: [...draftTaskIds.value],
      });
      const updated = await store.regenerateReport(scoped);
      selectedReportId.value = updated.id;
      editorDraft.value = updated.markdown;
      reportFilter.value = "all";
      reportDialogOpen.value = false;
      editorMode.value = "edit";
      ElMessage.success(`${reportTypeLabel(updated.type)}已覆盖并重新生成`);
    } else {
      ElMessage.error(errorText(error, "报告保存失败"));
    }
  } finally {
    reportSaving.value = false;
  }
}

async function selectReport(report: ReportRecord) {
  if (hasUnsavedChanges.value && selectedReport.value?.id !== report.id) {
    try {
      await ElMessageBox.confirm("当前报告有未保存修改，保存后再切换到其他报告。", "保存当前修改", {
        type: "warning",
        confirmButtonText: "保存并切换",
        cancelButtonText: "留在当前报告",
        customClass: "work-confirm-dialog",
        closeOnClickModal: false
      });
    } catch {
      return;
    }
    await saveMarkdown(false);
  }
  selectedReportId.value = report.id;
}

async function saveMarkdown(showMessage = true) {
  const report = selectedReport.value;
  if (!report) return;
  reportSaving.value = true;
  try {
    const updated = await store.saveReportContent(report, editorDraft.value);
    if (showMessage) ElMessage.success("报告内容已保存");
    return updated;
  } catch (error) {
    ElMessage.error(errorText(error, "报告内容保存失败"));
  } finally {
    reportSaving.value = false;
  }
}

async function openTemplateSettings(report: ReportRecord) {
  templateDialogOpen.value = true;
  templateLoading.value = true;
  templateType.value = report.type;
  templateSubjectId.value = report.subjectId;
  try {
    templateRecord.value = await store.getReportTemplate(report.type, report.subjectId);
    templateDraft.value = templateRecord.value.content;
  } catch (error) {
    templateDialogOpen.value = false;
    ElMessage.error(errorText(error, "模板加载失败"));
  } finally {
    templateLoading.value = false;
  }
}

function insertTemplateVariable(token: string) {
  templateDraft.value += token;
}

async function saveTemplate() {
  if (!templateDraft.value.trim()) {
    ElMessage.warning("报告模板不能为空");
    return;
  }
  templateSaving.value = true;
  try {
    templateRecord.value = await store.saveReportTemplate({
      type: templateType.value,
      subjectId: templateSubjectId.value,
      content: templateDraft.value,
      expectedVersion: templateRecord.value?.subjectId ? templateRecord.value.version : undefined,
    });
    templateDialogOpen.value = false;
    ElMessage.success("模板已保存，重新生成报告后生效");
  } catch (error) {
    ElMessage.error(errorText(error, "模板保存失败"));
  } finally {
    templateSaving.value = false;
  }
}

async function openObsidianPreview(report: ReportRecord) {
  obsidianLoading.value = true;
  try {
    if (report.id === selectedReport.value?.id && hasUnsavedChanges.value) {
      await saveMarkdown(false);
    }
    const current = reports.value.find((item) => item.id === report.id) ?? report;
    obsidianPreview.value = await store.previewObsidianReport(current);
    obsidianStrategy.value = obsidianPreview.value.fileExists ? "append" : "overwrite";
    obsidianDialogOpen.value = true;
  } catch (error) {
    ElMessage.error(errorText(error, "Obsidian 写入预览失败"));
  } finally {
    obsidianLoading.value = false;
  }
}

async function writeObsidianReport() {
  if (!obsidianPreview.value) return;
  obsidianWriting.value = true;
  try {
    const result = await store.writeObsidianReport(obsidianPreview.value.runId, obsidianStrategy.value);
    obsidianDialogOpen.value = false;
    ElMessage.success(`已写入 ${result.targetPath}`);
  } catch (error) {
    ElMessage.error(errorText(error, "Obsidian 写入失败"));
  } finally {
    obsidianWriting.value = false;
  }
}

function onWorkspaceKeydown(event: KeyboardEvent) {
  if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "s" || !selectedReport.value) return;
  event.preventDefault();
  if (hasUnsavedChanges.value) void saveMarkdown();
}

onMounted(async () => {
  window.addEventListener("keydown", onWorkspaceKeydown);
  try {
    const settings = await getSettings();
    reportDurationFormat.value = settings.shared.report_duration_format === "hours" ? "hours" : "minutes";
  } catch {
    reportDurationFormat.value = "minutes";
  }
});
onUnmounted(() => window.removeEventListener("keydown", onWorkspaceKeydown));

async function regenerateReport(report: ReportRecord) {
  try {
    await ElMessageBox.confirm("将重新导入当前事项数据并覆盖这份报告的 Markdown 内容。", `重新生成${reportTypeLabel(report.type)}`, {
      type: "warning",
      confirmButtonText: "重新生成",
      cancelButtonText: "取消",
      customClass: "work-confirm-dialog",
      closeOnClickModal: false
    });
  } catch {
    return;
  }
  reportSaving.value = true;
  try {
    const updated = await store.regenerateReport(report);
    if (selectedReportId.value === updated.id) editorDraft.value = updated.markdown;
    ElMessage.success("报告已重新生成");
  } catch (error) {
    ElMessage.error(errorText(error, "报告重新生成失败"));
  } finally {
    reportSaving.value = false;
  }
}

async function deleteReport(report: ReportRecord) {
  try {
    await ElMessageBox.confirm(`删除后将移除“${report.period}”。`, "删除报告", {
      type: "warning",
      confirmButtonText: "删除",
      cancelButtonText: "取消",
      customClass: "work-confirm-dialog",
      confirmButtonClass: "work-confirm-danger",
      closeOnClickModal: false
    });
  } catch {
    return;
  }
  reportSaving.value = true;
  try {
    await store.deleteReport(report);
    if (selectedReportId.value === report.id) selectedReportId.value = reports.value[0]?.id ?? "";
    ElMessage.success("报告已删除");
  } catch (error) {
    ElMessage.error(errorText(error, "报告删除失败"));
  } finally {
    reportSaving.value = false;
  }
}

async function refreshReimbursements(report: ReportRecord) {
  if (report.type === "daily") return;
  try {
    await ElMessageBox.confirm("将从 SeaTable 读取当前周期的待结算报销，并重新生成报告内容。", "更新待报销", {
      type: "warning",
      confirmButtonText: "查询并重新生成",
      cancelButtonText: "取消",
      customClass: "work-confirm-dialog",
      closeOnClickModal: false,
    });
  } catch {
    return;
  }
  reportSaving.value = true;
  try {
    const range = reportPeriodRange(report);
    const result = await querySeaTableReimbursements({ subjectId: report.subjectId, ...range });
    const current = reports.value.find((item) => item.id === report.id) ?? report;
    const updated = await store.regenerateReport(current);
    if (selectedReportId.value === updated.id) editorDraft.value = updated.markdown;
    ElMessage.success(`已汇总 ${result.items.length} 项待报销，共 ¥${result.total.toFixed(2)}`);
  } catch (error) {
    ElMessage.error(errorText(error, "待报销查询失败，报告内容未修改"));
  } finally {
    reportSaving.value = false;
  }
}

async function copyMarkdown(report = selectedReport.value) {
  if (!report) return;
  const content = report.id === selectedReport.value?.id ? editorDraft.value : report.markdown;
  await navigator.clipboard?.writeText(content);
  ElMessage.success("Markdown 已复制");
}

async function copyPublicReport(report = selectedReport.value) {
  if (!report) return;
  try {
    const content = await store.publicReportText(report);
    await navigator.clipboard?.writeText(content);
    ElMessage.success("上报版内容已复制");
  } catch (error) {
    ElMessage.error(errorText(error, "上报版生成失败"));
  }
}

function handleReportCommand(command: "scope" | "copy" | "public" | "obsidian" | "reimbursements" | "regenerate" | "delete", report: ReportRecord) {
  if (command === "scope") openScopeDialog(report);
  else if (command === "copy") void copyMarkdown(report);
  else if (command === "public") void copyPublicReport(report);
  else if (command === "obsidian") void openObsidianPreview(report);
  else if (command === "reimbursements") void refreshReimbursements(report);
  else if (command === "regenerate") void regenerateReport(report);
  else void deleteReport(report);
}

function handleSelectedReportCommand(command: "copy" | "public" | "obsidian" | "reimbursements" | "delete") {
  if (selectedReport.value) handleReportCommand(command, selectedReport.value);
}

function errorText(error: unknown, fallback: string) {
  if (typeof error === "string" && error.trim()) return error;
  return error instanceof Error ? error.message : fallback;
}

function existingReportId(error: unknown) {
  const message = errorText(error, "");
  if (!message.startsWith("REPORT_ALREADY_EXISTS:")) return "";
  return message.split("|")[1] ?? "";
}
</script>

<template>
  <section class="report-workspace">
    <aside class="report-library">
      <header class="report-library-head">
        <div><h2>报告</h2><span>{{ reports.length ? `${reports.length} 份记录` : '按日期与周期归档' }}</span></div>
        <button class="primary-button compact" type="button" @click="openCreateDialog"><Plus :size="14" />创建</button>
      </header>

      <label class="report-search-field">
        <Search :size="13" />
        <input v-model="reportQuery" type="search" placeholder="搜索日期或主体" />
      </label>

      <div class="report-filter-tabs" role="tablist" aria-label="报告类型筛选">
        <button v-for="option in [{ value: 'all', label: '全部' }, { value: 'daily', label: '日报' }, { value: 'weekly', label: '周报' }, { value: 'monthly', label: '月报' }]" :key="option.value" type="button" :class="[{ active: reportFilter === option.value }, option.value === 'all' ? 'type-all' : reportTypeClass(option.value as ReportType)]" @click="reportFilter = option.value as 'all' | ReportType"><span>{{ option.label }}</span><small>{{ reportCounts[option.value as keyof typeof reportCounts] }}</small></button>
      </div>

      <div v-if="filteredReports.length" class="report-record-list">
        <article v-for="report in filteredReports" :key="report.id" class="report-record" :class="[{ active: selectedReportId === report.id }, reportTypeClass(report.type)]">
          <button class="report-record-main" type="button" @click="selectReport(report)">
            <span class="report-record-icon"><FileText :size="15" /></span>
            <span class="report-record-copy">
              <span class="report-record-title"><strong>{{ report.period }}</strong><i>{{ reportTypeLabel(report.type) }}</i></span>
              <span>{{ report.subjectName }} · {{ report.taskIds.length }} 项</span>
              <small>{{ formatUpdatedAt(report.updatedAt) }} 更新<template v-if="report.generatedCount > 1"> · 生成 {{ report.generatedCount }} 次</template></small>
            </span>
          </button>
          <el-dropdown class="report-record-menu" trigger="click" placement="bottom-end" popper-class="report-action-popper" @command="(command: 'scope' | 'public' | 'obsidian' | 'reimbursements' | 'regenerate' | 'delete') => handleReportCommand(command, report)">
            <button type="button" title="更多操作"><MoreHorizontal :size="14" /></button>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item command="scope"><Pencil :size="13" />调整范围</el-dropdown-item>
                <el-dropdown-item command="public"><ClipboardCopy :size="13" />复制上报版</el-dropdown-item>
                <el-dropdown-item v-if="report.type !== 'daily'" command="reimbursements"><ReceiptText :size="13" />更新待报销</el-dropdown-item>
                <el-dropdown-item command="regenerate"><RefreshCcw :size="13" />重新生成</el-dropdown-item>
                <el-dropdown-item command="delete" divided><Trash2 :size="13" />删除报告</el-dropdown-item>
              </el-dropdown-menu>
            </template>
          </el-dropdown>
        </article>
      </div>

      <div v-else class="report-empty-list">
        <FileText :size="23" />
        <strong>暂无报告</strong>
        <span>创建日报、周报或月报后会显示在这里</span>
      </div>
    </aside>

    <section v-if="selectedReport" class="report-editor-panel">
      <header class="report-editor-head">
        <div>
          <div class="report-editor-title"><h2>{{ selectedReport.period }}</h2><span :class="['report-type-badge', reportTypeClass(selectedReport.type)]">{{ reportTypeLabel(selectedReport.type) }}</span></div>
          <p><span>{{ selectedReport.subjectName }}</span><i></i><span>{{ selectedReport.taskIds.length }} 个事项</span><i></i><span>{{ formatUpdatedAt(selectedReport.updatedAt) }} 更新</span><template v-if="selectedReport.generatedCount > 1"><i></i><span>已生成 {{ selectedReport.generatedCount }} 次</span></template></p>
        </div>
        <div class="report-editor-actions">
          <button class="icon-button subtle" type="button" title="调整报告范围" @click="openScopeDialog(selectedReport)"><Pencil :size="14" /></button>
          <button class="icon-button subtle" type="button" title="设置当前类型模板" @click="openTemplateSettings(selectedReport)"><Braces :size="14" /></button>
          <button class="secondary-button compact report-regenerate-button" type="button" @click="regenerateReport(selectedReport)"><Sparkles :size="13" />重新生成</button>
          <button class="primary-button compact report-save-button" type="button" :disabled="!hasUnsavedChanges || reportSaving" @click="saveMarkdown()"><Check :size="14" />{{ hasUnsavedChanges ? '保存修改' : '已保存' }}</button>
          <el-dropdown trigger="click" placement="bottom-end" popper-class="report-action-popper" @command="handleSelectedReportCommand">
            <button class="icon-button subtle" type="button" title="更多操作"><MoreHorizontal :size="15" /></button>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item command="copy"><ClipboardCopy :size="13" />复制 Markdown</el-dropdown-item>
                <el-dropdown-item command="public"><ClipboardCopy :size="13" />复制上报版</el-dropdown-item>
                <el-dropdown-item command="obsidian"><HardDriveDownload :size="13" />写入 Obsidian</el-dropdown-item>
                <el-dropdown-item v-if="selectedReport.type !== 'daily'" command="reimbursements"><ReceiptText :size="13" />更新待报销</el-dropdown-item>
                <el-dropdown-item command="delete" divided><Trash2 :size="13" />删除报告</el-dropdown-item>
              </el-dropdown-menu>
            </template>
          </el-dropdown>
        </div>
      </header>

      <div class="daily-editor-modebar">
        <div class="report-mode-tabs" role="tablist" aria-label="报告编辑模式">
          <button :class="{ active: editorMode === 'edit' }" type="button" @click="editorMode = 'edit'"><FilePenLine :size="14" />编辑</button>
          <button :class="{ active: editorMode === 'preview' }" type="button" @click="editorMode = 'preview'">预览</button>
        </div>
        <span class="report-save-state" :class="{ dirty: hasUnsavedChanges }"><i></i>{{ hasUnsavedChanges ? '未保存修改' : '已保存' }}<small>{{ editorDraft.length }} 字符</small></span>
      </div>

      <MdEditor
        v-model="editorDraft"
        class="daily-markdown-editor"
        editor-id="report-record-editor"
        language="zh-CN"
        theme="dark"
        preview-theme="github"
        code-theme="github"
        :toolbars="[...toolbars]"
        :preview="editorMode === 'edit'"
        :preview-only="editorMode === 'preview'"
        :footers="['markdownTotal', '=', 'scrollSwitch']"
        :show-code-row-number="true"
      />
    </section>

    <section v-else class="report-editor-empty">
      <span><FileText :size="28" /></span>
      <h2>选择或创建一份报告</h2>
      <p>报告会按日期或周期保存，可以继续编辑，也可以从事项重复生成。</p>
      <button class="primary-button" type="button" @click="openCreateDialog"><Plus :size="15" />创建第一份报告</button>
    </section>

    <el-dialog v-model="reportDialogOpen" class="report-create-dialog" :title="dialogTitle" width="min(680px, 94vw)" append-to-body destroy-on-close>
      <div class="report-create-form">
        <section class="report-create-section report-scope-section">
          <header><span>1</span><div><strong>报告范围</strong><small>确定报告的归档周期和所属主体</small></div></header>
          <div class="report-create-section-body">
            <el-radio-group v-model="draftType" class="report-type-picker">
              <el-radio-button class="type-daily" value="daily"><span class="report-type-option"><CalendarDays :size="15" /><span><strong>日报</strong><small>单日记录</small></span></span></el-radio-button>
              <el-radio-button class="type-weekly" value="weekly"><span class="report-type-option"><CalendarRange :size="15" /><span><strong>周报</strong><small>周一至周日</small></span></span></el-radio-button>
              <el-radio-button class="type-monthly" value="monthly"><span class="report-type-option"><FileText :size="15" /><span><strong>月报</strong><small>自然月汇总</small></span></span></el-radio-button>
            </el-radio-group>

            <div class="report-create-fields">
              <label class="report-scope-control"><span class="report-scope-control-icon"><CalendarDays :size="15" /></span><span class="report-scope-control-content"><span>日期或周期</span><el-date-picker v-model="draftReferenceDate" :type="draftType === 'daily' ? 'date' : draftType === 'weekly' ? 'week' : 'month'" :format="draftType === 'daily' ? 'YYYY-MM-DD' : draftType === 'weekly' ? 'YYYY 第 ww 周' : 'YYYY-MM'" :clearable="false" popper-class="report-control-popper" /></span></label>
              <label class="report-scope-control"><span class="report-scope-control-icon subject"><Folder :size="15" /></span><span class="report-scope-control-content"><span>主体</span><el-select v-model="draftSubjectId" placeholder="选择主体" popper-class="report-control-popper"><el-option v-for="subject in store.subjects" :key="subject.id" :label="subject.name" :value="subject.id" /></el-select></span></label>
            </div>
            <div :class="['report-scope-result', reportTypeClass(draftType)]"><span>归档范围</span><strong>{{ periodValue(draftType, draftReferenceDate) }}</strong></div>
          </div>
        </section>

        <section class="report-create-section report-import-section">
          <header><span>2</span><div><strong>导入事项</strong><small>已选择 {{ draftSummary.total }} 项，其中 {{ draftSummary.completed }} 项已完成</small></div></header>
          <div class="report-create-section-body">
            <div class="report-import-tools">
              <label><Search :size="13" /><input v-model="draftTaskQuery" type="search" placeholder="筛选事项" /></label>
              <div>
                <button class="secondary-button compact" type="button" :disabled="draftTasksLoading" @click="selectSuggestedTasks"><Sparkles :size="12" />{{ draftTasksLoading ? '加载中' : '按用时选择' }}</button>
                <button class="secondary-button compact" type="button" @click="toggleAllDraftTasks">{{ draftTaskIds.length === draftTasks.length ? '取消全选' : '全选' }}</button>
                <button class="icon-button subtle" type="button" title="清空选择" :disabled="!draftTaskIds.length" @click="clearDraftTasks"><Trash2 :size="13" /></button>
              </div>
            </div>
            <el-checkbox-group v-model="draftTaskIds" class="report-task-options">
              <el-checkbox v-for="task in visibleDraftTasks" :key="task.id" :value="task.id" :style="{ paddingLeft: `${taskDepth(task) * 18 + 10}px` }">
                <span class="report-task-option-title"><i :class="task.status === 'done' ? 'done' : draftMinutes(task.id) > 0 ? 'active' : 'planned'"></i>{{ task.title }}</span>
                <span class="report-task-option-meta"><template v-if="draftType === 'daily' && draftDailyEstimate(task.id)">今日预计 {{ formatHours(draftDailyEstimate(task.id)!) }} · </template><template v-if="task.estimate">整体预计 {{ formatHours(task.estimate) }} · </template><template v-if="draftMinutes(task.id)">实际 {{ formatHours(draftMinutes(task.id)) }} · </template>{{ task.status === 'done' ? '已完成' : draftMinutes(task.id) > 0 ? '有计时' : '未执行' }}</span>
              </el-checkbox>
              <div v-if="!visibleDraftTasks.length" class="report-task-empty">没有匹配的事项</div>
            </el-checkbox-group>
          </div>
        </section>
      </div>
      <template #footer>
        <div class="report-dialog-summary"><strong>{{ periodValue(draftType, draftReferenceDate) }}</strong><span>{{ store.subjects.find((subject) => subject.id === draftSubjectId)?.name }} · {{ draftSummary.total }} 项 · {{ formatHours(draftSummary.minutes) }}</span></div>
        <div class="report-dialog-actions"><button class="secondary-button" type="button" @click="reportDialogOpen = false">取消</button><button class="primary-button" type="button" :disabled="!draftTaskIds.length || reportSaving" @click="saveReportDialog"><Check :size="14" />{{ dialogMode === 'create' ? existingDraftReport ? '覆盖并生成' : '生成并保存' : '保存范围' }}</button></div>
      </template>
    </el-dialog>

    <el-dialog v-model="templateDialogOpen" class="report-template-dialog" :title="`${reportTypeLabel(templateType)}模板设置`" width="min(920px, 94vw)" append-to-body destroy-on-close>
      <div v-if="templateLoading" class="report-dialog-loading">正在加载模板...</div>
      <div v-else class="report-template-layout">
        <section class="template-editor-pane">
          <div class="template-pane-head"><div><strong>模板内容</strong><span>仅应用于当前主体的{{ reportTypeLabel(templateType) }}</span></div></div>
          <textarea v-model="templateDraft" spellcheck="false" aria-label="报告模板内容"></textarea>
          <div class="template-variable-list">
            <button v-for="variable in templateVariables" :key="variable.token" type="button" @click="insertTemplateVariable(variable.token)"><code>{{ variable.token }}</code><span>{{ variable.label }}</span></button>
          </div>
        </section>
        <section class="template-preview-pane">
          <div class="template-pane-head"><div><strong>实时预览</strong><span>示例数据不会写入报告</span></div></div>
          <MdPreview editor-id="report-template-preview" :model-value="templatePreview" language="zh-CN" theme="dark" preview-theme="github" code-theme="github" />
        </section>
      </div>
      <template #footer><button class="secondary-button" type="button" @click="templateDialogOpen = false">取消</button><button class="primary-button" type="button" :disabled="templateLoading || templateSaving" @click="saveTemplate">{{ templateSaving ? '保存中...' : '保存模板' }}</button></template>
    </el-dialog>

    <el-dialog v-model="obsidianDialogOpen" class="report-write-dialog" title="写入 Obsidian" width="min(720px, 94vw)" append-to-body destroy-on-close :close-on-click-modal="!obsidianWriting" :close-on-press-escape="!obsidianWriting">
      <div v-if="obsidianPreview" class="report-write-content">
        <div class="report-write-target"><span>目标文件</span><strong>{{ obsidianPreview.targetPath }}</strong><small>{{ obsidianPreview.fileExists ? '文件已存在，请选择处理方式' : '将创建新文件' }}</small></div>
        <el-radio-group v-if="obsidianPreview.fileExists" v-model="obsidianStrategy" class="report-write-strategy">
          <el-radio-button value="append">追加到末尾</el-radio-button>
          <el-radio-button value="overwrite">覆盖原文件</el-radio-button>
        </el-radio-group>
        <div class="report-write-preview-grid" :class="{ single: !obsidianPreview.fileExists }">
          <section v-if="obsidianPreview.fileExists"><header>现有内容</header><pre>{{ obsidianPreview.existingContent || '空文件' }}</pre></section>
          <section><header>本次报告</header><pre>{{ obsidianPreview.reportMarkdown }}</pre></section>
        </div>
      </div>
      <template #footer><button class="secondary-button" type="button" :disabled="obsidianWriting" @click="obsidianDialogOpen = false">取消</button><button class="primary-button" type="button" :disabled="!obsidianPreview || obsidianWriting" @click="writeObsidianReport"><HardDriveDownload :size="14" />{{ obsidianWriting ? '写入中...' : obsidianPreview?.fileExists ? (obsidianStrategy === 'append' ? '确认追加' : '确认覆盖') : '创建文件' }}</button></template>
    </el-dialog>
  </section>
</template>
