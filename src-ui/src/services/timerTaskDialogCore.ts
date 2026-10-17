export type TimerDialogSubject = { id: string };

export type TimerDialogTask = {
  id: string;
  subjectId: string;
  parentId?: string;
  title: string;
  status: "open" | "done";
  recurring?: boolean;
  selectable: boolean;
  todayEstimate?: number | null;
  directMinutes: number;
  totalMinutes: number;
  note?: string;
};

export type TimerDialogTaskRow<T extends TimerDialogTask = TimerDialogTask> = {
  task: T;
  depth: number;
  hasChildren: boolean;
  totalTodayEstimate: number;
  confirmedMinutes: number;
};

export type TimerDialogAction = {
  mode: "parent" | "start" | "switch" | "running" | "paused";
  label: string;
  ariaLabel: string;
  actionable: boolean;
};

export type TimerDialogDraftField = "title" | "todayEstimate" | "note";

export type TimerDialogDraft = {
  title: string;
  todayEstimate: string;
  note: string;
  baseline: Record<TimerDialogDraftField, string>;
  revisions: Record<TimerDialogDraftField, number>;
  saving: Partial<Record<TimerDialogDraftField, number>>;
  saved: Partial<Record<TimerDialogDraftField, number>>;
  errors: Partial<Record<TimerDialogDraftField, string>>;
};

export type TimerDialogSaveToken = { field: TimerDialogDraftField; revision: number };

export function resolveTimerDialogSubject(request: {
  subjects: readonly TimerDialogSubject[];
  contextSubjectId?: string;
  selectedSubjectId?: string;
  activeTaskSubjectId?: string;
}) {
  const valid = new Set(request.subjects.map((subject) => subject.id));
  return [request.contextSubjectId, request.selectedSubjectId, request.activeTaskSubjectId]
    .find((subjectId) => Boolean(subjectId && valid.has(subjectId)))
    ?? request.subjects[0]?.id
    ?? "";
}

export function timerDialogRows<T extends TimerDialogTask>(
  tasks: readonly T[],
  subjectId: string,
  query = "",
): TimerDialogTaskRow<T>[] {
  const active = tasks.filter((task) => task.subjectId === subjectId && (task.status === "open" || task.recurring));
  const activeIds = new Set(active.map((task) => task.id));
  const children = new Map<string, T[]>();
  const roots: T[] = [];
  for (const task of active) {
    if (task.parentId && activeIds.has(task.parentId)) {
      const siblings = children.get(task.parentId) ?? [];
      siblings.push(task);
      children.set(task.parentId, siblings);
    } else {
      roots.push(task);
    }
  }

  const keyword = query.trim().toLocaleLowerCase();
  const matches = (task: T) => !keyword || `${task.title} ${task.note ?? ""}`.toLocaleLowerCase().includes(keyword);
  const includeTask = (task: T): boolean => matches(task) || (children.get(task.id) ?? []).some(includeTask);
  const sumTodayEstimate = (task: T): number => {
    const descendants = children.get(task.id) ?? [];
    return descendants.length
      ? descendants.reduce((total, child) => total + sumTodayEstimate(child), 0)
      : Math.max(0, task.todayEstimate ?? 0);
  };

  const flatten = (task: T, depth: number, ancestorMatched = false): TimerDialogTaskRow<T>[] => {
    const descendants = children.get(task.id) ?? [];
    const taskMatches = matches(task);
    const visibleChildren = keyword && !ancestorMatched && !taskMatches ? descendants.filter(includeTask) : descendants;
    if (keyword && !ancestorMatched && !taskMatches && !visibleChildren.length) return [];
    const hasChildren = descendants.length > 0;
    return [{
      task,
      depth,
      hasChildren,
      totalTodayEstimate: sumTodayEstimate(task),
      confirmedMinutes: hasChildren ? task.totalMinutes : task.directMinutes,
    }, ...visibleChildren.flatMap((child) => flatten(child, depth + 1, ancestorMatched || taskMatches))];
  };

  return roots.filter(includeTask).flatMap((task) => flatten(task, 0));
}

export function timerDialogAction(request: {
  taskId: string;
  taskTitle: string;
  hasChildren: boolean;
  activeTaskId?: string;
  activeState?: "running" | "paused" | "ended";
}): TimerDialogAction {
  if (request.hasChildren) return { mode: "parent", label: "汇总", ariaLabel: `${request.taskTitle}为父事项，不可直接计时`, actionable: false };
  if (!request.activeTaskId) return { mode: "start", label: "开始", ariaLabel: `开始计时：${request.taskTitle}`, actionable: true };
  if (request.activeTaskId !== request.taskId) return { mode: "switch", label: "切换", ariaLabel: `切换计时到：${request.taskTitle}`, actionable: true };
  if (request.activeState === "paused") return { mode: "paused", label: "已暂停", ariaLabel: `${request.taskTitle}计时已暂停`, actionable: false };
  return { mode: "running", label: "计时中", ariaLabel: `${request.taskTitle}正在计时`, actionable: false };
}

export function createTimerDialogDraft(task: { title: string; todayEstimate?: number | null; note?: string | null }): TimerDialogDraft {
  const values = {
    title: task.title,
    todayEstimate: task.todayEstimate == null ? "" : String(task.todayEstimate),
    note: task.note ?? "",
  };
  return {
    ...values,
    baseline: { ...values },
    revisions: { title: 0, todayEstimate: 0, note: 0 },
    saving: {},
    saved: {},
    errors: {},
  };
}

export function updateTimerDialogDraft(draft: TimerDialogDraft, field: TimerDialogDraftField, value: string) {
  draft[field] = value;
  draft.revisions[field] += 1;
  delete draft.saved[field];
  delete draft.errors[field];
}

export function timerDialogDraftDirty(draft: TimerDialogDraft, field: TimerDialogDraftField) {
  return draft[field] !== draft.baseline[field];
}

export function beginTimerDialogSave(draft: TimerDialogDraft, field: TimerDialogDraftField): TimerDialogSaveToken {
  const token = { field, revision: draft.revisions[field] };
  draft.saving[field] = token.revision;
  delete draft.errors[field];
  return token;
}

export function settleTimerDialogSave(
  draft: TimerDialogDraft,
  token: TimerDialogSaveToken,
  result: { ok: true; savedValue: string } | { ok: false; message: string },
) {
  if (draft.saving[token.field] === token.revision) delete draft.saving[token.field];
  if (draft.revisions[token.field] !== token.revision) return false;
  if (result.ok === true) {
    draft.baseline[token.field] = result.savedValue;
    draft.saved[token.field] = token.revision;
    delete draft.errors[token.field];
  } else {
    delete draft.saved[token.field];
    draft.errors[token.field] = result.message;
  }
  return true;
}
