export type PlanTaskTreeItem = {
  id: string;
  parentId?: string;
};

export type PlanTaskMoveIntent = {
  parentId?: string;
  previousSiblingId?: string;
  nextSiblingId?: string;
};

export function taskSubtreeIds(rows: readonly PlanTaskTreeItem[], taskId: string) {
  const ids = new Set([taskId]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const row of rows) {
      if (row.parentId && ids.has(row.parentId) && !ids.has(row.id)) {
        ids.add(row.id);
        changed = true;
      }
    }
  }
  return ids;
}

export type HierarchicalTaskSelectionRow<T extends PlanTaskTreeItem> = {
  task: T;
  depth: number;
  selectableIds: string[];
  checked: boolean;
  indeterminate: boolean;
};

/**
 * Flattens a task forest for hierarchical checkbox pickers. Parent rows remain
 * visible as indentation context and act as a checkbox group for their
 * selectable descendants.
 */
export function hierarchicalTaskSelectionRows<T extends PlanTaskTreeItem>(
  tasks: readonly T[],
  selectableIds: ReadonlySet<string>,
  selectedIds: readonly string[],
  options: { query?: string; queryText?: (task: T) => string } = {},
): HierarchicalTaskSelectionRow<T>[] {
  const byId = new Map(tasks.map((task) => [task.id, task]));
  const children = new Map<string, T[]>();
  const roots: T[] = [];
  for (const task of tasks) {
    if (task.parentId && byId.has(task.parentId)) {
      const siblings = children.get(task.parentId) ?? [];
      siblings.push(task);
      children.set(task.parentId, siblings);
    } else {
      roots.push(task);
    }
  }

  const candidateIdsCache = new Map<string, string[]>();
  const candidateIds = (task: T): string[] => {
    const cached = candidateIdsCache.get(task.id);
    if (cached) return cached;
    const subtree = taskSubtreeIds(tasks, task.id);
    const ids = tasks.filter((candidate) => subtree.has(candidate.id) && selectableIds.has(candidate.id)).map((candidate) => candidate.id);
    candidateIdsCache.set(task.id, ids);
    return ids;
  };
  const branchHasCandidates = (task: T): boolean => candidateIds(task).length > 0;

  const keyword = options.query?.trim().toLocaleLowerCase() ?? "";
  const matches = (task: T) => !keyword || (options.queryText?.(task) ?? "").toLocaleLowerCase().includes(keyword);
  const branchMatches = (task: T): boolean => matches(task) || (children.get(task.id) ?? []).some(branchMatches);
  const selected = new Set(selectedIds);
  const rows: HierarchicalTaskSelectionRow<T>[] = [];

  const flatten = (task: T, depth: number) => {
    const ids = candidateIds(task);
    const selectedCount = ids.filter((taskId) => selected.has(taskId)).length;
    rows.push({
      task,
      depth,
      selectableIds: ids,
      checked: ids.length > 0 && selectedCount === ids.length,
      indeterminate: selectedCount > 0 && selectedCount < ids.length,
    });
    for (const child of children.get(task.id) ?? []) {
      if (branchHasCandidates(child)) flatten(child, depth + 1);
    }
  };

  for (const root of roots) {
    if (branchHasCandidates(root) && (!keyword || branchMatches(root))) flatten(root, 0);
  }
  return rows;
}

export function toggleTaskSelectionGroup(selectedIds: readonly string[], groupIds: readonly string[]) {
  const normalizedGroup = [...new Set(groupIds)];
  if (!normalizedGroup.length) return [...selectedIds];
  const selected = new Set(selectedIds);
  if (normalizedGroup.every((taskId) => selected.has(taskId))) {
    const group = new Set(normalizedGroup);
    return selectedIds.filter((taskId) => !group.has(taskId));
  }
  return [...selectedIds, ...normalizedGroup.filter((taskId) => !selected.has(taskId))];
}

export function taskDragTargetDepth(relatedDepth: number, pointerX: number, listLeft: number, indentThreshold = 92): 0 | 1 {
  return relatedDepth > 0 || pointerX > listLeft + indentThreshold ? 1 : 0;
}

export function taskDragTargetInvalid(targetDepth: 0 | 1, movingIds: ReadonlySet<string>) {
  return targetDepth === 1 && movingIds.size > 1;
}

export function todayEstimateDraft(current: number | undefined, parentId?: string) {
  return current ?? (parentId ? 30 : 60);
}

export function effectiveTodayEstimate(
  dailyEstimate: number | undefined,
  recurringEstimate: number | undefined,
  recurring: boolean,
) {
  return dailyEstimate ?? (recurring ? recurringEstimate : undefined);
}

export function planTaskMoveIntent(
  orderedRows: PlanTaskTreeItem[],
  movingId: string,
  movingIds: ReadonlySet<string>,
  targetDepth: 0 | 1,
  movingHasChildren: boolean,
): PlanTaskMoveIntent | undefined {
  if (targetDepth === 1 && movingHasChildren) return undefined;
  const movingIndex = orderedRows.findIndex((item) => item.id === movingId);
  if (movingIndex < 0) return undefined;

  let parentId: string | undefined;
  if (targetDepth === 1) {
    for (let index = movingIndex - 1; index >= 0; index -= 1) {
      const candidate = orderedRows[index];
      if (movingIds.has(candidate.id)) continue;
      if (!candidate.parentId) {
        parentId = candidate.id;
        break;
      }
    }
    if (!parentId) {
      const nextRoot = orderedRows
        .slice(movingIndex + 1)
        .find((candidate) => !movingIds.has(candidate.id) && !candidate.parentId);
      parentId = nextRoot?.id;
    }
    if (!parentId || movingIds.has(parentId)) return undefined;
  }

  const siblings = orderedRows.filter((item) => {
    if (movingIds.has(item.id)) return false;
    return (item.parentId ?? undefined) === parentId;
  });
  let previousSiblingId: string | undefined;
  let nextSiblingId: string | undefined;
  for (const sibling of siblings) {
    const siblingIndex = orderedRows.findIndex((item) => item.id === sibling.id);
    if (siblingIndex < movingIndex) previousSiblingId = sibling.id;
    if (siblingIndex > movingIndex) {
      nextSiblingId = sibling.id;
      break;
    }
  }
  return { parentId, previousSiblingId, nextSiblingId };
}

export function contextMenuPosition(
  clientX: number,
  clientY: number,
  menuWidth: number,
  menuHeight: number,
  viewportWidth: number,
  viewportHeight: number,
) {
  const margin = 8;
  return {
    left: Math.max(margin, Math.min(clientX, viewportWidth - menuWidth - margin)),
    top: Math.max(margin, Math.min(clientY, viewportHeight - menuHeight - margin)),
  };
}
