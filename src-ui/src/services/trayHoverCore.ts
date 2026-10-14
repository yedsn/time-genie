export type TrayTaskCandidate = {
  id: string;
  parentId?: string;
  selectable: boolean;
};

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

export function reconcileTrayTaskSelection(
  selectedTaskId: string,
  tasks: TrayTaskCandidate[],
): string {
  if (!selectedTaskId) return "";
  const selected = tasks.find((task) => task.id === selectedTaskId);
  if (!selected?.selectable) return "";
  if (tasks.some((task) => task.parentId === selectedTaskId)) return "";
  return selectedTaskId;
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
