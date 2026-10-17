import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const editor = await readFile(new URL("../src-ui/src/components/PlanListEditor.vue", import.meta.url), "utf8");
const workbench = await readFile(new URL("../src-ui/src/views/MainWorkbench.vue", import.meta.url), "utf8");
const store = await readFile(new URL("../src-ui/src/store.ts", import.meta.url), "utf8");
const sortableCoordinator = await readFile(new URL("../src-ui/src/services/taskTreeSortable.ts", import.meta.url), "utf8");
const executionPanel = await readFile(new URL("../src-ui/src/components/TimerTaskDialog.vue", import.meta.url), "utf8");

for (const text of ["在上方新增", "在下方新增", "新建子事项", "编辑备注", "复制", "删除"]) {
  assert.ok(editor.includes(text), `missing context menu command: ${text}`);
}
assert.ok(editor.includes('@contextmenu="openContextMenu($event, row.task)"'));
assert.ok(editor.includes('target.closest("input, textarea")'));
assert.ok(editor.includes('document.addEventListener("pointerdown"'));
assert.ok(editor.includes('document.addEventListener("keydown"'));
assert.ok(editor.includes('window.addEventListener("scroll"'));
assert.match(editor, /closeContextMenu\(\);\r?\n\s+isSorting\.value = true;/);
assert.ok(editor.includes('todayEstimateDraftValue'));
assert.ok(editor.includes('v-model.number="todayEstimateDraftValue"'));
assert.ok(editor.includes('noteDialogOpen'));
assert.ok(editor.includes('row.task.note?.trim()'));
assert.ok(editor.includes('createTaskTreeSortable'));
assert.ok(executionPanel.includes('createTaskTreeSortable'));
assert.ok(executionPanel.includes('store.moveExecutionTask'));
assert.ok(sortableCoordinator.includes('planTaskMoveIntent'));
assert.ok(sortableCoordinator.includes('taskDragTargetInvalid'));
assert.ok(workbench.includes('@move="handleTaskMove"'));
assert.ok(workbench.includes('@save-note="handleTaskNoteSave"'));
assert.ok(workbench.includes('@save-today-estimate="handleTodayEstimateSave"'));
assert.ok(store.includes('createPositionedTask'));
assert.ok(store.includes('addTaskRelative'));
assert.ok(store.includes('moveExecutionTask'));
console.log("plan task interaction UI verification passed");
