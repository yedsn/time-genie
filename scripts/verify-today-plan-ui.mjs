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
      record("child", { title: "写今日计划", parentId: "parent", pathLabel: "项目 / 写今日计划", todayMember: true, todayContext: false, dayPlanVersion: 1, todayEstimateMinutes: 30, sortOrder: 20 }),
      record("no-estimate", { title: "无预计事项", parentId: "parent", pathLabel: "项目 / 无预计事项", todayMember: true, todayContext: false, dayPlanVersion: 1, sortOrder: 30 }),
      record("recurring", { title: "每日复盘", status: "done", executionState: "done", completedToday: true, todayMember: true, todayContext: false, dayPlanVersion: 1, sortOrder: 35, recurrence: { id: "rule-recurring", taskId: "recurring", frequency: "daily", effectiveStart: "2026-10-19", summary: "每天", version: 1 }, occurrenceDate: "2026-10-19", occurrenceOrigin: "scheduled", occurrenceVersion: 1 }),
      record("done-today", { title: "今天已完成的成员", status: "done", executionState: "done", completedToday: true, version: 2, todayMember: true, todayContext: false, dayPlanVersion: 1, totalMinutes: 45, sortOrder: 40 }),
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

  const calls = { dayPlanSet: [], dayPlanBatch: [], estimateSet: [], update: [], completed: [], create: [], move: [] };
  let failDayPlanBatchOnce = false;
  let failSnapshotFor = "";
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
            title: args.request.title,
            projectName: "工作",
            pathLabel: args.request.title,
            sortOrder: 90 + created,
          });
          activeTasks.push(row);
          return { ...row };
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
  if (await page.getByRole("button", { name: "快速编辑计划" }).count() !== 1) throw new Error("今日页缺少快速编辑计划");
  if (await page.getByRole("button", { name: "分配今日预计" }).count() !== 1) throw new Error("今日页缺少分配今日预计");
  const contextRow = page.locator('.pending-reminder-row[data-task-id="parent"]');
  if (await contextRow.locator(".task-timer-action").count() !== 0) throw new Error("上下文父事项不应可执行");
  if (await contextRow.locator(".check-button").count() !== 1 || !await contextRow.locator(".check-button").isDisabled()) throw new Error("今日页父事项应保留禁用勾选框");
  if (await page.locator(".pending-reminder-row .check-button").count() !== 5) throw new Error("今日页事项勾选框未保持统一对齐");
  if (await page.locator(".actual-time-picker").count() === 0) throw new Error("今日页应继续显示今日用时");
  const todayParentCheckBox = await contextRow.locator(".check-button").boundingBox();
  const todayRecurringCheckBox = await page.locator('.pending-reminder-row[data-task-id="recurring"] .check-button').boundingBox();
  if (!todayParentCheckBox || !todayRecurringCheckBox || Math.abs(todayParentCheckBox.x - todayRecurringCheckBox.x) > 1) throw new Error("今日页父事项与重复事项勾选框未对齐");
  await page.screenshot({ path: path.join(evidenceDir, "01-paper-wide.png"), fullPage: true });

  await page.getByRole("button", { name: "快速编辑计划" }).click();
  await page.locator(".quick-plan-dialog").waitFor({ state: "visible" });
  await page.locator(".quick-plan-dialog button", { hasText: "取消" }).click();

  await page.getByRole("tab", { name: /进行中/ }).click();
  await page.locator(".pending-reminder-row").first().waitFor({ state: "visible" });
  if (await page.getByRole("button", { name: "快速编辑计划" }).count() !== 1) throw new Error("进行中页缺少快速编辑计划");
  if (await page.getByRole("button", { name: "分配今日预计" }).count() !== 0) throw new Error("分配今日预计不应出现在进行中页");
  if (await page.locator(".actual-time-picker").count() !== 0) throw new Error("进行中页不应显示今日用时");
  const activeParentRow = page.locator('.pending-reminder-row[data-task-id="parent"]');
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

  await page.getByRole("button", { name: "从进行中加入" }).click();
  const picker = page.locator(".el-dialog", { hasText: "从进行中加入今日" });
  await picker.locator("label", { hasText: "长期待办" }).locator("input").check();
  await page.screenshot({ path: path.join(evidenceDir, "02-paper-picker.png"), fullPage: true });
  await picker.locator("button", { hasText: "加入今日" }).click();
  await page.waitForFunction(() => document.querySelector('.pending-reminder-row[data-task-id="backlog"]') !== null);
  await picker.waitFor({ state: "hidden" });

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
