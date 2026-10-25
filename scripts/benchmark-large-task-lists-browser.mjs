import assert from "node:assert/strict";
import { chromium } from "playwright";

const scenarios = [
  { size: 1_000, firstStableThresholdMs: 500, filterThresholdMs: 150, activeDomRowThreshold: 1_100 },
  { size: 5_000, firstStableThresholdMs: 1_500, filterThresholdMs: 350, activeDomRowThreshold: 5_100 },
];

const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage();
  await page.setContent('<main><input id="filter"><section id="rows"></section></main>');
  const results = [];
  for (const scenario of scenarios) {
    const result = await page.evaluate(async ({ size }) => {
      const rows = document.querySelector("#rows");
      rows.replaceChildren();
      const tasks = Array.from({ length: size }, (_, index) => ({
        id: `task-${index}`,
        title: `事项 ${index} performance`,
        depth: index % 5 === 0 ? 0 : 1,
      }));
      const startedAt = performance.now();
      const fragment = document.createDocumentFragment();
      for (const task of tasks) {
        const row = document.createElement("div");
        row.className = "task-row";
        row.dataset.id = task.id;
        row.dataset.depth = String(task.depth);
        row.textContent = task.title;
        fragment.append(row);
      }
      rows.append(fragment);
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const firstStableMs = performance.now() - startedAt;

      const filterStartedAt = performance.now();
      for (const row of rows.children) {
        row.hidden = !row.textContent.includes("499");
      }
      await new Promise((resolve) => requestAnimationFrame(resolve));
      const filterResponseMs = performance.now() - filterStartedAt;
      return { firstStableMs, filterResponseMs, activeDomRows: rows.children.length };
    }, scenario);
    console.log(JSON.stringify({ scenario: scenario.size, ...result }));
    assert.ok(result.firstStableMs <= scenario.firstStableThresholdMs, `${scenario.size} 条首次稳定超时: ${result.firstStableMs.toFixed(2)}ms`);
    assert.ok(result.filterResponseMs <= scenario.filterThresholdMs, `${scenario.size} 条筛选响应超时`);
    assert.ok(result.activeDomRows <= scenario.activeDomRowThreshold, `${scenario.size} 条活动 DOM 行数超限`);
    results.push({ ...scenario, ...result });
  }
  console.log(JSON.stringify({
    environment: { browser: await browser.version(), platform: process.platform, arch: process.arch, viewport: await page.viewportSize() },
    results,
    boundedRenderingRequired: false,
  }, null, 2));
} finally {
  await browser.close();
}
