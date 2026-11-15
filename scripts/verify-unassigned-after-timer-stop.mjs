import assert from "node:assert/strict";
import { createServer } from "vite";
import { chromium } from "playwright";

const harness = `
import { createApp, h } from "vue";
import { createPinia } from "pinia";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { useWorkdayStore } from "/src/store.ts";
import GlobalTimerBar from "/src/components/GlobalTimerBar.vue";
import UnassignedTimeIndicator from "/src/components/UnassignedTimeIndicator.vue";

let sequence = 0;
let current;
let unassigned;
window.snapshotCalls = 0;
function startEntry() {
  unassigned = undefined;
  current = {
    id: "timer-" + ++sequence, workDate: "2026-11-15", label: "测试事项",
    startedAt: Date.now() - 60_000, durationSeconds: 60, settlementMinutes: 1,
    allocatedMinutes: 0, allocations: [], note: "", kind: "work",
    sourceType: "timer", state: "running", version: 1,
  };
  return current;
}
startEntry();
mockWindows("main");
mockIPC((command, args) => {
  switch (command) {
    case "timer_get_state": return current;
    case "unassigned_get_state_snapshot":
      window.snapshotCalls += 1;
      return { current: unassigned };
    case "timer_start": return startEntry();
    case "timer_pause":
      current = { ...current, state: "paused", version: current.version + 1 };
      return current;
    case "timer_stop": {
      if (args.request.entryId !== current.id) throw new Error("Unexpected timer");
      const endedAt = Date.now();
      const stopped = { ...current, endedAt, state: "ended", version: current.version + 1 };
      current = null;
      unassigned = {
        sessionId: "unassigned-" + sequence, workDate: stopped.workDate, state: "collecting",
        firstStartedAt: endedAt, currentSegmentStartedAt: endedAt, elapsedSeconds: 0,
        requiredMinutes: 0, thresholdSeconds: 300, mustResolve: false, version: 1,
      };
      return { currentEntry: stopped, slices: [stopped], totalDurationSeconds: 60, totalSettlementMinutes: 1 };
    }
    default: return undefined;
  }
}, { shouldMockEvents: true });

const pinia = createPinia();
const store = useWorkdayStore(pinia);
window.store = store;
createApp({ render: () => h("main", [h(GlobalTimerBar), h(UnassignedTimeIndicator)]) })
  .use(pinia).mount("#app");
store.startClock("hover");
`;

const server = await createServer({
  server: { host: "127.0.0.1", port: 0, strictPort: false },
  plugins: [{
    name: "unassigned-after-timer-stop-test",
    resolveId: (id) => id === "virtual:unassigned-after-stop" ? "\0" + id : undefined,
    load: (id) => id === "\0virtual:unassigned-after-stop" ? harness : undefined,
  }],
});
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.clock.install({ time: new Date("2026-11-15T10:00:00+08:00") });
  await page.clock.pauseAt(new Date("2026-11-15T10:01:00+08:00"));
  await page.route("**/__unassigned-after-stop", (route) => route.fulfill({
    contentType: "text/html",
    body: '<html><head><meta charset="utf-8"></head><body><div id="app"></div><script type="module" src="/@id/__x00__virtual:unassigned-after-stop"></script></body></html>',
  }));
  await page.goto(server.resolvedUrls.local[0] + "__unassigned-after-stop");
  await page.getByTitle("结束计时", { exact: true }).waitFor();
  await page.waitForFunction(() => window.snapshotCalls > 0);

  for (const paused of [false, true]) {
    if (paused) {
      await page.evaluate(() => window.store.startTimer(""));
      assert.equal(await page.locator(".unassigned-time-indicator").count(), 0, "开始事项后应停止显示未归属计时");
      await page.getByTitle("暂停", { exact: true }).click();
      await page.waitForFunction(() => window.store.runningEntry?.state === "paused");
    }
    const previousCalls = await page.evaluate(() => window.snapshotCalls);
    await page.getByTitle("结束计时", { exact: true }).click();
    await page.waitForFunction(() => Boolean(window.store.unassignedSessionId), undefined, { timeout: 5_000 });
    assert.ok(await page.evaluate(() => window.snapshotCalls) > previousCalls, "结束事项必须即时刷新未归属状态，不能复用刚缓存的空状态");
    const stopped = await page.evaluate(() => ({
      startedAt: window.store.unassignedStartedAt,
      endedAt: window.store.timerStopConfirmationEntry.endedAt,
      sessionId: window.store.unassignedCurrentSession.sessionId,
      selectedSessionId: window.store.unassignedSessionId,
      confirmationOpen: Boolean(window.store.timerStopConfirmationEntryId),
    }));
    assert.equal(stopped.startedAt, stopped.endedAt, "新的未归属时间应从事项结束时刻开始");
    assert.equal(stopped.sessionId, stopped.selectedSessionId);
    assert.equal(stopped.confirmationOpen, true, "分配确认尚未关闭时也应开始未归属计时");
    const indicator = page.locator(".unassigned-time-indicator time");
    assert.equal(await indicator.textContent(), "00:00");
    await page.clock.runFor(3_000);
    assert.equal(await indicator.textContent(), "00:03", "结束事项后未归属显示应继续逐秒增长");
    await page.evaluate(() => window.store.dismissTimerStopConfirmation());
    await page.clock.runFor(2_000);
    assert.equal(await indicator.textContent(), "00:05", "关闭分配确认不应重置未归属计时");
  }
  assert.deepEqual(errors, []);
  console.log("Unassigned timer restart after running/paused timer stop verification passed.");
} finally {
  await browser?.close();
  await server.close();
}
