import assert from "node:assert/strict";
import { createServer } from "vite";
import { chromium } from "playwright";

const harness = `
import { createApp, h, ref } from "vue";
import ElementPlus from "element-plus";
import "element-plus/dist/index.css";
import PlanListEditor from "/src/components/PlanListEditor.vue";
import "/src/styles.css";
const tasks = [
  { id: "parent", title: "一级事项", selectable: false, todayMember: false },
  { id: "child", title: "二级事项", parentId: "parent", selectable: true, todayMember: true },
  { id: "sibling", title: "另一条二级事项", parentId: "parent", selectable: true, todayMember: true },
  { id: "root", title: "另一条一级事项", selectable: true, todayMember: false },
  { id: "done", title: "已完成的今日事项", status: "done", selectable: true, todayMember: true },
].map(task => ({ subjectId: "subject", status: "open", version: 1, actual: 0, directMinutes: 0, ...task }));
const mode = ref("active");
window.setTestMode = value => { mode.value = value; };
window.moveCalls = [];
createApp({ setup: () => () => h(PlanListEditor, {
  tasks, displayTasks: mode.value === "today" ? tasks : undefined, mode: mode.value,
  subjectId: "subject", subjectName: "待办操作按钮", selectedId: "parent",
  todayDate: "2026-11-03", timezone: "Asia/Shanghai", formatMinutes: minutes => minutes + " 分钟",
  moveTask: async (...args) => window.moveCalls.push(args), saveTask: async () => false,
  saveTodayImport: async () => {}, createTask: async () => "", createRelativeTask: async () => "",
  duplicateSingleTask: async () => undefined, savePlanBatch: async () => ({}),
  loadActualSnapshot: async () => ({}), saveActualBatch: async () => ({}),
}) }).use(ElementPlus).mount("#app");
`;
const server = await createServer({
  server: { host: "127.0.0.1", port: 0, strictPort: false },
  plugins: [{
    name: "task-position-test",
    resolveId: id => id === "virtual:task-position-test" ? String.fromCharCode(0) + id : undefined,
    load: id => id === String.fromCharCode(0) + "virtual:task-position-test" ? harness : undefined,
  }],
});
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 760 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.route("**/__task-position-test", route => route.fulfill({
    contentType: "text/html",
    body: '<html><head><meta charset="utf-8"></head><body style="background:var(--panel);padding:20px"><div id="app"></div><script type="module" src="/@id/__x00__virtual:task-position-test"></script></body></html>',
  }));
  await page.goto(server.resolvedUrls.local[0] + "__task-position-test");
  const rows = page.locator(".pending-reminder-row");
  await rows.first().waitFor();
  const parent = page.locator('[data-task-id="parent"]');
  for (const mode of ["active", "today"]) {
    await page.evaluate(value => window.setTestMode(value), mode);
    for (const width of [1280, 920, 720]) {
      await page.setViewportSize({ width, height: 760 });
      await page.mouse.move(0, 0);
      await page.waitForTimeout(150);
      const geometry = await rows.evaluateAll(elements => elements.map(row => ({
        columns: [...row.querySelectorAll(".task-position-controls button")].map(button => {
          const bounds = button.getBoundingClientRect();
          return { left: bounds.left, width: bounds.width };
        }),
        right: row.querySelector(".task-secondary-controls").getBoundingClientRect().right,
        hidden: getComputedStyle(row.querySelector(".task-position-controls")).opacity === "0",
        blocked: getComputedStyle(row.querySelector(".task-position-controls")).pointerEvents === "none",
      })));
      assert.ok(geometry.length > 2);
      for (const item of geometry) {
        assert.equal(item.columns.length, 4);
        assert.equal(item.hidden, true, "selection alone must not display position controls");
        assert.equal(item.blocked, true);
        assert.ok(item.right <= width, "secondary actions must remain within the viewport");
        item.columns.forEach((column, index) => {
          assert.equal(column.width, 24);
          assert.ok(Math.abs(column.left - geometry[0].columns[index].left) < 0.5, "parent and child button columns must align");
        });
      }
      await parent.hover();
      await page.waitForTimeout(150);
      const group = parent.getByRole("group", { name: "调整事项位置" });
      assert.equal(await group.evaluate(element => getComputedStyle(element).opacity), "1");
      assert.equal(await group.evaluate(element => getComputedStyle(element).pointerEvents), "auto");
      assert.equal(await page.locator('[data-task-id="child"] .task-position-controls').evaluate(element => getComputedStyle(element).opacity), "0");
      assert.equal(await parent.getByRole("button", { name: "减少缩进", exact: true }).isDisabled(), true);
    }
  }
  await page.mouse.move(0, 0);
  const down = parent.getByRole("button", { name: "下移", exact: true });
  await down.focus();
  await page.waitForTimeout(150);
  assert.equal(await parent.locator(".task-position-controls").evaluate(element => getComputedStyle(element).opacity), "1");
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => window.moveCalls.length === 1);
  assert.equal(await page.evaluate(() => window.moveCalls[0][0]), "parent");
  const touch = await browser.newContext({ hasTouch: true, viewport: { width: 720, height: 760 } });
  const touchPage = await touch.newPage();
  await touchPage.route("**/__task-position-test", route => route.fulfill({ contentType: "text/html", body: '<html><body><div id="app"></div><script type="module" src="/@id/__x00__virtual:task-position-test"></script></body></html>' }));
  await touchPage.goto(server.resolvedUrls.local[0] + "__task-position-test");
  await touchPage.locator(".task-position-controls").first().waitFor();
  assert.equal(await touchPage.locator(".task-position-controls").first().evaluate(element => getComputedStyle(element).opacity), "1");
  await touch.close();
  assert.deepEqual(errors, []);
  console.log("Task position controls browser checks passed: both tabs, aligned columns at 1280/920/720px, hover-only visibility, keyboard access, touch fallback.");
} finally {
  await browser?.close();
  await server.close();
}
