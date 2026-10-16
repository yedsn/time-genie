import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolveTaskSelectionSubject, taskSelectionRows } from "../.tmp-task-selection-test/taskSelectionCore.js";

const subjects = [{ id: "work" }, { id: "personal" }];
const tasks = [
  { id: "work-root-a", subjectId: "work" },
  { id: "work-root-b", subjectId: "work" },
  { id: "work-child-a", subjectId: "work", parentId: "work-root-a" },
  { id: "personal-root", subjectId: "personal" },
];

assert.equal(resolveTaskSelectionSubject(subjects, undefined, "personal"), "personal");
assert.equal(resolveTaskSelectionSubject(subjects, tasks[0], "personal"), "work");
assert.equal(resolveTaskSelectionSubject(subjects, undefined, "missing"), "work");
assert.equal(resolveTaskSelectionSubject([], undefined, "missing"), "");
assert.deepEqual(taskSelectionRows(tasks, "work").map(({ task, depth }) => [task.id, depth]), [
  ["work-root-a", 0],
  ["work-child-a", 1],
  ["work-root-b", 0],
]);
assert.deepEqual(taskSelectionRows(tasks, "personal").map(({ task }) => task.id), ["personal-root"]);

const selector = readFileSync("src-ui/src/components/TaskSelectControl.vue", "utf8");
assert.match(selector, /resolveTaskSelectionSubject/);
assert.match(selector, /taskSelectionRows/);
assert.match(selector, /#header/);
assert.match(selector, /task-subject-tabs/);
assert.match(selector, /@mousedown\.prevent/);
assert.doesNotMatch(selector, /__all_subjects__/);
assert.doesNotMatch(selector, /task-subject-select/);
assert.doesNotMatch(selector, /store\.selectSubject/);
assert.doesNotMatch(selector, /store\.addQuickTask\(/);
assert.match(selector, /store\.addTaskSelectionDraft/);
assert.match(selector, /newTask: \{ subjectId: targetSubjectId\(\)/);

const store = readFileSync("src-ui/src/store.ts", "utf8");
assert.match(store, /function addTaskSelectionDraft/);
assert.match(store, /canAcceptInlineTimerChild/);
assert.match(store, /task\.quickCreate = true/);
assert.match(store, /async function persistTimerTask/);

for (const file of [
  "src-ui/src/components/GlobalTimerBar.vue",
  "src-ui/src/components/TimeTrackingWorkspace.vue",
  "src-ui/src/components/UnassignedTimeDialog.vue",
]) {
  assert.match(readFileSync(file, "utf8"), /TaskSelectControl/);
}

console.log("Task selection verification passed.");
