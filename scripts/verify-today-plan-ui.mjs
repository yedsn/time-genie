import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";
import path from "node:path";
import { chromium } from "playwright";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const port = Number(process.env.TIMEGENIE_TODAY_PLAN_TEST_PORT ?? 5203);
const baseUrl = "http://127.0.0.1:" + port + "/";
const evidenceDir = path.join(repoRoot, ".tmp-today-plan-visual-walkthrough");
mkdirSync(evidenceDir, { recursive: true });
const server = spawn(
  process.platform === "win32" ? "npm.cmd" : "npm",
  ["run", "dev", "--", "--host", "127.0.0.1", "--port", String(port), "--strictPort"],
  { cwd: repoRoot, stdio: ["ignore", "pipe", "pipe"], shell: process.platform === "win32" },
);

async function waitForServer() {
  const startedAt = Date.now();
  while (Date.now() - startedAt < 30_000) {
    try {
      if ((await fetch(baseUrl)).ok) return;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("开发服务器启动超时");
}

function mockInitScript() {
  let callbackId = 0;
  const callbacks = new Map();
  const subjects = [
    { id: "subject-a", name: "工作", sortOrder: 1, openTaskCount: 4, version: 1 },
    { id: "subject-b", name: "个人", sortOrder: 2, openTaskCount: 1, version: 1 },
  ];
  const record = (id, overrides) => ({
    id,
    subjectId: "subject-a",
    title: id,
    status: "open",
    sortOrder: 10,
    pathLabel: id,
    projectName: "工作",
    selectable: true,
    isLeaf: true,
    canAcceptChildren: false,
    directMinutes: 0,
    totalMinutes: 0,
    adjustmentFingerprint: id,
    executionState: "not_started",
    completedToday: false,
    version: 1,
    ...overrides,
  });

  const activeTasks = [
    record("parent", { title: "项目", selectable: false, isLeaf: false, canAcceptChildren: true, sortOrder: 10 }),
    record("child", { title: "写今日计划", parentId: "parent", pathLabel: "项目 / 写今日计划", sortOrder: 20 }),
    record("no-estimate", { title: "无预计事项", parentId: "parent", pathLabel: "项目 / 无预计事项", sortOrder: 30 }),
    record("estimate-auto", { title: "预计自动加入事项", estimateMinutes: 40, sortOrder: 35, recurrence: { id: "rule-estimate-auto", taskId: "estimate-auto", frequency: "daily", effectiveStart: "2026-10-19", summary: "每天", version: 1 } }),
    record("recurring", { title: "每日复盘", sortOrder: 38, recurrence: { id: "rule-recurring", taskId: "recurring", frequency: "daily", effectiveStart: "2026-10-19", summary: "每天", version: 1 } }),
    record("long-title", { title: "梳理一份足够长的事项标题用于检查今日计划列表在窄窗口下是否会溢出容器边界", sortOrder: 40 }),
    record("backlog", { title: "长期待办", estimateMinutes: 15, sortOrder: 50, recurrence: { id: "rule-backlog", taskId: "backlog", frequency: "daily", effectiveStart: "2026-10-19", summary: "每天", version: 1 } }),
    record("plain-done", { title: "已完成普通事项", status: "done", executionState: "done", version: 2, sortOrder: 60 }),
    record("personal", { subjectId: "subject-b", title: "阅读", projectName: "个人", sortOrder: 10 }),
  ];

  const planRows = {
    "subject-a": [
      record("parent", { title: "项目", selectable: false, isLeaf: false, canAcceptChildren: true, todayMember: false, todayContext: true, dayInstanceDate: "2026-10-19", dayInstanceKind: "context", dayInstanceSourceTaskId: "parent", dayPlanVersion: 1, sortOrder: 10 }),
      record("child", { title: "写今日计划", parentId: "parent", pathLabel: "项目 / 写今日计划", todayMember: true, todayContext: false, dayInstanceDate: "2026-10-19", dayInstanceKind: "member", dayInstanceSourceTaskId: "child", dayPlanVersion: 1, todayEstimateMinutes: 30, directMinutes: 20, totalMinutes: 20, sortOrder: 20 }),
      record("done-today", { title: "今天已完成的子项", parentId: "parent", pathLabel: "项目 / 今天已完成的子项", status: "done", executionState: "done", completedToday: true, version: 2, todayMember: true, todayContext: false, dayInstanceDate: "2026-10-19", dayInstanceKind: "member", dayPlanVersion: 1, directMinutes: 45, totalMinutes: 45, sortOrder: 25 }),
      record("no-estimate", { title: "无预计事项", parentId: "parent", pathLabel: "项目 / 无预计事项", todayMember: true, todayContext: false, dayInstanceDate: "2026-10-19", dayInstanceKind: "member", dayInstanceSourceTaskId: "no-estimate", dayPlanVersion: 1, sortOrder: 30 }),
      record("recurring", { title: "每日复盘", status: "done", executionState: "done", completedToday: true, todayMember: true, todayContext: false, dayInstanceDate: "2026-10-19", dayInstanceKind: "member", dayInstanceSourceTaskId: "recurring", dayPlanVersion: 1, sortOrder: 35, recurrence: { id: "rule-recurring", taskId: "recurring", frequency: "daily", effectiveStart: "2026-10-19", summary: "每天", version: 1 }, occurrenceDate: "2026-10-19", occurrenceOrigin: "scheduled", occurrenceVersion: 1 }),
      record("plain-done", { title: "已完成普通事项", status: "done", executionState: "done", completedToday: true, version: 2, todayMember: true, todayContext: false, dayInstanceDate: "2026-10-19", dayInstanceKind: "member", dayInstanceSourceTaskId: "plain-done", dayPlanVersion: 1, sortOrder: 50 }),
    ],
    "subject-b": [
      record("personal", { subjectId: "subject-b", title: "阅读", projectName: "个人", todayMember: true, todayContext: false, dayInstanceDate: "2026-10-19", dayInstanceKind: "member", dayInstanceSourceTaskId: "personal", dayPlanVersion: 1, sortOrder: 10 }),
    ],
  };
  const moveToFront = (rows, taskId) => {
    const index = rows.findIndex((row) => row.id === taskId);
    if (index > 0) rows.unshift(rows.splice(index, 1)[0]);
  };
  if (new URLSearchParams(window.location.search).has("text-import-empty")) planRows["subject-a"].splice(0);
  moveToFront(activeTasks, "recurring");
  moveToFront(planRows["subject-a"], "recurring");
  // Cloud snapshots can place children before their parents because sortOrder is sibling-scoped.
  moveToFront(planRows["subject-a"], "child");
  if (sessionStorage.getItem("timegenie-test-empty-subject-a") === "1") planRows["subject-a"].splice(0);

  const calls = { dayPlanSet: [], dayPlanBatch: [], planBatch: [], dayPlanSnapshot: 0, estimateSet: [], update: [], completed: [], create: [], move: [], actualAdjustment: [], cloudCatchUp: 0 };
  let failDayPlanBatchOnce = false;
  let failSnapshotFor = "";
  let snapshotDelayMs = 0;
  let created = 0;

  const planRowFor = (subjectId, taskId) => planRows[subjectId]?.find((row) => row.id === taskId);
  const snapshotFor = (subjectId) => {
    if (failSnapshotFor && failSnapshotFor === subjectId) {
      failSnapshotFor = "";
      throw new Error("今日计划快照读取失败");
    }
    return { subjectId, workDate: "2026-10-19", revision: 1, tasks: (planRows[subjectId] ?? []).map((row) => ({ ...row })) };
  };

  window.__TODAY_PLAN_TEST__ = {
    calls,
    failNextDayPlanBatch() { failDayPlanBatchOnce = true; },
    failNextSnapshotFor(subjectId) { failSnapshotFor = subjectId; },
    delayNextSnapshot(milliseconds) { snapshotDelayMs = milliseconds; },
    clearPlan(subjectId) {
      planRows[subjectId]?.splice(0);
      if (subjectId === "subject-a") sessionStorage.setItem("timegenie-test-empty-subject-a", "1");
    },
    setWorkDate(workDate) {
      currentWorkDate = workDate;
      sessionStorage.setItem("timegenie-test-work-date", workDate);
      if (workDate === "2026-10-19") sessionStorage.removeItem("timegenie-test-empty-subject-a");
    },
    planIds(subjectId) { return (planRows[subjectId] ?? []).map((row) => row.id); },
    planRowFor,
    activeTask(id) { return activeTasks.find((task) => task.id === id); },
  };

  let currentWorkDate = sessionStorage.getItem("timegenie-test-work-date") ?? "2026-10-19";
  let currentDayEndAt = Date.now() + 86_400_000;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    transformCallback(callback, once) {
      callbackId += 1;
      callbacks.set(callbackId, { callback, once });
      return callbackId;
    },
    unregisterCallback(id) { callbacks.delete(id); },
    async invoke(command, args = {}) {
      switch (command) {
        case "settings_get":
          return {
            storageMode: "local", workspaceId: "workspace", deviceId: "device", shared: {},
            device: { app_theme: "paper", completion_feedback_enabled: false }, integrations: {},
            secrets: { seatableTokenSet: false, supabaseSessionSet: false },
          };
        case "workbench_bootstrap_get":
          return { subjects, timezone: "Asia/Shanghai", currentWorkDate, currentDayStartAt: Date.now() - 3_600_000, currentDayEndAt };
        case "work_calendar_get_context":
          currentDayEndAt = Date.now() + 86_400_000;
          return { timezone: "Asia/Shanghai", currentWorkDate, currentDayStartAt: Date.now() - 3_600_000, currentDayEndAt };
        case "subject_list":
          return subjects;
        case "task_list": {
          const request = args.request ?? {};
          if (request.todayOnly) return { tasks: [], revision: 1 };
          return { tasks: activeTasks.filter((item) => !request.subjectId || item.subjectId === request.subjectId), revision: 1 };
        }
        case "task_list_completed_page":
          return { tasks: [], page: 1, pageSize: 20, total: 0, totalPages: 0 };
        case "task_day_plan_snapshot":
          calls.dayPlanSnapshot += 1;
          if (snapshotDelayMs) {
            const delay = snapshotDelayMs;
            snapshotDelayMs = 0;
            await new Promise((resolve) => setTimeout(resolve, delay));
          }
          return snapshotFor(args.request.subjectId);
        case "task_day_plan_set": {
          calls.dayPlanSet.push({ ...args.request });
          for (const subject of Object.keys(planRows)) {
            const rows = planRows[subject];
            const index = rows.findIndex((row) => row.id === args.request.taskId);
            if (args.request.state === "excluded" && index >= 0) rows.splice(index, 1);
          }
          return { taskId: args.request.taskId, workDate: args.request.workDate, state: args.request.state, version: 2 };
        }
        case "task_plan_batch_apply": {
          const request = args.request;
          calls.planBatch.push(request);
          const createdItems = request.finalItems.filter((item) => !item.taskId).map((item) => ({ lineKey: item.lineKey, taskId: `text-import-${++created}` }));
          const idByLineKey = new Map(createdItems.map((item) => [item.lineKey, item.taskId]));
          for (const item of request.finalItems) {
            if (item.taskId) continue;
            const createdRow = record(idByLineKey.get(item.lineKey), {
              subjectId: request.subjectId, title: item.title,
              parentId: item.parentLineKey ? idByLineKey.get(item.parentLineKey) : undefined,
              todayMember: Boolean(request.dayInstanceDate), todayContext: false, dayInstanceKind: request.dayInstanceDate ? "member" : undefined,
              dayInstanceDate: request.dayInstanceDate, sortOrder: created * 10,
            });
            if (request.dayInstanceDate) planRows[request.subjectId].push(createdRow);
            else activeTasks.push(createdRow);
          }
          return { createdCount: createdItems.length, updatedCount: 0, deletedCount: 0, todayPlanChangedCount: 0, createdItems };
        }
        case "task_day_plan_batch_set": {
          calls.dayPlanBatch.push({ ...args.request });
          if (failDayPlanBatchOnce) {
            failDayPlanBatchOnce = false;
            throw new Error("模拟加入今日失败");
          }
          const rows = planRows[args.request.subjectId] ??= [];
          for (const item of args.request.items ?? []) {
            const source = activeTasks.find((task) => task.id === item.taskId);
            if (!source) continue;
            if (rows.some((row) => row.dayInstanceSourceTaskId === item.taskId)) continue;
            rows.push({ ...source, id: `instance-${item.taskId}`, todayMember: true, todayContext: false, dayInstanceDate: currentWorkDate, dayInstanceKind: "member", dayInstanceSourceTaskId: item.taskId, dayPlanVersion: 1, todayEstimateMinutes: source.recurrence ? source.estimateMinutes : undefined });
          }
          return { changedCount: (args.request.items ?? []).length };
        }
        case "task_daily_estimate_set": {
          calls.estimateSet.push({ ...args.request });
          const rows = planRows[args.request.subjectId ?? "subject-a"];
          let found = false;
          for (const subject of Object.keys(planRows)) {
            const row = planRows[subject].find((item) => item.id === args.request.taskId);
            if (row) {
              row.todayEstimateMinutes = args.request.estimateMinutes;
              found = true;
            }
          }
          if (!found && args.request.estimateMinutes) {
            const source = activeTasks.find((task) => task.id === args.request.taskId);
            if (source) planRows[source.subjectId].push({ ...source, todayMember: true, todayContext: false, dayPlanVersion: 1, todayEstimateMinutes: args.request.estimateMinutes });
          }
          void rows;
          return null;
        }
        case "task_daily_estimate_batch_set": {
          for (const item of args.request.items ?? []) {
            calls.estimateSet.push({ ...item, subjectId: args.request.subjectId, workDate: args.request.workDate });
            const source = activeTasks.find((task) => task.id === item.taskId);
            let row = Object.values(planRows).flat().find((task) => task.id === item.taskId);
            if (!source && !row) continue;
            const subjectId = source?.subjectId ?? row.subjectId;
            if (!row && item.estimateMinutes) {
              row = { ...source, todayMember: true, todayContext: false, dayPlanVersion: 1 };
              planRows[subjectId].push(row);
            }
            if (row) {
              row.todayEstimateMinutes = item.estimateMinutes;
              row.dailyEstimateVersion = (row.dailyEstimateVersion ?? 0) + 1;
            }
          }
          return { changedCount: (args.request.items ?? []).length };
        }
        case "task_update": {
          calls.update.push({ ...args.request });
          for (const subject of Object.keys(planRows)) {
            const row = planRows[subject].find((item) => item.id === args.request.id);
            if (!row) continue;
            if (args.request.title !== undefined) {
              row.title = args.request.title;
              row.pathLabel = row.parentId ? `项目 / ${args.request.title}` : args.request.title;
            }
            if (args.request.note !== undefined) row.note = args.request.note ?? undefined;
            if (args.request.estimateMinutes !== undefined) row.estimateMinutes = args.request.estimateMinutes ?? undefined;
            row.version += 1;
          }
          const active = activeTasks.find((item) => item.id === args.request.id);
          if (active) {
            if (args.request.title !== undefined) active.title = args.request.title;
            if (args.request.note !== undefined) active.note = args.request.note ?? undefined;
            active.version += 1;
          }
          const updated = planRowFor("subject-a", args.request.id) ?? active;
          return updated ? {
            ...updated,
            todayMember: false,
            todayContext: false,
            dayPlanState: undefined,
            dayPlanVersion: undefined,
            directMinutes: 0,
            totalMinutes: 0,
            executionState: "not_started",
          } : {};
        }
        case "task_set_completed": {
          calls.completed.push({ ...args.request });
          const row = planRowFor("subject-a", args.request.id);
          if (row) {
            row.status = args.request.done ? "done" : "open";
            row.executionState = args.request.done ? "done" : "started";
            row.completedToday = Boolean(args.request.done);
            row.version += 1;
          }
          return { ...(row ?? {}) };
        }
        case "task_create":
        case "task_create_quick": {
          created += 1;
          const id = "created-" + created;
          calls.create.push({ id, ...args.request });
          const row = record(id, {
            subjectId: args.request.subjectId,
            parentId: args.request.parentId,
            title: args.request.title,
            projectName: "工作",
            pathLabel: args.request.parentId ? `项目 / ${args.request.title}` : args.request.title,
            sortOrder: 90 + created,
          });
          if (args.request.dayInstanceDate) {
            row.todayMember = true;
            row.todayContext = false;
            row.dayInstanceDate = args.request.dayInstanceDate;
            row.dayInstanceKind = "member";
            planRows[row.subjectId].push(row);
          } else activeTasks.push(row);
          return { ...row };
        }
        case "task_create_positioned": {
          created += 1;
          const id = "created-" + created;
          calls.create.push({ id, ...args.request });
          const row = record(id, {
            subjectId: args.request.subjectId,
            parentId: args.request.parentId,
            title: args.request.title,
            projectName: args.request.projectName ?? "工作",
            plannedDate: args.request.plannedDate,
            estimateMinutes: args.request.estimateMinutes,
            note: args.request.note,
            pathLabel: args.request.parentId ? `项目 / ${args.request.title}` : args.request.title,
            sortOrder: 90 + created,
          });
          if (args.request.dayInstanceDate) {
            row.todayMember = true;
            row.todayContext = false;
            row.dayInstanceDate = args.request.dayInstanceDate;
            row.dayInstanceKind = "member";
            planRows[row.subjectId].push(row);
          } else activeTasks.push(row);
          return { ...row };
        }
        case "actual_adjustment_snapshot":
          return {
            subjectId: args.request.subjectId,
            workDate: args.request.workDate,
            tasks: [...activeTasks, ...Object.values(planRows).flat()]
              .filter((task) => task.subjectId === args.request.subjectId && task.status === "open")
              .map((task) => ({
                ...task,
                isLeaf: ![...activeTasks, ...Object.values(planRows).flat()].some((candidate) => candidate.parentId === task.id),
                canAcceptChildren: !task.recurrence,
              })),
            adjustments: [],
          };
        case "actual_adjustment_batch_apply": {
          calls.actualAdjustment.push({ ...args.request });
          for (const item of args.request.items ?? []) {
            const active = activeTasks.find((task) => task.id === item.taskId);
            if (active) {
              active.directMinutes = item.targetMinutes;
              active.totalMinutes = item.targetMinutes;
            }
            for (const rows of Object.values(planRows)) {
              const row = rows.find((task) => task.id === item.taskId);
              if (row) {
                row.directMinutes = item.targetMinutes;
                row.totalMinutes = item.targetMinutes;
              }
            }
          }
          return { batchId: args.request.batchId, createdAdjustmentCount: (args.request.items ?? []).length, createdTaskCount: 0, createdTasks: [], adjustments: [] };
        }
        case "task_move": {
          calls.move.push({ ...args.request });
          const applyMove = (rows) => {
            const index = rows.findIndex((row) => row.id === args.request.id);
            if (index < 0) return undefined;
            const [moving] = rows.splice(index, 1);
            moving.parentId = args.request.parentId;
            const anchorId = args.request.nextSiblingId ?? args.request.previousSiblingId;
            const anchorIndex = anchorId ? rows.findIndex((row) => row.id === anchorId) : -1;
            const insertAt = anchorIndex < 0
              ? rows.length
              : args.request.nextSiblingId ? anchorIndex : anchorIndex + 1;
            rows.splice(insertAt, 0, moving);
            return moving;
          };
          applyMove(activeTasks);
          const owningRows = Object.values(planRows).find((rows) => rows.some((row) => row.id === args.request.id));
          const movingRow = owningRows ? applyMove(owningRows) : undefined;
          return { ...(movingRow ?? activeTasks.find((task) => task.id === args.request.id) ?? {}) };
        }
        case "task_delete_subtree":
        case "task_reorder_subtree":
          return null;
        case "time_entry_list":
          return { entries: [], rawMinutes: 0, allocatedMinutes: 0, pendingMinutes: 0, breakMinutes: 0, revision: 1 };
        case "time_entry_history_list":
          return { page: 1, pageSize: 7, totalDays: 0, totalPages: 0, groups: [], revision: 1 };
        case "timer_get_state":
          return null;
        case "unassigned_get_state":
          return { current: undefined };
        case "today_work_overview_get":
          return { scope: { subjectName: "全部主体", todayDate: "2026-10-19" }, summary: { actualMinutes: 0, estimatedMinutes: 0, completedCount: 0, activeCount: 0, notStartedCount: 0, unassignedMinutes: 0, completionRate: 0 }, days: [], timeline: [], tasks: [], revision: 1 };
        case "report_list":
          return { reports: [], revision: 1 };
        case "automation_hook_list":
          return { hooks: [] };
        case "storage_mode_get":
        case "cloud_sync_status":
          return { mode: "local", workspaceId: "workspace", deviceId: "device", online: true, syncState: "synced", pendingOperations: 0, conflictCount: 0, lastChangeSeq: 0, authBlocked: false };
        case "cloud_sync_catch_up":
          calls.cloudCatchUp += 1;
          return { changes: [], lastChangeSeq: 0, hasMore: false, domains: [] };
        case "cloud_session_get":
          return { signedIn: false };
        case "get_app_version":
          return "0.2.8";
        case "plugin:event|listen":
        case "plugin:event|unlisten":
        case "plugin:event|emit":
          return 0;
        default:
          return null;
      }
    },
  };
}

async function verifyQuickPlanTextImport(page, calls) {
  // Text import stays in the editor draft until preview/save, and defaults to all active items.
  await page.goto(baseUrl + "?text-import-empty=1", { waitUntil: "domcontentloaded" });
  await page.locator(".nav-list button", { hasText: "待办" }).click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.dayPlanSnapshot > 0);
  await page.waitForFunction(() => document.querySelectorAll(".pending-reminder-row").length === 0);
  await page.getByRole("button", { name: "编辑今日计划", exact: true }).click();
  const textDialog = page.locator(".quick-plan-dialog");
  await textDialog.waitFor({ state: "visible" });
  const editorContent = textDialog.locator(".cm-content");
  await editorContent.waitFor({ state: "visible" });
  const editorText = () => editorContent.locator(".cm-line").evaluateAll((elements) => elements.map((element) => {
    const line = element.cloneNode(true);
    line.querySelectorAll(".cm-placeholder").forEach((placeholder) => placeholder.remove());
    return line.textContent;
  }).join("\n"));
  if ((await editorText()).trim()) throw new Error("空今日计划应从空草稿开始");
  await textDialog.getByRole("button", { name: "从进行中导入", exact: true }).click();
  const textPicker = page.locator(".today-source-import-dialog");
  await textPicker.waitFor({ state: "visible" });
  if (await textPicker.getByRole("checkbox").count() !== 7) throw new Error("文本导入应包含当前主体的全部进行中事项和父级");
  if (await textPicker.getByRole("checkbox").evaluateAll((inputs) => inputs.some((input) => !input.checked))) throw new Error("文本导入没有默认全选");
  await textPicker.getByRole("button", { name: "取消全选", exact: true }).click();
  if (!await textPicker.locator(".el-dialog__footer .primary-button").isDisabled()) throw new Error("未选事项时导入按钮应禁用");
  await textPicker.getByRole("button", { name: "全选", exact: true }).click();
  await textPicker.getByRole("checkbox", { name: "长期待办导入今日计划" }).uncheck();
  await textPicker.locator(".el-dialog__footer .primary-button").click();
  await textPicker.waitFor({ state: "hidden" });
  const importedText = await editorText();
  if (!importedText.includes("- 项目\n  - 写今日计划\n  - 无预计事项")) throw new Error("文本导入没有保留父子层级: " + importedText);
  if (importedText.includes("长期待办") || importedText.includes("已完成普通事项") || importedText.includes("阅读")) throw new Error("文本导入包含未选、已完成或其他主体事项");
  if ((await calls()).planBatch.length || (await calls()).dayPlanBatch.length) throw new Error("文本导入不应提前保存");
  await textDialog.getByRole("button", { name: "从进行中导入", exact: true }).click();
  await textPicker.waitFor({ state: "visible" });
  await textPicker.getByRole("checkbox", { name: "长期待办导入今日计划" }).uncheck();
  await textPicker.locator(".el-dialog__footer .primary-button").click();
  await textPicker.waitFor({ state: "hidden" });
  if (await editorText() !== importedText) throw new Error("重复文本导入产生重复事项");
  await page.setViewportSize({ width: 720, height: 760 });
  await page.screenshot({ path: path.join(evidenceDir, "10-paper-today-text-import.png"), fullPage: true });
  if (await textDialog.evaluate((element) => element.scrollWidth > element.clientWidth + 1)) throw new Error("文本导入编辑器出现水平溢出");
  await textDialog.getByRole("button", { name: "预览变化", exact: true }).click();
  if (!await textDialog.getByText("新增 6 项", { exact: true }).isVisible()) throw new Error("文本导入预览数量错误");
  await textDialog.getByRole("button", { name: "确认保存", exact: true }).click();
  await textDialog.waitFor({ state: "hidden" });
  const textRequest = (await calls()).planBatch[0];
  if (textRequest.dayInstanceDate !== "2026-10-19" || textRequest.finalItems.length !== 6 || textRequest.finalItems.some((item) => item.taskId)) throw new Error("文本导入保存没有创建独立今日事项");
  if (textRequest.confirmedDeleteTaskIds.length || textRequest.baselineTasks.length) throw new Error("空计划导入不应删除原计划");
  await page.waitForFunction(() => document.querySelectorAll(".pending-reminder-row").length === 6);
}

async function mainSharedPlanEditor() {
  await waitForServer();
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.addInitScript(mockInitScript);
  const calls = () => page.evaluate(() => window.__TODAY_PLAN_TEST__.calls);

  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
  if (process.env.TIMEGENIE_TODAY_PLAN_TEXT_IMPORT_ONLY === "1") {
    await verifyQuickPlanTextImport(page, calls);
    if (pageErrors.length) throw new Error("页面错误: " + pageErrors.join("；"));
    await browser.close();
    console.log("today plan text import browser checks passed");
    return;
  }
  await page.locator(".nav-list button", { hasText: "待办" }).click();
  await page.locator(".pending-reminder-row").first().waitFor({ state: "visible" });

  if (await page.locator(".pending-reminder-row").count() !== 6) throw new Error("今日计划行数量异常");
  const initialTodayOrder = await page.locator(".pending-reminder-row").evaluateAll((rows) => rows.map((row) => row.dataset.taskId));
  if ((await page.evaluate(() => window.__TODAY_PLAN_TEST__.planIds("subject-a")))[0] !== "child") throw new Error("排序回归数据未覆盖子项先于父项返回");
  if (initialTodayOrder.join("|") !== "recurring|parent|child|done-today|no-estimate|plain-done") throw new Error("已完成事项未保持原始顺序: " + initialTodayOrder.join("|"));
  if (process.argv.includes("--sorting-only")) {
    await page.getByRole("button", { name: "配置今日事项" }).click();
    const dialog = page.locator(".today-import-dialog");
    await dialog.waitFor({ state: "visible" });
    const dialogOrder = await dialog.locator(".today-import-row").evaluateAll((rows) => rows.map((row) => row.dataset.todayImportTaskId));
    if (dialogOrder.join("|") !== initialTodayOrder.join("|")) throw new Error("今日配置弹框父子排序不一致");
    await dialog.getByRole("button", { name: "取消", exact: true }).click();
    await dialog.waitFor({ state: "hidden" });
    const listBox = await page.locator(".reminder-list").first().boundingBox();
    const handleBox = await page.locator('[data-task-id="no-estimate"] .drag-handle').boundingBox();
    const targetBox = await page.locator('.pending-reminder-row[data-task-id="child"]').boundingBox();
    await page.mouse.move(handleBox.x + handleBox.width / 2, handleBox.y + handleBox.height / 2);
    await page.mouse.down();
    await page.mouse.move(handleBox.x + handleBox.width / 2, handleBox.y - 24, { steps: 12 });
    await page.mouse.move(listBox.x + 260, targetBox.y + 6, { steps: 24 });
    await page.mouse.up();
    await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.move.length > 0);
    const moveCall = (await calls()).move.at(-1);
    if (moveCall.id !== "no-estimate" || moveCall.parentId !== "parent" || moveCall.nextSiblingId !== "child" || moveCall.previousSiblingId) throw new Error("今日排序修复影响同级拖拽: " + JSON.stringify(moveCall));
    if (pageErrors.length) throw new Error("页面错误: " + pageErrors.join("；"));
    await page.screenshot({ path: path.join(evidenceDir, "today-hierarchy-sorting.png"), fullPage: true });
    await browser.close();
    console.log("today hierarchy sorting UI checks passed");
    return;
  }
  const snapshotCallsBeforeRefresh = (await calls()).dayPlanSnapshot;
  await page.evaluate(() => window.__TODAY_PLAN_TEST__.delayNextSnapshot(450));
  await page.getByRole("tab", { name: /今日/ }).click();
  await page.getByRole("tab", { name: /今日/ }).click();
  await page.waitForTimeout(120);
  if (await page.locator(".today-plan-skeleton").count() !== 0) throw new Error("今日计划后台刷新仍用骨架替换已有列表");
  if (await page.locator(".pending-reminder-row").count() !== 6) throw new Error("今日计划后台刷新未保持已有列表稳定");
  if (await page.locator(".today-plan-loading-line, .today-plan-refresh-label, .today-plan-refreshing, .today-plan-list-refreshing").count() !== 0) throw new Error("今日计划刷新仍显示横条、提示或旧列表弱化效果");
  if (await page.getByText("正在更新", { exact: true }).count() !== 0 || await page.getByText("正在整理今日计划", { exact: true }).count() !== 0) throw new Error("今日计划仍显示额外加载提示");
  await page.screenshot({ path: path.join(evidenceDir, "08-paper-today-refreshing.png"), fullPage: true });
  await page.waitForTimeout(450);
  if ((await calls()).dayPlanSnapshot > snapshotCallsBeforeRefresh + 1) throw new Error("同一主体和日期的重复今日计划请求未合并");

  await page.evaluate(() => window.__TODAY_PLAN_TEST__.delayNextSnapshot(450));
  await page.locator('.subject-select-button[title="个人"]').click();
  await page.locator(".today-plan-skeleton").waitFor({ state: "visible" });
  if (await page.locator(".today-plan-skeleton-row").count() !== 4) throw new Error("首次加载今日计划未展示稳定事项骨架");
  if (await page.locator(".pending-reminder-row").count() !== 0) throw new Error("切换主体首次加载时不应短暂显示旧主体事项");
  const initialLoadingListBounds = await page.locator(".reminder-list").boundingBox();
  if (!initialLoadingListBounds || initialLoadingListBounds.height < 220) throw new Error("首次加载骨架未保持列表区域稳定高度");
  await page.screenshot({ path: path.join(evidenceDir, "09-paper-today-initial-loading.png"), fullPage: true });
  await page.locator('.pending-reminder-row[data-task-id="personal"]').waitFor({ state: "visible" });
  await page.locator('.subject-select-button[title="工作"]').click();
  await page.locator('.pending-reminder-row[data-task-id="recurring"]').waitFor({ state: "visible" });
  const firstTodayRow = page.locator(".pending-reminder-row").first();
  if (await firstTodayRow.getAttribute("data-task-id") !== "recurring") throw new Error("视觉验收未覆盖重复事项位于今日首项");
  if (!await firstTodayRow.evaluate((element) => element.classList.contains("task-group-single") && !element.classList.contains("task-group-start"))) throw new Error("首个重复事项未使用独立一级行样式");
  const firstRecurringTitleBox = await firstTodayRow.locator(':scope > .reminder-main > input').boundingBox();
  const firstRegularRootTitleBox = await page.locator('.pending-reminder-row[data-task-id="parent"] > .reminder-main > input').boundingBox();
  if (!firstRecurringTitleBox || !firstRegularRootTitleBox || Math.abs(firstRecurringTitleBox.x - firstRegularRootTitleBox.x) > 1) throw new Error("首个重复事项标题未与其他一级事项对齐");
  const firstRecurringStyle = await firstTodayRow.evaluate((element) => ({ borderRadius: getComputedStyle(element).borderRadius, marginBottom: getComputedStyle(element).marginBottom }));
  if (firstRecurringStyle.borderRadius !== "7px" || firstRecurringStyle.marginBottom !== "6px") throw new Error("首个重复事项未形成完整独立卡片");
  const completedTitleColor = await firstTodayRow.locator(':scope > .reminder-main > input').evaluate((element) => getComputedStyle(element).color);
  const openTitleColor = await page.locator('.pending-reminder-row[data-task-id="child"] > .reminder-main > input').evaluate((element) => getComputedStyle(element).color);
  if (completedTitleColor === openTitleColor) throw new Error("今日已完成事项未显示为灰色");
  if ((await firstTodayRow.innerText()).includes("今日已完成")) throw new Error("今日已完成事项仍显示额外完成态文字");
  const listBounds = await page.locator(".reminder-list").boundingBox();
  const firstRecurringBounds = await firstTodayRow.boundingBox();
  if (!listBounds || !firstRecurringBounds || firstRecurringBounds.x - listBounds.x < 7 || listBounds.x + listBounds.width - firstRecurringBounds.x - firstRecurringBounds.width < 7) throw new Error("已完成首个重复事项卡片缺少左右外边距");
  if (await page.locator(".pending-reminder-row .drag-handle").count() !== 6) throw new Error("今日页未复用进行中的拖动入口");
  if (await page.locator(".plan-placeholder-row").count() !== 1) throw new Error("今日页缺少与进行中一致的底部新增行");
  if (await page.getByRole("button", { name: "新增今日事项" }).count() !== 0) throw new Error("仍显示独立的新增今日事项按钮");
  if (await page.getByRole("button", { name: "管理完整计划" }).count() !== 0) throw new Error("仍显示管理完整计划按钮");
  if (await page.getByRole("button", { name: "编辑今日计划" }).count() !== 1) throw new Error("今日页缺少编辑今日计划");
  if (await page.getByRole("button", { name: "配置今日事项" }).count() !== 1) throw new Error("今日页缺少配置今日事项");
  const todayMetrics = page.locator(".plan-today-metrics");
  if (!await todayMetrics.getByText("今日预计", { exact: true }).isVisible() || !await todayMetrics.getByText("30min", { exact: true }).isVisible()) throw new Error("今日页预计总时长统计异常");
  if (!await todayMetrics.getByText("今日实际", { exact: true }).isVisible() || !await todayMetrics.getByText("1h5min", { exact: true }).isVisible()) throw new Error("今日页实际总时长统计异常");
  if (await page.getByRole("button", { name: "分配今日预计" }).count() !== 0 || await page.getByRole("button", { name: "从进行中加入" }).count() !== 0) throw new Error("今日页仍显示已合并的旧入口");
  const contextRow = page.locator('.pending-reminder-row[data-task-id="parent"]');
  if (await contextRow.locator(".task-timer-action").count() !== 0) throw new Error("上下文父事项不应可执行");
  if (await contextRow.locator(".check-button").count() !== 1 || !await contextRow.locator(".check-button").isDisabled()) throw new Error("今日页父事项应保留禁用勾选框");
  if (await page.locator(".pending-reminder-row .check-button").count() !== 6) throw new Error("今日页事项勾选框未保持统一对齐");
  if (await page.locator(".actual-time-picker").count() === 0) throw new Error("今日页应继续显示今日用时");
  const todayParentCheckBox = await contextRow.locator(".check-button").boundingBox();
  const todayRecurringCheckBox = await page.locator('.pending-reminder-row[data-task-id="recurring"] .check-button').boundingBox();
  if (!todayParentCheckBox || !todayRecurringCheckBox || Math.abs(todayParentCheckBox.x - todayRecurringCheckBox.x) > 1) throw new Error("今日页父事项与重复事项勾选框未对齐");
  const todayParentTitleBox = await contextRow.locator(':scope > .reminder-main > input').boundingBox();
  const todayChildTitleBox = await page.locator('.pending-reminder-row[data-task-id="child"] > .reminder-main > input').boundingBox();
  const todayParentDragBox = await contextRow.locator('.drag-handle').boundingBox();
  const todayChildDragBox = await page.locator('.pending-reminder-row[data-task-id="child"] .drag-handle').boundingBox();
  const todayChildCheckBox = await page.locator('.pending-reminder-row[data-task-id="child"] .check-button').boundingBox();
  if (!todayParentTitleBox || !todayChildTitleBox || Math.abs(todayChildTitleBox.x - todayParentTitleBox.x - 17) > 1) throw new Error("今日页二级事项未按事项弹框规则缩进");
  if (!todayParentDragBox || !todayChildDragBox || Math.abs(todayChildDragBox.x - todayParentDragBox.x - 17) > 1) throw new Error("今日页二级事项拖动柄未随整行缩进");
  if (!todayParentCheckBox || !todayChildCheckBox || Math.abs(todayChildCheckBox.x - todayParentCheckBox.x - 17) > 1) throw new Error("今日页二级事项勾选框未随整行缩进");
  if (await contextRow.locator('.drag-handle').evaluate((element) => getComputedStyle(element).opacity) !== "1") throw new Error("待办页拖动柄应常显");
  await contextRow.dispatchEvent("contextmenu", { clientX: 240, clientY: 240 });
  const todayListContextMenu = page.locator('.plan-task-context-menu:visible').filter({ has: page.locator('button', { hasText: "编辑备注" }) });
  await todayListContextMenu.waitFor({ state: "visible" });
  const todayListMenuItems = await todayListContextMenu.getByRole("menuitem").allTextContents();
  if (todayListMenuItems.join("|") !== "复制事项|在上方新增|在下方新增|新建子事项|编辑备注|删除事项") throw new Error(`今日待办右键菜单顺序或复制文案不正确：${todayListMenuItems.join("|")}`);
  await page.keyboard.press("Escape");
  await page.screenshot({ path: path.join(evidenceDir, "01-paper-wide.png"), fullPage: true });

  await page.getByRole("button", { name: "编辑今日计划" }).click();
  await page.locator(".quick-plan-dialog").waitFor({ state: "visible" });
  const todayQuickPlanText = await page.locator(".quick-plan-dialog .cm-content").innerText();
  if (!todayQuickPlanText.includes("写今日计划") || !todayQuickPlanText.includes("无预计事项")) throw new Error("编辑今日计划未包含今日事项");
  if (todayQuickPlanText.includes("长期待办") || todayQuickPlanText.includes("预计自动加入事项")) throw new Error("编辑今日计划混入了非今日事项");
  if (!todayQuickPlanText.includes("每日复盘") || !todayQuickPlanText.includes("今天已完成的子项") || !todayQuickPlanText.includes("已完成普通事项")) throw new Error("编辑今日计划应包含并保持已完成今日事项");
  await page.locator(".quick-plan-dialog button", { hasText: "取消" }).click();

  await page.getByRole("button", { name: "配置今日事项" }).click();
  const todayImport = page.locator(".today-import-dialog");
  await todayImport.waitFor({ state: "visible" });
  const importTodayOrder = await todayImport.locator(".today-import-row").evaluateAll((rows) => rows.map((row) => row.dataset.todayImportTaskId));
  if (importTodayOrder.join("|") !== initialTodayOrder.join("|")) throw new Error("配置今日事项未保持与今日列表一致的父子顺序: " + importTodayOrder.join("|"));
  await page.waitForTimeout(300);
  const todayImportBounds = await todayImport.boundingBox();
  if (!todayImportBounds || Math.abs(todayImportBounds.y + todayImportBounds.height / 2 - 450) > 3) throw new Error("配置今日事项弹框未在窗口内垂直居中: " + JSON.stringify(todayImportBounds));
  if (todayImportBounds.width < 940) throw new Error("配置今日事项弹框宽度不足：" + JSON.stringify(todayImportBounds));
  const todayImportBodyOverflow = await todayImport.locator(".el-dialog__body").evaluate((element) => ({ overflowY: getComputedStyle(element).overflowY, scrollHeight: element.scrollHeight, clientHeight: element.clientHeight }));
  if (todayImportBodyOverflow.overflowY !== "hidden" || todayImportBodyOverflow.scrollHeight > todayImportBodyOverflow.clientHeight + 1) throw new Error("配置今日事项弹框出现外层滚动：" + JSON.stringify(todayImportBodyOverflow));
  const importOverview = todayImport.locator(".today-import-overview");
  if (!await importOverview.getByText("1h5min", { exact: true }).isVisible() || !await importOverview.getByText("30min", { exact: true }).isVisible()) throw new Error("配置今日事项顶部时长统计异常");
  if (await todayImport.locator(".today-import-candidate-row").count()) throw new Error("配置今日事项主弹框仍同时展示进行中候选列表");
  if (await todayImport.getByPlaceholder("搜索进行中事项或父级路径").count()) throw new Error("配置今日事项主弹框不应包含来源搜索");
  if (await todayImport.locator(".today-import-list-head").getByText("操作", { exact: true }).count()) throw new Error("配置今日事项仍显示无内容的操作列");
  if (await todayImport.locator(".today-import-actions").count()) throw new Error("配置今日事项仍保留空操作单元格");
  const todayEstimateEditorBounds = await todayImport.locator('.today-import-row[data-today-import-task-id="child"] .today-import-estimate-editor').boundingBox();
  if (!todayEstimateEditorBounds || todayEstimateEditorBounds.width < 210 || todayEstimateEditorBounds.width > 230) throw new Error("配置今日事项的今日预计编辑器宽度不够紧凑：" + JSON.stringify(todayEstimateEditorBounds));
  const todayEstimateInputAlign = await todayImport.locator('input[aria-label="写今日计划今日预计分钟，可点击选择常用时长"]').evaluate((element) => getComputedStyle(element).textAlign);
  if (todayEstimateInputAlign !== "right") throw new Error("配置今日事项的今日预计内容未右对齐：" + todayEstimateInputAlign);
  const todayTaskTitleStyles = await todayImport.locator('input[aria-label="编辑事项名称：写今日计划"]').evaluate((element) => ({ textOverflow: getComputedStyle(element).textOverflow, whiteSpace: getComputedStyle(element).whiteSpace }));
  if (todayTaskTitleStyles.textOverflow !== "ellipsis" || todayTaskTitleStyles.whiteSpace !== "nowrap") throw new Error("配置今日事项标题未按溢出时省略：" + JSON.stringify(todayTaskTitleStyles));
  const todayActualBounds = await todayImport.locator('.today-import-row[data-today-import-task-id="child"] .today-import-actual').boundingBox();
  if (!todayActualBounds || todayEstimateEditorBounds.x - (todayActualBounds.x + todayActualBounds.width) < 40) throw new Error("配置今日事项的今日实际与今日预计间距不足：" + JSON.stringify({ todayActualBounds, todayEstimateEditorBounds }));
  const todayImportChildRowBounds = await todayImport.locator('.today-import-row[data-today-import-task-id="child"]').boundingBox();
  if (!todayImportChildRowBounds || Math.abs((todayImportChildRowBounds.x + todayImportChildRowBounds.width - 12) - (todayEstimateEditorBounds.x + todayEstimateEditorBounds.width)) > 1) throw new Error("配置今日事项的今日预计控件组未统一右对齐：" + JSON.stringify({ todayImportChildRowBounds, todayEstimateEditorBounds }));
  const compactTodayRowBounds = await todayImport.locator('.today-import-row[data-today-import-task-id="child"]').boundingBox();
  if (!compactTodayRowBounds || compactTodayRowBounds.height > 48) throw new Error("配置今日事项行高仍然过高：" + JSON.stringify(compactTodayRowBounds));
  if (await todayImport.getByRole("button", { name: "从进行中导入", exact: true }).count() !== 1) throw new Error("配置今日事项缺少从进行中导入入口");
  if (await todayImport.locator('.today-import-row[data-unused="never"]').count() !== 0) throw new Error("导入弹框状态异常");
  if (await todayImport.locator(".today-import-check, .today-import-check-spacer").count()) throw new Error("配置今日事项主列表不应显示勾选列");
  const importParentTaskBox = await todayImport.locator('.today-import-row[data-today-import-task-id="parent"] .today-import-title-line').boundingBox();
  const importChildTaskBox = await todayImport.locator('.today-import-row[data-today-import-task-id="child"] .today-import-title-line').boundingBox();
  const importParentTitleInputBox = await todayImport.locator('.today-import-row[data-today-import-task-id="parent"] .today-import-title-line > input').boundingBox();
  const importChildTitleInputBox = await todayImport.locator('.today-import-row[data-today-import-task-id="child"] .today-import-title-line > input').boundingBox();
  const importParentDragBox = await todayImport.locator('.today-import-row[data-today-import-task-id="parent"] .today-import-drag-handle').boundingBox();
  const importChildDragBox = await todayImport.locator('.today-import-row[data-today-import-task-id="child"] .today-import-drag-handle').boundingBox();
  if (!importParentTaskBox || !importChildTaskBox || Math.abs(importChildTaskBox.x - importParentTaskBox.x - 17) > 1) throw new Error("配置今日事项二级事项未按事项弹框规则缩进");
  if (!importParentTitleInputBox || importParentTaskBox.width - importParentTitleInputBox.width > 1) throw new Error("配置今日事项一级事项输入框未铺满事项列：" + JSON.stringify({ importParentTaskBox, importParentTitleInputBox }));
  if (!importChildTitleInputBox || !importChildTaskBox || importChildTitleInputBox.width < importChildTaskBox.width - 24) throw new Error("配置今日事项二级事项输入框未铺满图标后的剩余空间：" + JSON.stringify({ importChildTaskBox, importChildTitleInputBox }));
  if (!importParentDragBox || !importChildDragBox || Math.abs(importChildDragBox.x - importParentDragBox.x - 17) > 1) throw new Error("配置今日事项二级事项拖动柄未随整行缩进");
  await todayImport.getByRole("button", { name: "新增事项" }).click();
  await todayImport.getByRole("textbox", { name: "新增事项名称" }).fill("配置弹框新增事项");
  await todayImport.locator('.today-import-create-bar button', { hasText: "新增" }).click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.create.some((call) => call.title === "配置弹框新增事项" && !call.parentId));
  const createdRootId = (await calls()).create.find((call) => call.title === "配置弹框新增事项").id;
  const createdRootRow = todayImport.locator(`[data-today-import-task-id="${createdRootId}"]`);
  await createdRootRow.waitFor({ state: "visible" });
  if (await createdRootRow.locator('.today-import-check, .today-import-check-spacer').count()) throw new Error("配置弹框新增今日实例后不应显示勾选列");
  if (await todayImport.locator('.timer-task-add-child').count()) throw new Error("配置今日事项仍显示逐行新增子项按钮");
  const existingChildRow = todayImport.locator('[data-today-import-task-id="child"]');
  await existingChildRow.locator('.today-import-drag-handle').dispatchEvent("contextmenu", { clientX: 320, clientY: 180 });
  const rowMenu = page.locator('.task-row-context-menu:visible');
  const menuItems = await rowMenu.getByRole("menuitem").allTextContents();
  if (menuItems.join("|") !== "复制事项|在上方插入行|在下方插入行|删除事项") throw new Error("配置今日事项右键菜单顺序或文案不正确");
  await rowMenu.getByRole("menuitem", { name: "在下方插入行" }).dispatchEvent("click");
  await todayImport.getByRole("textbox", { name: "新增事项名称" }).fill("配置弹框新增子项");
  await todayImport.locator('.today-import-create-bar button', { hasText: "新增" }).click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.create.some((call) => call.title === "配置弹框新增子项" && call.parentId === "parent"));
  const createdChild = (await calls()).create.find((call) => call.title === "配置弹框新增子项");
  const createdChildRow = todayImport.locator(`[data-today-import-task-id="${createdChild.id}"]`);
  await createdChildRow.waitFor({ state: "visible" });
  if (await createdChildRow.getAttribute("data-depth") !== "1") throw new Error("配置弹框新增子项未形成二级层级");
  await createdChildRow.locator('.today-import-drag-handle').dispatchEvent("contextmenu", { clientX: 340, clientY: 180 });
  await page.locator('.task-row-context-menu:visible').getByRole("menuitem", { name: "复制事项" }).dispatchEvent("click");
  await page.waitForFunction((taskId) => window.__TODAY_PLAN_TEST__.calls.create.some((call) => call.previousSiblingId === taskId && call.parentId === "parent"), createdChild.id);
  const createdTitleInput = createdChildRow.getByRole("textbox", { name: /编辑事项名称/ });
  await createdTitleInput.fill("配置弹框子项改名");
  await createdTitleInput.blur();
  await page.waitForFunction((taskId) => window.__TODAY_PLAN_TEST__.calls.update.some((call) => call.id === taskId && call.title === "配置弹框子项改名"), createdChild.id);
  if (await page.locator('.pending-reminder-row[data-task-id="' + createdChild.id + '"] .task-timer-action').count() !== 1) throw new Error("今日事项修改后丢失计时入口");
  await todayImport.getByRole("button", { name: "调整写今日计划今日实际" }).click();
  const actualPopover = page.locator('.el-popper:visible .actual-adjustment-inline').filter({ hasText: "写今日计划 · 今日实际" });
  await actualPopover.first().waitFor({ state: "visible" });
  const actualPopoverCount = await actualPopover.count();
  if (actualPopoverCount !== 1) throw new Error(`配置今日事项点击今日实际时重复打开了调整浮层：${actualPopoverCount}`);
  await actualPopover.getByRole("textbox", { name: "目标实际分钟" }).fill("35");
  await actualPopover.getByRole("button", { name: "确认调整" }).click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.actualAdjustment.some((call) => call.items?.some((item) => item.taskId === "child" && item.targetMinutes === 35)));
  await todayImport.locator('.today-import-row[data-today-import-task-id="child"] .today-import-actual').getByText("35min", { exact: true }).waitFor({ state: "visible" });
  const sourceOrderBeforeMove = await page.evaluate(() => ["long-title", "backlog"].map((id) => window.__TODAY_PLAN_TEST__.activeTask(id).sortOrder));
  await createdChildRow.locator(".today-import-drag-handle").dragTo(createdRootRow, { targetPosition: { x: 64, y: 5 } });
  await page.waitForFunction((taskId) => window.__TODAY_PLAN_TEST__.calls.move.some((call) => call.id === taskId), createdChild.id);
  const importMoveCall = (await calls()).move.findLast((call) => call.id === createdChild.id);
  if (!importMoveCall || importMoveCall.id !== createdChild.id) throw new Error("配置今日事项实例拖动结果异常: " + JSON.stringify(importMoveCall));
  const sourceOrderAfterMove = await page.evaluate(() => ["long-title", "backlog"].map((id) => window.__TODAY_PLAN_TEST__.activeTask(id).sortOrder));
  if (JSON.stringify(sourceOrderAfterMove) !== JSON.stringify(sourceOrderBeforeMove)) throw new Error("配置今日事项拖动不应改变来源事项顺序");
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.locator(".nav-list button", { hasText: "待办" }).click();
  await page.locator(".pending-reminder-row").first().waitFor({ state: "visible" });
  await page.getByRole("button", { name: "配置今日事项" }).click();
  await todayImport.waitFor({ state: "visible" });
  await todayImport.getByRole("button", { name: "从进行中导入", exact: true }).dispatchEvent("click");
  const sourceImportDialog = page.locator(".today-source-import-dialog");
  const sourceImport = sourceImportDialog.locator(".today-source-import-view");
  await sourceImportDialog.waitFor({ state: "visible" });
  await sourceImport.waitFor({ state: "visible" });
  if (!await todayImport.isVisible()) throw new Error("打开来源导入弹框时配置今日事项弹框不应关闭");
  if (await todayImport.locator(".today-source-import-view").count()) throw new Error("来源选择不应继续嵌在配置今日事项弹框内");
  const sourceImportCloseButton = sourceImportDialog.getByRole("button", { name: "Close" });
  if (!await sourceImportCloseButton.isVisible()) throw new Error("来源导入弹框缺少右上角关闭入口");
  await sourceImportCloseButton.click();
  await sourceImportDialog.waitFor({ state: "hidden" });
  if (!await todayImport.isVisible()) throw new Error("右上角关闭来源导入弹框后配置今日事项弹框不应关闭");
  await todayImport.getByRole("button", { name: "从进行中导入", exact: true }).click();
  await sourceImportDialog.waitFor({ state: "visible" });
  if (await sourceImport.getByRole("textbox").count()) throw new Error("从进行中导入不应包含搜索或时间输入");
  if (await sourceImport.locator(".today-import-estimate").count()) throw new Error("从进行中导入不应显示预计时间编辑器");
  if (await sourceImport.getByText("父事项", { exact: true }).count()) throw new Error("从进行中导入仍显示父事项标签");
  await sourceImport.getByRole("button", { name: "全选", exact: true }).click();
  if (!await sourceImport.getByRole("checkbox", { name: "长期待办导入今日计划" }).isChecked()) throw new Error("来源导入弹框未选中事项");
  if (!await sourceImport.getByRole("checkbox", { name: "预计自动加入事项导入今日计划" }).isChecked()) throw new Error("全选未选中全部可导入事项");
  await sourceImport.getByRole("checkbox", { name: "预计自动加入事项导入今日计划" }).uncheck();
  await page.waitForTimeout(100);
  const footerButtons = await sourceImportDialog.locator(".el-dialog__footer button").allTextContents();
  const importAction = sourceImportDialog.locator(".el-dialog__footer button").filter({ hasText: "导入" });
  if (await importAction.count() !== 1) throw new Error("来源导入底部操作未显示：" + JSON.stringify(footerButtons));
  await importAction.click();
  await sourceImportDialog.waitFor({ state: "hidden" });
  await todayImport.locator('[data-today-import-task-id="instance-long-title"]').waitFor({ state: "visible" });
  if (!await todayImport.isVisible()) throw new Error("导入完成后没有返回配置今日事项主弹框");
  if (await todayImport.locator(".today-import-candidate-row").count()) throw new Error("导入后主弹框重新混入来源候选列表");
  const importedEstimate = todayImport.getByRole("textbox", { name: /长期待办今日预计分钟/ });
  if (await importedEstimate.inputValue() !== "15") throw new Error("重复来源的每次预计未自动成为今日预计");
  await todayImport.getByRole("textbox", { name: "写今日计划今日预计分钟" }).fill("45");
  await page.screenshot({ path: path.join(evidenceDir, "02-paper-picker.png"), fullPage: true });
  if ((await page.evaluate(() => window.__TODAY_PLAN_TEST__.activeTask("child").title)) !== "写今日计划") throw new Error("配置今日事项编辑不得回写来源标题");
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.locator(".nav-list button", { hasText: "待办" }).click();
  await page.locator(".pending-reminder-row").first().waitFor({ state: "visible" });

  const tabLabels = await page.getByRole("tab").allTextContents();
  if (!tabLabels.some((label) => label.includes("进行中"))) throw new Error("待办页缺少进行中标签: " + JSON.stringify(tabLabels));
  await page.locator('.plan-task-tabs button').filter({ hasText: "进行中" }).dispatchEvent("click");
  await page.waitForTimeout(200);
  await page.locator(".pending-reminder-row").first().waitFor({ state: "visible" });
  if (await page.getByRole("button", { name: "编辑全部计划" }).count() !== 1) throw new Error("进行中页缺少编辑全部计划: " + JSON.stringify({ selected: await page.locator('.plan-task-tabs [aria-selected="true"] span').allTextContents(), buttons: await page.locator('.task-toolbar button').allTextContents() }));
  await page.getByRole("button", { name: "编辑全部计划" }).click();
  const allQuickPlanDialog = page.locator(".quick-plan-dialog");
  await allQuickPlanDialog.waitFor({ state: "visible" });
  const allQuickPlanEditor = allQuickPlanDialog.locator(".cm-content");
  const allQuickPlanText = await allQuickPlanEditor.innerText();
  if (!allQuickPlanText.includes("长期待办") || !allQuickPlanText.includes("预计自动加入事项")) throw new Error("编辑全部计划未包含完整进行中事项");
  const allQuickPlanLastLine = allQuickPlanEditor.locator(".cm-line").last();
  await allQuickPlanLastLine.click();
  await page.keyboard.press("End");
  await page.keyboard.press("Enter");
  await page.keyboard.type("- 全部计划即时刷新验证");
  await page.waitForFunction(() => document.querySelector(".quick-plan-dialog .cm-content")?.textContent?.includes("全部计划即时刷新验证"));
  await allQuickPlanDialog.getByRole("button", { name: "预览变化", exact: true }).click();
  await allQuickPlanDialog.getByRole("button", { name: "确认保存", exact: true }).click();
  await allQuickPlanDialog.waitFor({ state: "hidden" });
  try {
    await page.waitForFunction(() => [...document.querySelectorAll(".pending-reminder-row input")].some((input) => input.value.includes("全部计划即时刷新验证")), undefined, { timeout: 5_000 });
  } catch {
    throw new Error("编辑全部计划保存后列表未即时更新：" + JSON.stringify({
      request: (await calls()).planBatch.at(-1),
      visibleRows: await page.locator(".pending-reminder-row input").evaluateAll((inputs) => inputs.map((input) => input.value)),
    }));
  }
  if (await page.getByRole("button", { name: "分配今日预计" }).count() !== 0) throw new Error("分配今日预计不应出现在进行中页");
  if (await page.locator(".actual-time-picker").count() !== 0) throw new Error("进行中页不应显示今日用时");
  if (await page.locator(".today-estimate-picker").count() !== 0) throw new Error("进行中页不应显示今日预计");
  const activeParentRow = page.locator('.pending-reminder-row[data-task-id="parent"]');
  const activeRecurringRow = page.locator('.pending-reminder-row[data-task-id="recurring"]');
  if (!await activeRecurringRow.evaluate((element) => element.classList.contains("completed"))) throw new Error("进行中页的今日已完成重复事项未显示完成态");
  const activeRecurringTitleColor = await activeRecurringRow.locator(':scope > .reminder-main > input').evaluate((element) => getComputedStyle(element).color);
  const activeOpenTitleColor = await page.locator('.pending-reminder-row[data-task-id="estimate-auto"] > .reminder-main > input').evaluate((element) => getComputedStyle(element).color);
  if (activeRecurringTitleColor === activeOpenTitleColor) throw new Error("进行中页的今日已完成重复事项未变灰");
  await activeParentRow.dispatchEvent("contextmenu", { clientX: 240, clientY: 240 });
  const activeListContextMenu = page.locator('.plan-task-context-menu:visible').filter({ has: page.locator('button', { hasText: "编辑备注" }) });
  await activeListContextMenu.waitFor({ state: "visible" });
  const activeListMenuItems = await activeListContextMenu.getByRole("menuitem").allTextContents();
  if (activeListMenuItems.join("|") !== "复制事项|在上方新增|在下方新增|新建子事项|编辑备注|删除事项") throw new Error("进行中待办右键菜单顺序或复制文案不正确");
  await page.keyboard.press("Escape");
  if (!await activeParentRow.locator(".check-button").isDisabled()) throw new Error("进行中父事项勾选框应为禁用状态");
  const completedBeforeDisabledClick = (await calls()).completed.length;
  await activeParentRow.locator(".check-button").click({ force: true });
  if ((await calls()).completed.length !== completedBeforeDisabledClick) throw new Error("禁用父事项勾选框错误触发完成操作");
  if (await page.locator('.pending-reminder-row[data-task-id="child"] .add-today-row-action').count() !== 0) throw new Error("已加入今日的事项不应重复显示加入按钮");
  const quickAddRow = page.locator('.pending-reminder-row[data-task-id="long-title"]');
  await quickAddRow.hover();
  const quickAddButton = quickAddRow.locator(".add-today-row-action");
  if (await quickAddButton.count() !== 1) throw new Error("进行中未加入事项缺少快捷加入今日按钮");
  await page.screenshot({ path: path.join(evidenceDir, "06-paper-active-quick-add.png"), fullPage: true });
  await quickAddButton.click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.dayPlanBatch.some((call) => (call.items ?? []).some((item) => item.taskId === "long-title" && item.state === "included")));
  await page.waitForFunction(() => !document.querySelector('.pending-reminder-row[data-task-id="long-title"] .add-today-row-action'));
  if (await page.getByRole("tab", { name: /进行中/ }).getAttribute("aria-selected") !== "true") throw new Error("快捷加入今日后不应离开进行中标签");
  await page.getByRole("tab", { name: /今日/ }).click();
  await page.locator('.pending-reminder-row[data-task-id="instance-long-title"]').waitFor({ state: "visible" });

  const childRow = page.locator('.pending-reminder-row[data-task-id="child"]');
  const titleInput = childRow.locator('input[data-task-input="child"]');
  await titleInput.fill("写今日计划（已改名）");
  await titleInput.blur();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.update.some((call) => call.id === "child" && call.title === "写今日计划（已改名）"));

  await childRow.locator(".note-picker").click();
  await page.locator(".task-note-dialog textarea").fill("今日先完成共享列表改造");
  await page.locator(".task-note-dialog button", { hasText: "保存" }).click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.update.some((call) => call.note === "今日先完成共享列表改造"));

  const noEstimateRow = page.locator('.pending-reminder-row[data-task-id="no-estimate"]');
  await noEstimateRow.locator(".today-estimate-picker").click();
  const estimateInput = noEstimateRow.locator('input[data-today-estimate-input="no-estimate"]');
  await estimateInput.fill("25");
  await estimateInput.blur();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.estimateSet.some((call) => call.taskId === "no-estimate" && call.estimateMinutes === 25));

  await noEstimateRow.locator(".today-estimate-picker").click();
  await estimateInput.fill("0");
  await estimateInput.blur();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.estimateSet.some((call) => call.taskId === "no-estimate" && call.estimateMinutes == null));
  if (await page.locator('.pending-reminder-row[data-task-id="no-estimate"]').count() !== 1) throw new Error("清除预计后今日成员被错误移除");

  await page.getByRole("tab", { name: /进行中/ }).click();
  const estimateAutoRow = page.locator('.pending-reminder-row[data-task-id="estimate-auto"]');
  if (!await estimateAutoRow.getByText("每次预计 40min", { exact: true }).isVisible()) throw new Error("进行中重复事项未显示每次预计");
  if (await estimateAutoRow.locator(".today-estimate-picker").count()) throw new Error("进行中重复事项仍显示今日预计入口");
  await estimateAutoRow.getByRole("button", { name: "加入今日" }).dispatchEvent("click");
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.planIds("subject-a").includes("instance-estimate-auto"));
  await page.getByRole("tab", { name: /今日/ }).click();
  await page.locator('.pending-reminder-row[data-task-id="instance-estimate-auto"]').waitFor({ state: "visible" });
  if (!await page.locator('.pending-reminder-row[data-task-id="instance-estimate-auto"] .today-estimate-picker').getByText("今日预计 40min", { exact: true }).isVisible()) throw new Error("快捷加入后未使用每次预计初始化今日预计");

  // 今日页拖动排序复用进行中的整组拖动：把今日成员“无预计事项”拖到同父的“写今日计划”之前，
  // 断言落点被解析为对完整计划的结构化写入（父项 + 下一个同级），而不是只改动过滤后的视图。
  const listBox = await page.locator(".reminder-list").first().boundingBox();
  const dragHandleBox = await noEstimateRow.locator(".drag-handle").boundingBox();
  const dropTargetBox = await childRow.boundingBox();
  await page.mouse.move(dragHandleBox.x + dragHandleBox.width / 2, dragHandleBox.y + dragHandleBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(dragHandleBox.x + dragHandleBox.width / 2, dragHandleBox.y - 24, { steps: 12 });
  await page.mouse.move(listBox.x + 260, dropTargetBox.y + 6, { steps: 24 });
  await page.mouse.up();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.move.length > 0);
  const moveCall = (await calls()).move.at(-1);
  if (moveCall.id !== "no-estimate" || moveCall.parentId !== "parent" || moveCall.nextSiblingId !== "child" || moveCall.previousSiblingId) {
    throw new Error("今日页拖动结果与预期结构写入不一致: " + JSON.stringify(moveCall));
  }
  await page.waitForFunction(() => {
    const ids = [...document.querySelectorAll(".reminder-list .pending-reminder-row")].map((row) => row.dataset.taskId);
    return ids.indexOf("no-estimate") >= 0 && ids.indexOf("no-estimate") < ids.indexOf("child");
  });

  await noEstimateRow.locator(".check-button").click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.completed.some((call) => call.id === "no-estimate" && call.done === true));
  await page.locator('.pending-reminder-row[data-task-id="no-estimate"].completed').waitFor({ state: "visible" });
  await page.locator('.pending-reminder-row[data-task-id="no-estimate"] .check-button').click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.completed.some((call) => call.id === "no-estimate" && call.done === false));

  const recurringRow = page.locator('.pending-reminder-row[data-task-id="recurring"]');
  await recurringRow.locator(".check-button").click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.completed.some((call) => call.id === "recurring" && call.done === false));
  await page.waitForFunction(() => !document.querySelector('.pending-reminder-row[data-task-id="recurring"].completed'));
  await page.locator('.pending-reminder-row[data-task-id="recurring"] .check-button').click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.completed.some((call) => call.id === "recurring" && call.done === true));
  await page.locator('.pending-reminder-row[data-task-id="recurring"].completed').waitFor({ state: "visible" });
  await page.locator('.pending-reminder-row[data-task-id="recurring"] button[title="移出今日"]').click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.dayPlanSet.some((call) => call.taskId === "recurring" && call.state === "excluded"));
  await page.waitForFunction(() => document.querySelector('.pending-reminder-row[data-task-id="recurring"]') === null);
  await page.getByRole("tab", { name: /进行中/ }).click();
  const recurringQuickAdd = page.locator('.pending-reminder-row[data-task-id="recurring"] .add-today-row-action');
  await recurringQuickAdd.waitFor({ state: "visible" });
  await recurringQuickAdd.dispatchEvent("click");
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.dayPlanBatch.some((call) =>
    (call.items ?? []).some((item) => item.taskId === "recurring" && item.state === "included" && item.expectedVersion === 2)));
  await page.getByRole("tab", { name: /今日/ }).click();
  await page.locator('.pending-reminder-row[data-task-id="instance-recurring"]').waitFor({ state: "visible" });

  await childRow.locator('button[title="移出今日"]').click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.dayPlanSet.some((call) => call.taskId === "child" && call.state === "excluded"));
  await page.waitForFunction(() => document.querySelector('.pending-reminder-row[data-task-id="child"]') === null);

  await page.locator(".plan-placeholder-row").click();
  const newInput = page.locator('.pending-reminder-row input[data-task-input^="task-"]').last();
  await newInput.waitFor({ state: "visible" });
  await newInput.fill("今天新增的事项");
  await newInput.blur();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.dayPlanBatch.some((call) => (call.items ?? []).some((item) => item.state === "included")));

  await page.evaluate(() => {
    window.__TODAY_PLAN_TEST__.setWorkDate("2026-10-20");
    window.__TODAY_PLAN_TEST__.clearPlan("subject-a");
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.locator(".nav-list button", { hasText: "待办" }).click();
  await page.waitForFunction(() => document.querySelectorAll(".pending-reminder-row").length === 0);
  if (await page.locator(".plan-placeholder-row").count() !== 1) throw new Error("跨日空计划缺少底部新增入口");

  await page.evaluate(() => { window.__TODAY_PLAN_TEST__.setWorkDate("2026-10-19"); });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.locator(".nav-list button", { hasText: "待办" }).click();
  await page.locator(".pending-reminder-row").first().waitFor({ state: "visible" });

  await page.setViewportSize({ width: 720, height: 760 });
  const list = page.locator(".reminder-list").first();
  if (await list.evaluate((element) => element.scrollWidth > element.clientWidth + 1)) throw new Error("窄窗口下今日列表出现水平溢出");
  await page.getByRole("button", { name: "配置今日事项" }).click();
  const narrowImport = page.locator(".today-import-dialog");
  await narrowImport.waitFor({ state: "visible" });
  await page.waitForTimeout(300);
  const narrowImportBounds = await narrowImport.boundingBox();
  if (!narrowImportBounds || narrowImportBounds.x < 0 || narrowImportBounds.x + narrowImportBounds.width > 720) throw new Error("窄窗口下配置今日事项弹框超出视口");
  if (Math.abs(narrowImportBounds.y + narrowImportBounds.height / 2 - 380) > 3) throw new Error("窄窗口下配置今日事项弹框未垂直居中");
  const narrowImportBodyOverflow = await narrowImport.locator(".el-dialog__body").evaluate((element) => ({ overflowY: getComputedStyle(element).overflowY, scrollHeight: element.scrollHeight, clientHeight: element.clientHeight }));
  if (narrowImportBodyOverflow.overflowY !== "hidden" || narrowImportBodyOverflow.scrollHeight > narrowImportBodyOverflow.clientHeight + 1) throw new Error("窄窗口下配置今日事项弹框出现外层滚动：" + JSON.stringify(narrowImportBodyOverflow));
  if ((await narrowImport.locator(".today-import-list").evaluateAll((elements) => elements.some((element) => element.scrollWidth > element.clientWidth + 1)))) throw new Error("窄窗口下配置今日事项列表出现水平溢出");
  await narrowImport.getByRole("button", { name: "从进行中导入", exact: true }).click();
  const narrowSourceDialog = page.locator(".today-source-import-dialog");
  const narrowSourceImport = narrowSourceDialog.locator(".today-source-import-view");
  await narrowSourceDialog.waitFor({ state: "visible" });
  await narrowSourceImport.waitFor({ state: "visible" });
  await page.waitForTimeout(300);
  const narrowSourceBounds = await narrowSourceDialog.boundingBox();
  if (!narrowSourceBounds || narrowSourceBounds.x < 0 || narrowSourceBounds.x + narrowSourceBounds.width > 720) throw new Error("窄窗口下来源导入弹框超出视口");
  if ((await narrowSourceImport.locator(".today-import-list").evaluate((element) => element.scrollWidth > element.clientWidth + 1))) throw new Error("窄窗口下来源导入列表出现水平溢出");
  await page.screenshot({ path: path.join(evidenceDir, "07-paper-import-narrow.png"), fullPage: true });
  await narrowSourceDialog.getByRole("button", { name: "取消", exact: true }).click();
  await narrowSourceDialog.waitFor({ state: "hidden" });
  if (!await narrowImport.isVisible()) throw new Error("关闭来源导入弹框后配置今日事项弹框不应关闭");
  await narrowImport.getByRole("button", { name: "取消" }).click();
  await page.screenshot({ path: path.join(evidenceDir, "03-paper-narrow.png"), fullPage: true });

  await page.setViewportSize({ width: 1280, height: 900 });
  await page.evaluate(() => { document.documentElement.dataset.theme = "forest"; document.documentElement.style.colorScheme = "dark"; });
  await page.screenshot({ path: path.join(evidenceDir, "04-forest-wide.png"), fullPage: true });
  await verifyQuickPlanTextImport(page, calls);
  if (pageErrors.length) throw new Error("页面错误: " + pageErrors.join("；"));
  await browser.close();
  console.log("today plan shared editor UI checks passed");
}

try {
  await mainSharedPlanEditor();
} finally {
  if (process.platform === "win32") {
    const terminator = spawn("taskkill.exe", ["/pid", String(server.pid), "/t", "/f"], { stdio: "ignore" });
    await new Promise((resolve) => terminator.once("exit", resolve));
  } else {
    server.kill("SIGTERM");
  }
}
