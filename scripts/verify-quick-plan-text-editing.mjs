import assert from "node:assert/strict";
import { analyzeQuickPlanDraft, buildQuickPlanBatchRequest, createQuickPlanBaseline, createQuickPlanDraft, parseQuickPlanText, reconcileQuickPlanText, removeQuickPlanSubtree, serializeQuickPlanLines } from "../.tmp-quick-plan-text-editing-test/quickPlanTextCore.js";

const tasks = [
  { id: "root-a", title: "重复标题", version: 1 },
  { id: "child-a", parentId: "root-a", title: "子项", version: 2 },
  { id: "root-b", title: "重复标题", version: 3 },
];
const baseline = createQuickPlanBaseline(tasks);
const draft = createQuickPlanDraft(tasks);
assert.equal(draft[0].taskId, "root-a");
assert.equal(draft[2].taskId, "root-b");
assert.equal(serializeQuickPlanLines(draft), "重复标题\n  子项\n重复标题");

assert.equal(parseQuickPlanText("父项\n\t子项").lines[1].depth, 1);
assert.equal(parseQuickPlanText("父项\n  子项").lines[1].depth, 1);
assert.equal(parseQuickPlanText("父项\n    子项").lines[1].depth, 1);
assert.equal(parseQuickPlanText("父项\n - 保留符号").errors.length, 1);
assert.equal(parseQuickPlanText("  孤立子项").errors[0].message, "二级事项前需要一个一级事项");
assert.equal(parseQuickPlanText("父项\n   三空格").errors.length, 1);
assert.equal(parseQuickPlanText("父项\n\t  混合").errors.length, 1);
assert.equal(parseQuickPlanText("父项\n\t\t三级").errors.length, 1);
assert.equal(parseQuickPlanText("父项\n\n新项").lines.length, 2);
assert.equal(parseQuickPlanText("- 这是标题").lines[0].text, "- 这是标题");
const renamedText = reconcileQuickPlanText(draft, "改名父项\n  子项\n重复标题", "edit");
assert.equal(renamedText.lines[0].taskId, undefined);
assert.equal(renamedText.lines[1].taskId, "child-a");
const reorderedText = reconcileQuickPlanText(renamedText.lines, "重复标题\n改名父项\n  子项", "edit2");
assert.equal(reorderedText.lines[0].taskId, "root-a");
assert.equal(reorderedText.lines[1].taskId, undefined);
const restoredText = reconcileQuickPlanText(draft, "重复标题\n  子项\n重复标题", "restored");
assert.deepEqual(restoredText.lines.map((line) => line.taskId), ["root-a", "child-a", "root-b"]);
const duplicateText = reconcileQuickPlanText(draft, "重复标题\n重复标题", "duplicate");
assert.deepEqual(duplicateText.lines.map((line) => line.taskId), ["root-a", "root-b"]);

const edited = draft.map((line) => ({ ...line }));
edited[0].text = "改名父项";
edited.splice(1, 1);
edited.push({ lineKey: "new:1", text: "新事项", depth: 0 });
const summary = analyzeQuickPlanDraft(baseline, edited);
assert.equal(summary.renamed[0].taskId, "root-a");
assert.equal(summary.deleted[0].taskId, "child-a");
assert.equal(summary.added[0].title, "新事项");
assert.equal(summary.dirty, true);

const movedChild = createQuickPlanDraft(tasks).map((line) => ({ ...line }));
movedChild[1].depth = 0;
const movedSummary = analyzeQuickPlanDraft(baseline, movedChild.filter((line) => line.taskId !== "root-a"));
assert.deepEqual(movedSummary.deleted.map((item) => item.taskId), ["root-a"]);
assert.equal(movedSummary.finalItems.find((item) => item.taskId === "child-a").parentLineKey, undefined);

const invalidParentTasks = [{ id: "recurring", title: "重复", version: 1, recurrence: {} }, { id: "timed", title: "工时", version: 1, directMinutes: 5 }];
const invalidBaseline = createQuickPlanBaseline(invalidParentTasks);
const invalidDraft = createQuickPlanDraft(invalidParentTasks);
invalidDraft.splice(1, 0, { lineKey: "new:child-1", text: "子项", depth: 1 });
invalidDraft.push({ lineKey: "new:child-2", text: "子项", depth: 1 });
const invalidSummary = analyzeQuickPlanDraft(invalidBaseline, invalidDraft);
assert.equal(invalidSummary.errors.some((error) => error.message.includes("重复")), true);
assert.equal(invalidSummary.errors.some((error) => error.message.includes("直接工时")), true);

const removal = removeQuickPlanSubtree(createQuickPlanDraft(tasks), 0);
assert.deepEqual(removal.removed.map((line) => line.taskId), ["root-a", "child-a"]);
const request = buildQuickPlanBatchRequest("subject-1", baseline, summary);
assert.equal(request.baselineTasks.length, 3);
assert.deepEqual(request.confirmedDeleteTaskIds, ["child-a"]);
assert.equal(request.finalItems.some((item) => item.lineKey === "new:1"), true);

console.log("quick plan text editing checks passed");
