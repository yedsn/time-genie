export type TodayPlanRequestToken = {
  sequence: number;
  subjectId: string;
  workDate: string;
};

/** 开始一次今日计划请求：序号自增，并记录该请求对应的主体与自然日。 */
export function beginTodayPlanRequest(sequence: number, subjectId: string, workDate: string): TodayPlanRequestToken {
  return { sequence: sequence + 1, subjectId, workDate };
}

/**
 * 只有仍是最新序号、且回来的自然日仍是当前工作空间自然日的响应才允许落地。
 * 因此主体切换或跨日之后到达的旧响应会被丢弃，不会覆盖新的权威数据。
 */
export function isCurrentTodayPlanRequest(token: TodayPlanRequestToken, latestSequence: number, currentWorkDate: string): boolean {
  return token.sequence === latestSequence && token.workDate === currentWorkDate;
}

/**
 * 失败时保留上一份权威列表，只更新错误信息；成功且响应仍当前时才替换列表。
 */
export function resolveTodayPlanOutcome<T>(
  previous: { tasks: T[]; subjectId: string },
  result:
    | { ok: true; token: TodayPlanRequestToken; tasks: T[] }
    | { ok: false },
): { tasks: T[]; subjectId: string } {
  if (!result.ok) return previous;
  return { tasks: result.tasks, subjectId: result.token.subjectId };
}
