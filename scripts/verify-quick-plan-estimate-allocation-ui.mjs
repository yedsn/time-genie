import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const component = readFileSync(new URL("../src-ui/src/components/PlanListEditor.vue", import.meta.url), "utf8");
const styles = readFileSync(new URL("../src-ui/src/styles.css", import.meta.url), "utf8");
const store = readFileSync(new URL("../src-ui/src/store.ts", import.meta.url), "utf8");
const tauri = readFileSync(new URL("../src-ui/src/services/tauri.ts", import.meta.url), "utf8");
for (const token of ["分配今日预计", "直接输入分钟数", "data-quick-estimate-input", "data-quick-estimate-submit", "adjustQuickMinutes(row.task.id, -15)", "adjustQuickMinutes(row.task.id, 15)", "quickDraftHours(row.task.id)", "String(minutes ?? 0)", "inputmode=\"numeric\"", "作为分组，不参与合计", "重复事项的修改会同时更新", "quickCapacitySummary.overMinutes", "align-center", "modal-class=\"quick-estimate-overlay\""]) {
  assert.ok(component.includes(token), `PlanListEditor missing ${token}`);
}
assert.ok(!component.includes("v-for=\"minutes in [15, 30, 45, 60]\""));
for (const token of [".quick-estimate-overlay .el-overlay-dialog", "align-items: center", "justify-content: center", "height: min(90vh, 760px)", "max-height: 90vh", ".quick-estimate-input-wrap .error", "overflow: auto"]) {
  assert.ok(styles.includes(token), `styles missing ${token}`);
}
assert.ok(component.includes('v-if="quickEstimateErrors[row.task.id]" class="error"'));
assert.ok(!component.includes("quickEstimateErrors[row.task.id] || '&nbsp;'"));
assert.ok(!styles.includes("grid-template-rows: 32px 15px"));
assert.ok(!styles.includes(".quick-estimate-input-wrap .error.visible"));
assert.ok(store.includes("saveTaskEstimateBatch"));
assert.ok(tauri.includes("task_daily_estimate_batch_set"));
console.log("quick plan estimate allocation UI checks passed");
