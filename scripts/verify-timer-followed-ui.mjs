import assert from "node:assert/strict";
import { chromium } from "playwright";
import { preview } from "vite";

const host = "127.0.0.1";
const port = 5298;
const entryId = "timer-followed-entry";
const taskId = "plan-direct-start-task";
const now = Date.now();
const timeEntry = {
  id: entryId,
  workDate: "2026-10-18",
  label: "多设备计时验证",
  startedAt: now - 60_000,
  endedAt: now,
  durationSeconds: 60,
  settlementMinutes: 1,
  allocatedMinutes: 0,
  note: "",
  kind: "work",
  sourceType: "timer",
  state: "ended",
  version: 2,
  allocations: [],
};

const server = await preview({
  configFile: "vite.config.ts",
  preview: { host, port, strictPort: true },
});
const browser = await chromium.launch({ headless: true });

try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await page.addInitScript(({ entry, currentNow }) => {
    const callbacks = new Map();
    const listeners = new Map();
    let callbackId = 1;
    const emptyOverview = {
      scope: { subjectName: "全部主体", todayDate: "2026-10-18" },
      summary: { actualMinutes: 0, estimatedMinutes: 0, completedCount: 0, activeCount: 0, notStartedCount: 0, unassignedMinutes: 0, completionRate: 0 },
      days: [], timeline: [], tasks: [], revision: 1,
    };
    const settings = {
      storageMode: "local", workspaceId: "workspace", deviceId: "device",
      shared: { timezone: "Asia/Shanghai" }, device: { app_theme: "forest" },
      integrations: {}, secrets: { seatableTokenSet: false, supabaseSessionSet: false },
    };
    const storage = {
      mode: "local", workspaceId: "workspace", deviceId: "device", online: true,
      syncState: "synced", pendingOperations: 0, conflictCount: 0, lastChangeSeq: 0,
      authBlocked: false, workspaceEpoch: 1, epochBlocked: false,
    };
    window.__timerTransitionCalls = 0;
    const task = { id: "plan-direct-start-task", subjectId: "subject", title: "直接开始计时", status: "open", sortOrder: 10, depth: 0, pathLabel: "直接开始计时", selectable: true, directMinutes: 0, totalMinutes: 0, executionState: "not_started", version: 1 };
    const commandResult = (cmd) => {
      switch (cmd) {
        case "settings_get": return settings;
        case "storage_mode_get": return storage;
        case "cloud_session_get": return { signedIn: false, status: "reauth_required" };
        case "cloud_device_list": return [];
        case "window_is_main_maximized": return false;
        case "plugin:window|is_maximized": return false;
        case "plugin:window|is_focused": return true;
        case "work_calendar_get_context": return { timezone: "Asia/Shanghai", currentWorkDate: "2026-10-18", currentDayStartAt: currentNow - 3_600_000, currentDayEndAt: currentNow + 86_400_000 };
        case "subject_list": return [{ id: "subject", name: "默认", sortOrder: 10, openTaskCount: 1, version: 1 }];
        case "task_list": return { tasks: [task], revision: 1 };
        case "time_entry_list": return { entries: [entry], rawMinutes: 1, allocatedMinutes: 0, pendingMinutes: 1, breakMinutes: 0, revision: 1 };
        case "timer_get_state": return null;
        case "time_entry_history_list": return { page: 1, pageSize: 20, totalDays: 1, totalPages: 1, groups: [{ workDate: "2026-10-18", entryCount: 1, rawMinutes: 1, allocatedMinutes: 0, pendingMinutes: 1, breakMinutes: 0, adjustmentMinutes: 0, finalActualMinutes: 1, adjustments: [], entries: [entry] }], revision: 1 };
        case "report_list": return { reports: [], revision: 1 };
        case "unassigned_get_state_snapshot": return {};
        case "today_work_overview_get": return emptyOverview;
        case "timer_transition": {
          window.__timerTransitionCalls += 1;
          return {
            activeEntry: { ...entry, id: "direct-start-entry", label: task.title, startedAt: Date.now(), endedAt: undefined, durationSeconds: 0, settlementMinutes: 0, defaultTaskId: task.id, state: "running", version: 1 },
            transitionAt: Date.now(),
            noOp: false,
          };
        }
        default: return undefined;
      }
    };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
      transformCallback(callback, once = false) {
        const id = callbackId++;
        callbacks.set(id, (payload) => {
          if (once) callbacks.delete(id);
          return callback?.(payload);
        });
        return id;
      },
      unregisterCallback(id) { callbacks.delete(id); },
      runCallback(id, payload) { callbacks.get(id)?.(payload); },
      convertFileSrc(path) { return path; },
      async invoke(cmd, args = {}) {
        if (cmd === "plugin:event|listen") {
          const eventListeners = listeners.get(args.event) ?? [];
          eventListeners.push(args.handler);
          listeners.set(args.event, eventListeners);
          return args.handler;
        }
        if (cmd === "plugin:event|unlisten") {
          const eventListeners = listeners.get(args.event) ?? [];
          listeners.set(args.event, eventListeners.filter((id) => id !== args.eventId));
          return null;
        }
        if (cmd === "plugin:event|emit") {
          for (const id of listeners.get(args.event) ?? []) {
            callbacks.get(id)?.({ event: args.event, id, payload: args.payload });
          }
          return null;
        }
        return commandResult(cmd);
      },
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener(_event, id) { callbacks.delete(id); },
    };
  }, { entry: timeEntry, currentNow: now });

  await page.goto(`http://${host}:${port}`, { waitUntil: "networkidle" });
  await page.waitForSelector(".topbar-title h1");
  await page.getByRole("navigation").getByRole("button", { name: "待办", exact: true }).click();
  await page.getByRole("tab", { name: /进行中/ }).click();
  const timerButton = page.locator(`.reminder-row[data-task-id="${taskId}"] .task-timer-action`);
  await timerButton.waitFor({ state: "visible" });
  await timerButton.click();
  await page.waitForFunction(() => window.__timerTransitionCalls === 1);
  await page.waitForFunction(() => document.querySelector('.global-timer-main')?.textContent?.includes('直接开始计时'));
  assert.equal(await page.locator(".timer-task-dialog:visible").count(), 0, "待办页开始计时不应打开执行弹框");

  await page.evaluate(async (payload) => {
    await window.__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: "timer-fact-followed", payload });
  }, { operationType: "timer_stop", entityId: entryId, version: 2 });

  const message = page.locator(".el-message .timer-followed-message");
  await message.waitFor({ state: "visible" });
  assert.match(await message.textContent(), /已在另一台设备保存，已跟随该结果/);
  assert.equal(await page.locator(".el-overlay:visible").count(), 0, "跟随提示不应创建可见模态遮罩");

  const link = message.locator(".timer-followed-message__link");
  assert.equal(await link.getAttribute("type"), "button");
  assert.equal(await link.evaluate((node) => getComputedStyle(node).cursor), "pointer");
  await link.click();
  await page.waitForFunction(() => document.querySelector(".topbar-title h1")?.textContent?.includes("时间记录"));
  assert.equal(await message.isVisible(), true, "跳转期间提示应保持非阻塞可见");
  await page.locator(`[data-entry-id="${entryId}"]`).waitFor({ state: "visible" });

  console.log("timer followed UI verification passed");
} finally {
  await browser.close();
  await server.httpServer.close();
}
