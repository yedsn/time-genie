import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const component = readFileSync(new URL("../src-ui/src/components/PlanListEditor.vue", import.meta.url), "utf8");
const styles = readFileSync(new URL("../src-ui/src/styles.css", import.meta.url), "utf8");
const store = readFileSync(new URL("../src-ui/src/store.ts", import.meta.url), "utf8");
const tauri = readFileSync(new URL("../src-ui/src/services/tauri.ts", import.meta.url), "utf8");
for (const token of ["配置今日事项", "选择今日事项并填写今日预计", "新加入", "保存配置", "todayImportSelection", "todayImportEstimates", "todayImportTotalMinutes", "saveTodayImport", "todayImportEstimatePresets", "adjustTodayImportEstimate", "setTodayImportEstimate", "adjustQuickEstimateMinutes", "inputmode=\"numeric\"", "今日预计", "可留空", "父事项 · 仅用于层级识别", "减少 15 分钟", "增加 15 分钟", "30 分钟", "1 小时 30 分钟", "2 小时", "2 小时 30 分钟", "today-import-dialog", "today-import-overlay", "today-import-list", "today-import-list-head", "today-import-table-tools", "today-import-estimate", "today-import-preset-popper", "width=\"min(820px, 95vw)\"", "align-center"]) {
  assert.ok(component.includes(token), `PlanListEditor missing ${token}`);
}
assert(!component.includes("尚未加入今日"), "配置今日事项不应逐行重复显示尚未加入今日");
assert(!component.includes("已在今日，可调整预计"), "配置今日事项不应逐行重复显示已在今日提示");
assert.ok(!component.includes('title="分配今日预计"'));
assert.ok(!component.includes("导入今日事项"));
for (const token of [".today-import-overlay .el-overlay-dialog", "align-items: center", "justify-content: center", ".today-import-dialog", "height: min(86vh, 720px)", ".today-import-list", "overflow: auto", ".today-import-row", ".today-import-estimate", ".today-import-estimate-editor", "grid-template-columns: 30px minmax(92px, 1fr) 30px", ".today-import-minute-dropdown", ".today-import-preset-popper .el-dropdown-menu", "text-overflow: ellipsis", "white-space: nowrap", "@media (max-width: 760px)"]) {
  assert.ok(styles.includes(token), `styles missing ${token}`);
}
assert.ok(store.includes("saveTaskEstimateBatch"));
assert.ok(store.includes("saveTodayImport"));
assert.ok(tauri.includes("task_daily_estimate_batch_set"));
console.log("today import estimate allocation UI checks passed");
