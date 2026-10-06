export type WorkDataRefreshPlan = {
  workspace: boolean;
  time: boolean;
  reports: boolean;
  unassigned: boolean;
  todayOverview: boolean;
};

export function workDataRefreshPlan(domains: Iterable<string>, mainView: boolean): WorkDataRefreshPlan {
  const changed = new Set(domains);
  const workspace = changed.has("subjects") || changed.has("tasks");
  const time = changed.has("time");
  return {
    workspace,
    time,
    reports: mainView && changed.has("reports"),
    unassigned: changed.has("unassigned"),
    todayOverview: mainView && (workspace || time),
  };
}
