export const TRANSIENT_MISSING_STATE_REFRESH_LIMIT = 4;

export type TransientMissingStateResult = {
  missingRefreshes: number;
  preservePrevious: boolean;
};

export function transientMissingStateResult(
  hasPreviousState: boolean,
  hasCurrentState: boolean,
  missingRefreshes: number,
  limit = TRANSIENT_MISSING_STATE_REFRESH_LIMIT,
): TransientMissingStateResult {
  if (!hasPreviousState || hasCurrentState) {
    return { missingRefreshes: 0, preservePrevious: false };
  }
  const nextMissingRefreshes = missingRefreshes + 1;
  return {
    missingRefreshes: nextMissingRefreshes,
    preservePrevious: nextMissingRefreshes < limit,
  };
}
