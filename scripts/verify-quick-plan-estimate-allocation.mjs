import assert from "node:assert/strict";
import { adjustQuickEstimateMinutes, allocatedMinutes, capacitySummary, changedEstimateDrafts, formatQuickEstimateHours, isAllocatableTask, parseQuickDuration, parseQuickEstimateMinutes, quickEstimateRows, scheduleCapacity, weekdayIndexForDate } from "../.tmp-quick-plan-estimate-allocation-test/quickPlanEstimateCore.js";

assert.deepEqual(parseQuickDuration("0.5"), { ok: true, minutes: 30 });
assert.deepEqual(parseQuickDuration("1.5h"), { ok: true, minutes: 90 });
assert.deepEqual(parseQuickDuration("45min"), { ok: true, minutes: 45 });
assert.equal(parseQuickDuration("30").ok, false);
assert.equal(parseQuickDuration("-1h").ok, false);
assert.equal(parseQuickDuration("25h").ok, false);
assert.deepEqual(parseQuickEstimateMinutes("30"), { ok: true, minutes: 30 });
assert.deepEqual(parseQuickEstimateMinutes("1440"), { ok: true, minutes: 1440 });
assert.deepEqual(parseQuickEstimateMinutes("0"), { ok: true, minutes: undefined });
assert.deepEqual(parseQuickEstimateMinutes(""), { ok: true, minutes: undefined });
for (const invalid of ["1.5", "30m", "-15", "1441", "abc"]) assert.equal(parseQuickEstimateMinutes(invalid).ok, false);
assert.equal(adjustQuickEstimateMinutes(undefined, 15), 15);
assert.equal(adjustQuickEstimateMinutes(30, -15), 15);
assert.equal(adjustQuickEstimateMinutes(15, -15), undefined);
assert.equal(adjustQuickEstimateMinutes(1440, 15), 1440);
assert.equal(formatQuickEstimateHours(30), "0.5 小时");
assert.equal(formatQuickEstimateHours(60), "1 小时");
assert.equal(formatQuickEstimateHours(90), "1.5 小时");
assert.equal(formatQuickEstimateHours(undefined), "0 小时");
const tasks = [
  { id: "parent", title: "父项", status: "open", version: 1 },
  { id: "child", parentId: "parent", title: "子项", status: "open", version: 1 },
  { id: "leaf", title: "叶子", status: "open", estimate: 60, version: 1 },
  { id: "done", title: "完成", status: "done", version: 1 },
];
assert.equal(isAllocatableTask(tasks[0], tasks), false);
assert.equal(isAllocatableTask(tasks[2], tasks), true);
assert.equal(quickEstimateRows(tasks).filter((row) => row.allocatable).length, 2);
assert.equal(quickEstimateRows(tasks).find((row) => row.task.id === "leaf").suggestion, 60);
const drafts = [
  { taskId: "child", originalMinutes: undefined, minutes: 30, text: "30", expectedTaskVersion: 1 },
  { taskId: "leaf", originalMinutes: 60, minutes: 90, text: "90", expectedTaskVersion: 1 },
];
assert.equal(allocatedMinutes(drafts), 120);
assert.equal(changedEstimateDrafts(drafts).length, 2);
assert.deepEqual(capacitySummary(390, 360), { allocatedMinutes: 390, capacityMinutes: 360, remainingMinutes: 0, overMinutes: 30 });
assert.equal(scheduleCapacity([{ start: "10:00", end: "12:00" }, { start: "13:30", end: "17:30" }]), 360);
assert.equal(scheduleCapacity([], false), 0);
assert.equal(weekdayIndexForDate("2026-10-15", "Asia/Shanghai"), 3);
console.log("quick plan estimate allocation checks passed");
