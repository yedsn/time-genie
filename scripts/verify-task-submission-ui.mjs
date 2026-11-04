import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const store = await readFile(new URL("../src-ui/src/store.ts", import.meta.url), "utf8");
const coordinator = await readFile(new URL("../src-ui/src/services/taskSubmissionCoordinatorCore.ts", import.meta.url), "utf8");

assert.ok(store.includes("TaskSubmissionCoordinator"));
assert.ok(store.includes("taskSubmissionCoordinator.schedule"));
assert.ok(store.includes("taskSubmissionCoordinator.flush"));
assert.ok(store.includes("mergeAuthoritativeTaskWithLatestDraft"));
assert.ok(store.includes("context.rebindTaskId(created.id)"));
assert.ok(coordinator.includes("get taskId()"));
assert.ok(coordinator.includes("if (slot.pending)"));
assert.ok(coordinator.includes("this.cleanup(slot)"));

for (const functionName of [
  "commitTaskToggle", "deleteTask", "duplicateTask", "reorderTask", "indentTask", "outdentTask",
  "movePlanTask", "saveTaskNote", "commitTodayTaskToggle", "deleteTodayTask", "moveTodayTask",
  "changeTodayTaskParent", "saveExecutionTaskTitle", "saveExecutionTaskNote", "saveExecutionTaskDetails",
  "deleteExecutionTask", "persistTimerTask", "transitionTimer",
]) {
  const start = store.indexOf(`async function ${functionName}`);
  assert.ok(start >= 0, `missing ${functionName}`);
  const end = store.indexOf("\n  async function ", start + 1);
  const body = store.slice(start, end >= 0 ? end : undefined);
  assert.ok(body.includes("flushTaskSubmission") || body.includes("flushTaskSubtree"), `${functionName} must flush pending task edits`);
}

assert.ok(store.includes("saveTaskEstimateBatch"));
assert.ok(store.includes("saveTaskPlanBatch"));
assert.ok(store.includes("saveActualAdjustmentBatch"));
console.log("task submission UI boundary verification passed");
