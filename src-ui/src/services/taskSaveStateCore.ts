export type EditableTaskState = {
  title: string;
  priority?: string;
  plannedDate?: string;
  estimateMinutes?: number;
  note?: string;
  projectName?: string;
};

export function editableTaskStateEquals(left: EditableTaskState, right: EditableTaskState) {
  return left.title === right.title
    && left.priority === right.priority
    && left.plannedDate === right.plannedDate
    && left.estimateMinutes === right.estimateMinutes
    && left.note === right.note
    && left.projectName === right.projectName;
}

export function mergeAuthoritativeTaskWithLatestDraft<T extends EditableTaskState>(
  authoritative: T,
  current: EditableTaskState | undefined,
  submitted: EditableTaskState,
) {
  if (!current || editableTaskStateEquals(current, submitted)) return authoritative;
  return {
    ...authoritative,
    title: current.title,
    priority: current.priority,
    plannedDate: current.plannedDate,
    estimateMinutes: current.estimateMinutes,
    note: current.note,
    projectName: current.projectName,
  };
}
