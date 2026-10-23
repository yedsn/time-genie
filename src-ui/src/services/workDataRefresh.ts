export type WorkDataRefreshPlan = {
  workspace: boolean;
  time: boolean;
  reports: boolean;
  unassigned: boolean;
  todayOverview: boolean;
};

export type WorkDataRefreshPartition = {
  visible: string[];
  deferred: string[];
};

const sharedPageDomains = new Set(["subjects", "tasks", "time", "unassigned"]);

export function isWorkDataDomainVisible(domain: string, activePage: string, mainView: boolean) {
  if (sharedPageDomains.has(domain)) return true;
  if (!mainView) return false;
  if (domain === "reports") return activePage === "reports";
  if (domain === "settings") return activePage === "settings";
  return true;
}

export function partitionWorkDataRefreshDomains(
  domains: Iterable<string>,
  activePage: string,
  mainView: boolean,
): WorkDataRefreshPartition {
  const visible: string[] = [];
  const deferred: string[] = [];
  for (const domain of new Set(domains)) {
    (isWorkDataDomainVisible(domain, activePage, mainView) ? visible : deferred).push(domain);
  }
  return { visible, deferred };
}

export function workDataRefreshPlan(
  domains: Iterable<string>,
  mainView: boolean,
  activePage = "today",
): WorkDataRefreshPlan {
  const changed = new Set(domains);
  const workspace = changed.has("subjects") || changed.has("tasks");
  const time = changed.has("time");
  return {
    workspace,
    time,
    reports: mainView && activePage === "reports" && changed.has("reports"),
    unassigned: changed.has("unassigned"),
    todayOverview: mainView && activePage === "today" && (workspace || time),
  };
}

export function workDataChangeDomainsForMutation(mutation: "subject" | "task" | "time"): string[] {
  return mutation === "subject" ? ["subjects"] : mutation === "time" ? ["time", "tasks"] : ["tasks"];
}

export function shouldClearDirectCloudMemory(accessState: string, previouslyWritable: boolean) {
  return !previouslyWritable && accessState !== "online_writable";
}
