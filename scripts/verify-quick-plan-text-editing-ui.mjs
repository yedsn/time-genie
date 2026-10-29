import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const component = readFileSync(new URL("../src-ui/src/components/PlanListEditor.vue", import.meta.url), "utf8");
const styles = readFileSync(new URL("../src-ui/src/styles.css", import.meta.url), "utf8");
const workbench = readFileSync(new URL("../src-ui/src/views/MainWorkbench.vue", import.meta.url), "utf8");

for (const marker of ["编辑今日计划", "编辑全部计划", "预览变化", "确认删除并保存", "确认移出并保存", "<MdEditor", "quickPlanMarkdownToolbars", '"underline"', '"unorderedList"', "quickPlanText", "mode.value === \"today\" ? props.todayDate : undefined", "handleQuickPlanTextareaKey", '@keydown.capture="handleQuickPlanTextareaKey"', "requestCloseQuickPlan"]) {
  assert.equal(component.includes(marker), true, `missing UI marker: ${marker}`);
}
const actionMarkup = component.slice(component.indexOf('class="plan-head-actions"'));
assert.equal(actionMarkup.indexOf("配置今日事项") < actionMarkup.indexOf("编辑今日计划"), true);
assert.equal(workbench.includes(':save-plan-batch="store.saveTaskPlanBatch"'), true);
assert.equal(component.includes('mode.value === "today" ? (props.displayTasks ?? []) : props.tasks'), true);
assert.equal(component.includes('props.savePlanBatch(request, mode.value === "today" ? props.todayDate : undefined)'), true);
assert.equal(workbench.includes(':display-tasks="store.todayPlanTasks"'), true);
for (const marker of [".quick-plan-dialog", ".quick-plan-editor", ".quick-plan-editor .md-editor-toolbar-wrapper", ".quick-plan-editor .md-editor-content .cm-content", '"Microsoft YaHei UI"', "font-weight: 500", "font-size: 16px", ".quick-plan-delete-summary", "max-height: 88vh", "overflow: auto"]) {
  assert.equal(styles.includes(marker), true, `missing style marker: ${marker}`);
}
console.log("quick plan text editing UI checks passed");

const importBar = component.slice(component.indexOf('class="quick-plan-import-bar"'), component.indexOf("<MdEditor"));
assert.equal(component.includes("v-if=\"mode === 'today'\" class=\"quick-plan-import-bar\""), true);
assert.equal(importBar.includes('@click="openQuickPlanImport"'), true);
assert.equal(importBar.includes("从进行中导入"), true);
const openImport = component.slice(component.indexOf("function openQuickPlanImport()"), component.indexOf("function importQuickPlanText()"));
assert.equal(openImport.includes('todaySourceImportTarget.value = "text"'), true);
assert.equal(openImport.includes("todayImportSelection.value = [...todayImportSelectableIds.value]"), true);
const importText = component.slice(component.indexOf("function importQuickPlanText()"), component.indexOf("function handleQuickPlanTextareaKey"));
assert.equal(importText.includes("appendQuickPlanImport"), true);
assert.equal(importText.includes("orderedTaskSelectionWithAncestors"), true);
assert.equal(importText.includes("props.saveTodayImport"), false, "text import must remain an unsaved draft");
assert.equal(styles.includes(".quick-plan-import-bar"), true);
console.log("quick plan text import UI checks passed");
