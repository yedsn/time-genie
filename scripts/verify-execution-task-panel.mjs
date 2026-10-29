import assert from "node:assert/strict";
import {
  executionPanelAction,
  executionPanelCurrentStatus,
  executionPanelPresentation,
  executionPanelRows,
  executionPanelSourceTasks,
  mergeDraftRecord,
  reconcileDraftRecord,
} from "../.tmp-execution-task-panel-test/executionTaskPanelCore.js";

const execute = executionPanelPresentation("execute");
assert.equal(execute.title, "执行事项");
assert.equal(execute.showTimerActions, true);
assert.equal(execute.actualEditable, true);
assert.equal(execute.showAllocation, false);
const unassigned = executionPanelPresentation("unassigned-allocation");
assert.equal(unassigned.showTimerActions, false);
assert.equal(unassigned.actualEditable, true);
assert.equal(unassigned.showAllocation, true);
const entryAllocation = executionPanelPresentation("entry-allocation");
assert.equal(entryAllocation.showTimerActions, false);
assert.equal(entryAllocation.actualEditable, false);
assert.equal(entryAllocation.showAllocation, true);
assert.equal(executionPanelPresentation("unassigned-allocation").showBreakDiscard, true);
assert.equal(executionPanelPresentation("entry-allocation").showBreakDiscard, true);

const tasks = [
  { id: "p", subjectId: "a", title: "产品", status: "done", selectable: false, directMinutes: 0, totalMinutes: 50, sortOrder: 10 },
  { id: "c1", subjectId: "a", parentId: "p", title: "登录页", status: "open", selectable: true, todayEstimate: 30, directMinutes: 20, totalMinutes: 20, sortOrder: 20 },
  { id: "c2", subjectId: "a", parentId: "p", title: "同步", status: "done", completedToday: true, selectable: true, todayEstimate: 40, directMinutes: 30, totalMinutes: 30, sortOrder: 30 },
  { id: "old", subjectId: "a", title: "历史完成", status: "done", selectable: true, directMinutes: 0, totalMinutes: 0, sortOrder: 40 },
  { id: "pin", subjectId: "a", title: "当前归属", status: "done", pinned: true, selectable: true, directMinutes: 5, totalMinutes: 5, sortOrder: 50 },
  { id: "b1", subjectId: "b", title: "其他主体", status: "open", selectable: true, directMinutes: 5, totalMinutes: 5 },
];
assert.deepEqual(executionPanelRows(tasks, "a").map((row) => row.task.id), ["p", "c1", "c2", "pin"]);
assert.deepEqual(executionPanelRows(tasks, "a", "同步").map((row) => row.task.id), ["p", "c2"]);
assert.deepEqual(executionPanelRows(tasks, "a", "产品").map((row) => row.task.id), ["p", "c1", "c2"]);
assert.deepEqual(executionPanelRows(tasks, "a").map((row) => [row.task.id, row.depth]), [["p", 0], ["c1", 1], ["c2", 1], ["pin", 0]]);

// 预计显示：今日预计优先，没有今日预计时回退整体预计，都没有才显示未设置
const estimateTasks = [
  { id: "g", subjectId: "a", title: "分组", status: "open", selectable: false, directMinutes: 0, totalMinutes: 0 },
  { id: "today-only", subjectId: "a", parentId: "g", title: "只有今日预计", status: "open", selectable: true, todayEstimate: 30, directMinutes: 0, totalMinutes: 0 },
  { id: "overall-only", subjectId: "a", parentId: "g", title: "只有整体预计", status: "open", selectable: true, estimate: 45, directMinutes: 0, totalMinutes: 0 },
  { id: "both", subjectId: "a", parentId: "g", title: "两者都有", status: "open", selectable: true, todayEstimate: 20, estimate: 90, directMinutes: 0, totalMinutes: 0 },
  { id: "neither", subjectId: "a", parentId: "g", title: "都没有", status: "open", selectable: true, directMinutes: 0, totalMinutes: 0 },
];
const estimateRows = Object.fromEntries(executionPanelRows(estimateTasks, "a").map((row) => [row.task.id, row]));
assert.equal(estimateRows["today-only"].totalTodayEstimate, 30);
assert.equal(estimateRows["today-only"].estimateSource, "today");
assert.equal(estimateRows["overall-only"].totalTodayEstimate, 45);
assert.equal(estimateRows["overall-only"].estimateSource, "overall");
assert.equal(estimateRows["both"].totalTodayEstimate, 20);
assert.equal(estimateRows["both"].estimateSource, "today");
assert.equal(estimateRows["neither"].totalTodayEstimate, 0);
assert.equal(estimateRows["neither"].estimateSource, "none");
assert.equal(estimateRows.g.totalTodayEstimate, 95);
assert.equal(estimateRows.g.hasChildren, true);

assert.equal(executionPanelAction({ mode: "execute", taskId: "c2", taskTitle: "同步", hasChildren: false, completedToday: true }).mode, "continue");
assert.equal(executionPanelAction({ mode: "unassigned-allocation", taskId: "c1", taskTitle: "登录页", hasChildren: false }).mode, "allocate");
assert.equal(executionPanelAction({ mode: "entry-allocation", taskId: "c1", taskTitle: "登录页", hasChildren: false, completedToday: true }).mode, "allocate");
assert.notEqual(executionPanelAction({ mode: "entry-allocation", taskId: "c1", taskTitle: "登录页", hasChildren: false }).mode, "start");

const reconciled = reconcileDraftRecord({ c1: { value: 10 }, c2: { value: 20 }, gone: { value: 30 } }, ["c1"], ["c2"]);
assert.deepEqual(reconciled, { c1: { value: 10 }, c2: { value: 20 } });
const merged = mergeDraftRecord(reconciled, ["c1", "new"], (taskId) => ({ value: taskId === "new" ? 0 : 99 }));
assert.deepEqual(merged, { c1: { value: 10 }, new: { value: 0 } });

// 所有模式使用相同的今日成员（含当天已完成）及父级上下文
const planTasks = [
  { id: "parent", subjectId: "a", title: "项目", status: "open", selectable: false, directMinutes: 0, totalMinutes: 0, todayContext: true },
  { id: "today-open", subjectId: "a", parentId: "parent", title: "今日未完成", status: "open", selectable: true, directMinutes: 0, totalMinutes: 0, todayMember: true },
  { id: "today-done", subjectId: "a", title: "今日已完成", status: "done", selectable: true, directMinutes: 10, totalMinutes: 10, todayMember: true },
  { id: "not-planned", subjectId: "a", title: "长期待办", status: "open", selectable: true, directMinutes: 0, totalMinutes: 0 },
  { id: "other-subject", subjectId: "b", title: "其他主体今日", status: "open", selectable: true, directMinutes: 0, totalMinutes: 0, todayMember: true },
  { id: "cross-day-active", subjectId: "a", title: "跨日计时", status: "open", selectable: true, directMinutes: 0, totalMinutes: 0 },
];
assert.deepEqual(executionPanelSourceTasks(planTasks, "a", "execute").map((task) => task.id), ["parent", "today-open", "today-done"]);
assert.deepEqual(executionPanelSourceTasks(planTasks, "a", "entry-allocation").map((task) => task.id), ["parent", "today-open", "today-done"]);
assert.deepEqual(executionPanelSourceTasks(planTasks, "a", "unassigned-allocation").map((task) => task.id), ["parent", "today-open", "today-done"]);

// 同名长期来源不混入；已完成原归属及父级仍可调整，ID 不映射到今日副本。
const originalTasks = [
  { ...planTasks[1], id: "source", todayMember: false, title: "今日未完成" },
  { ...planTasks[0], id: "old-parent", status: "done", todayContext: false },
  { ...planTasks[2], id: "old-allocation", parentId: "old-parent", todayMember: false, completedToday: false },
  { ...planTasks[1], title: "过期标题" },
  { ...planTasks[4], id: "other-allocation", todayMember: false, status: "done" },
];
const allocationRows = executionPanelRows(executionPanelSourceTasks(planTasks, "a", "entry-allocation", originalTasks, ["old-allocation", "other-allocation", "missing"]), "a");
assert.deepEqual(allocationRows.map((row) => [row.task.id, row.depth]), [["parent", 0], ["today-open", 1], ["today-done", 0], ["old-parent", 0], ["old-allocation", 1]]);
assert.equal(allocationRows.find((row) => row.task.id === "today-open").task.title, "今日未完成");
assert.equal(allocationRows.find((row) => row.task.id === "old-allocation").pinned, true);
assert.equal(allocationRows.some((row) => row.task.id === "source"), false);
const sourceAllocated = executionPanelSourceTasks(planTasks, "a", "entry-allocation", originalTasks, ["source"]);
assert.equal(sourceAllocated.filter((task) => task.title === "今日未完成").length, 2, "长期来源本身是原归属时保留两个确切 ID");
assert.equal(sourceAllocated.find((task) => task.id === "source").pinned, true);
assert.deepEqual(executionPanelRows(executionPanelSourceTasks(planTasks, "b", "entry-allocation", originalTasks, ["old-allocation", "other-allocation"]), "b").map((row) => row.task.id), ["other-subject", "other-allocation"]);

const executeRows = executionPanelRows(executionPanelSourceTasks(planTasks, "a", "execute"), "a");
assert.deepEqual(executeRows.map((row) => row.task.id), ["parent", "today-open", "today-done"]);
const unassignedRows = executionPanelRows(executionPanelSourceTasks(planTasks, "a", "unassigned-allocation"), "a");
assert.deepEqual(unassignedRows.map((row) => [row.task.id, row.depth]), executeRows.map((row) => [row.task.id, row.depth]));
assert.equal(executeRows[0].hasChildren, true);
assert.equal(executionPanelAction({ mode: "execute", taskId: "today-done", taskTitle: "今日已完成", hasChildren: false, completedToday: true }).mode, "continue");

const crossDayStatus = executionPanelCurrentStatus({
  taskId: "cross-day-active",
  taskTitle: "跨日计时",
  entryState: "running",
  candidateTaskIds: executeRows.map((row) => row.task.id),
});
assert.deepEqual(crossDayStatus, { title: "跨日计时", stateLabel: "计时中", outsideTodayPlan: true });
assert.equal(executionPanelCurrentStatus({ taskId: "today-open", taskTitle: "今日未完成", entryState: "paused", candidateTaskIds: ["today-open"] })?.outsideTodayPlan, false);
assert.equal(executionPanelCurrentStatus({ candidateTaskIds: [] }), undefined);

console.log("execution task panel verification passed");
