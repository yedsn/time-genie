import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { chromium } from "playwright";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const port = 5199;
const baseUrl = `http://127.0.0.1:${port}/`;

const server = spawn(
  process.platform === "win32" ? "npm.cmd" : "npm",
  ["run", "dev", "--", "--host", "127.0.0.1", "--port", String(port), "--strictPort"],
  { cwd: repoRoot, stdio: ["ignore", "pipe", "pipe"], shell: process.platform === "win32" },
);

async function waitForServer() {
  const startedAt = Date.now();
  while (Date.now() - startedAt < 30_000) {
    try {
      const response = await fetch(baseUrl);
      if (response.ok) return;
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
  page.on("pageerror", (error) => console.error(`页面错误: ${error.message}`));

  await page.addInitScript(() => {
    let callbackId = 0;
    const callbacks = new Map();
    window.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      transformCallback(callback, once) {
        callbackId += 1;
        callbacks.set(callbackId, { callback, once });
        return callbackId;
      },
      unregisterCallback(id) {
        callbacks.delete(id);
      },
      async invoke(command, args = {}) {
        switch (command) {
          case "settings_get":
            return {
              storageMode: "local",
              workspaceId: "workspace",
              deviceId: "device",
              shared: {},
              device: { app_theme: "forest" },
              integrations: {},
              secrets: { seatableTokenSet: false, supabaseSessionSet: false },
            };
          case "work_calendar_get_context":
            return {
              timezone: "Asia/Shanghai",
              currentWorkDate: "2026-10-13",
              currentDayStartAt: 1791907200000,
              currentDayEndAt: 1791993600000,
            };
          case "subject_list":
            return [{ id: "subject-1", name: "默认", sortOrder: 1, openTaskCount: 0, version: 1 }];
          case "task_list":
            return { tasks: [], revision: 1 };
          case "time_entry_list":
            return {
              entries: [{
                id: "running-entry", workDate: "2026-10-13", label: "界面计时验收",
                startedAt: Date.now() - 65_000, durationSeconds: 65, settlementMinutes: 2,
                allocatedMinutes: 0, note: "", kind: "work", sourceType: "timer",
                state: "running", version: 1, timerChainId: "running-entry", allocations: [],
              }],
              rawMinutes: 2, allocatedMinutes: 0, pendingMinutes: 2, breakMinutes: 0, revision: 1,
            };
          case "timer_get_state":
            return {
              id: "running-entry", workDate: "2026-10-13", label: "界面计时验收",
              startedAt: Date.now() - 65_000, durationSeconds: 65, settlementMinutes: 2,
              allocatedMinutes: 0, note: "", kind: "work", sourceType: "timer",
              state: "running", version: 1, timerChainId: "running-entry", allocations: [],
            };
          case "time_entry_history_list":
            return { page: 1, pageSize: 7, totalDays: 0, totalPages: 0, groups: [], revision: 1 };
          case "today_work_overview_get":
            return {
              scope: { subjectId: undefined, subjectName: "全部主体", todayDate: "2026-10-13" },
              summary: { actualMinutes: 0, estimatedMinutes: 0, completedCount: 0, activeCount: 0, notStartedCount: 0, unassignedMinutes: 0, completionRate: 0 },
              days: [],
              timeline: [],
              tasks: [],
              revision: 1,
            };
          case "unassigned_get_state":
            return null;
          case "report_list":
            return { reports: [], revision: 1 };
          case "automation_hook_list":
            return { hooks: [] };
          case "storage_mode_get":
          case "cloud_sync_status":
            return {
              mode: "cloud", workspaceId: "workspace", deviceId: "device", online: false,
              syncState: "synced", pendingOperations: 0, conflictCount: 0, lastChangeSeq: 0,
              authBlocked: false,
            };
          case "get_app_version":
            return "0.0.0";
          case "plugin:event|listen":
          case "plugin:event|unlisten":
            return 0;
          case "time_allocation_export_preview": {
            const request = args.request;
            if (request.startDate > request.endDate) throw new Error("VALIDATION_ERROR: 开始日期不能晚于结束日期");
            if (request.startDate === "2026-01-01" && request.endDate === "2026-01-02") {
              return { rowCount: 0, totalMinutes: 0, defaultFileName: "TimeGenie_工时归属_2026-01-01_2026-01-02.xlsx" };
            }
            return request.includeSpecialTime
              ? { rowCount: 3, totalMinutes: 90, defaultFileName: "TimeGenie_工时归属_2026-10-01_2026-10-31.xlsx" }
              : { rowCount: 2, totalMinutes: 60, defaultFileName: "TimeGenie_工时归属_2026-10-01_2026-10-31.xlsx" };
          }
          case "plugin:dialog|save":
            return window.__TEST_SAVE_RESULT ?? null;
          case "time_allocation_export_execute": {
            const request = args.request;
            window.__TEST_EXECUTE_CALLS = (window.__TEST_EXECUTE_CALLS ?? 0) + 1;
            if (window.__TEST_EXPORT_MODE === "error") {
              throw new Error("FILE_ERROR: 测试路径不可写");
            }
            if (window.__TEST_EXPORT_MODE === "delay") {
              await new Promise((resolve) => setTimeout(resolve, 1_200));
            }
            return {
              rowCount: request.includeSpecialTime ? 3 : 2,
              totalMinutes: request.includeSpecialTime ? 90 : 60,
              path: request.targetPath,
            };
          }
          default:
            return null;
        }
      },
    };
  });

  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
  await page.locator(".nav-list button", { hasText: "时间记录" }).click();
  await page.locator(".time-history-head").waitFor({ state: "visible" });
  await page.locator(".time-history-actions button", { hasText: "导出" }).click();
  const dialog = page.locator(".time-export-dialog");
  await dialog.waitFor({ state: "visible" });

  const dateInputs = dialog.locator(".time-export-range input");
  await Promise.all([dateInputs.nth(0), dateInputs.nth(1)].map(async (input) => input.waitFor({ state: "visible" })));
  const startDate = await dateInputs.nth(0).inputValue();
  const endDate = await dateInputs.nth(1).inputValue();
  if (startDate !== "2026-10-01" || endDate !== "2026-10-31") {
    throw new Error(`默认月份错误: ${startDate} ~ ${endDate}`);
  }

  await dialog.getByText("预计导出 2 行").waitFor({ state: "visible" });
  await dialog.getByText(/合计 60 分钟/).waitFor({ state: "visible" });

  await dateInputs.nth(1).fill("2026-09-30");
  await dialog.getByText("开始日期不能晚于结束日期").waitFor({ state: "visible" });
  if (await dialog.getByRole("button", { name: /选择位置并导出/ }).isDisabled()) {
    // expected
  } else {
    throw new Error("反向日期时导出按钮未禁用");
  }

  await dateInputs.nth(0).fill("2026-01-01");
  await dateInputs.nth(1).fill("2026-01-02");
  await dialog.getByText("没有可导出的工时").waitFor({ state: "visible" });
  if (await dialog.getByRole("button", { name: /选择位置并导出/ }).isDisabled()) {
    // expected
  } else {
    throw new Error("零行时导出按钮未禁用");
  }

  await dateInputs.nth(0).fill("2026-10-01");
  await dateInputs.nth(1).fill("2026-10-31");
  await dialog.locator(".time-export-special input").check();
  await dialog.getByText("预计导出 3 行").waitFor({ state: "visible" });
  await dialog.getByText(/合计 90 分钟/).waitFor({ state: "visible" });

  await page.evaluate(() => {
    window.__TEST_SAVE_RESULT = "D:/Temp/TimeGenie_工时归属_2026-10-01_2026-10-31.xlsx";
    window.__TEST_EXPORT_MODE = "error";
  });
  await dialog.getByRole("button", { name: /选择位置并导出/ }).click();
  await dialog.getByText(/测试路径不可写/).waitFor({ state: "visible" });
  if (!(await dialog.isVisible())) throw new Error("写入失败后应保留弹框和输入");
  if (await dateInputs.nth(0).inputValue() !== "2026-10-01") throw new Error("写入失败后开始日期丢失");

  const clockBefore = await page.locator(".global-timer-main time").textContent();
  await page.evaluate(() => {
    window.__TEST_EXPORT_MODE = "delay";
    window.__TEST_EXECUTE_CALLS = 0;
  });
  await dialog.getByRole("button", { name: /选择位置并导出/ }).click();
  await page.waitForTimeout(150);
  if (!(await dialog.getByRole("button", { name: /正在导出/ }).isDisabled())) {
    throw new Error("导出执行期间按钮未禁用");
  }
  await page.waitForFunction((previousClock) => document.querySelector(".global-timer-main time")?.textContent !== previousClock, clockBefore);
  const clockAfter = await page.locator(".global-timer-main time").textContent();
  if (clockBefore === clockAfter) throw new Error("导出执行期间计时器没有继续刷新");
  await page.getByText(/已导出 3 行、90 分钟/).waitFor({ state: "visible" });
  await dialog.waitFor({ state: "hidden" });
  const executeCalls = await page.evaluate(() => window.__TEST_EXECUTE_CALLS);
  if (executeCalls !== 1) throw new Error(`重复提交保护失败，执行了 ${executeCalls} 次`);

  await page.locator(".time-history-actions button", { hasText: "导出" }).click();
  await dialog.waitFor({ state: "visible" });
  await page.evaluate(() => {
    window.__TEST_SAVE_RESULT = null;
  });
  await dialog.getByRole("button", { name: /选择位置并导出/ }).click();
  await page.waitForTimeout(300);
  if (!(await dialog.isVisible())) throw new Error("取消保存后弹框不应关闭");

  await page.setViewportSize({ width: 460, height: 760 });
  const box = await dialog.boundingBox();
  if (!box || box.x < 0 || box.x + box.width > 460) throw new Error("窄窗口下导出弹框超出视口");
  const firstDateBox = await dateInputs.nth(0).boundingBox();
  const secondDateBox = await dateInputs.nth(1).boundingBox();
  if (!firstDateBox || !secondDateBox || secondDateBox.y <= firstDateBox.y) {
    throw new Error("窄窗口下日期输入未改为纵向布局");
  }

  await browser.close();
  console.log("time allocation export UI checks passed");
}

try {
  await main();
} finally {
  if (process.platform === "win32") {
    const terminator = spawn("taskkill.exe", ["/pid", String(server.pid), "/t", "/f"], {
      stdio: "ignore",
    });
    await new Promise((resolve) => terminator.once("exit", resolve));
  } else {
    server.kill("SIGTERM");
  }
}
