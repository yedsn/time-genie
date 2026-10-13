export type HistoryDateGroup<T> = {
  workDate: string;
  entries: T[];
};

export function isTodayTimeEntry(entry: { workDate: string }, workspaceToday: string) {
  return Boolean(workspaceToday) && entry.workDate === workspaceToday;
}

export function findEntryInHistory<T extends { id: string }>(groups: HistoryDateGroup<T>[], entryId: string) {
  if (!entryId) return undefined;
  for (const group of groups) {
    const entry = group.entries.find((item) => item.id === entryId);
    if (entry) return entry;
  }
  return undefined;
}

export function clampHistoryPage(page: number, totalPages: number) {
  return Math.max(1, Math.min(Math.round(page), Math.max(1, totalPages)));
}

export function historyPaginationState(page: number, totalDays: number, pageSize = 7) {
  const totalPages = totalDays > 0 ? Math.ceil(totalDays / pageSize) : 0;
  const currentPage = totalPages ? clampHistoryPage(page, totalPages) : 1;
  return {
    currentPage,
    totalPages,
    canGoPrevious: currentPage > 1,
    canGoNext: currentPage < totalPages,
  };
}

export function collectTodayEntryTaskIds(entries: Array<{
  workDate: string;
  kind?: "work" | "break";
  defaultTask?: string;
  allocations?: Array<{ taskId: string; minutes: number }>;
}>, workspaceToday: string) {
  const taskIds = new Set<string>();
  for (const entry of entries) {
    if (!isTodayTimeEntry(entry, workspaceToday) || entry.kind === "break") continue;
    const allocations = (entry.allocations ?? []).filter((allocation) => allocation.minutes > 0);
    if (!allocations.length && entry.defaultTask) taskIds.add(entry.defaultTask);
    for (const allocation of allocations) taskIds.add(allocation.taskId);
  }
  return taskIds;
}

export function shouldRefreshTimeHistory(activePage: string, timeDomainChanged: boolean) {
  return activePage === "timer" && timeDomainChanged;
}

export function timerStopHistoryTarget(entryId: string) {
  return {
    activePage: "timer",
    historyPage: 1,
    selectedEntryId: entryId,
  } as const;
}

export function historyDateLabel(workDate: string, workspaceToday: string) {
  const explicitDate = formatDate(workDate);
  if (workDate === workspaceToday) return `今天 · ${explicitDate}`;
  if (workDate === previousDate(workspaceToday)) return `昨天 · ${explicitDate}`;
  return `${explicitDate} · ${weekdayLabel(workDate)}`;
}

function previousDate(dateText: string) {
  const date = parseDate(dateText);
  if (!date) return "";
  date.setUTCDate(date.getUTCDate() - 1);
  return date.toISOString().slice(0, 10);
}

function formatDate(dateText: string) {
  const date = parseDate(dateText);
  if (!date) return dateText;
  return `${date.getUTCFullYear()}年${date.getUTCMonth() + 1}月${date.getUTCDate()}日`;
}

function weekdayLabel(dateText: string) {
  const date = parseDate(dateText);
  if (!date) return "";
  return ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"][date.getUTCDay()];
}

function parseDate(dateText: string) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(dateText)) return undefined;
  const date = new Date(`${dateText}T12:00:00Z`);
  return Number.isNaN(date.getTime()) ? undefined : date;
}
