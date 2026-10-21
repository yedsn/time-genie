import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const component = readFileSync(new URL("../src-ui/src/components/PlanListEditor.vue", import.meta.url), "utf8");
const styles = readFileSync(new URL("../src-ui/src/styles.css", import.meta.url), "utf8");
const store = readFileSync(new URL("../src-ui/src/store.ts", import.meta.url), "utf8");
const tauri = readFileSync(new URL("../src-ui/src/services/tauri.ts", import.meta.url), "utf8");
for (const token of ["配置今日事项", "今日事项", "从进行中导入", "保存配置", "todayImportSelection", "todayImportInstanceRows", "todayImportCandidateRows", "todayImportEstimates", "todayImportTotalMinutes", "saveTodayImport", "instanceEstimates", "todayImportEstimatePresets", "adjustTodayImportEstimate", "setTodayImportEstimate", "adjustQuickEstimateMinutes", "inputmode=\"numeric\"", "今日预计", "可留空", "hierarchicalTaskSelectionRows", "toggleTodaySourceTaskGroup", "减少 15 分钟", "增加 15 分钟", "30 分钟", "1 小时", "1 小时 30 分钟", "2 小时", "2 小时 30 分钟", "today-import-dialog", "today-import-overlay", "today-import-list", "today-import-list-head", "today-import-table-tools", "today-import-estimate", "today-import-preset-popper", "width=\"min(980px, 96vw)\"", "align-center"]) {
  assert.ok(component.includes(token), `PlanListEditor missing ${token}`);
}
assert(!component.includes("尚未加入今日"), "配置今日事项不应逐行重复显示尚未加入今日");
assert(!component.includes("已在今日，可调整预计"), "配置今日事项不应逐行重复显示已在今日提示");
assert.ok(!component.includes('title="分配今日预计"'));
assert.ok(!component.includes("导入今日事项"));
assert.ok(!component.includes(">父事项</small>"), "来源导入一级事项不再显示父事项标签");
for (const token of [".today-import-overlay .el-overlay-dialog", "align-items: center", "justify-content: center", ".today-import-dialog", "height: min(760px, 86vh)", ".today-import-list", "overflow: auto", ".today-import-row", ".today-import-estimate", ".today-import-estimate-editor", "grid-template-columns: 28px minmax(0, 1fr) 28px", ".today-import-minute-dropdown", ".today-import-preset-popper .el-dropdown-menu", "text-overflow: ellipsis", "white-space: nowrap", "@media (max-width: 760px)"]) {
  assert.ok(styles.includes(token), `styles missing ${token}`);
}
assert.ok(store.includes("saveTaskEstimateBatch"));
assert.ok(store.includes("saveTodayImport"));
assert.ok(store.includes("const directInstanceEstimates = (request.instanceEstimates ?? []).flatMap"));
assert.ok(store.includes("await saveTaskEstimateBatch([...instanceEstimates, ...directInstanceEstimates])"));
assert.ok(component.includes("todayImportInstanceTasks.value.find((candidate) => candidate.id === task.id)"));
assert.ok(!component.includes("const source = props.tasks.find((candidate) => candidate.id === task.id)"), "配置今日事项不得把标题编辑写入来源事项");
assert.ok(tauri.includes("task_daily_estimate_batch_set"));
console.log("today import estimate allocation UI checks passed");
