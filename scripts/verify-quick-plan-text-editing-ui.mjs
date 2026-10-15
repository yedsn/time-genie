import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const component = readFileSync(new URL("../src-ui/src/components/PlanListEditor.vue", import.meta.url), "utf8");
const styles = readFileSync(new URL("../src-ui/src/styles.css", import.meta.url), "utf8");
const workbench = readFileSync(new URL("../src-ui/src/views/MainWorkbench.vue", import.meta.url), "utf8");

for (const marker of ["快速编辑计划", "预览变化", "确认删除并保存", "<textarea", "quickPlanText", "handleQuickPlanTextareaKey", "requestCloseQuickPlan"]) {
  assert.equal(component.includes(marker), true, `missing UI marker: ${marker}`);
}
const actionMarkup = component.slice(component.indexOf('class="plan-head-actions"'));
assert.equal(actionMarkup.indexOf("分配今日预计") < actionMarkup.indexOf("快速编辑计划"), true);
assert.equal(workbench.includes(':save-plan-batch="store.saveTaskPlanBatch"'), true);
for (const marker of [".quick-plan-dialog", ".quick-plan-editor", ".quick-plan-delete-summary", "max-height: 88vh", "overflow: auto"]) {
  assert.equal(styles.includes(marker), true, `missing style marker: ${marker}`);
}
console.log("quick plan text editing UI checks passed");
