export type ActualAdjustmentTask = {
  id: string;
  parentId?: string;
  title: string;
  version: number;
  isLeaf: boolean;
  directMinutes: number;
  totalMinutes: number;
  adjustmentFingerprint: string;
  canAcceptChildren: boolean;
};

export type ActualAdjustmentDraft = {
  taskId: string;
  baselineMinutes: number;
  targetMinutes: number;
  text: string;
  expectedTaskVersion: number;
  adjustmentFingerprint: string;
};

export type ActualAdjustmentNewTaskDraft = {
  clientTaskId: string;
  title: string;
  parentId?: string;
  expectedParentVersion?: number;
  targetMinutes: number;
  text: string;
};

export function parseActualMinutes(text: string) {
  const normalized = text.trim();
  if (!/^\d+$/.test(normalized)) return { ok: false as const, message: "请输入 0–1440 的整数分钟" };
  const minutes = Number(normalized);
  if (!Number.isSafeInteger(minutes) || minutes < 0 || minutes > 1440) return { ok: false as const, message: "分钟必须在 0–1440 之间" };
  return { ok: true as const, minutes };
}

export function actualAdjustmentChanges(drafts: readonly ActualAdjustmentDraft[]) {
  return drafts.filter((draft) => draft.targetMinutes !== draft.baselineMinutes).map((draft) => ({
    taskId: draft.taskId,
    expectedTaskVersion: draft.expectedTaskVersion,
    baselineMinutes: draft.baselineMinutes,
    targetMinutes: draft.targetMinutes,
    deltaMinutes: draft.targetMinutes - draft.baselineMinutes,
    adjustmentFingerprint: draft.adjustmentFingerprint,
  }));
}

export function actualAdjustmentSummary(drafts: readonly ActualAdjustmentDraft[], newTasks: readonly ActualAdjustmentNewTaskDraft[]) {
  const changes = actualAdjustmentChanges(drafts);
  const addedMinutes = changes.filter((item) => item.deltaMinutes > 0).reduce((sum, item) => sum + item.deltaMinutes, 0)
    + newTasks.reduce((sum, item) => sum + item.targetMinutes, 0);
  const reducedMinutes = -changes.filter((item) => item.deltaMinutes < 0).reduce((sum, item) => sum + item.deltaMinutes, 0);
  return { changes, addedMinutes, reducedMinutes, netMinutes: addedMinutes - reducedMinutes, adjustmentCount: changes.length + newTasks.filter((item) => item.targetMinutes > 0).length, newTaskCount: newTasks.length };
}

export function eligibleActualAdjustmentParents(tasks: readonly ActualAdjustmentTask[]) {
  return tasks.filter((task) => !task.parentId && task.canAcceptChildren);
}

export function parentTargetTotal(parentId: string, tasks: readonly ActualAdjustmentTask[], drafts: readonly ActualAdjustmentDraft[], newTasks: readonly ActualAdjustmentNewTaskDraft[]) {
  const taskById = new Map(tasks.map((task) => [task.id, task]));
  const belongsTo = (taskId: string) => {
    let current = taskById.get(taskId);
    while (current?.parentId) {
      if (current.parentId === parentId) return true;
      current = taskById.get(current.parentId);
    }
    return false;
  };
  return drafts.filter((draft) => belongsTo(draft.taskId)).reduce((sum, draft) => sum + draft.targetMinutes, 0)
    + newTasks.filter((draft) => draft.parentId === parentId).reduce((sum, draft) => sum + draft.targetMinutes, 0);
}
