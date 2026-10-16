import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { canAcceptInlineTimerChild, inlineTimerParentUnavailableReason } from "../.tmp-inline-timer-task-creation/inlineTimerTaskCreationCore.js";

const base = { subjectId: "s1", status: "open", directMinutes: 0 };
assert.equal(canAcceptInlineTimerChild(base, "s1"), true);
assert.equal(canAcceptInlineTimerChild({ ...base, parentId: "p" }, "s1"), false);
assert.equal(canAcceptInlineTimerChild({ ...base, status: "done" }, "s1"), false);
assert.equal(canAcceptInlineTimerChild({ ...base, recurrence: {} }, "s1"), false);
assert.equal(canAcceptInlineTimerChild({ ...base, directMinutes: 1 }, "s1"), false);
assert.equal(canAcceptInlineTimerChild(base, "s2"), false);
assert.equal(inlineTimerParentUnavailableReason({ ...base, status: "done" }, "s1"), "已完成，不能再建子项");

const selector = readFileSync("src-ui/src/components/TaskSelectControl.vue", "utf8");
assert.match(selector, /新建为一级事项/);
assert.match(selector, /新建为二级事项/);
assert.doesNotMatch(selector, /在此事项下新建/);
assert.doesNotMatch(selector, /task-option-create-child/);
assert.match(selector, /if \(choosingParent\.value\)/);
assert.match(selector, /if \(props\.intentMode\) emit\("submit"/);
assert.match(selector, /const title = item\.label\.trim\(\)/);
assert.match(selector, /pendingTitle\.value = item\.label\.trim\(\)/);
assert.match(selector, /inputText\.value = title/);
assert.match(selector, /store\.addTaskSelectionDraft/);
assert.doesNotMatch(selector, /@blur=/);
assert.match(selector, /@keydown.esc.stop="handleEscape"/);

const timerBar = readFileSync("src-ui/src/components/GlobalTimerBar.vue", "utf8");
assert.match(timerBar, /@submit="submitTimerIntent"/);
assert.match(timerBar, /上一段计时可稍后确认归属/);

console.log("Inline timer task creation verification passed.");
