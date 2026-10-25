import assert from "node:assert/strict";
import { chromium } from "playwright";
import { preview } from "vite";

const host = "127.0.0.1";
const port = 5302;
const now = Date.now();
const server = await preview({ configFile: "vite.config.ts", preview: { host, port, strictPort: true } });
const browser = await chromium.launch({ headless: true });

function installTauriMock(page, hoverNavigationEnabled = true) {
  return page.addInitScript(({ currentNow, hoverEnabled }) => {
    const originalAddEventListener = EventTarget.prototype.addEventListener;
    const originalRemoveEventListener = EventTarget.prototype.removeEventListener;
    window.__listenerCounts = { added: 0, removed: 0 };
    EventTarget.prototype.addEventListener = function (...args) {
      window.__listenerCounts.added += 1;
      return originalAddEventListener.apply(this, args);
    };
    EventTarget.prototype.removeEventListener = function (...args) {
      window.__listenerCounts.removed += 1;
      return originalRemoveEventListener.apply(this, args);
    };
    const callbacks = new Map();
    const listeners = new Map();
    const delayedReportSubjects = new Map();
    let callbackId = 1;
    window.__commandCounts = {};
    const settings = {
      storageMode: "local", workspaceId: "workspace", deviceId: "device",
      shared: { timezone: "Asia/Shanghai" }, device: { app_theme: "forest", hover_navigation_enabled: hoverEnabled },
      integrations: {}, secrets: { seatableTokenSet: false, supabaseSessionSet: false },
    };
    const storage = { mode: "local", workspaceId: "workspace", deviceId: "device", online: true, syncState: "synced", pendingOperations: 0, conflictCount: 0, lastChangeSeq: 0, authBlocked: false, workspaceEpoch: 1, epochBlocked: false };
    const subject = { id: "subject", name: "默认", sortOrder: 10, openTaskCount: 1, version: 1 };
    const rapidSubjectA = { id: "rapid-a", name: "快速 A", sortOrder: 20, openTaskCount: 0, version: 1 };
    const rapidSubjectB = { id: "rapid-b", name: "快速 B", sortOrder: 30, openTaskCount: 0, version: 1 };
    const subjects = [subject, rapidSubjectA, rapidSubjectB];
    const task = { id: "task", subjectId: subject.id, title: "缓存事项", status: "open", sortOrder: 10, depth: 0, pathLabel: "缓存事项", selectable: true, directMinutes: 0, totalMinutes: 0, executionState: "not_started", version: 1 };
    const reports = [
      { id: "report", reportType: "daily", subjectId: subject.id, subjectName: subject.name, period: "2026-10-25", referenceDate: "2026-10-25", taskIds: [], markdown: "初始报告内容", contentSource: "generated", generatedCount: 1, updatedAt: currentNow, version: 1 },
      { id: "report-previous", reportType: "daily", subjectId: subject.id, subjectName: subject.name, period: "2026-10-24", referenceDate: "2026-10-24", taskIds: [], markdown: "上一份报告内容", contentSource: "generated", generatedCount: 1, updatedAt: currentNow - 86_400_000, version: 1 },
    ];
    const emptyOverview = { scope: { subjectName: "全部主体", todayDate: "2026-10-25" }, summary: { actualMinutes: 0, estimatedMinutes: 0, completedCount: 0, activeCount: 0, notStartedCount: 0, unassignedMinutes: 0, completionRate: 0 }, days: [], timeline: [], tasks: [], revision: 1 };
    const result = (cmd, args = {}) => {
      window.__commandCounts[cmd] = (window.__commandCounts[cmd] ?? 0) + 1;
      switch (cmd) {
        case "settings_get": return settings;
        case "storage_mode_get": return storage;
        case "cloud_session_get": return { signedIn: false, status: "reauth_required" };
        case "window_is_main_maximized":
        case "plugin:window|is_maximized": return false;
        case "plugin:window|is_focused": return true;
        case "workbench_bootstrap_get": return { timezone: "Asia/Shanghai", currentWorkDate: "2026-10-25", currentDayStartAt: currentNow - 3_600_000, currentDayEndAt: currentNow + 86_400_000, subjects };
        case "work_calendar_get_context": return { timezone: "Asia/Shanghai", currentWorkDate: "2026-10-25", currentDayStartAt: currentNow - 3_600_000, currentDayEndAt: currentNow + 86_400_000 };
        case "subject_list": return subjects;
        case "task_list": return { tasks: [task], revision: 1 };
        case "task_day_plan_snapshot": return { subjectId: subject.id, workDate: "2026-10-25", tasks: [task], revision: 1 };
        case "time_entry_list": return { entries: [], rawMinutes: 0, allocatedMinutes: 0, pendingMinutes: 0, breakMinutes: 0, revision: 1 };
        case "timer_get_state": return null;
        case "time_entry_history_list": return { page: 1, pageSize: 20, totalDays: 0, totalPages: 0, groups: [], revision: 1 };
        case "report_list": return { reports, page: 1, pageSize: 20, total: reports.length, hasMore: false, typeCounts: { all: reports.length, daily: reports.length, weekly: 0, monthly: 0 } };
        case "report_save_content": {
          const index = reports.findIndex((item) => item.id === args.request.reportId);
          reports[index] = { ...reports[index], markdown: args.request.markdown, updatedAt: currentNow + 1, version: reports[index].version + 1 };
          return reports[index];
        }
        case "unassigned_get_state_snapshot": return {};
        case "today_work_overview_get": return emptyOverview;
        case "data_backup_history_list": return [];
        case "automation_hook_list": return [];
        default: return undefined;
      }
    };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
      transformCallback(callback, once = false) { const id = callbackId++; callbacks.set(id, (payload) => { if (once) callbacks.delete(id); return callback?.(payload); }); return id; },
      unregisterCallback(id) { callbacks.delete(id); }, runCallback(id, payload) { callbacks.get(id)?.(payload); }, convertFileSrc(path) { return path; },
      async invoke(cmd, args = {}) {
        if (cmd === "plugin:event|listen") { const values = listeners.get(args.event) ?? []; values.push(args.handler); listeners.set(args.event, values); return args.handler; }
        if (cmd === "plugin:event|unlisten") return null;
        if (cmd === "plugin:event|emit") { for (const id of listeners.get(args.event) ?? []) callbacks.get(id)?.({ event: args.event, id, payload: args.payload }); return null; }
        if (cmd === "report_list" && args.request?.subjectId?.startsWith("rapid-")) {
          return await new Promise((resolve) => {
            const pending = delayedReportSubjects.get(args.request.subjectId) ?? [];
            pending.push(() => resolve(result(cmd, args)));
            delayedReportSubjects.set(args.request.subjectId, pending);
          });
        }
        return result(cmd, args);
      },
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__emitMockEvent = (event, payload) => {
      for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
    };
    window.__resolveDelayedReport = (subjectId) => {
      const pending = delayedReportSubjects.get(subjectId) ?? [];
      pending.shift()?.();
      if (!pending.length) delayedReportSubjects.delete(subjectId);
    };
    window.__delayedReportCount = (subjectId) => delayedReportSubjects.get(subjectId)?.length ?? 0;
  }, { currentNow: now, hoverEnabled: hoverNavigationEnabled });
}

try {
  const page = await browser.newPage({ viewport: { width: 1360, height: 900 } });
  await installTauriMock(page);
  await page.goto(`http://${host}:${port}`, { waitUntil: "networkidle" });
  await page.waitForSelector('[data-workbench-page="today"]');
  const nav = page.locator(".nav-list");
  const navButton = (label) => nav.getByRole("button", { name: label, exact: true });
  const pageMountCounts = async () => page.evaluate(() => Object.fromEntries(
    Object.entries(window.__TIMEGENIE_WORKBENCH_METRICS__().pages).filter(([key]) => key.startsWith("pageMounted:")),
  ));

  await page.evaluate(() => {
    const buttons = [...document.querySelectorAll(".nav-list > button")];
    for (const button of buttons.slice(1, 4)) button.dispatchEvent(new MouseEvent("mouseenter", { bubbles: false }));
  });
  await page.waitForFunction(() => document.querySelector(".topbar-title h1")?.textContent?.includes("报告"));
  assert.equal(await page.locator('[data-workbench-page="plan"]').count(), 0, "同帧扫过不应挂载中间待办页");
  assert.equal(await page.locator('[data-workbench-page="timer"]').count(), 0, "同帧扫过不应挂载中间时间页");
  await page.locator('[data-workbench-page="reports"] .daily-markdown-editor').waitFor({ state: "visible" });
  assert.equal(await page.locator('[data-workbench-page="reports"] .report-record').count(), 2, "报告列表首次进入应显示已加载记录");
  const subjectTabs = page.locator('[data-workbench-page="reports"] .report-subject-tabs');
  await subjectTabs.getByRole("tab", { name: "快速 A", exact: true }).click();
  await subjectTabs.getByRole("tab", { name: "快速 B", exact: true }).click();
  await subjectTabs.getByRole("tab", { name: "快速 A", exact: true }).click();
  await page.waitForFunction(() => window.__delayedReportCount("rapid-a") === 1 && window.__delayedReportCount("rapid-b") === 1);
  await page.evaluate(() => window.__resolveDelayedReport("rapid-a"));
  await page.waitForFunction(() => window.__delayedReportCount("rapid-a") === 1);
  await page.evaluate(() => { window.__resolveDelayedReport("rapid-a"); window.__resolveDelayedReport("rapid-b"); });
  await page.locator('[data-workbench-page="reports"] .report-record').first().waitFor();
  assert.equal(await page.locator('[data-workbench-page="reports"] .report-record').count(), 2, "快速往返筛选后应补发当前请求并恢复列表");
  await subjectTabs.getByRole("tab", { name: "默认", exact: true }).click();
  assert.equal(await page.locator('[data-workbench-page="reports"] .report-save-state').textContent().then((text) => text?.includes("已保存")), true, "首次加载报告不应误报未保存修改");
  assert.equal(await page.locator('[data-workbench-page="reports"] .report-save-state small').textContent(), "6 字符", "首次加载应装载已保存正文");

  const reportRecords = page.locator('[data-workbench-page="reports"] .report-record');
  await reportRecords.nth(1).locator(".report-record-main").click();
  await reportRecords.nth(1).filter({ has: page.locator(".report-record-main") }).waitFor();
  assert.equal(await page.locator(".report-switch-dialog").count(), 0, "没有编辑时切换报告不应弹出保存确认");
  assert.equal(await reportRecords.nth(1).getAttribute("class").then((value) => value?.includes("active")), true, "没有编辑时应直接切换报告");
  await reportRecords.nth(0).locator(".report-record-main").click();

  await page.locator('[data-workbench-page="reports"] .report-filter-tabs button.type-daily').click();
  await page.locator('[data-workbench-page="reports"] .report-record.active').waitFor();
  const editor = page.locator('[data-workbench-page="reports"] .cm-content[contenteditable=true]');
  await editor.click();
  await page.keyboard.press("Control+A");
  await page.keyboard.insertText("准备放弃的修改");
  await reportRecords.nth(1).locator(".report-record-main").click();
  const switchDialog = page.locator(".report-switch-dialog");
  await switchDialog.waitFor({ state: "visible" });
  for (const label of ["取消", "放弃修改并切换", "保存并切换"]) {
    assert.equal(await switchDialog.getByRole("button", { name: label, exact: true }).count(), 1, `切换确认框应提供“${label}”操作`);
  }
  await switchDialog.getByRole("button", { name: "取消", exact: true }).click();
  assert.equal(await reportRecords.nth(0).getAttribute("class").then((value) => value?.includes("active")), true, "取消后应留在当前报告");
  assert.equal(await page.locator('[data-workbench-page="reports"] .report-save-state small').textContent(), "7 字符", "取消后应保留当前草稿");
  await reportRecords.nth(1).locator(".report-record-main").click();
  await switchDialog.getByRole("button", { name: "放弃修改并切换", exact: true }).click();
  assert.equal(await reportRecords.nth(1).getAttribute("class").then((value) => value?.includes("active")), true, "放弃修改后应切换目标报告");
  assert.equal(await page.evaluate(() => window.__commandCounts.report_save_content ?? 0), 0, "放弃修改不应调用保存接口");
  await reportRecords.nth(0).locator(".report-record-main").click();

  const draftText = "缓存草稿：离开报告后仍需保留";
  await editor.click();
  await page.keyboard.press("Control+A");
  await page.keyboard.insertText(draftText);
  await page.locator('[data-workbench-page="reports"] .report-mode-tabs').getByRole("button", { name: "预览", exact: true }).click();
  await page.locator('[data-workbench-page="reports"] .report-save-state').filter({ hasText: "未保存修改" }).waitFor();
  const reportsBeforeRealtime = await page.evaluate(() => window.__commandCounts.report_list ?? 0);
  await page.evaluate(() => window.__emitMockEvent("work-data-changed", { sourceWindowLabel: "hover", changeSeq: 1, workspaceEpoch: 1, domains: ["reports"] }));
  await page.waitForFunction((previous) => (window.__commandCounts.report_list ?? 0) > previous, reportsBeforeRealtime);
  assert.equal(await page.locator('[data-workbench-page="reports"] .report-save-state small').textContent(), `${draftText.length} 字符`, "后台刷新不应覆盖未保存草稿");

  await navButton("时间记录").click();
  await page.waitForFunction(() => document.querySelector('[data-workbench-page="timer"]')?.checkVisibility());
  const historyAfterFirstVisit = await page.evaluate(() => window.__commandCounts.time_entry_history_list ?? 0);
  await page.getByRole("button", { name: "手动补录", exact: true }).click();
  await page.locator('[data-workbench-page="timer"] .manual-entry-row input[type="number"]').fill("47");
  await page.locator('[data-workbench-page="timer"]').evaluate((element) => { element.style.minHeight = "1200px"; });
  await page.locator(".content-scroll").evaluate((element) => { element.scrollTop = 120; element.dispatchEvent(new Event("scroll")); });
  await navButton("报告").click();
  await page.waitForFunction(() => document.querySelector('[data-workbench-page="reports"]')?.checkVisibility());
  assert.equal((await page.locator('[data-workbench-page="reports"] .report-filter-tabs button.active').textContent())?.includes("日报"), true, "报告类型筛选应保留");
  assert.equal(await page.locator('[data-workbench-page="reports"] .report-mode-tabs button.active').textContent(), "预览", "报告编辑模式应保留");
  assert.equal(await page.locator('[data-workbench-page="reports"] .report-record.active').count(), 1, "所选报告应保留");
  assert.equal(await page.locator('[data-workbench-page="reports"] .report-save-state').textContent().then((text) => text?.includes("未保存修改")), true, "报告草稿的未保存状态应保留");
  await page.locator('[data-workbench-page="reports"] .report-mode-tabs').getByRole("button", { name: "编辑", exact: true }).click();
  assert.equal(await page.locator('[data-workbench-page="reports"] .report-save-state small').textContent(), `${draftText.length} 字符`, "报告草稿内容长度应保留");
  const savesBeforeSwitch = await page.evaluate(() => window.__commandCounts.report_save_content ?? 0);
  await reportRecords.nth(1).locator(".report-record-main").click();
  await switchDialog.getByRole("button", { name: "保存并切换", exact: true }).click();
  await page.waitForFunction((previous) => (window.__commandCounts.report_save_content ?? 0) > previous, savesBeforeSwitch);
  assert.equal(await reportRecords.nth(1).getAttribute("class").then((value) => value?.includes("active")), true, "保存成功后应切换目标报告");
  await navButton("时间记录").click();
  assert.equal(await page.locator('[data-workbench-page="timer"] .manual-entry-row input[type="number"]').inputValue(), "47", "时间记录局部表单应保留");
  assert.ok(await page.locator(".content-scroll").evaluate((element) => element.scrollTop) >= 100, "页面滚动位置应恢复");
  assert.equal(await page.evaluate(() => window.__commandCounts.time_entry_history_list ?? 0), historyAfterFirstVisit, "返回时间记录不应重复查询未失效缓存");

  await navButton("待办").click();
  const planCalls = await page.evaluate(() => window.__commandCounts.task_day_plan_snapshot ?? 0);
  await page.getByRole("tab", { name: /进行中/ }).click();
  await page.waitForFunction(() => document.querySelector('[data-workbench-page="plan"] [role="tab"][aria-selected="true"]')?.textContent?.includes("进行中"));
  await navButton("今日").click();
  await page.waitForFunction(() => document.querySelector('[data-workbench-page="today"]')?.checkVisibility());
  await navButton("待办").click();
  await page.waitForFunction(() => document.querySelector('[data-workbench-page="plan"]')?.checkVisibility());
  assert.equal(await page.locator('[data-workbench-page="plan"] [role="tab"]', { hasText: "进行中" }).getAttribute("aria-selected"), "true", "待办标签应在返回后保留");
  assert.equal(await page.evaluate(() => window.__commandCounts.task_day_plan_snapshot ?? 0), planCalls, "返回待办不应强制刷新今日计划");

  await navButton("设置").click();
  for (const selector of [".backup-settings", ".operation-history-settings", ".snapshot-settings", ".hooks-panel"]) {
    assert.equal(await page.locator(selector).count(), 0, `初次进入设置不应创建 ${selector}`);
  }
  const backupQueries = await page.evaluate(() => window.__commandCounts.data_backup_history_list ?? 0);
  await page.getByRole("button", { name: "本地备份", exact: true }).hover();
  await page.locator(".backup-settings").waitFor({ state: "attached" });
  assert.equal(await page.evaluate(() => window.__commandCounts.data_backup_history_list ?? 0), backupQueries + 1, "备份区域首次接近时才查询");
  await page.getByRole("button", { name: "自动化钩子", exact: true }).click();
  await page.locator(".hooks-panel").waitFor({ state: "attached" });
  assert.equal(await page.evaluate(() => window.__commandCounts.automation_hook_list ?? 0), 1, "自动化设置首次访问只查询一次");
  await navButton("今日").click();
  await navButton("设置").click();
  assert.equal(await page.locator(".backup-settings").count(), 1, "已创建的重量设置区域应保留");
  assert.equal(await page.locator(".hooks-panel").count(), 1, "已创建的自动化设置区域应保留");
  assert.equal(await page.evaluate(() => window.__commandCounts.automation_hook_list ?? 0), 1, "返回设置不应重复查询自动化配置");

  for (const label of ["今日", "待办", "时间记录", "报告", "设置"]) await navButton(label).click();
  const mountedAfterWarmup = await pageMountCounts();
  for (const pageId of ["today", "plan", "timer", "reports", "settings"]) assert.equal(mountedAfterWarmup[`pageMounted:${pageId}`], 1, `${pageId} 页面应只创建一次`);
  const samples = await page.evaluate(async () => {
    const result = [];
    const buttons = [...document.querySelectorAll(".nav-list > button")];
    for (let index = 0; index < 25; index += 1) {
      const target = index % 2 ? "今日" : "报告";
      const button = buttons.find((item) => item.textContent?.includes(target));
      const started = performance.now();
      button.click();
      await new Promise((resolve) => requestAnimationFrame(resolve));
      if (!document.querySelector(".topbar-title h1")?.textContent?.includes(target)) throw new Error(`页面未切换到${target}`);
      result.push(performance.now() - started);
    }
    return result;
  });
  samples.sort((a, b) => a - b);
  const p95 = samples[Math.ceil(samples.length * 0.95) - 1];
  assert.ok(p95 <= 50, `缓存页面切换 P95 ${p95.toFixed(2)}ms 超过 50ms`);

  const beforeStress = await page.evaluate(() => ({ nodes: document.querySelectorAll("*").length, metrics: window.__TIMEGENIE_WORKBENCH_METRICS__(), listeners: window.__listenerCounts.added - window.__listenerCounts.removed }));
  const mountCountsBeforeStress = await pageMountCounts();
  for (let index = 0; index < 100; index += 1) await navButton(index % 2 ? "今日" : "报告").click();
  const afterStress = await page.evaluate(() => ({ nodes: document.querySelectorAll("*").length, metrics: window.__TIMEGENIE_WORKBENCH_METRICS__(), listeners: window.__listenerCounts.added - window.__listenerCounts.removed, memory: performance.memory?.usedJSHeapSize }));
  assert.deepEqual(await pageMountCounts(), mountCountsBeforeStress, "100 次切换不应创建新的页面实例");
  assert.ok(afterStress.nodes <= beforeStress.nodes + 20, "往返切换不应线性增加 DOM");
  assert.ok(afterStress.listeners <= beforeStress.listeners + 2, "往返切换不应持续增加事件监听");

  const preloadPage = await browser.newPage({ viewport: { width: 1360, height: 900 } });
  await installTauriMock(preloadPage, false);
  await preloadPage.goto(`http://${host}:${port}`, { waitUntil: "networkidle" });
  await preloadPage.waitForSelector('[data-workbench-page="today"]');
  await preloadPage.locator(".nav-list").getByRole("button", { name: "报告", exact: true }).hover();
  await preloadPage.waitForFunction(() => window.__TIMEGENIE_WORKBENCH_METRICS__().modulePreloaded >= 1);
  assert.equal(await preloadPage.locator('[data-workbench-page="reports"]').count(), 0, "代码预加载不应挂载报告页");
  assert.equal(await preloadPage.evaluate(() => window.__commandCounts.report_list ?? 0), 0, "代码预加载不应查询报告数据");
  await preloadPage.close();

  const reducedPage = await browser.newPage({ viewport: { width: 1360, height: 900 }, reducedMotion: "reduce" });
  await installTauriMock(reducedPage);
  await reducedPage.goto(`http://${host}:${port}`, { waitUntil: "networkidle" });
  await reducedPage.waitForSelector('[data-workbench-page="today"]');
  assert.equal(await reducedPage.locator('[data-workbench-page="today"]').evaluate((element) => getComputedStyle(element).animationName), "none", "减少动态效果时不应播放页面入场动画");
  await reducedPage.close();

  console.log(JSON.stringify({ cachedSwitchP95Ms: p95, samples: samples.length, beforeStress, afterStress, commandCounts: await page.evaluate(() => window.__commandCounts) }, null, 2));
  console.log("workbench navigation UI verification passed");
} finally {
  await browser.close();
  await server.httpServer.close();
}
