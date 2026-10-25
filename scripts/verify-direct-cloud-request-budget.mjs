import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { DirectCloudRequestBudget } from "../.tmp-direct-cloud-performance-test/directCloudRequestBudgetCore.js";

const budget = new DirectCloudRequestBudget();
budget.mutation("task:update", () => undefined);
budget.refresh("workspace", "mutation");
budget.refresh("workspace", "realtime");
budget.query("workspace", "visible-domain", () => undefined);
assert.equal(budget.count("mutation"), 1);
assert.equal(budget.count("refresh", "workspace"), 2, "基线必须能识别 mutation 与 Realtime 的重复刷新");
assert.equal(budget.count("query", "workspace"), 1);

const store = readFileSync(new URL("../src-ui/src/store.ts", import.meta.url), "utf8");
const functionBody = (name, nextName) => store.slice(store.indexOf(`async function ${name}`), store.indexOf(`async function ${nextName}`));
const mainInitialization = functionBody("initializeWorkspace", "initializeHoverData");
assert.equal((mainInitialization.match(/loadWorkbenchBootstrap\(\)/g) ?? []).length, 1, "主窗口冷启动必须且只能调用一次 bootstrap");
assert.match(mainInitialization, /loadWorkspaceData\(false, false\)/, "bootstrap 后事项查询必须复用主体数据");
assert.doesNotMatch(mainInitialization, /refreshWorkspaceCalendar|listSubjects/, "主窗口冷启动不得重复读取日历或主体");
const hoverInitialization = store.slice(store.indexOf("async function initializeHoverData"), store.indexOf("function clearCloudBusinessMemory"));
assert.match(hoverInitialization, /loadHoverTimeData\(\)/);
assert.match(hoverInitialization, /loadUnassignedState\(\)/);
assert.doesNotMatch(hoverInitialization, /loadWorkbenchBootstrap|loadWorkspaceData|loadReports|loadTodayOverview|listSubjects/, "悬浮窗必须保持最小请求预算");
for (const [name, nextName] of [
  ["saveTask", "duplicateTask"],
  ["duplicateTask", "duplicateSingleTask"],
  ["saveTaskNote", "saveTaskTodayEstimate"],
  ["saveTaskTodayEstimate", "saveTaskEstimateBatch"],
  ["saveTodayTask", "saveTodayTaskToFuture"],
  ["saveTodayTaskToFuture", "deleteTodayTask"],
  ["deleteTodayTask", "saveTodayTaskNote"],
  ["saveTodayTaskNoteToFuture", "saveTodayTaskTodayEstimate"],
  ["duplicateTodayTask", "duplicateSingleTodayTask"],
  ["startTimer", "pauseTimer"],
  ["pauseTimer", "resumeTimer"],
  ["resumeTimer", "stopTimer"],
  ["stopTimer", "movePlanTask"],
]) {
  const body = functionBody(name, nextName);
  assert.ok(body, `${name} 测试范围必须存在`);
  assert.doesNotMatch(body, /await (?:Promise\.all\(\[)?(?:loadWorkspaceData|loadTimeData|loadReports|loadTodayOverview|loadTimeHistory)/, `${name} 不得等待附加全量查询`);
}
const recurrenceBody = functionBody("setTaskRecurrence", "addTask");
assert.doesNotMatch(recurrenceBody, /await (?:Promise\.all\(\[)?(?:loadWorkspaceData|loadTimeData|loadReports|loadTodayOverview)/, "重复事项设置不得等待全量查询");
const movePlanBody = functionBody("movePlanTask", "moveExecutionTask");
assert.match(movePlanBody, /catch \(error\)[\s\S]*await loadWorkspaceData\(\)/, "移动失败必须保留最小权威恢复");
assert.doesNotMatch(movePlanBody.slice(0, movePlanBody.indexOf("catch (error)")), /await loadWorkspaceData\(\)/, "移动成功不得等待工作台重载");
const resolveUnassignedBody = store.slice(store.indexOf("async function resolveUnassignedTime"), store.lastIndexOf("  return {"));
assert.doesNotMatch(resolveUnassignedBody, /await (?:Promise\.all\(\[)?(?:loadWorkspaceData|loadTimeData|loadTodayOverview|loadTimeHistory)/, "未归属处理成功不得等待全量查询");
assert.match(resolveUnassignedBody, /if \(resolution\.nextSession\)[\s\S]*applyUnassignedState\(resolution\.nextSession\)/, "未归属处理成功后必须立即切换到服务端返回的新起点");
assert.match(resolveUnassignedBody, /loadUnassignedState\(false, true\)/, "旧返回格式必须轻量刷新未归属起点");
const saveTaskBody = functionBody("saveTask", "duplicateTask");
assert.doesNotMatch(saveTaskBody, /loadReports|loadTimeData/, "普通事项保存不得查询报告或计时");
console.log("direct cloud request budget verification passed");
