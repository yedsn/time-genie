import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { chromium } from "playwright";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const port = 5201;
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

async function main() {
  await waitForServer();
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 860 } });
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));

  await page.addInitScript(() => {
    let callbackId = 0;
    const callbacks = new Map();
    const subjects = [
      { id: "subject-a", name: "工作", sortOrder: 1, openTaskCount: 3, version: 1 },
      { id: "subject-b", name: "个人", sortOrder: 2, openTaskCount: 1, version: 1 },
    ];
    const task = (id, subjectId, title, overrides = {}) => ({
      id, subjectId, title, status: "open", sortOrder: 1, depth: 0, pathLabel: title,
      selectable: true, directMinutes: 0, totalMinutes: 0, executionState: "not_started",
      version: 1, ...overrides,
    });
    const active = [
      task("active-parent", "subject-a", "当前项目"),
      task("active-child", "subject-a", "实现进行中与已完成标签", { parentId: "active-parent", sortOrder: 2, depth: 1, pathLabel: "当前项目 / 实现进行中与已完成标签", totalMinutes: 48 }),
      task("recurring-done", "subject-a", "每日回顾", {
        status: "done", executionState: "done", occurrenceDate: "2026-10-14", occurrenceVersion: 2,
        recurrence: { id: "rule-1", taskId: "recurring-done", frequency: "daily", effectiveStart: "2026-10-01", summary: "每天", version: 1 },
      }),
      task("personal-active", "subject-b", "阅读", { totalMinutes: 20 }),
    ];
    const completed = Array.from({ length: 25 }, (_, index) => {
      const number = String(index + 1).padStart(2, "0");
      return task("completed-" + number, "subject-a", "已完成事项 " + number, {
        status: "done", executionState: "done", version: 2,
        completedAt: Date.UTC(2026, 9, 14, 12, 0) - index * 60_000,
        pathLabel: "历史项目 / 已完成事项 " + number,
        totalMinutes: index + 5,
      });
    });
    let completedRequests = 0;

    window.__TASK_STATUS_TEST__ = { get completedRequests() { return completedRequests; } };
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
              device: { app_theme: "forest", completion_feedback_enabled: false }, integrations: {},
              secrets: { seatableTokenSet: false, supabaseSessionSet: false },
            };
          case "work_calendar_get_context":
            return { timezone: "Asia/Shanghai", currentWorkDate: "2026-10-14", currentDayStartAt: 1791993600000, currentDayEndAt: 1792080000000 };
          case "subject_list":
            return subjects;
          case "task_list": {
            const request = args.request ?? {};
            if (request.todayOnly) return { tasks: [], revision: 1 };
            return { tasks: active.filter((item) => !request.subjectId || item.subjectId === request.subjectId), revision: 1 };
          }
          case "task_list_completed_page": {
            completedRequests += 1;
            const { subjectId, page = 1, pageSize = 20 } = args.request;
            const rows = completed.filter((item) => item.subjectId === subjectId);
            const start = (page - 1) * pageSize;
            return { tasks: rows.slice(start, start + pageSize), page, pageSize, total: rows.length, totalPages: Math.ceil(rows.length / pageSize) };
          }
          case "task_set_completed": {
            const { id, done } = args.request;
            const index = completed.findIndex((item) => item.id === id);
            if (!done && index >= 0) {
              const [item] = completed.splice(index, 1);
              const reopened = { ...item, status: "open", executionState: "started", completedAt: undefined, version: item.version + 1 };
              active.push(reopened);
              return reopened;
            }
            return active.find((item) => item.id === id);
          }
          case "task_update": {
            const request = args.request;
            const item = completed.find((candidate) => candidate.id === request.id);
            Object.assign(item, { title: request.title, version: item.version + 1 });
            return item;
          }
          case "task_delete_subtree": {
            const index = completed.findIndex((item) => item.id === args.request.id);
            if (index >= 0) completed.splice(index, 1);
            return [];
          }
          case "time_entry_list":
            return { entries: [], rawMinutes: 0, allocatedMinutes: 0, pendingMinutes: 0, breakMinutes: 0, revision: 1 };
          case "time_entry_history_list":
            return { page: 1, pageSize: 7, totalDays: 0, totalPages: 0, groups: [], revision: 1 };
          case "timer_get_state":
          case "unassigned_get_state":
            return null;
          case "today_work_overview_get":
            return { scope: { subjectName: "全部主体", todayDate: "2026-10-14" }, summary: { actualMinutes: 0, estimatedMinutes: 0, completedCount: 0, activeCount: 0, notStartedCount: 0, unassignedMinutes: 0, completionRate: 0 }, days: [], timeline: [], tasks: [], revision: 1 };
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
  });

  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
  await page.locator(".nav-list button", { hasText: "待办" }).click();
  await page.locator(".plan-task-tabs").waitFor({ state: "visible" });
  if (await page.evaluate(() => window.__TASK_STATUS_TEST__.completedRequests) !== 0) throw new Error("进入进行中标签时不应请求完成明细");
  if ((await page.getByRole("tab", { name: /进行中/ }).getAttribute("aria-selected")) !== "true") throw new Error("待办默认标签不是进行中");
  if (await page.locator(".pending-reminder-row").count() !== 3) throw new Error("进行中列表未保留重复事项当日完成轮次");

  await page.getByRole("tab", { name: /已完成/ }).click();
  await page.locator(".completed-history-row").first().waitFor({ state: "visible" });
  if (await page.locator(".completed-history-row").count() !== 20) throw new Error("已完成首屏不是 20 条");
  if (await page.evaluate(() => window.__TASK_STATUS_TEST__.completedRequests) !== 1) throw new Error("首次打开已完成标签请求次数异常");
  await page.getByTitle("下一页").click();
  await page.getByText("2 / 2").waitFor({ state: "visible" });
  if (await page.locator(".completed-history-row").count() !== 5) throw new Error("已完成第二页条数错误");

  await page.setViewportSize({ width: 720, height: 720 });
  const panel = await page.locator(".completed-history-panel").boundingBox();
  if (!panel || panel.x < 0 || panel.x + panel.width > 720) throw new Error("最小窗口下已完成面板超出视口");
  const horizontalOverflow = await page.locator(".completed-history-panel").evaluate((element) => element.scrollWidth > element.clientWidth + 1);
  if (horizontalOverflow) throw new Error("最小窗口下已完成面板出现水平溢出");

  await page.setViewportSize({ width: 1280, height: 860 });
  for (let index = 0; index < 5; index += 1) {
    await page.locator(".completed-history-delete").first().click();
    await page.getByRole("button", { name: "删除", exact: true }).last().click();
    const expected = 4 - index;
    await page.waitForTimeout(350);
    const actual = await page.locator(".completed-history-row").count();
    if (index < 4 && actual !== expected) throw new Error("删除后第二页记录数错误: " + actual);
  }
  await page.waitForFunction(() => document.querySelectorAll(".completed-history-row").length === 20);
  if (await page.locator(".completed-history-pagination").count()) throw new Error("回退到单页后仍显示分页控件");
  if (await page.locator(".completed-history-row").count() !== 20) throw new Error("删除空页后未回到上一页");
  const firstTitle = page.locator(".completed-history-main input").first();
  await firstTitle.fill("已编辑的完成事项");
  await firstTitle.blur();
  await page.waitForFunction(() => document.querySelector(".completed-history-main input")?.value === "已编辑的完成事项");
  await page.locator(".completed-history-reopen").first().click();
  await page.getByText("事项已恢复到进行中").waitFor({ state: "visible" });
  if (await page.locator(".completed-history-row").count() !== 19) throw new Error("恢复后历史列表未刷新");

  await page.locator(".subject-select-button", { hasText: "个人" }).click();
  if ((await page.getByRole("tab", { name: /进行中/ }).getAttribute("aria-selected")) !== "true") throw new Error("切换主体后未回到进行中");
  if (pageErrors.length) throw new Error("页面错误: " + pageErrors.join("；"));

  await browser.close();
  console.log("task status tabs UI checks passed");
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
