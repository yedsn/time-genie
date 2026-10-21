import assert from "node:assert/strict";
import { contextMenuPosition, effectiveTodayEstimate, hierarchicalTaskSelectionRows, planTaskMoveIntent, taskDragTargetDepth, taskDragTargetInvalid, taskSubtreeIds, todayEstimateDraft, toggleTaskSelectionGroup } from "../.tmp-plan-task-interactions-test/planTaskInteractionCore.js";

const rows = [
  { id: "a" },
  { id: "a1", parentId: "a" },
  { id: "a2", parentId: "a" },
  { id: "b" },
  { id: "b1", parentId: "b" },
];
assert.deepEqual(planTaskMoveIntent([rows[0], rows[2], rows[1], ...rows.slice(3)], "a2", new Set(["a2"]), 1, false), { parentId: "a", previousSiblingId: undefined, nextSiblingId: "a1" });
assert.deepEqual(planTaskMoveIntent([rows[0], rows[1], rows[2], rows[3], rows[4]], "a1", new Set(["a1"]), 1, false), { parentId: "a", previousSiblingId: undefined, nextSiblingId: "a2" });
assert.deepEqual(planTaskMoveIntent([rows[0], rows[2], rows[1], rows[3], rows[4]], "a2", new Set(["a2"]), 1, false), { parentId: "a", previousSiblingId: undefined, nextSiblingId: "a1" });
assert.deepEqual(planTaskMoveIntent([rows[0], rows[1], rows[2], rows[3], rows[4]], "a2", new Set(["a2"]), 1, false), { parentId: "a", previousSiblingId: "a1", nextSiblingId: undefined });
assert.deepEqual(planTaskMoveIntent([rows[0], rows[1], rows[3], rows[2], rows[4]], "a2", new Set(["a2"]), 0, false), { parentId: undefined, previousSiblingId: "b", nextSiblingId: undefined });
assert.deepEqual(planTaskMoveIntent([rows[0], rows[1], rows[2], rows[3], rows[4]], "b", new Set(["b"]), 1, false), { parentId: "a", previousSiblingId: "a2", nextSiblingId: undefined });
assert.equal(planTaskMoveIntent(rows, "a", new Set(["a", "a1", "a2"]), 1, true), undefined);
assert.deepEqual([...taskSubtreeIds(rows, "a")], ["a", "a1", "a2"]);
const selectableIds = new Set(["a1", "a2", "b1"]);
const pickerRows = hierarchicalTaskSelectionRows(rows, selectableIds, []);
assert.deepEqual(pickerRows.map((row) => [row.task.id, row.depth, row.selectableIds]), [
  ["a", 0, ["a1", "a2"]],
  ["a1", 1, ["a1"]],
  ["a2", 1, ["a2"]],
  ["b", 0, ["b1"]],
  ["b1", 1, ["b1"]],
]);
const partialPickerRows = hierarchicalTaskSelectionRows(rows, selectableIds, ["a1"]);
assert.deepEqual(partialPickerRows[0], {
  task: rows[0], depth: 0, selectableIds: ["a1", "a2"], checked: false, indeterminate: true,
});
const completePickerRows = hierarchicalTaskSelectionRows(rows, selectableIds, ["a1", "a2"]);
assert.equal(completePickerRows[0].checked, true);
assert.equal(completePickerRows[0].indeterminate, false);
assert.deepEqual(hierarchicalTaskSelectionRows(rows, selectableIds, [], { query: "a2", queryText: (task) => task.id }).map((row) => row.task.id), ["a", "a1", "a2"]);
assert.deepEqual(toggleTaskSelectionGroup([], ["a1", "a2"]), ["a1", "a2"]);
assert.deepEqual(toggleTaskSelectionGroup(["a1", "a2"], ["a1", "a2"]), []);
assert.deepEqual(toggleTaskSelectionGroup(["b1"], ["a1", "a2"]), ["b1", "a1", "a2"]);
assert.equal(taskDragTargetDepth(0, 80, 0), 0);
assert.equal(taskDragTargetDepth(0, 100, 0), 1);
assert.equal(taskDragTargetDepth(1, 20, 0), 1);
assert.equal(taskDragTargetInvalid(1, new Set(["a", "a1"])), true);
assert.equal(taskDragTargetInvalid(1, new Set(["a1"])), false);
assert.equal(todayEstimateDraft(undefined), 60);
assert.equal(todayEstimateDraft(undefined, "a"), 30);
assert.equal(todayEstimateDraft(15, "a"), 15);
assert.equal(todayEstimateDraft(15), 15);
assert.equal(effectiveTodayEstimate(undefined, 10, true), 10);
assert.equal(effectiveTodayEstimate(20, 10, true), 20);
assert.equal(effectiveTodayEstimate(undefined, 10, false), undefined);
assert.deepEqual(contextMenuPosition(990, 790, 180, 240, 1000, 800), { left: 812, top: 552 });
console.log("plan task interaction verification passed");
