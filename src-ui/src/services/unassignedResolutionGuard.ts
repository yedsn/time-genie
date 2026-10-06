export function canSubmitUnassignedResolution(
  expectedSessionId: string,
  expectedVersion: number,
  currentSessionId: string,
  currentVersion: number,
) {
  return Boolean(expectedSessionId)
    && expectedSessionId === currentSessionId
    && expectedVersion === currentVersion;
}
