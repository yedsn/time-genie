import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { chromium } from "playwright";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const port = 5204;
const baseUrl = "http://127.0.0.1:" + port + "/";
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
    { id: "subject-b", name: "个人", sortOrder: 2, openTaskCount: 0, version: 1 },
  ];
  const workDate = "2026-10-19";
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
    record("parent", { title: "项目", selectable: false, isLeaf: false, canAcceptChildren: true }),
    record("child", { title: "写今日计划", parentId: "parent", pathLabel: "项目 / 写今日计划", sortOrder: 20 }),
    record("backlog", { title: "跨日继续的长期待办", sortOrder: 30 }),
    record("plain-done", { title: "已完成普通事项", status: "done", executionState: "done", version: 2, sortOrder: 40 }),
  ];
  const planRows = {
    "subject-a": [
      record("parent", { title: "项目", selectable: false, isLeaf: false, canAcceptChildren: true, todayMember: false, todayContext: true, dayPlanVersion: 1 }),
      record("child", { title: "写今日计划", parentId: "parent", pathLabel: "项目 / 写今日计划", todayMember: true, todayContext: false, dayPlanVersion: 1, todayEstimateMinutes: 30, sortOrder: 20 }),
      record("done-today", { title: "今天已完成的成员", status: "done", executionState: "done", completedToday: true, version: 2, todayMember: true, todayContext: false, dayPlanVersion: 1, totalMinutes: 45, sortOrder: 30 }),
    ],
    "subject-b": [],
  };
  const runningEntry = () => ({
    id: "entry-overnight",
    workDate,
    label: "跨日继续计时",
    startedAt: Date.now() - 3_600_000,
    durationSeconds: 3600,
    settlementMinutes: 0,
    allocatedMinutes: 0,
    defaultTaskId: "backlog",
    allocations: [],
    kind: "work",
    sourceType: "timer",
    state: "running",
    version: 1,
  });

  const calls = { update: [], dayPlanSet: [], dayPlanBatch: [], transition: [], create: [], completed: [], move: [], actualAdjustment: [], cloudCatchUp: 0 };
  let failDayPlanBatchOnce = false;
  let created = 0;

  window.__EXECUTION_DIALOG_TEST__ = {
    calls,
    failNextDayPlanBatch() { failDayPlanBatchOnce = true; },
  };

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
          return { storageMode: "local", workspaceId: "workspace", deviceId: "device", shared: {}, device: { app_theme: "forest", completion_feedback_enabled: false }, integrations: {}, secrets: { seatableTokenSet: false, supabaseSessionSet: false } };
        case "work_calendar_get_context":
          return { timezone: "Asia/Shanghai", currentWorkDate: workDate, currentDayStartAt: Date.now() - 3_600_000, currentDayEndAt: Date.now() + 86_400_000 };
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
          return { subjectId: args.request.subjectId, workDate, revision: 1, tasks: (planRows[args.request.subjectId] ?? []).map((row) => ({ ...row })) };
        case "task_day_plan_set":
          calls.dayPlanSet.push({ ...args.request });
          return { taskId: args.request.taskId, workDate, state: args.request.state, version: 2 };
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
            if (source) rows.push({ ...source, todayMember: true, todayContext: false, dayPlanVersion: 1 });
          }
          return { changedCount: (args.request.items ?? []).length };
        }
        case "task_update": {
          calls.update.push({ ...args.request });
          const row = planRows["subject-a"].find((item) => item.id === args.request.id);
          if (row) {
            if (args.request.title !== undefined) row.title = args.request.title;
            if (args.request.note !== undefined) row.note = args.request.note ?? undefined;
            row.version += 1;
          }
          const active = activeTasks.find((item) => item.id === args.request.id);
          if (active) {
            if (args.request.title !== undefined) active.title = args.request.title;
            if (args.request.note !== undefined) active.note = args.request.note ?? undefined;
            active.version += 1;
          }
          return { ...(row ?? active ?? {}) };
        }
        case "task_set_completed": {
          calls.completed.push({ ...args.request });
          const row = planRows["subject-a"].find((item) => item.id === args.request.id);
          if (row) {
            row.status = args.request.done ? "done" : "open";
            row.completedToday = Boolean(args.request.done);
            row.version += 1;
          }
          return { ...(row ?? {}) };
        }
        case "task_move": {
          calls.move.push({ ...args.request });
          const applyMove = (rows) => {
            const index = rows.findIndex((row) => row.id === args.request.id);
            if (index < 0) return undefined;
            const [moving] = rows.splice(index, 1);
            moving.parentId = args.request.parentId;
            moving.version += 1;
            const anchorId = args.request.nextSiblingId ?? args.request.previousSiblingId;
            const anchorIndex = anchorId ? rows.findIndex((row) => row.id === anchorId) : -1;
            const insertAt = anchorIndex < 0 ? rows.length : args.request.nextSiblingId ? anchorIndex : anchorIndex + 1;
            rows.splice(insertAt, 0, moving);
            return moving;
          };
          const moved = applyMove(activeTasks);
          applyMove(planRows["subject-a"]);
          return { ...(moved ?? {}) };
        }
        case "task_create":
        case "task_create_quick": {
          created += 1;
          const id = "created-" + created;
          calls.create.push({ id, ...args.request });
          const row = record(id, { subjectId: args.request.subjectId, parentId: args.request.parentId, title: args.request.title, pathLabel: args.request.parentId ? `子项 / ${args.request.title}` : args.request.title, sortOrder: 90 + created });
          activeTasks.push(row);
          return { ...row };
        }
        case "task_create_positioned": {
          created += 1;
          const id = "created-" + created;
          calls.create.push({ id, ...args.request });
          const row = record(id, { subjectId: args.request.subjectId, parentId: args.request.parentId, title: args.request.title, pathLabel: args.request.parentId ? `子项 / ${args.request.title}` : args.request.title, sortOrder: 90 + created, estimateMinutes: args.request.estimateMinutes, plannedDate: args.request.plannedDate, note: args.request.note });
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
                totalMinutes: activeTasks.filter((candidate) => candidate.parentId === task.id).reduce((sum, child) => sum + child.directMinutes, task.directMinutes),
              })),
            adjustments: [],
          };
        case "actual_adjustment_batch_apply": {
          calls.actualAdjustment.push({ ...args.request });
          for (const item of args.request.items ?? []) {
            const active = activeTasks.find((task) => task.id === item.taskId);
            if (active) { active.directMinutes = item.targetMinutes; active.totalMinutes = item.targetMinutes; }
            for (const rows of Object.values(planRows)) {
              const row = rows.find((task) => task.id === item.taskId);
              if (row) { row.directMinutes = item.targetMinutes; row.totalMinutes = item.targetMinutes; }
            }
          }
          return { batchId: args.request.batchId, createdAdjustmentCount: (args.request.items ?? []).length, createdTaskCount: 0, createdTasks: [], adjustments: [] };
        }
        case "timer_transition": {
          calls.transition.push({ ...args.request });
          const targetTaskId = args.request.taskId ?? args.request.newTask?.title ?? "created";
          return {
            activeEntry: { ...runningEntry(), id: "entry-" + targetTaskId, defaultTaskId: args.request.taskId ?? "created-1", label: args.request.newTask?.title ?? undefined },
            transitionAt: Date.now(),
            noOp: false,
          };
        }
        case "task_delete_subtree":
          return null;
        case "time_entry_list":
          return { entries: [], rawMinutes: 0, allocatedMinutes: 0, pendingMinutes: 0, breakMinutes: 0, revision: 1 };
        case "time_entry_history_list":
          return { page: 1, pageSize: 7, totalDays: 0, totalPages: 0, groups: [], revision: 1 };
        case "timer_get_state":
          return runningEntry();
        case "unassigned_get_state_snapshot":
          return { current: { sessionId: "unassigned-1", workDate, state: "awaiting_resolution", firstStartedAt: Date.now() - 1_800_000, lastEndedAt: Date.now(), elapsedSeconds: 1_800, requiredMinutes: 30, thresholdSeconds: 300, mustResolve: true, version: 1 } };
        case "today_work_overview_get":
          return { scope: { subjectName: "全部主体", todayDate: workDate }, summary: { actualMinutes: 0, estimatedMinutes: 0, completedCount: 0, activeCount: 0, notStartedCount: 0, unassignedMinutes: 0, completionRate: 0 }, days: [], timeline: [], tasks: [], revision: 1 };
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

async function main() {
  await waitForServer();
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  await context.addInitScript(mockInitScript);
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  const calls = () => page.evaluate(() => window.__EXECUTION_DIALOG_TEST__.calls);
  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
  await page.locator(".global-timer-main").click();

  const dialog = page.locator(".timer-task-dialog");
  await dialog.waitFor({ state: "visible" });
  const rows = dialog.locator(".timer-task-row:not(.new-task)");
  await rows.first().waitFor({ state: "visible" });

  // 1. 展示今日成员（含当天已完成）与父级上下文：长期待办、非今日的已完成普通事项不进入候选
  const rowTitles = await rows.locator('input[data-timer-task-field="title"]').evaluateAll((inputs) => inputs.map((input) => input.getAttribute("aria-label")));
  const initialRowCount = rowTitles.length;
  if (rowTitles.length !== 3) throw new Error("执行弹框候选未收敛到今日计划：" + JSON.stringify(rowTitles));
  if (!rowTitles.includes("事项名称：写今日计划")) throw new Error("今日未完成成员未出现在执行弹框");
  if (rowTitles.some((label) => label.includes("跨日继续的长期待办"))) throw new Error("未加入今日的进行中事项混入了执行候选");
  if (rowTitles.some((label) => label.includes("已完成普通事项"))) throw new Error("已完成普通事项混入了执行候选");
  if (!rowTitles.includes("事项名称：今天已完成的成员")) throw new Error("当天已完成成员未出现在执行弹框");
  const completedRow = dialog.locator('.timer-task-row[data-timer-task-id="done-today"]');
  if (await completedRow.locator(".timer-task-name-cell > small").count() !== 0) throw new Error("当天已完成成员仍显示额外完成态文字");
  if (!await completedRow.evaluate((element) => element.classList.contains("completed"))) throw new Error("当天已完成成员缺少灰色完成态样式");
  if ((await completedRow.locator(".timer-task-primary-action").innerText()).trim() !== "继续") throw new Error("当天已完成成员没有继续执行入口");
  if (!(await dialog.locator(".timer-task-dialog-summary").innerText()).includes("1 个今天已完成")) throw new Error("执行弹框汇总未区分今天已完成事项");
  if (await rows.filter({ has: page.locator('input[aria-label="事项名称：项目"]') }).locator(".timer-task-primary-action").count() !== 0) throw new Error("父级上下文可执行");

  // 2. 跨日继续计时的当前事项只显示状态，不进入候选
  const status = dialog.locator(".timer-task-current-status");
  await status.waitFor({ state: "visible" });
  if (!(await status.innerText()).includes("未加入今天计划")) throw new Error("跨日计时状态区没有说明未加入今天计划");
  if (!(await status.innerText()).includes("跨日继续的长期待办")) throw new Error("跨日计时状态区没有显示当前事项");

  // 3. 名称与备注编辑仍复用同一执行弹框能力
  const titleInput = dialog.locator('input[aria-label="事项名称：写今日计划"]');
  await titleInput.fill("写今日计划（弹框改名）");
  await titleInput.blur();
  await page.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.update.some((call) => call.id === "child" && call.title === "写今日计划（弹框改名）"));
  await dialog.locator('button[aria-label="写今日计划（弹框改名）备注"]').click();
  const noteArea = page.locator(".timer-task-note-popover:visible textarea");
  await noteArea.fill("弹框内备注");
  await page.locator(".timer-task-note-popover:visible button", { hasText: "保存" }).click();
  await page.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.update.some((call) => call.note === "弹框内备注"));

  // 4. 从进行中加入今日：加入后弹框保持打开并立即成为候选
  await dialog.locator("button", { hasText: "从进行中加入" }).click();
  const picker = page.locator(".timer-today-picker-dialog");
  await picker.waitFor({ state: "visible" });
  await page.waitForTimeout(350);
  const executePickerVisual = await picker.evaluate((element) => {
    const body = element.querySelector(".el-dialog__body");
    const list = element.querySelector(".timer-today-picker-list");
    const row = element.querySelector(".timer-today-picker-row");
    const rect = element.getBoundingClientRect();
    return {
      width: rect.width,
      height: rect.height,
      background: getComputedStyle(element).backgroundColor,
      bodyOverflow: body ? getComputedStyle(body).overflow : "",
      bodyPadding: body ? getComputedStyle(body).padding : "",
      listOverflow: list ? getComputedStyle(list).overflowY : "",
      rowHeight: row?.getBoundingClientRect().height ?? 0,
      opacity: getComputedStyle(element).opacity,
    };
  });
  if (await picker.locator(".today-plan-search, .today-plan-candidates").count()) throw new Error("执行弹框的进行中选择器仍依赖待办页样式");
  if (executePickerVisual.bodyOverflow !== "hidden" || executePickerVisual.listOverflow !== "auto" || executePickerVisual.rowHeight < 46 || executePickerVisual.opacity !== "1") throw new Error("执行弹框的进行中选择器布局异常：" + JSON.stringify(executePickerVisual));
  await page.screenshot({ path: path.join(repoRoot, ".tmp-execution-today-picker.png"), fullPage: true });
  await picker.locator("label", { hasText: "跨日继续的长期待办" }).locator("input").check();
  await picker.locator("button", { hasText: "加入今日" }).click();
  await page.waitForFunction((expected) => document.querySelectorAll(".timer-task-dialog .timer-task-row:not(.new-task)").length === expected, initialRowCount + 1);
  if (!await dialog.isVisible()) throw new Error("加入今日后执行弹框不应关闭");

  // 5. 执行弹框通过拖动柄右键菜单同层插入、复制当前行，并可点击实际用时调整。
  if (await dialog.locator('.timer-task-add-child').count()) throw new Error("执行弹框仍显示逐行新增子项按钮");
  const existingChildRow = dialog.locator('.timer-task-row[data-timer-task-id="child"]');
  await existingChildRow.locator('.timer-task-drag-handle').click({ button: "right" });
  const executeRowMenu = page.locator('.task-row-context-menu:visible');
  const executeMenuItems = await executeRowMenu.getByRole("menuitem").allTextContents();
  if (executeMenuItems.join("|") !== "复制事项|新增事项|在上方插入行|在下方插入行|删除事项") throw new Error("执行弹框右键菜单顺序或文案不正确");
  await executeRowMenu.getByRole("menuitem", { name: "新增事项" }).click();
  await dialog.locator('input[aria-label="新事项名称"]').fill("执行右键新增事项");
  await dialog.locator('.timer-task-row.new-task button', { hasText: "保存" }).click();
  await page.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.create.some((call) => call.title === "执行右键新增事项" && !call.parentId));
  await existingChildRow.locator('.timer-task-drag-handle').click({ button: "right" });
  const refreshedExecuteRowMenu = page.locator('.task-row-context-menu:visible');
  await refreshedExecuteRowMenu.getByRole("menuitem", { name: "在下方插入行" }).click();
  await dialog.locator('input[aria-label="新事项名称"]').fill("执行弹框新增子项");
  await dialog.locator('.timer-task-row.new-task button', { hasText: "保存" }).click();
  await page.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.create.some((call) => call.title === "执行弹框新增子项" && call.parentId === "parent"));
  await existingChildRow.locator('.timer-task-drag-handle').click({ button: "right" });
  await page.locator('.task-row-context-menu:visible').getByRole("menuitem", { name: "复制事项" }).click();
  await page.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.create.some((call) => call.title === "写今日计划（弹框改名）" && call.parentId === "parent" && call.previousSiblingId === "child"));
  await dialog.locator('.timer-task-row[data-timer-task-id="child"]').getByRole("button", { name: "调整写今日计划（弹框改名）今日实际" }).click();
  const executeActualPopover = page.locator('.el-popper:visible .actual-adjustment-inline').filter({ hasText: "写今日计划（弹框改名） · 今日实际" }).last();
  await executeActualPopover.getByRole("textbox", { name: "目标实际分钟" }).fill("15");
  await executeActualPopover.getByRole("button", { name: "确认调整" }).click();
  await page.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.actualAdjustment.some((call) => call.items?.some((item) => item.taskId === "child" && item.targetMinutes === 15)));
  if ((await calls()).cloudCatchUp < 2) throw new Error("执行弹框实际调整未在打开和保存前追平云端增量");

  // 6. 今日成员写入失败时保留弹框并提示，不得对未成为今日成员的事项开始计时
  await page.evaluate(() => window.__EXECUTION_DIALOG_TEST__.failNextDayPlanBatch());
  await dialog.locator(".timer-task-table-tools button", { hasText: "新增事项" }).click();
  const newTaskTitle = dialog.locator('input[aria-label="新事项名称"]');
  await newTaskTitle.fill("加入会失败的新事项");
  await dialog.locator(".timer-task-row.new-task button", { hasText: "保存" }).click();
  const rowError = dialog.locator(".timer-task-row-error");
  await rowError.waitFor({ state: "visible" });
  if (!(await rowError.innerText()).includes("事项已创建，但加入今日失败")) throw new Error("加入失败时未给出补偿提示：" + await rowError.innerText());
  if (!(await calls()).transition.length) { /* 未触发计时转换即为预期 */ } 
  if ((await calls()).transition.length !== 0) throw new Error("加入今日失败后不应开始计时");
  await dialog.locator(".timer-task-row.new-task button", { hasText: "取消" }).click();

  // 7. 新建并开始走原子计时转换请求，成功后关闭弹框
  await dialog.locator("button", { hasText: "新增事项" }).click();
  await dialog.locator('input[aria-label="新事项名称"]').fill("新建并开始的事项");
  await dialog.locator(".timer-task-row.new-task button", { hasText: "开始" }).click();
  await page.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.transition.some((call) => call.newTask?.title === "新建并开始的事项"));
  const transitionCall = (await calls()).transition.at(-1);
  if (!transitionCall.newTask || transitionCall.newTask.subjectId !== "subject-a") throw new Error("新建并开始没有携带主体信息");
  await dialog.waitFor({ state: "hidden" });

  // 8. 空今日计划的主体：空状态保留两条补充入口
  await page.locator(".global-timer-main").click();
  await dialog.waitFor({ state: "visible" });
  await dialog.locator(".timer-task-subject-tabs button", { hasText: "个人" }).click();
  const empty = dialog.locator(".timer-task-empty");
  await empty.waitFor({ state: "visible" });
  if (await empty.locator("button", { hasText: "新增事项" }).count() !== 1) throw new Error("空状态缺少新增今日事项入口");
  if (await empty.locator("button", { hasText: "从进行中加入" }).count() !== 1) throw new Error("空状态缺少从进行中加入入口");
  if (await dialog.locator(".timer-task-row:not(.new-task)").count() !== 0) throw new Error("空计划主体不应展示其他主体事项");

  // 9. 回到工作主体后验证当天已完成事项可从执行弹框继续，计时转换携带恢复版本。
  await dialog.locator(".timer-task-subject-tabs button", { hasText: "工作" }).click();
  const completedTaskRow = dialog.locator('.timer-task-row[data-timer-task-id="done-today"]');
  const continueButton = completedTaskRow.locator(".timer-task-primary-action", { hasText: "继续" });
  await continueButton.click();
  await page.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.transition.some((call) => call.taskId === "done-today"));
  const continueCall = (await calls()).transition.find((call) => call.taskId === "done-today");
  if (continueCall?.reopenCompletedTaskVersion !== 2) throw new Error("继续已完成事项未携带恢复版本：" + JSON.stringify(continueCall));
  await dialog.waitFor({ state: "hidden" });

  // 9.1 重新打开执行弹框，验证拖动写入正式事项顺序。
  await page.locator(".global-timer-main").click();
  await dialog.waitFor({ state: "visible" });
  await dialog.locator(".timer-task-subject-tabs button", { hasText: "工作" }).click();
  const backlogRow = dialog.locator('.timer-task-row[data-timer-task-id="backlog"]');
  const childRow = dialog.locator('.timer-task-row[data-timer-task-id="child"]');
  await backlogRow.waitFor({ state: "visible" });
  if (await backlogRow.locator(".timer-task-drag-handle").count() !== 1) throw new Error("执行弹框缺少拖动排序入口");
  await backlogRow.locator(".timer-task-drag-handle").dragTo(childRow, { targetPosition: { x: 150, y: 5 } });
  await page.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.move.length > 0);
  const moveCall = (await calls()).move.at(-1);
  if (moveCall.id !== "backlog" || moveCall.parentId || (!moveCall.previousSiblingId && !moveCall.nextSiblingId)) throw new Error("执行弹框拖动结果异常：" + JSON.stringify(moveCall));

  // 10. 未归属时间分配复用同一套事项配置：新增一级/子项，并可独立调整实际用时。
  const unassignedPage = await context.newPage();
  unassignedPage.on("pageerror", (error) => pageErrors.push(error.message));
  await unassignedPage.goto(baseUrl, { waitUntil: "domcontentloaded" });
  const unassignedCalls = () => unassignedPage.evaluate(() => window.__EXECUTION_DIALOG_TEST__.calls);
  const unassignedDialog = unassignedPage.locator('.timer-task-dialog');
  await unassignedPage.waitForFunction(() => document.querySelector('.unassigned-time-indicator time')?.textContent !== "00:00");
  await unassignedPage.locator('.unassigned-time-indicator').click();
  await unassignedDialog.waitFor({ state: "visible" });
  await unassignedDialog.locator('.timer-task-allocation-overview').waitFor({ state: "visible" });
  await unassignedDialog.locator('button', { hasText: "从进行中加入" }).click();
  const unassignedPicker = unassignedPage.locator('.timer-today-picker-dialog');
  await unassignedPicker.waitFor({ state: "visible" });
  await unassignedPage.waitForTimeout(350);
  const unassignedPickerVisual = await unassignedPicker.evaluate((element) => {
    const body = element.querySelector(".el-dialog__body");
    const list = element.querySelector(".timer-today-picker-list");
    const row = element.querySelector(".timer-today-picker-row");
    const rect = element.getBoundingClientRect();
    return {
      width: rect.width,
      height: rect.height,
      background: getComputedStyle(element).backgroundColor,
      bodyOverflow: body ? getComputedStyle(body).overflow : "",
      bodyPadding: body ? getComputedStyle(body).padding : "",
      listOverflow: list ? getComputedStyle(list).overflowY : "",
      rowHeight: row?.getBoundingClientRect().height ?? 0,
      opacity: getComputedStyle(element).opacity,
    };
  });
  if (JSON.stringify(unassignedPickerVisual) !== JSON.stringify(executePickerVisual)) throw new Error("执行事项与未归属时间的进行中选择器样式不一致：" + JSON.stringify({ executePickerVisual, unassignedPickerVisual }));
  if (await unassignedPicker.locator(".today-plan-search, .today-plan-candidates").count()) throw new Error("未归属时间的进行中选择器仍依赖待办页样式");
  await unassignedPage.screenshot({ path: path.join(repoRoot, ".tmp-unassigned-today-picker.png"), fullPage: true });
  await unassignedPicker.getByRole("button", { name: "取消", exact: true }).click();
  await unassignedPicker.waitFor({ state: "hidden" });
  await unassignedDialog.locator('.timer-task-table-tools button', { hasText: "新增事项" }).click();
  await unassignedDialog.locator('input[aria-label="新事项名称"]').fill("未归属弹框新增事项");
  await unassignedDialog.locator('.timer-task-row.new-task button', { hasText: "保存" }).click();
  await unassignedPage.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.create.some((call) => call.title === "未归属弹框新增事项" && !call.parentId));
  const unassignedRoot = (await unassignedCalls()).create.find((call) => call.title === "未归属弹框新增事项");
  const unassignedRootRow = unassignedDialog.locator(`[data-timer-task-id="${unassignedRoot.id}"]`);
  if (await unassignedDialog.locator('.timer-task-add-child').count()) throw new Error("未归属弹框仍显示逐行新增子项按钮");
  const unassignedChildRow = unassignedDialog.locator('.timer-task-row[data-timer-task-id="child"]');
  await unassignedChildRow.locator('.timer-task-drag-handle').click({ button: "right" });
  const unassignedRowMenu = unassignedPage.locator('.task-row-context-menu:visible');
  const unassignedMenuItems = await unassignedRowMenu.getByRole("menuitem").allTextContents();
  if (unassignedMenuItems.join("|") !== "复制事项|新增事项|在上方插入行|在下方插入行|删除事项") throw new Error("未归属弹框右键菜单顺序或文案不正确");
  await unassignedRowMenu.getByRole("menuitem", { name: "新增事项" }).click();
  await unassignedDialog.locator('input[aria-label="新事项名称"]').fill("未归属右键新增事项");
  await unassignedDialog.locator('.timer-task-row.new-task button', { hasText: "保存" }).click();
  await unassignedPage.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.create.some((call) => call.title === "未归属右键新增事项" && !call.parentId));
  await unassignedChildRow.locator('.timer-task-drag-handle').click({ button: "right" });
  await unassignedPage.locator('.task-row-context-menu:visible').getByRole("menuitem", { name: "在下方插入行" }).click();
  await unassignedDialog.locator('input[aria-label="新事项名称"]').fill("未归属弹框新增子项");
  await unassignedDialog.locator('.timer-task-row.new-task button', { hasText: "保存" }).click();
  await unassignedPage.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.create.some((call) => call.title === "未归属弹框新增子项" && call.parentId === "parent"));
  await unassignedDialog.getByRole("button", { name: "调整写今日计划今日实际" }).click();
  const unassignedActualPopover = unassignedPage.locator('.el-popper:visible .actual-adjustment-inline').filter({ hasText: "写今日计划 · 今日实际" }).last();
  await unassignedActualPopover.getByRole("textbox", { name: "目标实际分钟" }).fill("25");
  await unassignedActualPopover.getByRole("button", { name: "确认调整" }).click();
  await unassignedPage.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.actualAdjustment.some((call) => call.items?.some((item) => item.taskId === "child" && item.targetMinutes === 25)));

  if (pageErrors.length) throw new Error("页面错误: " + pageErrors.join("；"));
  await browser.close();
  console.log("execution dialog UI checks passed");
}

try {
  await main();
} finally {
  if (process.platform === "win32") {
    const terminator = spawn("taskkill.exe", ["/pid", String(server.pid), "/t", "/f"], { stdio: "ignore" });
    await new Promise((resolve) => terminator.once("exit", resolve));
  } else {
    server.kill("SIGTERM");
  }
}
