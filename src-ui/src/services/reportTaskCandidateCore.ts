export type ReportTaskCandidateLike = {
  taskId: string;
  parentId?: string;
  defaultSelected: boolean;
  candidateRole: "selectable" | "context";
};

export function selectableReportTaskIds<T extends ReportTaskCandidateLike>(candidates: T[]) {
  return candidates.filter((candidate) => candidate.candidateRole === "selectable").map((candidate) => candidate.taskId);
}

export function defaultReportTaskIds<T extends ReportTaskCandidateLike>(candidates: T[]) {
  return candidates.filter((candidate) => candidate.defaultSelected).map((candidate) => candidate.taskId);
}

export function preserveReportTaskSelection<T extends ReportTaskCandidateLike>(candidates: T[], selectedIds: string[]) {
  const validIds = new Set(selectableReportTaskIds(candidates));
  return selectedIds.filter((taskId) => validIds.has(taskId));
}

export function reportTaskDepth<T extends ReportTaskCandidateLike>(candidate: T, candidates: T[]) {
  const byId = new Map(candidates.map((item) => [item.taskId, item]));
  const visited = new Set<string>();
  let depth = 0;
  let parentId = candidate.parentId;
  while (parentId && depth < 64 && !visited.has(parentId)) {
    visited.add(parentId);
    const parent = byId.get(parentId);
    if (!parent) break;
    depth += 1;
    parentId = parent.parentId;
  }
  return depth;
}
