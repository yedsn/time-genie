import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";
import path from "node:path";
import { chromium } from "playwright";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const port = 5203;
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
    record("estimate-auto", { title: "预计自动加入事项", sortOrder: 35 }),
    record("recurring", { title: "每日复盘", sortOrder: 38, recurrence: { id: "rule-recurring", taskId: "recurring", frequency: "daily", effectiveStart: "2026-10-19", summary: "每天", version: 1 } }),
    record("long-title", { title: "梳理一份足够长的事项标题用于检查今日计划列表在窄窗口下是否会溢出容器边界", sortOrder: 40 }),
    record("backlog", { title: "长期待办", sortOrder: 50 }),
    record("plain-done", { title: "已完成普通事项", status: "done", executionState: "done", version: 2, sortOrder: 60 }),
    record("personal", { subjectId: "subject-b", title: "阅读", projectName: "个人", sortOrder: 10 }),
  ];

  const planRows = {
    "subject-a": [
      record("parent", { title: "项目", selectable: false, isLeaf: false, canAcceptChildren: true, todayMember: false, todayContext: true, dayPlanVersion: 1, sortOrder: 10 }),
      record("child", { title: "写今日计划", parentId: "parent", pathLabel: "项目 / 写今日计划", todayMember: true, todayContext: false, dayPlanVersion: 1, todayEstimateMinutes: 30, directMinutes: 20, totalMinutes: 20, sortOrder: 20 }),
      record("no-estimate", { title: "无预计事项", parentId: "parent", pathLabel: "项目 / 无预计事项", todayMember: true, todayContext: false, dayPlanVersion: 1, sortOrder: 30 }),
      record("recurring", { title: "每日复盘", status: "done", executionState: "done", completedToday: true, todayMember: true, todayContext: false, dayPlanVersion: 1, sortOrder: 35, recurrence: { id: "rule-recurring", taskId: "recurring", frequency: "daily", effectiveStart: "2026-10-19", summary: "每天", version: 1 }, occurrenceDate: "2026-10-19", occurrenceOrigin: "scheduled", occurrenceVersion: 1 }),
      record("done-today", { title: "今天已完成的成员", status: "done", executionState: "done", completedToday: true, version: 2, todayMember: true, todayContext: false, dayPlanVersion: 1, directMinutes: 45, totalMinutes: 45, sortOrder: 40 }),
    ],
    "subject-b": [
      record("personal", { subjectId: "subject-b", title: "阅读", projectName: "个人", todayMember: true, todayContext: false, dayPlanVersion: 1, sortOrder: 10 }),
    ],
  };
  const moveToFront = (rows, taskId) => {
    const index = rows.findIndex((row) => row.id === taskId);
    if (index > 0) rows.unshift(rows.splice(index, 1)[0]);
  };
  moveToFront(activeTasks, "recurring");
  moveToFront(planRows["subject-a"], "recurring");

  const calls = { dayPlanSet: [], dayPlanBatch: [], dayPlanSnapshot: 0, estimateSet: [], update: [], completed: [], create: [], move: [], actualAdjustment: [], cloudCatchUp: 0 };
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
    clearPlan(subjectId) { planRows[subjectId]?.splice(0); },
    setWorkDate(workDate) { currentWorkDate = workDate; },
    planIds(subjectId) { return (planRows[subjectId] ?? []).map((row) => row.id); },
    planRowFor,
    activeTask(id) { return activeTasks.find((task) => task.id === id); },
  };

  let currentWorkDate = "2026-10-19";
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
        case "work_calendar_get_context":
          return { timezone: "Asia/Shanghai", currentWorkDate, currentDayStartAt: Date.now() - 3_600_000, currentDayEndAt: Date.now() + 86_400_000 };
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
        case "task_day_plan_batch_set": {
          calls.dayPlanBatch.push({ ...args.request });
          if (failDayPlanBatchOnce) {
            failDayPlanBatchOnce = false;
            throw new Error("模拟加入今日失败");
          }
          const rows = planRows[args.request.subjectId] ??= [];
          for (const item of args.request.items ?? []) {
            if (rows.some((row) => row.id === item.taskId)) continue;
            const source = activeTasks.find((task) => task.id === item.taskId);
            if (!source) continue;
            rows.push({ ...source, todayMember: true, todayContext: false, dayPlanVersion: 1 });
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
            if (!source) continue;
            let row = planRowFor(source.subjectId, item.taskId);
            if (!row && item.estimateMinutes) {
              row = { ...source, todayMember: true, todayContext: false, dayPlanVersion: 1 };
              planRows[source.subjectId].push(row);
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
          return { ...(active ?? {}), ...(planRowFor("subject-a", args.request.id) ?? {}) };
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
          activeTasks.push(row);
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
          activeTasks.push(row);
          return { ...row };
        }
        case "actual_adjustment_snapshot":
          return {
            subjectId: args.request.subjectId,
            workDate: args.request.workDate,
            tasks: activeTasks
              .filter((task) => task.subjectId === args.request.subjectId && task.status === "open")
              .map((task) => ({
                ...task,
                isLeaf: !activeTasks.some((candidate) => candidate.parentId === task.id),
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
  await page.locator(".nav-list button", { hasText: "待办" }).click();
  await page.locator(".pending-reminder-row").first().waitFor({ state: "visible" });

  if (await page.locator(".pending-reminder-row").count() !== 5) throw new Error("今日计划行数量异常");
  const snapshotCallsBeforeRefresh = (await calls()).dayPlanSnapshot;
  await page.evaluate(() => window.__TODAY_PLAN_TEST__.delayNextSnapshot(450));
  await page.getByRole("tab", { name: /今日/ }).click();
  await page.getByRole("tab", { name: /今日/ }).click();
  await page.waitForTimeout(120);
  if (await page.locator(".today-plan-skeleton").count() !== 0) throw new Error("今日计划后台刷新仍用骨架替换已有列表");
  if (await page.locator(".pending-reminder-row").count() !== 5) throw new Error("今日计划后台刷新未保持已有列表稳定");
  if (await page.locator(".today-plan-loading-line, .today-plan-refresh-label, .today-plan-refreshing, .today-plan-list-refreshing").count() !== 0) throw new Error("今日计划刷新仍显示横条、提示或旧列表弱化效果");
  if (await page.getByText("正在更新", { exact: true }).count() !== 0 || await page.getByText("正在整理今日计划", { exact: true }).count() !== 0) throw new Error("今日计划仍显示额外加载提示");
  await page.screenshot({ path: path.join(evidenceDir, "08-paper-today-refreshing.png"), fullPage: true });
  await page.waitForTimeout(450);
  if ((await calls()).dayPlanSnapshot !== snapshotCallsBeforeRefresh + 1) throw new Error("同一主体和日期的重复今日计划请求未合并");

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
  const listBounds = await page.locator(".reminder-list").boundingBox();
  const firstRecurringBounds = await firstTodayRow.boundingBox();
  if (!listBounds || !firstRecurringBounds || firstRecurringBounds.x - listBounds.x < 7 || listBounds.x + listBounds.width - firstRecurringBounds.x - firstRecurringBounds.width < 7) throw new Error("已完成首个重复事项卡片缺少左右外边距");
  if (await page.locator(".pending-reminder-row .drag-handle").count() !== 5) throw new Error("今日页未复用进行中的拖动入口");
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
  if (await page.locator(".pending-reminder-row .check-button").count() !== 5) throw new Error("今日页事项勾选框未保持统一对齐");
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
  if (todayListMenuItems.join("|") !== "复制事项|在上方新增|在下方新增|新建子事项|编辑备注|删除") throw new Error(`今日待办右键菜单顺序或复制文案不正确：${todayListMenuItems.join("|")}`);
  await page.keyboard.press("Escape");
  await page.screenshot({ path: path.join(evidenceDir, "01-paper-wide.png"), fullPage: true });

  await page.getByRole("button", { name: "编辑今日计划" }).click();
  await page.locator(".quick-plan-dialog").waitFor({ state: "visible" });
  const todayQuickPlanText = await page.locator(".quick-plan-dialog .cm-content").innerText();
  if (!todayQuickPlanText.includes("写今日计划") || !todayQuickPlanText.includes("无预计事项")) throw new Error("编辑今日计划未包含今日事项");
  if (todayQuickPlanText.includes("长期待办") || todayQuickPlanText.includes("预计自动加入事项")) throw new Error("编辑今日计划混入了非今日事项");
  if (todayQuickPlanText.includes("每日复盘") || todayQuickPlanText.includes("今天已完成的成员")) throw new Error("编辑今日计划不应混入已完成今日事项");
  await page.locator(".quick-plan-dialog button", { hasText: "取消" }).click();

  await page.getByRole("button", { name: "配置今日事项" }).click();
  const todayImport = page.locator(".today-import-dialog");
  await todayImport.waitFor({ state: "visible" });
  await page.waitForTimeout(300);
  const todayImportBounds = await todayImport.boundingBox();
  if (!todayImportBounds || Math.abs(todayImportBounds.y + todayImportBounds.height / 2 - 450) > 3) throw new Error("配置今日事项弹框未在窗口内垂直居中: " + JSON.stringify(todayImportBounds));
  const importOverview = todayImport.locator(".today-import-overview");
  if (!await importOverview.getByText("1h5min", { exact: true }).isVisible() || !await importOverview.getByText("30min", { exact: true }).isVisible()) throw new Error("配置今日事项顶部时长统计异常");
  if (await todayImport.locator('.today-import-row[data-unused="never"]').count() !== 0) throw new Error("导入弹框状态异常");
  if (!await todayImport.getByRole("checkbox", { name: "写今日计划加入今日" }).isChecked() || !await todayImport.getByRole("checkbox", { name: "写今日计划加入今日" }).isDisabled()) throw new Error("已在今日事项应保持勾选锁定");
  const importParentTaskBox = await todayImport.locator('.today-import-row[data-today-import-task-id="parent"] .today-import-title-line').boundingBox();
  const importChildTaskBox = await todayImport.locator('.today-import-row[data-today-import-task-id="child"] .today-import-title-line').boundingBox();
  const importParentDragBox = await todayImport.locator('.today-import-row[data-today-import-task-id="parent"] .today-import-drag-handle').boundingBox();
  const importChildDragBox = await todayImport.locator('.today-import-row[data-today-import-task-id="child"] .today-import-drag-handle').boundingBox();
  const importParentCheckBox = await todayImport.locator('.today-import-row[data-today-import-task-id="parent"] .today-import-check-spacer').boundingBox();
  const importChildCheckBox = await todayImport.locator('.today-import-row[data-today-import-task-id="child"] .today-import-check').boundingBox();
  if (!importParentTaskBox || !importChildTaskBox || Math.abs(importChildTaskBox.x - importParentTaskBox.x - 17) > 1) throw new Error("配置今日事项二级事项未按事项弹框规则缩进");
  if (!importParentDragBox || !importChildDragBox || Math.abs(importChildDragBox.x - importParentDragBox.x - 17) > 1) throw new Error("配置今日事项二级事项拖动柄未随整行缩进");
  if (!importParentCheckBox || !importChildCheckBox || Math.abs(importChildCheckBox.x - importParentCheckBox.x - 17) > 1) throw new Error("配置今日事项二级事项选择框未随整行缩进");

  await todayImport.getByRole("button", { name: "新增事项" }).click();
  await todayImport.getByRole("textbox", { name: "新增事项名称" }).fill("配置弹框新增事项");
  await todayImport.locator('.today-import-create-bar button', { hasText: "新增" }).click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.create.some((call) => call.title === "配置弹框新增事项" && !call.parentId));
  const createdRootId = (await calls()).create.find((call) => call.title === "配置弹框新增事项").id;
  const createdRootRow = todayImport.locator(`[data-today-import-task-id="${createdRootId}"]`);
  await createdRootRow.waitFor({ state: "visible" });
  if (!await createdRootRow.locator('.today-import-check').isChecked()) throw new Error("配置弹框新增事项后未自动选中");
  if (await todayImport.locator('.timer-task-add-child').count()) throw new Error("配置今日事项仍显示逐行新增子项按钮");
  const existingChildRow = todayImport.locator('[data-today-import-task-id="child"]');
  await existingChildRow.locator('.today-import-drag-handle').click({ button: "right" });
  const rowMenu = page.locator('.task-row-context-menu:visible');
  const menuItems = await rowMenu.getByRole("menuitem").allTextContents();
  if (menuItems.join("|") !== "复制事项|在上方插入行|在下方插入行") throw new Error("配置今日事项右键菜单顺序或文案不正确");
  await rowMenu.getByRole("menuitem", { name: "在下方插入行" }).click();
  await todayImport.getByRole("textbox", { name: "新增事项名称" }).fill("配置弹框新增子项");
  await todayImport.locator('.today-import-create-bar button', { hasText: "新增" }).click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.create.some((call) => call.title === "配置弹框新增子项" && call.parentId === "parent"));
  const createdChild = (await calls()).create.find((call) => call.title === "配置弹框新增子项");
  const createdChildRow = todayImport.locator(`[data-today-import-task-id="${createdChild.id}"]`);
  await createdChildRow.waitFor({ state: "visible" });
  if (await createdChildRow.getAttribute("data-depth") !== "1") throw new Error("配置弹框新增子项未形成二级层级");
  await createdChildRow.locator('.today-import-drag-handle').click({ button: "right" });
  await page.locator('.task-row-context-menu:visible').getByRole("menuitem", { name: "复制事项" }).click();
  await page.waitForFunction((taskId) => window.__TODAY_PLAN_TEST__.calls.create.some((call) => call.previousSiblingId === taskId && call.parentId === "parent"), createdChild.id);
  const createdTitleInput = createdChildRow.getByRole("textbox", { name: /编辑事项名称/ });
  await createdTitleInput.fill("配置弹框子项改名");
  await createdTitleInput.blur();
  await page.waitForFunction((taskId) => window.__TODAY_PLAN_TEST__.calls.update.some((call) => call.id === taskId && call.title === "配置弹框子项改名"), createdChild.id);
  await todayImport.getByRole("button", { name: "调整写今日计划今日实际" }).click();
  const actualPopover = page.locator('.el-popper:visible .actual-adjustment-inline').filter({ hasText: "写今日计划 · 今日实际" });
  await actualPopover.first().waitFor({ state: "visible" });
  const actualPopoverCount = await actualPopover.count();
  if (actualPopoverCount !== 1) throw new Error(`配置今日事项点击今日实际时重复打开了调整浮层：${actualPopoverCount}`);
  await actualPopover.getByRole("textbox", { name: "目标实际分钟" }).fill("35");
  await actualPopover.getByRole("button", { name: "确认调整" }).click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.actualAdjustment.some((call) => call.items?.some((item) => item.taskId === "child" && item.targetMinutes === 35)));
  if ((await calls()).cloudCatchUp < 2) throw new Error("今日实际调整未在打开和保存前追平云端增量");
  const importBacklogRow = todayImport.locator('.today-import-row[data-today-import-task-id="backlog"]');
  const importLongTitleRow = todayImport.locator('.today-import-row[data-today-import-task-id="long-title"]');
  if (await importBacklogRow.locator(".today-import-drag-handle").count() !== 1) throw new Error("配置今日事项缺少拖动排序入口");
  await importLongTitleRow.locator(".today-import-drag-handle").dragTo(importBacklogRow, { targetPosition: { x: 64, y: 5 } });
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.move.some((call) => call.id === "long-title"));
  const importMoveCall = (await calls()).move.findLast((call) => call.id === "long-title");
  if (!importMoveCall || importMoveCall.nextSiblingId !== "backlog" || importMoveCall.parentId) throw new Error("配置今日事项拖动结果异常: " + JSON.stringify(importMoveCall));
  await todayImport.getByRole("checkbox", { name: "长期待办加入今日" }).check();
  await page.evaluate(() => window.__TODAY_PLAN_TEST__.failNextDayPlanBatch());
  await todayImport.getByRole("button", { name: /保存配置/ }).click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.dayPlanBatch.some((call) => (call.items ?? []).some((item) => item.taskId === "backlog")));
  if (!await todayImport.isVisible() || !await todayImport.getByRole("checkbox", { name: "长期待办加入今日" }).isChecked()) throw new Error("配置失败后弹框或选择草稿未保留");
  const presetInput = todayImport.getByRole("textbox", { name: /预计自动加入事项今日预计分钟/ });
  await presetInput.click();
  const presetMenu = page.locator(".today-import-preset-popper:visible");
  await presetMenu.waitFor({ state: "visible" });
  const presetLabels = await presetMenu.locator(".el-dropdown-menu__item").allTextContents();
  if (JSON.stringify(presetLabels) !== JSON.stringify(["30 分钟", "1 小时", "1 小时 30 分钟", "2 小时", "2 小时 30 分钟"])) throw new Error("今日预计常用时长选项异常: " + JSON.stringify(presetLabels));
  await presetMenu.getByText("1 小时 30 分钟", { exact: true }).click();
  if (await presetInput.inputValue() !== "90") throw new Error("选择常用时长后未写入 90 分钟");
  if (!await todayImport.getByRole("checkbox", { name: "预计自动加入事项加入今日" }).isChecked()) throw new Error("选择常用时长后未自动选中事项");
  if (!await importOverview.getByText("2h", { exact: true }).isVisible()) throw new Error("配置今日事项预计总时长未随草稿即时更新");
  await todayImport.getByRole("button", { name: "预计自动加入事项增加 15 分钟" }).click();
  if (await presetInput.inputValue() !== "105") throw new Error("今日预计加号未增加 15 分钟");
  await todayImport.getByRole("button", { name: "预计自动加入事项减少 15 分钟" }).click();
  if (await presetInput.inputValue() !== "90") throw new Error("今日预计减号未减少 15 分钟");
  await todayImport.getByRole("textbox", { name: "写今日计划今日预计分钟" }).fill("45");
  await presetInput.fill("40");
  await todayImport.getByPlaceholder("搜索事项或父级路径").fill("长期待办");
  await page.waitForTimeout(100);
  const filteredImportTitles = await todayImport.locator(".today-import-title-line > input").evaluateAll((inputs) => inputs.map((input) => input.value));
  if (filteredImportTitles.length !== 1 || filteredImportTitles[0] !== "长期待办") throw new Error("配置今日事项搜索结果异常: " + JSON.stringify(filteredImportTitles));
  await todayImport.getByPlaceholder("搜索事项或父级路径").fill("");
  await page.screenshot({ path: path.join(evidenceDir, "02-paper-picker.png"), fullPage: true });
  await todayImport.getByRole("button", { name: /保存配置/ }).click();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.estimateSet.some((call) => call.taskId === "child" && call.estimateMinutes === 45));
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.estimateSet.some((call) => call.taskId === "estimate-auto" && call.estimateMinutes === 40));
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.dayPlanBatch.some((call) => (call.items ?? []).some((item) => item.taskId === "backlog" && item.state === "included")));
  await todayImport.waitFor({ state: "hidden" });
  await page.locator('.pending-reminder-row[data-task-id="backlog"]').waitFor({ state: "visible" });
  await page.locator('.pending-reminder-row[data-task-id="estimate-auto"]').waitFor({ state: "visible" });

  await page.getByRole("tab", { name: /进行中/ }).click();
  await page.locator(".pending-reminder-row").first().waitFor({ state: "visible" });
  if (await page.getByRole("button", { name: "编辑全部计划" }).count() !== 1) throw new Error("进行中页缺少编辑全部计划");
  await page.getByRole("button", { name: "编辑全部计划" }).click();
  await page.locator(".quick-plan-dialog").waitFor({ state: "visible" });
  const allQuickPlanText = await page.locator(".quick-plan-dialog .cm-content").innerText();
  if (!allQuickPlanText.includes("长期待办") || !allQuickPlanText.includes("预计自动加入事项")) throw new Error("编辑全部计划未包含完整进行中事项");
  await page.locator(".quick-plan-dialog button", { hasText: "取消" }).click();
  if (await page.getByRole("button", { name: "分配今日预计" }).count() !== 0) throw new Error("分配今日预计不应出现在进行中页");
  if (await page.locator(".actual-time-picker").count() !== 0) throw new Error("进行中页不应显示今日用时");
  const activeParentRow = page.locator('.pending-reminder-row[data-task-id="parent"]');
  await activeParentRow.dispatchEvent("contextmenu", { clientX: 240, clientY: 240 });
  const activeListContextMenu = page.locator('.plan-task-context-menu:visible').filter({ has: page.locator('button', { hasText: "编辑备注" }) });
  await activeListContextMenu.waitFor({ state: "visible" });
  const activeListMenuItems = await activeListContextMenu.getByRole("menuitem").allTextContents();
  if (activeListMenuItems.join("|") !== "复制事项|在上方新增|在下方新增|新建子事项|编辑备注|删除") throw new Error("进行中待办右键菜单顺序或复制文案不正确");
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
  await page.locator('.pending-reminder-row[data-task-id="long-title"]').waitFor({ state: "visible" });

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
  await estimateAutoRow.locator(".today-estimate-picker").click();
  const estimateAutoInput = estimateAutoRow.locator('input[data-today-estimate-input="estimate-auto"]');
  await estimateAutoInput.fill("40");
  await estimateAutoInput.blur();
  await page.waitForFunction(() => window.__TODAY_PLAN_TEST__.calls.estimateSet.some((call) => call.taskId === "estimate-auto" && call.estimateMinutes === 40));
  await page.getByRole("tab", { name: /今日/ }).click();
  await page.locator('.pending-reminder-row[data-task-id="estimate-auto"]').waitFor({ state: "visible" });

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
  await page.getByRole("tab", { name: /进行中/ }).click();
  await page.getByRole("tab", { name: /今日/ }).click();
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
  if (await narrowImport.locator(".today-import-list").evaluate((element) => element.scrollWidth > element.clientWidth + 1)) throw new Error("窄窗口下配置今日事项列表出现水平溢出");
  await page.screenshot({ path: path.join(evidenceDir, "07-paper-import-narrow.png"), fullPage: true });
  await narrowImport.getByRole("button", { name: "取消" }).click();
  await page.screenshot({ path: path.join(evidenceDir, "03-paper-narrow.png"), fullPage: true });

  await page.setViewportSize({ width: 1280, height: 900 });
  await page.evaluate(() => { document.documentElement.dataset.theme = "forest"; document.documentElement.style.colorScheme = "dark"; });
  await page.screenshot({ path: path.join(evidenceDir, "04-forest-wide.png"), fullPage: true });
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
