import assert from "node:assert/strict";
import { analyzeQuickPlanDraft, appendQuickPlanImport, buildQuickPlanBatchRequest, createQuickPlanBaseline, createQuickPlanDraft, mergeScopedQuickPlanDraft, parseQuickPlanText, reconcileQuickPlanText, removeQuickPlanSubtree, serializeQuickPlanLines } from "../.tmp-quick-plan-text-editing-test/quickPlanTextCore.js";

const tasks = [
  { id: "root-a", title: "重复标题", version: 1 },
  { id: "child-a", parentId: "root-a", title: "子项", version: 2 },
  { id: "root-b", title: "重复标题", version: 3 },
];
const baseline = createQuickPlanBaseline(tasks);
const draft = createQuickPlanDraft(tasks);
assert.equal(draft[0].taskId, "root-a");
assert.equal(draft[2].taskId, "root-b");
assert.equal(serializeQuickPlanLines(draft), "- 重复标题\n  - 子项\n- 重复标题");

assert.equal(parseQuickPlanText("- 父项\n\t- 子项").lines[1].depth, 1);
assert.equal(parseQuickPlanText("- 父项\n  - 子项").lines[1].depth, 1);
assert.equal(parseQuickPlanText("- 父项\n    - 子项").lines[1].depth, 1);
assert.equal(parseQuickPlanText("- 父项\n - 错误缩进").errors.length, 1);
assert.equal(parseQuickPlanText("  - 孤立子项").errors[0].message, "二级事项前需要一个一级事项");
assert.equal(parseQuickPlanText("- 父项\n   - 三空格").errors.length, 1);
assert.equal(parseQuickPlanText("- 父项\n\t  - 混合").errors.length, 1);
assert.equal(parseQuickPlanText("- 父项\n\t\t- 三级").errors.length, 1);
assert.equal(parseQuickPlanText("- 父项\n\n- 新项").lines.length, 2);
assert.equal(parseQuickPlanText("- 这是标题").lines[0].text, "这是标题");
assert.equal(parseQuickPlanText("没有列表符号").errors[0].message, "每一行事项必须以 '- ' 开头");
assert.equal(parseQuickPlanText("- ").lines.length, 0);
const renamedText = reconcileQuickPlanText(draft, "- 改名父项\n  - 子项\n- 重复标题", "edit");
assert.equal(renamedText.lines[0].taskId, undefined);
assert.equal(renamedText.lines[1].taskId, "child-a");
const reorderedText = reconcileQuickPlanText(renamedText.lines, "- 重复标题\n- 改名父项\n  - 子项", "edit2");
assert.equal(reorderedText.lines[0].taskId, "root-a");
assert.equal(reorderedText.lines[1].taskId, undefined);
const restoredText = reconcileQuickPlanText(draft, "- 重复标题\n  - 子项\n- 重复标题", "restored");
assert.deepEqual(restoredText.lines.map((line) => line.taskId), ["root-a", "child-a", "root-b"]);
const duplicateText = reconcileQuickPlanText(draft, "- 重复标题\n- 重复标题", "duplicate");
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

const fullTasks = [
  { id: "context", title: "父项", version: 1 },
  { id: "today-child", parentId: "context", title: "今日子项", version: 1 },
  { id: "hidden-child", parentId: "context", title: "长期子项", version: 1 },
  { id: "hidden-root", title: "长期根事项", version: 1 },
];
const scopedTasks = fullTasks.filter((task) => task.id === "context" || task.id === "today-child");
const scopedBaseline = createQuickPlanBaseline(scopedTasks);
const scopedDraft = createQuickPlanDraft(scopedTasks);
scopedDraft[1].text = "今日子项已改名";
scopedDraft.push({ lineKey: "today:new", text: "今日新增", depth: 0 });
const scopedSummary = analyzeQuickPlanDraft(scopedBaseline, scopedDraft);
const mergedDraft = mergeScopedQuickPlanDraft(createQuickPlanDraft(fullTasks), createQuickPlanBaseline(fullTasks), scopedBaseline, scopedSummary);
assert.deepEqual(mergedDraft.map((line) => line.taskId ?? line.lineKey), ["context", "today-child", "hidden-child", "today:new", "hidden-root"]);
assert.equal(mergedDraft.find((line) => line.taskId === "today-child").text, "今日子项已改名");
assert.equal(mergedDraft.find((line) => line.taskId === "hidden-child").depth, 1);
const mergedSummary = analyzeQuickPlanDraft(createQuickPlanBaseline(fullTasks), mergedDraft);
assert.deepEqual(mergedSummary.deleted, []);
assert.equal(mergedSummary.added.some((item) => item.lineKey === "today:new"), true);

const sourceTasks = [
  { id: "source-root", title: "进行中父项", version: 5 },
  { id: "source-child", title: "进行中子项", parentId: "source-root", version: 8 },
  { id: "source-other", title: "另一事项", version: 2 },
];
const imported = appendQuickPlanImport([], sourceTasks);
assert.equal(serializeQuickPlanLines(imported), "- 进行中父项\n  - 进行中子项\n- 另一事项");
assert.equal(imported.every((line) => !line.taskId && !line.expectedVersion), true, "source identities must not enter the today batch");
assert.deepEqual(appendQuickPlanImport(imported, sourceTasks), imported, "repeated imports must not duplicate items");
assert.deepEqual(appendQuickPlanImport(imported, []), imported);
const importSummary = analyzeQuickPlanDraft([], imported);
assert.equal(importSummary.dirty, true);
assert.deepEqual(importSummary.errors, []);
assert.equal(importSummary.added.length, 3);
const importRequest = buildQuickPlanBatchRequest("subject-1", [], importSummary);
assert.deepEqual(importRequest.baselineTasks, []);
assert.deepEqual(importRequest.confirmedDeleteTaskIds, []);
assert.equal(importRequest.finalItems[1].parentLineKey, "import:source-root");

const existingToday = [
  { id: "today-root", title: "今日父项已编辑", version: 2 },
  { id: "today-child", title: "今日子项", parentId: "today-root", version: 1 },
  { id: "today-other", title: "已有顶层事项", version: 1 },
];
const existingTodayDraft = createQuickPlanDraft(existingToday);
existingTodayDraft.push({ lineKey: "edit:new", text: "手写草稿", depth: 0 });
const mergedImport = appendQuickPlanImport(existingTodayDraft, sourceTasks, new Map([["today-root", "source-root"]]));
assert.deepEqual(mergedImport.map((line) => line.text), ["今日父项已编辑", "今日子项", "进行中子项", "已有顶层事项", "手写草稿", "另一事项"]);
assert.deepEqual(mergedImport.slice(0, 2), existingTodayDraft.slice(0, 2));
assert.equal(mergedImport[2].depth, 1, "imported child belongs under its existing today parent");
const mergedImportSummary = analyzeQuickPlanDraft(createQuickPlanBaseline(existingToday), mergedImport);
assert.deepEqual(mergedImportSummary.deleted, []);
assert.deepEqual(mergedImportSummary.errors, []);
assert.equal(mergedImportSummary.finalItems.find((item) => item.title === "进行中子项").parentLineKey, "task:today-root");
const retainedImports = reconcileQuickPlanText(mergedImport, serializeQuickPlanLines(mergedImport), "edit:next");
assert.equal(retainedImports.lines.find((line) => line.text === "进行中子项").lineKey, "import:source-child");
assert.deepEqual(appendQuickPlanImport([], [{ id: "same-1", title: "同名", version: 1 }, { id: "same-2", title: "同名", version: 1 }]).map((line) => line.lineKey), ["import:same-1", "import:same-2"]);

console.log("quick plan text editing checks passed");
