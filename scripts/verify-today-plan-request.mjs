import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  beginTodayPlanRequest,
  isCurrentTodayPlanRequest,
  resolveTodayPlanOutcome,
} from "../.tmp-today-plan-request-test/todayPlanRequestCore.js";

const first = beginTodayPlanRequest(0, "subject-a", "2026-10-19");
assert.deepEqual(first, { sequence: 1, subjectId: "subject-a", workDate: "2026-10-19" });
assert.equal(isCurrentTodayPlanRequest(first, 1, "2026-10-19"), true);

// 主体切换：旧主体的响应即使后到达，也不能覆盖新主体的今日快照
const second = beginTodayPlanRequest(first.sequence, "subject-b", "2026-10-19");
assert.equal(second.sequence, 2);
assert.equal(isCurrentTodayPlanRequest(first, second.sequence, "2026-10-19"), false);
assert.equal(isCurrentTodayPlanRequest(second, second.sequence, "2026-10-19"), true);

// 跨日：即使序号仍是最新，昨天发出的响应也不得落到新的一天的列表里
const crossDay = beginTodayPlanRequest(second.sequence, "subject-b", "2026-10-19");
assert.equal(isCurrentTodayPlanRequest(crossDay, crossDay.sequence, "2026-10-20"), false);
assert.equal(isCurrentTodayPlanRequest(crossDay, crossDay.sequence, "2026-10-19"), true);

// 失败后保留上一份权威数据，只有成功且仍当前时才替换列表
const previous = { tasks: [{ id: "kept" }], subjectId: "subject-a" };
assert.deepEqual(resolveTodayPlanOutcome(previous, { ok: false }), previous);
assert.deepEqual(
  resolveTodayPlanOutcome(previous, { ok: true, token: second, tasks: [{ id: "fresh" }] }),
  { tasks: [{ id: "fresh" }], subjectId: "subject-b" },
);

// store 与执行弹框必须真正复用这些判定，而不是各自实现一套
const storeSource = readFileSync(new URL("../src-ui/src/store.ts", import.meta.url), "utf8");
assert.match(storeSource, /beginTodayPlanRequest\(todayPlanRequestSeq/);
assert.match(storeSource, /isCurrentTodayPlanRequest\(token, todayPlanRequestSeq, workspaceToday\.value\)/);
assert.match(storeSource, /resolveTodayPlanOutcome\(/);
assert.match(storeSource, /reloadTodayPlanForSubject/);
assert.match(storeSource, /const todayPlanTasks = reactive<Task\[\]>\(\[\]\)/);
assert.match(storeSource, /const tasks = reactive<Task\[\]>\(initialTasks\)/);
assert.match(storeSource, /function todayPlanTask\(taskId: string\)[\s\S]*return todayPlanTasks\.find/);
assert.match(storeSource, /function replaceTodayTask\(record: TaskRecord\)[\s\S]*todayPlanTasks\.splice/);
assert.match(storeSource, /async function saveTodayTask\([\s\S]*updateTaskRecord\([\s\S]*reloadTodayPlanForSubject/);
assert.match(storeSource, /async function saveTaskPlanBatch\([\s\S]*dayInstanceDate\) await loadTodayPlan\(request\.subjectId, true\)[\s\S]*loadWorkspaceData\(true, false\)/, "批量编辑计划保存后必须强制刷新对应列表");
assert.match(storeSource, /async function saveActualAdjustmentBatch\([\s\S]*loadWorkspaceData\(true, false\)[\s\S]*loadTodayPlan\(request\.subjectId, true\)/, "实际用时保存后必须刷新待办和今日计划");
assert.match(storeSource, /function preserveTaskDerivedState\([\s\S]*todayMember: current\.todayMember[\s\S]*directMinutes: current\.directMinutes[\s\S]*selectable: current\.selectable/, "事项基础更新不得清空今日状态、实际用时和可计时状态");
assert.match(storeSource, /async function refreshTaskRowsAfterTimeMutation\([\s\S]*loadActualAdjustmentSnapshot\(subjectId\)[\s\S]*applyActualSnapshotTask/, "时间变更后必须读取受影响主体的权威事项快照");
for (const functionName of ["confirmTimerStopAllocation", "resolveTimerStopDisposition", "allocate", "addManualEntry", "correctTimeEntry", "setTimeEntryDisposition", "resolveUnassignedTime"]) {
  const functionStart = storeSource.indexOf("async function " + functionName);
  const functionEnd = storeSource.indexOf("\n  async function ", functionStart + 1);
  const functionBody = storeSource.slice(functionStart, functionEnd < 0 ? storeSource.length : functionEnd);
  assert.match(functionBody, /refreshTaskRowsAfterTimeMutation/, functionName + " 完成后必须立即更新受影响事项行");
}
assert.doesNotMatch(storeSource, /function replaceTodayTask\(record: TaskRecord\)[\s\S]{0,500}replaceTask\(/, "今日实例写入不得复用来源事项数组替换函数");
assert.match(storeSource, /function replaceTask\(record: TaskRecord\)[\s\S]*if \(record\.dayInstanceDate\) return replaceTodayTask\(record\)/, "执行面板更新今日实例时不得写入进行中事项数组");

const mainSource = readFileSync(new URL("../src-ui/src/views/MainWorkbench.vue", import.meta.url), "utf8");
assert.match(mainSource, /confirmPlanImport\([\s\S]*loadWorkspaceData\(true\)[\s\S]*loadTodayPlan\(importSubjectId\.value, true\)/, "日报导入后必须绕过旧缓存刷新今日计划");

const dialogSource = readFileSync(new URL("../src-ui/src/components/TimerTaskDialog.vue", import.meta.url), "utf8");
assert.match(dialogSource, /const todayPlanPanelTasks = store\.todayPlanTasks/);
assert.match(dialogSource, /executionPanelSourceTasks\(todayPlanPanelTasks, subjectId\.value, props\.mode\)/);
assert.match(dialogSource, /executionPanelCurrentStatus\(/);

console.log("today plan request verification passed");
