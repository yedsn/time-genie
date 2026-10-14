import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-work-data-refresh-test/workDataRefresh.js");
const { workDataChangeDomainsForMutation, workDataRefreshPlan } = await import(pathToFileURL(modulePath));

assert.deepEqual(workDataRefreshPlan(["reports"], true), {
  workspace: false, time: false, reports: true, unassigned: false, todayOverview: false,
});
assert.deepEqual(workDataRefreshPlan(["task_daily_estimates", "tasks"], true), {
  workspace: true, time: false, reports: false, unassigned: false, todayOverview: true,
});
assert.deepEqual(workDataRefreshPlan(["time", "unassigned"], false), {
  workspace: false, time: true, reports: false, unassigned: true, todayOverview: false,
});
assert.deepEqual(workDataChangeDomainsForMutation("subject"), ["subjects"]);
assert.deepEqual(workDataChangeDomainsForMutation("task"), ["tasks"]);

rmSync(resolve(".tmp-work-data-refresh-test"), { recursive: true, force: true });
console.log("work data refresh verification passed");
