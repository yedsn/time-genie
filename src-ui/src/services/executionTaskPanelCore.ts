export type ExecutionPanelMode = "execute" | "unassigned-allocation" | "entry-allocation";

export type ExecutionPanelContext =
  | { mode: "execute"; subjectId?: string; timerNote?: string }
  | { mode: "unassigned-allocation"; sessionId: string; version: number; workDate: string }
  | { mode: "entry-allocation"; entryId: string; entryVersion: number; workDate: string };

export type ExecutionPanelPresentation = {
  title: string;
  showTimerActions: boolean;
  showActual: boolean;
  actualEditable: boolean;
  showAllocation: boolean;
  showBreakDiscard: boolean;
  primaryLabel: string;
  protectsBusinessDraft: boolean;
};

export function executionPanelPresentation(mode: ExecutionPanelMode): ExecutionPanelPresentation {
  if (mode === "execute") {
    return {
      title: "执行事项",
      showTimerActions: true,
      showActual: true,
      actualEditable: true,
      showAllocation: false,
      showBreakDiscard: false,
      primaryLabel: "开始执行事项",
      protectsBusinessDraft: true,
    };
  }
  return {
    title: "执行事项",
    showTimerActions: false,
    showActual: true,
    actualEditable: false,
    showAllocation: true,
    showBreakDiscard: true,
    primaryLabel: mode === "unassigned-allocation" ? "确认分配" : "保存归属",
    protectsBusinessDraft: true,
  };
}

export type ExecutionPanelEstimateSource = "today" | "overall" | "none";

export type ExecutionPanelTask = {
  id: string;
  subjectId: string;
  parentId?: string;
  title: string;
  note?: string;
  status: "open" | "done";
  completedToday?: boolean;
  pinned?: boolean;
  selectable: boolean;
  todayEstimate?: number | null;
  estimate?: number | null;
  directMinutes: number;
  totalMinutes: number;
  sortOrder?: number;
  timerState?: "running" | "paused";
};

export type ExecutionPanelTaskRow<T extends ExecutionPanelTask = ExecutionPanelTask> = {
  task: T;
  depth: number;
  hasChildren: boolean;
  totalTodayEstimate: number;
  estimateSource: ExecutionPanelEstimateSource;
  confirmedMinutes: number;
  pinned: boolean;
};

/**
 * 面板里的预计值：优先今天的预计，没有今日预计时回退到整体预计（重复事项即每次预计）。
 * 两者都没有才视为未设置。
 */
export function taskEstimateDisplay<T extends ExecutionPanelTask>(
  task: T,
): { minutes: number; source: ExecutionPanelEstimateSource } {
  const today = Math.max(0, task.todayEstimate ?? 0);
  if (today > 0) return { minutes: today, source: "today" };
  const overall = Math.max(0, task.estimate ?? 0);
  return overall > 0 ? { minutes: overall, source: "overall" } : { minutes: 0, source: "none" };
}

function stableTree<T extends ExecutionPanelTask>(tasks: readonly T[], subjectId: string) {
  const subjectTasks = tasks.filter((task) => task.subjectId === subjectId);
  const byId = new Map(subjectTasks.map((task) => [task.id, task]));
  const included = new Set(subjectTasks
    .filter((task) => task.status === "open" || task.completedToday || task.pinned)
    .map((task) => task.id));
  for (const taskId of [...included]) {
    let parentId = byId.get(taskId)?.parentId;
    while (parentId && byId.has(parentId)) {
      included.add(parentId);
      parentId = byId.get(parentId)?.parentId;
    }
  }
  const selected = subjectTasks.filter((task) => included.has(task.id));
  const selectedIds = new Set(selected.map((task) => task.id));
  const children = new Map<string, T[]>();
  const roots: T[] = [];
  for (const task of selected) {
    if (task.parentId && selectedIds.has(task.parentId)) {
      const siblings = children.get(task.parentId) ?? [];
      siblings.push(task);
      children.set(task.parentId, siblings);
    } else {
      roots.push(task);
    }
  }
  return { roots, children };
}

export function executionPanelRows<T extends ExecutionPanelTask>(
  tasks: readonly T[],
  subjectId: string,
  query = "",
): ExecutionPanelTaskRow<T>[] {
  const { roots, children } = stableTree(tasks, subjectId);
  const keyword = query.trim().toLocaleLowerCase();
  const matches = (task: T) => !keyword || `${task.title} ${task.note ?? ""}`.toLocaleLowerCase().includes(keyword);
  const includeTask = (task: T): boolean => matches(task) || (children.get(task.id) ?? []).some(includeTask);
  const sumTodayEstimate = (task: T): number => {
    const descendants = children.get(task.id) ?? [];
    return descendants.length
      ? descendants.reduce((total, child) => total + sumTodayEstimate(child), 0)
      : taskEstimateDisplay(task).minutes;
  };
  const flatten = (task: T, depth: number, ancestorMatched = false): ExecutionPanelTaskRow<T>[] => {
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
      estimateSource: hasChildren ? "none" : taskEstimateDisplay(task).source,
      confirmedMinutes: hasChildren ? task.totalMinutes : task.directMinutes,
      pinned: Boolean(task.pinned),
    }, ...visibleChildren.flatMap((child) => flatten(child, depth + 1, ancestorMatched || taskMatches))];
  };
  return roots.filter(includeTask).flatMap((task) => flatten(task, 0));
}

export type ExecutionPanelAction = {
  mode: "parent" | "start" | "switch" | "running" | "paused" | "continue" | "allocate";
  label: string;
  ariaLabel: string;
  actionable: boolean;
};

export function executionPanelAction(request: {
  mode: ExecutionPanelMode;
  taskId: string;
  taskTitle: string;
  hasChildren: boolean;
  completedToday?: boolean;
  activeTaskId?: string;
  activeState?: "running" | "paused" | "ended";
}): ExecutionPanelAction {
  if (request.hasChildren) return { mode: "parent", label: "汇总", ariaLabel: `${request.taskTitle}为父事项，不可直接执行`, actionable: false };
  if (request.mode !== "execute") return { mode: "allocate", label: "分配", ariaLabel: `分配时间到：${request.taskTitle}`, actionable: true };
  if (request.completedToday) return { mode: "continue", label: "继续", ariaLabel: `继续执行：${request.taskTitle}`, actionable: true };
  if (!request.activeTaskId) return { mode: "start", label: "开始", ariaLabel: `开始执行：${request.taskTitle}`, actionable: true };
  if (request.activeTaskId !== request.taskId) return { mode: "switch", label: "切换", ariaLabel: `切换执行到：${request.taskTitle}`, actionable: true };
  if (request.activeState === "paused") return { mode: "paused", label: "已暂停", ariaLabel: `${request.taskTitle}计时已暂停`, actionable: false };
  return { mode: "running", label: "计时中", ariaLabel: `${request.taskTitle}正在计时`, actionable: false };
}

export function reconcileDraftRecord<T>(
  current: Readonly<Record<string, T>>,
  validTaskIds: readonly string[],
  pinnedTaskIds: readonly string[] = [],
) {
  const valid = new Set([...validTaskIds, ...pinnedTaskIds]);
  return Object.fromEntries(Object.entries(current).filter(([taskId]) => valid.has(taskId))) as Record<string, T>;
}

export function mergeDraftRecord<T>(
  current: Readonly<Record<string, T>>,
  incomingTaskIds: readonly string[],
  create: (taskId: string) => T,
) {
  const next = reconcileDraftRecord(current, incomingTaskIds);
  for (const taskId of incomingTaskIds) next[taskId] ??= create(taskId);
  return next;
}
