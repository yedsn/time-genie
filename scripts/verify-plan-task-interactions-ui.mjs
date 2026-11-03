import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const editor = await readFile(new URL("../src-ui/src/components/PlanListEditor.vue", import.meta.url), "utf8");
const workbench = await readFile(new URL("../src-ui/src/views/MainWorkbench.vue", import.meta.url), "utf8");
const store = await readFile(new URL("../src-ui/src/store.ts", import.meta.url), "utf8");
const sortableCoordinator = await readFile(new URL("../src-ui/src/services/taskTreeSortable.ts", import.meta.url), "utf8");
const executionPanel = await readFile(new URL("../src-ui/src/components/TimerTaskDialog.vue", import.meta.url), "utf8");
const styles = await readFile(new URL("../src-ui/src/styles.css", import.meta.url), "utf8");

for (const [label, direction] of [["上移", -1], ["下移", 1]]) {
  const button = editor.match(new RegExp(`<button[^>]*aria-label="${label}"[^>]*>`))?.[0];
  assert.ok(button, `missing task step move button: ${label}`);
  assert.ok(button.includes("task-move-action"));
  assert.ok(!button.includes("hover-row-action"), "position controls share group-level hover visibility");
  assert.ok(button.includes(`:disabled="!canMoveTask(row.task, ${direction})"`));
  assert.ok(button.includes(`@click.stop="moveTaskByStep(row.task, ${direction})"`));
}
assert.ok(editor.includes("planTaskStepMoveIntent(visiblePendingRows.value.map((row) => row.task), task.id, direction)"));
assert.ok(editor.includes("await props.moveTask(task.id, intent.parentId, intent.previousSiblingId, intent.nextSiblingId)"));
assert.ok(editor.includes("task.version > 0 && !isSorting.value && !isMovingTask.value"));
assert.ok(editor.includes("disabled: () => isMovingTask.value"));
assert.ok(styles.includes(".task-move-action:focus-visible"));
const positionControls = editor.match(/<div class="task-position-controls"[^>]*>(.*?)<[/]div>/s)?.[1];
assert.ok(positionControls, "missing fixed task position control group");
assert.deepEqual([...positionControls.matchAll(/aria-label="([^"]+)"/g)].map(match => match[1]), ["上移", "下移", "减少缩进", "增加缩进"]);
assert.ok(!positionControls.includes("v-if="), "all four position slots must remain present at every task depth");
assert.ok(editor.includes('class="task-secondary-controls"'));
assert.ok(styles.includes(".pending-reminder-row .reminder-inline-controls { display: grid; grid-template-columns: 111px 111px; }"));
assert.ok(styles.includes(".task-position-controls { opacity: 0; pointer-events: none;"));
assert.ok(styles.includes(".pending-reminder-row:hover .task-position-controls"));
assert.ok(styles.includes(".pending-reminder-row:focus-within .task-position-controls"));
assert.ok(!styles.includes(".pending-reminder-row.selected .task-position-controls"), "selection alone must not reveal position controls");

for (const text of ["复制事项", "在上方新增", "在下方新增", "新建子事项", "编辑备注", "删除事项"]) {
  assert.ok(editor.includes(text), `missing context menu command: ${text}`);
}
assert.ok(editor.indexOf(">复制事项</button>") < editor.indexOf(">在上方新增</button>"));
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
