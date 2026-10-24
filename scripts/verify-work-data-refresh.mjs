import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-work-data-refresh-test/workDataRefresh.js");
const {
  partitionWorkDataRefreshDomains,
  shouldClearDirectCloudMemory,
  shouldRefreshDirectCloudData,
  workDataChangeDomainsForMutation,
  workDataRefreshPlan,
} = await import(pathToFileURL(modulePath));

assert.deepEqual(workDataRefreshPlan(["reports"], true, "reports"), {
  workspace: false, time: false, reports: true, unassigned: false, todayOverview: false,
});
assert.deepEqual(workDataRefreshPlan(["task_daily_estimates", "tasks"], true, "today"), {
  workspace: true, time: false, reports: false, unassigned: false, todayOverview: true,
});
assert.deepEqual(workDataRefreshPlan(["time", "unassigned"], false), {
  workspace: false, time: true, reports: false, unassigned: true, todayOverview: false,
});
assert.deepEqual(workDataRefreshPlan(["reports"], true, "today"), {
  workspace: false, time: false, reports: false, unassigned: false, todayOverview: false,
});
assert.deepEqual(partitionWorkDataRefreshDomains(["tasks", "reports", "settings", "time"], "today", true), {
  visible: ["tasks", "time"],
  deferred: ["reports", "settings"],
});
assert.deepEqual(partitionWorkDataRefreshDomains(["reports", "settings"], "reports", true), {
  visible: ["reports"],
  deferred: ["settings"],
});

const staleDomains = new Set();
let notificationCount = 0;
for (const payload of [["reports"], ["reports"], ["reports"]]) {
  const before = staleDomains.size;
  payload.forEach((domain) => staleDomains.add(domain));
  if (staleDomains.size !== before) notificationCount += 1;
}
assert.equal(notificationCount, 1);
assert.deepEqual(partitionWorkDataRefreshDomains(staleDomains, "reports", true).visible, ["reports"]);
assert.deepEqual(partitionWorkDataRefreshDomains(staleDomains, "today", true).visible, []);
assert.deepEqual(workDataChangeDomainsForMutation("subject"), ["subjects"]);
assert.deepEqual(workDataChangeDomainsForMutation("task"), ["tasks"]);
assert.equal(shouldClearDirectCloudMemory("offline_readonly"), false);
assert.equal(shouldClearDirectCloudMemory("degraded_readonly"), false);
assert.equal(shouldClearDirectCloudMemory("connecting"), false);
assert.equal(shouldClearDirectCloudMemory("online_writable"), false);
assert.equal(shouldClearDirectCloudMemory("reauth_required"), true);
assert.equal(shouldClearDirectCloudMemory("protocol_incompatible"), true);
assert.equal(shouldRefreshDirectCloudData("", "online_writable"), true);
assert.equal(shouldRefreshDirectCloudData("connecting", "online_writable"), true);
assert.equal(shouldRefreshDirectCloudData("offline_readonly", "online_writable"), true);
assert.equal(shouldRefreshDirectCloudData("online_writable", "online_writable"), false);
assert.equal(shouldRefreshDirectCloudData("online_writable", "offline_readonly"), false);

rmSync(resolve(".tmp-work-data-refresh-test"), { recursive: true, force: true });
console.log("work data refresh verification passed");
