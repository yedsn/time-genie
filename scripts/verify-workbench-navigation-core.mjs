import assert from "node:assert/strict";
import { createHoverFrameCommitter } from "../.tmp-workbench-navigation-test/hoverNavigationCore.js";
import { recordWorkbenchNavigationMetric, resetWorkbenchNavigationMetrics, workbenchNavigationMetricsSnapshot } from "../.tmp-workbench-navigation-test/workbenchNavigationMetrics.js";
import { DirectCloudQueryCoordinator } from "../.tmp-workbench-navigation-test/directCloudQueryCoordinatorCore.js";

let queued;
let cancelled = false;
const committer = createHoverFrameCommitter({
  request(callback) { queued = callback; return 1; },
  cancel() { cancelled = true; queued = undefined; },
});
const committed = [];
committer.schedule(() => committed.push("today"));
committer.schedule(() => committed.push("reports"));
queued();
assert.deepEqual(committed, ["reports"]);
committer.schedule(() => committed.push("settings"));
committer.cancel();
assert.equal(cancelled, true);
assert.deepEqual(committed, ["reports"]);

resetWorkbenchNavigationMetrics();
recordWorkbenchNavigationMetric("pageMounted", "reports");
recordWorkbenchNavigationMetric("domainQueryStarted", "reports");
recordWorkbenchNavigationMetric("modulePreloaded");
const snapshot = workbenchNavigationMetricsSnapshot();
assert.equal(snapshot.pageMounted, 1);
assert.equal(snapshot.domainQueryStarted, 1);
assert.equal(snapshot.modulePreloaded, 1);
assert.deepEqual(snapshot.pages, { "pageMounted:reports": 1 });
assert.deepEqual(snapshot.domains, { reports: 1 });
const serialized = JSON.stringify(snapshot);
for (const sensitive of ["taskTitle", "reportMarkdown", "accessToken", "anonKey", "credential"]) assert.equal(serialized.includes(sensitive), false);

let resolveReportQuery;
let reportQueryCalls = 0;
const coordinator = new DirectCloudQueryCoordinator();
const firstReportQuery = coordinator.run("reports:subject", async () => {
  reportQueryCalls += 1;
  await new Promise((resolve) => { resolveReportQuery = resolve; });
  return ["report"];
}, { maxAgeMs: Number.POSITIVE_INFINITY });
const reusedReportQuery = coordinator.run("reports:subject", async () => {
  reportQueryCalls += 1;
  return [];
}, { maxAgeMs: Number.POSITIVE_INFINITY });
resolveReportQuery();
assert.deepEqual(await firstReportQuery, ["report"]);
assert.deepEqual(await reusedReportQuery, ["report"]);
assert.equal(reportQueryCalls, 1, "相同报告查询并发时应复用同一个结果");
console.log("workbench navigation core verification passed");
