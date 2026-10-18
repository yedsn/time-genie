export function derivedUnassignedSeconds(
  authoritativeSeconds: number,
  syncedAtMillis: number,
  nowMillis: number,
): number {
  if (!syncedAtMillis) return 0;
  const localElapsed = Math.max(0, Math.floor((nowMillis - syncedAtMillis) / 1_000));
  return Math.max(0, Math.floor(authoritativeSeconds) + localElapsed);
}
