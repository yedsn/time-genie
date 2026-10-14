export type TrayIdlePresentation = "timer" | "unassigned" | "idle";

export function createSingleFlight<TArgs extends unknown[], TResult>(
  runner: (...args: TArgs) => Promise<TResult>,
) {
  let active: Promise<TResult> | undefined;
  return (...args: TArgs) => {
    if (!active) {
      active = runner(...args).finally(() => {
        active = undefined;
      });
    }
    return active;
  };
}

export function trayIdlePresentation(input: {
  hasActiveEntry: boolean;
  unassignedSessionId: string;
  unassignedSeconds: number;
}): TrayIdlePresentation {
  if (input.hasActiveEntry) return "timer";
  if (input.unassignedSessionId && input.unassignedSeconds > 0) return "unassigned";
  return "idle";
}
