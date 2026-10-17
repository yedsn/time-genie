import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-time-entry-history-test/timeEntryHistoryCore.js");
const history = await import(pathToFileURL(modulePath));

assert.equal(history.clampHistoryPage(9, 3), 3);
assert.equal(history.clampHistoryPage(0, 0), 1);
assert.deepEqual(history.historyPaginationState(1, 6), { currentPage: 1, totalPages: 1, canGoPrevious: false, canGoNext: false });
assert.deepEqual(history.historyPaginationState(1, 7), { currentPage: 1, totalPages: 1, canGoPrevious: false, canGoNext: false });
assert.deepEqual(history.historyPaginationState(1, 8), { currentPage: 1, totalPages: 2, canGoPrevious: false, canGoNext: true });
assert.deepEqual(history.historyPaginationState(2, 8), { currentPage: 2, totalPages: 2, canGoPrevious: true, canGoNext: false });
assert.equal(history.historyDateLabel("2026-10-13", "2026-10-13"), "今天 · 2026年10月13日");
assert.equal(history.historyDateLabel("2026-10-12", "2026-10-13"), "昨天 · 2026年10月12日");
assert.equal(history.historyDateLabel("2026-10-11", "2026-10-13"), "2026年10月11日 · 星期日");

const todayEntries = [{ id: "today", workDate: "2026-10-13", minutes: 20, allocated: 10 }];
const historicalGroups = [{ workDate: "2026-10-01", entries: [{ id: "old", workDate: "2026-10-01", minutes: 600, allocated: 500 }] }];
assert.equal(todayEntries.filter((entry) => history.isTodayTimeEntry(entry, "2026-10-13")).reduce((sum, entry) => sum + entry.minutes, 0), 20);
assert.equal(history.isTodayTimeEntry(historicalGroups[0].entries[0], "2026-10-13"), false);
assert.equal(history.findEntryInHistory(historicalGroups, "old")?.minutes, 600);
assert.deepEqual([...history.collectTodayEntryTaskIds([
  { workDate: "2026-10-13", kind: "work", defaultTask: "today-task", allocations: [] },
  { workDate: "2026-10-13", kind: "work", allocations: [{ taskId: "allocated-today", minutes: 10 }] },
  { workDate: "2026-10-12", kind: "work", defaultTask: "old-task", allocations: [] },
  { workDate: "2026-10-13", kind: "break", defaultTask: "break-task", allocations: [] },
], "2026-10-13")].sort(), ["allocated-today", "today-task"]);
assert.equal(history.shouldRefreshTimeHistory("timer", true), true);
assert.equal(history.shouldRefreshTimeHistory("timer", false), false);
assert.equal(history.shouldRefreshTimeHistory("today", true), false);
const refreshEvents = [
  ...Array.from({ length: 20 }, () => ["timer", false]),
  ["timer", true],
  ["today", true],
];
assert.equal(refreshEvents.filter(([page, changed]) => history.shouldRefreshTimeHistory(page, changed)).length, 1);
const workspace = await import("node:fs/promises");
const timeWorkspaceSource = await workspace.readFile(resolve("src-ui/src/components/TimeTrackingWorkspace.vue"), "utf8");
const stopDialogSource = await workspace.readFile(resolve("src-ui/src/components/TimerStopConfirmationDialog.vue"), "utf8");
const workbenchSource = await workspace.readFile(resolve("src-ui/src/views/MainWorkbench.vue"), "utf8");
assert(timeWorkspaceSource.includes('message.startsWith("HISTORICAL_RECORD_READ_ONLY:")'), "跨日写入拒绝应使用稳定错误码");
assert(timeWorkspaceSource.includes('delete correctionDrafts[entry.id]'), "跨日修正拒绝后应丢弃旧日期修正草稿");
assert(timeWorkspaceSource.includes('expandedEntryId.value = ""'), "跨日修正拒绝后应退出旧日期编辑状态");
assert(timeWorkspaceSource.includes('await store.refreshWorkspaceCalendar()'), "跨日写入拒绝后应刷新工作日期");
assert(timeWorkspaceSource.includes('await store.loadTimeHistory(store.timeHistoryPage)'), "跨日写入拒绝后应刷新时间历史");
assert(stopDialogSource.includes('pendingAllocationTarget.value = { entryId: targetEntry.id, subjectId }'), "结束确认框应记住待打开的目标记录");
assert(stopDialogSource.includes("store.dismissTimerStopConfirmation()"), "打开归属面板前应关闭结束确认框");
assert(stopDialogSource.includes('if (target) emit("adjustAllocation", target.entryId, target.subjectId)'), "确认框关闭后应直接请求打开目标记录的归属面板");
assert(stopDialogSource.includes('@closed="handleClosed"'), "应等待结束确认框关闭后再打开归属面板，避免弹框重叠");
assert(!stopDialogSource.includes("store.adjustTimerStopAllocation"), "结束确认框不应先跳转到时间记录页");
assert(workbenchSource.includes('<TimerStopConfirmationDialog @adjust-allocation="openEntryAllocation" />'), "主工作台应把结束确认框直接接到共享归属面板");

rmSync(resolve(".tmp-time-entry-history-test"), { recursive: true, force: true });
console.log("time entry history verification passed");
