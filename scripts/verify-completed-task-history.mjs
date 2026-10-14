import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-completed-task-history-test/completedTaskHistoryCore.js");
const { completedTaskHistoryFallbackPage, isActivePlanTask, isCurrentCompletedTaskHistoryRequest } = await import(pathToFileURL(modulePath));

assert.equal(isCurrentCompletedTaskHistoryRequest(
  { subjectId: "subject-a", page: 2, sequence: 4 },
  "subject-a", 2, 4,
), true);
assert.equal(isCurrentCompletedTaskHistoryRequest(
  { subjectId: "subject-a", page: 2, sequence: 4 },
  "subject-b", 2, 4,
), false);
assert.equal(isCurrentCompletedTaskHistoryRequest(
  { subjectId: "subject-a", page: 1, sequence: 3 },
  "subject-a", 2, 4,
), false);
assert.equal(completedTaskHistoryFallbackPage(3, 0), 2);
assert.equal(completedTaskHistoryFallbackPage(1, 0), 1);
assert.equal(completedTaskHistoryFallbackPage(3, 1), 3);
assert.equal(isActivePlanTask({ status: "open", recurring: false }), true);
assert.equal(isActivePlanTask({ status: "done", recurring: false }), false);
assert.equal(isActivePlanTask({ status: "done", recurring: true }), true);

rmSync(resolve(".tmp-completed-task-history-test"), { recursive: true, force: true });
console.log("completed task history verification passed");
