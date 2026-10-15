import assert from "node:assert/strict";
import {
  actualAdjustmentChanges,
  actualAdjustmentSummary,
  eligibleActualAdjustmentParents,
  parentTargetTotal,
  parseActualMinutes,
} from "../.tmp-quick-actual-adjustment-test/quickActualAdjustmentCore.js";

assert.deepEqual(parseActualMinutes("0"), { ok: true, minutes: 0 });
assert.deepEqual(parseActualMinutes("1440"), { ok: true, minutes: 1440 });
assert.equal(parseActualMinutes("1.5").ok, false);
assert.equal(parseActualMinutes("-1").ok, false);
assert.equal(parseActualMinutes("abc").ok, false);
assert.equal(parseActualMinutes("1441").ok, false);

const drafts = [
  { taskId: "a", baselineMinutes: 30, targetMinutes: 45, text: "45", expectedTaskVersion: 1, adjustmentFingerprint: "a" },
  { taskId: "b", baselineMinutes: 40, targetMinutes: 10, text: "10", expectedTaskVersion: 2, adjustmentFingerprint: "b" },
  { taskId: "c", baselineMinutes: 5, targetMinutes: 5, text: "5", expectedTaskVersion: 1, adjustmentFingerprint: "c" },
];
assert.deepEqual(actualAdjustmentChanges(drafts).map((item) => item.deltaMinutes), [15, -30]);
const summary = actualAdjustmentSummary(drafts, [
  { clientTaskId: "new-1", title: "新增", parentId: "parent", targetMinutes: 20, text: "20" },
  { clientTaskId: "new-2", title: "零分钟", targetMinutes: 0, text: "0" },
]);
assert.deepEqual(summary, { changes: actualAdjustmentChanges(drafts), addedMinutes: 35, reducedMinutes: 30, netMinutes: 5, adjustmentCount: 3, newTaskCount: 2 });

const tasks = [
  { id: "parent", title: "父", version: 3, isLeaf: false, directMinutes: 0, totalMinutes: 40, adjustmentFingerprint: "p", canAcceptChildren: true },
  { id: "blocked", title: "有直接时间", version: 2, isLeaf: true, directMinutes: 10, totalMinutes: 10, adjustmentFingerprint: "x", canAcceptChildren: false },
  { id: "child", parentId: "parent", title: "子", version: 1, isLeaf: true, directMinutes: 40, totalMinutes: 40, adjustmentFingerprint: "c", canAcceptChildren: false },
];
assert.deepEqual(eligibleActualAdjustmentParents(tasks).map((task) => [task.id, task.version]), [["parent", 3]]);
assert.equal(parentTargetTotal("parent", tasks, [
  { taskId: "child", baselineMinutes: 40, targetMinutes: 25, text: "25", expectedTaskVersion: 1, adjustmentFingerprint: "c" },
], [
  { clientTaskId: "new", title: "新增子项", parentId: "parent", targetMinutes: 15, text: "15" },
]), 40);

console.log("Quick actual adjustment verification passed.");
