export type CompletedTaskHistoryRequest = {
  subjectId: string;
  page: number;
  sequence: number;
};

export function isCurrentCompletedTaskHistoryRequest(
  request: CompletedTaskHistoryRequest,
  currentSubjectId: string,
  currentPage: number,
  latestSequence: number,
) {
  return request.subjectId === currentSubjectId
    && request.page === currentPage
    && request.sequence === latestSequence;
}

export function completedTaskHistoryFallbackPage(page: number, itemCount: number) {
  return itemCount === 0 && page > 1 ? page - 1 : page;
}

export function isActivePlanTask(task: { status: "open" | "done"; recurring: boolean }) {
  return task.status === "open" || task.recurring;
}
