import assert from "node:assert/strict";
import { DirectCloudQueryCoordinator, directCloudQueryKey } from "../.tmp-direct-cloud-performance-test/directCloudQueryCoordinatorCore.js";

let now = 1_000;
const metrics = [];
const coordinator = new DirectCloudQueryCoordinator(() => now, (metric, key) => metrics.push([metric, key]));
assert.equal(directCloudQueryKey("todayPlan", "subject-a", "2026-10-23"), "todayPlan:subject-a:2026-10-23");

let resolveFirst;
let calls = 0;
const first = coordinator.run("workspace", async () => {
  calls += 1;
  return await new Promise((resolve) => { resolveFirst = resolve; });
}, { force: true, sequence: 1, epoch: 7 });
const reused = coordinator.run("workspace", async () => { calls += 1; return "duplicate"; }, { force: true, sequence: 1, epoch: 7 });
assert.equal(calls, 1, "同键并发查询必须复用进行中的请求");
resolveFirst("loaded");
assert.equal(await first, "loaded");
assert.equal(await reused, "loaded");
assert.equal(metrics.some(([metric]) => metric === "reused"), true);

now += 10;
assert.equal(await coordinator.run("workspace", async () => "unexpected", { maxAgeMs: 100, epoch: 7 }), undefined);
assert.equal(metrics.some(([metric]) => metric === "skipped_fresh"), true);

let releaseRefresh;
const refresh = coordinator.run("time:2026-10-23", async () => {
  calls += 1;
  if (!releaseRefresh) return await new Promise((resolve) => { releaseRefresh = resolve; });
  return "followup";
}, { force: true, sequence: 5, epoch: 7 });
coordinator.markStale("time:2026-10-23", 6, 7);
releaseRefresh("first-pass");
assert.equal(await refresh, "followup", "请求期间再次失效时最多追加一次校准");
assert.equal(metrics.filter(([metric, key]) => metric === "followup" && key === "time:2026-10-23").length, 1);
assert.equal(coordinator.snapshot("time:2026-10-23").appliedSequence, 6);

let releaseOld;
const old = coordinator.run("reports", async () => await new Promise((resolve) => { releaseOld = resolve; }), { force: true, epoch: 7 });
coordinator.clear();
releaseOld("stale-result");
assert.equal(await old, undefined, "清理后的旧世代响应不得落地");
assert.equal(coordinator.snapshot("reports"), undefined);

console.log("direct cloud query coordinator verification passed");
