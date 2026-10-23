import assert from "node:assert/strict";
import { applyCreatedTask, collectTaskSubtreeIds, removeTaskSubtree, taskMutationDomains, upsertAuthoritativeTask } from "../.tmp-direct-cloud-performance-test/taskMutationStateCore.js";

const tasks = [
  { id: "draft", version: 0 },
  { id: "child", parentId: "draft", version: 0 },
];
const selected = applyCreatedTask({ tasks, draftId: "draft", authoritative: { id: "saved", version: 1 }, selectedTaskId: "draft" });
assert.equal(selected, "saved");
assert.equal(tasks[1].parentId, "saved");
assert.equal(tasks[0].version, 1);
upsertAuthoritativeTask(tasks, { id: "saved", version: 2 });
assert.equal(tasks[0].version, 2);
assert.deepEqual([...collectTaskSubtreeIds(tasks, "saved")].sort(), ["child", "saved"]);
const removed = removeTaskSubtree(tasks, "saved");
assert.deepEqual([...removed].sort(), ["child", "saved"]);
assert.equal(tasks.length, 0);
assert.deepEqual(taskMutationDomains.update.covered, ["tasks"]);
assert.equal(taskMutationDomains.update.stale.includes("reports"), false);
console.log("task mutation state verification passed");
