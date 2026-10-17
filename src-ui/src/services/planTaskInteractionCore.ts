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
