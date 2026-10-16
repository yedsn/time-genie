export type InlineTimerParentCandidate = {
  parentId?: string;
  status: "open" | "done";
  recurrence?: unknown;
  directMinutes: number;
  subjectId: string;
};

export function canAcceptInlineTimerChild(task: InlineTimerParentCandidate, subjectId: string) {
  return inlineTimerParentUnavailableReason(task, subjectId) === undefined;
}

export function inlineTimerParentUnavailableReason(task: InlineTimerParentCandidate, subjectId: string) {
  if (task.subjectId !== subjectId) return "属于其他主体";
  if (task.parentId) return "二级事项不能再建子项";
  if (task.status === "done") return "已完成，不能再建子项";
  if (task.recurrence) return "重复事项不能作为父级";
  if (task.directMinutes > 0) return "已有直接实际时间，不能改为分组事项";
  return undefined;
}
