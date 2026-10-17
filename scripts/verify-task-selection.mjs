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
assert.match(selector, /'is-root-task': item\.kind === 'task' && item\.depth === 0/);
assert.match(selector, /'is-child-task': item\.kind === 'task' && item\.depth > 0/);
assert.match(selector, /@pointerdown\.stop="switchSubject\(subject\.id\)"/);
assert.doesNotMatch(selector, /@pointerdown\.prevent[^>]*switchSubject/);
assert.doesNotMatch(selector, /@mousedown[^>]*switchSubject/);
assert.match(selector, /async function switchSubject/);
assert.match(selector, /await autocompleteRef\.value\?\.getData\(inputText\.value\)/);
assert.match(selector.match(/async function switchSubject[\s\S]*?\n}\n\nfunction createDraft/)?.[0] ?? "", /activated\.value = true/);
assert.doesNotMatch(selector.match(/async function switchSubject[\s\S]*?\n}\n\nfunction createDraft/)?.[0] ?? "", /setTimeout|focusTaskInput|showTaskSuggestions/);
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

const tracking = readFileSync("src-ui/src/components/TimeTrackingWorkspace.vue", "utf8");
assert.match(tracking, /TaskSelectControl/);
assert.match(tracking, /aria-label="补录事项"/);
assert.doesNotMatch(tracking, /aria-label="分配事项"/);
const globalTimer = readFileSync("src-ui/src/components/GlobalTimerBar.vue", "utf8");
assert.doesNotMatch(globalTimer, /TaskSelectControl/);
assert.match(globalTimer, /开始执行事项/);
assert.match(globalTimer, /切换执行事项/);

const styles = readFileSync("src-ui/src/styles.css", "utf8");
const workbench = readFileSync("src-ui/src/views/MainWorkbench.vue", "utf8");
assert.doesNotMatch(workbench, /UnassignedTimeDialog/);
assert.match(workbench, /mode: "unassigned-allocation"/);
assert.match(styles, /\.task-autocomplete-option\.is-root-task \.task-option-name/);
assert.match(styles, /\.task-autocomplete-option\.is-root-task \.task-option-name\s*\{[^}]*font-weight:\s*520/);
assert.doesNotMatch(styles, /\.task-autocomplete-option\.is-root-task \.task-option-name\s*\{[^}]*font-weight:\s*(?:6[0-9]{2}|7[0-9]{2}|bold)/);
assert.match(styles, /\.task-autocomplete-option\.is-parent\s*\{\s*opacity:\s*1/);
assert.match(styles, /\.task-autocomplete-option\.is-child-task \.task-option-name\s*\{[^}]*var\(--text\) 86%/);
assert.match(styles, /\.task-autocomplete-option\.is-child-task \.task-option-indent::after/);

console.log("Task selection verification passed.");
