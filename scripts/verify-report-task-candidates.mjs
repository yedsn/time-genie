import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-report-task-candidates-test/reportTaskCandidateCore.js");
const { defaultReportTaskIds, preserveReportTaskSelection, reportTaskDepth, selectableReportTaskIds } = await import(pathToFileURL(modulePath));

const candidates = [
  { taskId: "parent", defaultSelected: false, candidateRole: "context" },
  { taskId: "done", parentId: "parent", defaultSelected: true, candidateRole: "selectable" },
  { taskId: "active", defaultSelected: true, candidateRole: "selectable" },
  { taskId: "planned", defaultSelected: false, candidateRole: "selectable" },
];

assert.deepEqual(defaultReportTaskIds(candidates), ["done", "active"]);
assert.deepEqual(selectableReportTaskIds(candidates), ["done", "active", "planned"]);
assert.deepEqual(preserveReportTaskSelection(candidates, ["parent", "planned", "missing"]), ["planned"]);
assert.equal(reportTaskDepth(candidates[1], candidates), 1);
assert.equal(reportTaskDepth({ taskId: "cycle-a", parentId: "cycle-b", defaultSelected: false, candidateRole: "selectable" }, [
  { taskId: "cycle-a", parentId: "cycle-b", defaultSelected: false, candidateRole: "selectable" },
  { taskId: "cycle-b", parentId: "cycle-a", defaultSelected: false, candidateRole: "selectable" },
]), 2);

rmSync(resolve(".tmp-report-task-candidates-test"), { recursive: true, force: true });
console.log("report task candidate verification passed");
