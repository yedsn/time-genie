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

  const calls = { update: [], dayPlanSet: [], dayPlanBatch: [], transition: [], create: [], completed: [] };
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
        case "task_create":
        case "task_create_quick": {
          created += 1;
          const id = "created-" + created;
          calls.create.push({ id, ...args.request });
          const row = record(id, { subjectId: args.request.subjectId, title: args.request.title, sortOrder: 90 + created });
          activeTasks.push(row);
          return { ...row };
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
        case "unassigned_get_state":
          return { current: undefined };
        case "today_work_overview_get":
          return { scope: { subjectName: "全部主体", todayDate: workDate }, summary: { actualMinutes: 0, estimatedMinutes: 0, completedCount: 0, activeCount: 0, notStartedCount: 0, unassignedMinutes: 0, completionRate: 0 }, days: [], timeline: [], tasks: [], revision: 1 };
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

async function main() {
  await waitForServer();
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.addInitScript(mockInitScript);

  const calls = () => page.evaluate(() => window.__EXECUTION_DIALOG_TEST__.calls);
  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
  await page.locator(".global-timer-main").click();

  const dialog = page.locator(".timer-task-dialog");
  await dialog.waitFor({ state: "visible" });
  const rows = dialog.locator(".timer-task-row:not(.new-task)");
  await rows.first().waitFor({ state: "visible" });

  // 1. 只展示未完成今日成员与父级上下文：长期待办、已完成普通事项、当天已完成成员都不进入候选
  const rowTitles = await rows.locator('input[data-timer-task-field="title"]').evaluateAll((inputs) => inputs.map((input) => input.getAttribute("aria-label")));
  if (rowTitles.length !== 2) throw new Error("执行弹框候选未收敛到今日计划：" + JSON.stringify(rowTitles));
  if (!rowTitles.includes("事项名称：写今日计划")) throw new Error("今日未完成成员未出现在执行弹框");
  if (rowTitles.some((label) => label.includes("跨日继续的长期待办"))) throw new Error("未加入今日的进行中事项混入了执行候选");
  if (rowTitles.some((label) => label.includes("已完成普通事项"))) throw new Error("已完成普通事项混入了执行候选");
  if (rowTitles.some((label) => label.includes("今天已完成的成员"))) throw new Error("当天已完成成员不应进入执行候选");
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
  const picker = page.locator(".el-dialog", { hasText: "从进行中加入今日" });
  await picker.locator("label", { hasText: "跨日继续的长期待办" }).locator("input").check();
  await picker.locator("button", { hasText: "加入今日" }).click();
  await page.waitForFunction(() => document.querySelectorAll(".timer-task-dialog .timer-task-row:not(.new-task)").length === 3);
  if (!await dialog.isVisible()) throw new Error("加入今日后执行弹框不应关闭");

  // 5. 今日成员写入失败时保留弹框并提示，不得对未成为今日成员的事项开始计时
  await page.evaluate(() => window.__EXECUTION_DIALOG_TEST__.failNextDayPlanBatch());
  await dialog.locator("button", { hasText: "新增事项" }).click();
  const newTaskTitle = dialog.locator('input[aria-label="新事项名称"]');
  await newTaskTitle.fill("加入会失败的新事项");
  await dialog.locator(".timer-task-row.new-task button", { hasText: "保存" }).click();
  const rowError = dialog.locator(".timer-task-row-error");
  await rowError.waitFor({ state: "visible" });
  if (!(await rowError.innerText()).includes("事项已创建，但加入今日失败")) throw new Error("加入失败时未给出补偿提示：" + await rowError.innerText());
  if (!(await calls()).transition.length) { /* 未触发计时转换即为预期 */ } 
  if ((await calls()).transition.length !== 0) throw new Error("加入今日失败后不应开始计时");
  await dialog.locator(".timer-task-row.new-task button", { hasText: "取消" }).click();

  // 6. 新建并开始走原子计时转换请求，成功后关闭弹框
  await dialog.locator("button", { hasText: "新增事项" }).click();
  await dialog.locator('input[aria-label="新事项名称"]').fill("新建并开始的事项");
  await dialog.locator(".timer-task-row.new-task button", { hasText: "开始" }).click();
  await page.waitForFunction(() => window.__EXECUTION_DIALOG_TEST__.calls.transition.some((call) => call.newTask?.title === "新建并开始的事项"));
  const transitionCall = (await calls()).transition.at(-1);
  if (!transitionCall.newTask || transitionCall.newTask.subjectId !== "subject-a") throw new Error("新建并开始没有携带主体信息");
  await dialog.waitFor({ state: "hidden" });

  // 7. 空今日计划的主体：空状态保留两条补充入口
  await page.locator(".global-timer-main").click();
  await dialog.waitFor({ state: "visible" });
  await dialog.locator(".timer-task-subject-tabs button", { hasText: "个人" }).click();
  const empty = dialog.locator(".timer-task-empty");
  await empty.waitFor({ state: "visible" });
  if (await empty.locator("button", { hasText: "新增事项" }).count() !== 1) throw new Error("空状态缺少新增今日事项入口");
  if (await empty.locator("button", { hasText: "从进行中加入" }).count() !== 1) throw new Error("空状态缺少从进行中加入入口");
  if (await dialog.locator(".timer-task-row:not(.new-task)").count() !== 0) throw new Error("空计划主体不应展示其他主体事项");

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
