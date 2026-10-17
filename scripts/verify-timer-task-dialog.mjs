import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  beginTimerDialogSave,
  createTimerDialogDraft,
  resolveTimerDialogSubject,
  settleTimerDialogSave,
  timerDialogAction,
  timerDialogDraftDirty,
  timerDialogRows,
  updateTimerDialogDraft,
} from "../.tmp-timer-task-dialog-test/timerTaskDialogCore.js";

const subjects = [{ id: "a" }, { id: "b" }];
assert.equal(resolveTimerDialogSubject({ subjects, contextSubjectId: "b", selectedSubjectId: "a" }), "b");
assert.equal(resolveTimerDialogSubject({ subjects, contextSubjectId: "missing", selectedSubjectId: "a" }), "a");
assert.equal(resolveTimerDialogSubject({ subjects: [] }), "");

const tasks = [
  { id: "p", subjectId: "a", title: "产品", status: "open", selectable: false, directMinutes: 0, totalMinutes: 50 },
  { id: "c1", subjectId: "a", parentId: "p", title: "登录页", status: "open", selectable: true, todayEstimate: 30, directMinutes: 20, totalMinutes: 20 },
  { id: "c2", subjectId: "a", parentId: "p", title: "同步", status: "done", selectable: true, todayEstimate: 40, directMinutes: 30, totalMinutes: 30 },
  { id: "b1", subjectId: "b", title: "其他主体", status: "open", selectable: true, directMinutes: 5, totalMinutes: 5 },
];
assert.deepEqual(timerDialogRows(tasks, "a").map((row) => [row.task.id, row.depth, row.totalTodayEstimate, row.confirmedMinutes]), [["p", 0, 30, 50], ["c1", 1, 30, 20]]);
assert.deepEqual(timerDialogRows(tasks, "a", "登录").map((row) => row.task.id), ["p", "c1"]);
assert.deepEqual(timerDialogRows(tasks, "a", "产品").map((row) => row.task.id), ["p", "c1"]);
assert.deepEqual(timerDialogRows(tasks, "b").map((row) => row.task.id), ["b1"]);

assert.equal(timerDialogAction({ taskId: "p", taskTitle: "产品", hasChildren: true }).mode, "parent");
assert.equal(timerDialogAction({ taskId: "c1", taskTitle: "登录页", hasChildren: false }).mode, "start");
assert.equal(timerDialogAction({ taskId: "c1", taskTitle: "登录页", hasChildren: false, activeTaskId: "b1", activeState: "running" }).mode, "switch");
assert.equal(timerDialogAction({ taskId: "c1", taskTitle: "登录页", hasChildren: false, activeTaskId: "c1", activeState: "paused" }).mode, "paused");
assert.match(timerDialogAction({ taskId: "c1", taskTitle: "登录页", hasChildren: false }).ariaLabel, /登录页/);

const draft = createTimerDialogDraft({ title: "旧标题", todayEstimate: 30 });
assert.equal(createTimerDialogDraft({ title: "空预计", todayEstimate: null, note: null }).todayEstimate, "");
updateTimerDialogDraft(draft, "title", "第一版");
const stale = beginTimerDialogSave(draft, "title");
updateTimerDialogDraft(draft, "title", "第二版");
assert.equal(settleTimerDialogSave(draft, stale, { ok: true, savedValue: "第一版" }), false);
assert.equal(timerDialogDraftDirty(draft, "title"), true);
const current = beginTimerDialogSave(draft, "title");
assert.equal(settleTimerDialogSave(draft, current, { ok: true, savedValue: "第二版" }), true);
assert.equal(timerDialogDraftDirty(draft, "title"), false);
assert.equal(draft.saved.title, current.revision);
updateTimerDialogDraft(draft, "title", "第三版");
assert.equal(draft.saved.title, undefined);

const dialog = readFileSync("src-ui/src/components/TimerTaskDialog.vue", "utf8");
const globalTimer = readFileSync("src-ui/src/components/GlobalTimerBar.vue", "utf8");
const tracking = readFileSync("src-ui/src/components/TimeTrackingWorkspace.vue", "utf8");
const today = readFileSync("src-ui/src/components/TodayOverviewWorkspace.vue", "utf8");
const workbench = readFileSync("src-ui/src/views/MainWorkbench.vue", "utf8");
const plan = readFileSync("src-ui/src/components/PlanListEditor.vue", "utf8");
const storeSource = readFileSync("src-ui/src/store.ts", "utf8");
const styles = readFileSync("src-ui/src/styles.css", "utf8");
assert.match(dialog, /timer-task-subject-tabs/);
assert.match(dialog, /store\.workspaceLoading/);
assert.match(dialog, /@closed="restoreTriggerFocus"/);
assert.match(dialog, /@opened="focusInitialControl"/);
assert.match(dialog, /returnFocusElement/);
assert.match(dialog, /saveField\(row\.task, 'title'\)/);
assert.match(dialog, /while \(timerDialogDraftDirty\(draftFor\(task\), field\)\)/);
assert.match(dialog, /已保存/);
assert.match(dialog, /store\.transitionTimer/);
assert.match(dialog, /if \(field === "title" && mutatedStoreTitle\) await store\.loadWorkspaceData\(\)/);
assert.match(dialog, /focusActionAfterFailure = true;[\s\S]*timerBusyTaskId\.value = "";[\s\S]*focusTaskControl\(task\.id\)/);
assert.doesNotMatch(dialog, /data-timer-task-field="todayEstimate"/);
assert.doesNotMatch(dialog, /新事项今日预计分钟/);
assert.doesNotMatch(dialog, /saveExecutionTaskTodayEstimate/);
assert.match(dialog, /今日预计请在待办页调整/);
assert.match(dialog, /timer-task-estimate-cell readonly/);
assert.ok(dialog.includes("estimateCellLabel(row)"));
assert.ok(dialog.includes("row.estimateSource === 'overall'"));
assert.ok(dialog.includes("{{ row.task.recurrence ? '每次预计' : '整体预计' }}"));
assert.ok(dialog.includes("{ summary: row.hasChildren, empty: row.totalTodayEstimate <= 0 }"));
assert.match(dialog, /executionPanelAction/);
assert.match(dialog, /executionPanelRows/);
assert.match(dialog, /presentation.title/);
assert.match(dialog, /Sortable/);
assert.match(dialog, /timer-task-drag-handle/);
assert.match(dialog, /taskSubtreeIds/);
assert.match(dialog, /query.value.trim()/);
assert.match(dialog, /store\.moveExecutionTask/);
assert.match(dialog, /store\.saveExecutionTaskDetails/);
assert.match(dialog, /store\.deleteExecutionTask/);
assert.match(dialog, /allocationTaskTitles/);
assert.match(dialog, /HISTORICAL_RECORD_READ_ONLY/);
assert.match(dialog, /row\.task\.status !== 'done'/);
assert.match(dialog, /v-if="presentation\.showTimerActions"/);
assert.match(dialog, /presentation\.showAllocation/);
assert.match(dialog, /!rows\.length && !newTask\.open/);
assert.match(dialog, /actionFor\(row\.task, row\.hasChildren\)/);
assert.match(dialog, /panelInitialLoading/);
assert.match(dialog, /timer-task-list[^>]*allocation: presentation\.showAllocation/);
assert.match(dialog, /timer-task-complete-toggle[^>]*empty:/);
assert.doesNotMatch(dialog, /分配保存前只读/);
assert.match(styles, /timer-task-list\.allocation[^}]*--timer-task-actions-width: 312px/);
assert.match(styles, /timer-task-complete-toggle\.empty[^}]*visibility: hidden/);
assert.match(storeSource, /const moved = await moveTaskRecord\([\s\S]*snapshot\.tasks\.splice\([\s\S]*replaceTask\(moved\);[\s\S]*return await loadExecutionSnapshot\(subjectId\)/);
assert.doesNotMatch(globalTimer, /TaskSelectControl/);
assert.match(globalTimer, /emit\('open'\)/);
assert.match(tracking, /time-task-dialog-trigger/);
assert.match(tracking, /<TaskSelectControl[\s\S]*aria-label="补录事项"/);
assert.doesNotMatch(tracking, /aria-label="分配事项"/);
assert.match(tracking, /调整归属/);
assert.match(tracking, /历史记录只读/);
assert.match(today, /emit\('openTimer', task\.subjectId\)/);
assert.match(today, /store\.runningEntry \? '切换执行事项' : '开始执行事项'/);
assert.match(workbench, /onOpenTimerTaskSelection\(\(\) => openTimerTaskDialog\(\)\)/);
assert.match(workbench, /executionPanelMode/);
assert.match(workbench, /mode: "unassigned-allocation"/);
assert.match(workbench, /@open-entry-allocation="openEntryAllocation"/);
assert.doesNotMatch(workbench, /<UnassignedTimeDialog/);
assert.match(workbench, /label: "待办"/);
assert.match(workbench, /label: "计时"/);
assert.doesNotMatch(plan, /TimerTaskDialog|timer-task-dialog/);

console.log("timer task dialog verification passed");
