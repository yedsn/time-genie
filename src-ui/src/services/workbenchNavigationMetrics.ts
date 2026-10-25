export type WorkbenchNavigationMetric =
  | "hoverScheduled"
  | "hoverCommitted"
  | "hoverCancelled"
  | "pageMounted"
  | "pageActivated"
  | "pageDeactivated"
  | "modulePreloaded"
  | "domainQueryStarted";

export type WorkbenchNavigationMetricsSnapshot = Record<WorkbenchNavigationMetric, number> & {
  pages: Record<string, number>;
  domains: Record<string, number>;
};

const counters: Record<WorkbenchNavigationMetric, number> = {
  hoverScheduled: 0,
  hoverCommitted: 0,
  hoverCancelled: 0,
  pageMounted: 0,
  pageActivated: 0,
  pageDeactivated: 0,
  modulePreloaded: 0,
  domainQueryStarted: 0,
};
const pages = new Map<string, number>();
const domains = new Map<string, number>();

export function recordWorkbenchNavigationMetric(metric: WorkbenchNavigationMetric, key = "") {
  counters[metric] += 1;
  const target = metric === "domainQueryStarted" ? domains : metric.startsWith("page") ? pages : undefined;
  if (target && key) {
    const metricKey = target === pages ? `${metric}:${key}` : key;
    target.set(metricKey, (target.get(metricKey) ?? 0) + 1);
  }
}

export function workbenchNavigationMetricsSnapshot(): WorkbenchNavigationMetricsSnapshot {
  return {
    ...counters,
    pages: Object.fromEntries(pages),
    domains: Object.fromEntries(domains),
  };
}

export function resetWorkbenchNavigationMetrics() {
  for (const metric of Object.keys(counters) as WorkbenchNavigationMetric[]) counters[metric] = 0;
  pages.clear();
  domains.clear();
}

if (typeof window !== "undefined") {
  Object.assign(window, {
    __TIMEGENIE_WORKBENCH_METRICS__: workbenchNavigationMetricsSnapshot,
    __TIMEGENIE_RESET_WORKBENCH_METRICS__: resetWorkbenchNavigationMetrics,
  });
}
