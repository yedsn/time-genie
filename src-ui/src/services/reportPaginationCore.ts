export type ReportListType = "all" | "daily" | "weekly" | "monthly";

export type ReportListFilters = {
  reportType: ReportListType;
  subjectId?: string;
  query?: string;
};

export type ReportTypeCounts = {
  all: number;
  daily: number;
  weekly: number;
  monthly: number;
};

export type ReportPage<T> = {
  reports: T[];
  page: number;
  pageSize: number;
  total: number;
  hasMore: boolean;
  typeCounts: ReportTypeCounts;
};

export type ReportPaginationState<T extends { id: string }> = {
  items: T[];
  filters: ReportListFilters;
  page: number;
  pageSize: number;
  loadedPageDepth: number;
  total: number;
  hasMore: boolean;
  typeCounts: ReportTypeCounts;
  loading: boolean;
  loadingMore: boolean;
  error: string;
  requestSeq: number;
};

export type ReportPageToken = {
  requestSeq: number;
  filterKey: string;
  page: number;
};

const emptyCounts = (): ReportTypeCounts => ({ all: 0, daily: 0, weekly: 0, monthly: 0 });

export function normalizeReportFilters(filters: Partial<ReportListFilters> = {}): ReportListFilters {
  const query = filters.query?.trim();
  return {
    reportType: filters.reportType ?? "all",
    subjectId: filters.subjectId || undefined,
    query: query || undefined,
  };
}

export function reportFilterKey(filters: ReportListFilters) {
  return `${filters.subjectId ?? ""}\u001f${filters.reportType}\u001f${filters.query ?? ""}`;
}

export function createReportPaginationState<T extends { id: string }>(pageSize = 20): ReportPaginationState<T> {
  return {
    items: [],
    filters: normalizeReportFilters(),
    page: 0,
    pageSize,
    loadedPageDepth: 0,
    total: 0,
    hasMore: false,
    typeCounts: emptyCounts(),
    loading: false,
    loadingMore: false,
    error: "",
    requestSeq: 0,
  };
}

export function beginReportReset<T extends { id: string }>(
  state: ReportPaginationState<T>,
  filters: Partial<ReportListFilters>,
): ReportPageToken {
  state.requestSeq += 1;
  state.filters = normalizeReportFilters(filters);
  state.items = [];
  state.page = 0;
  state.loadedPageDepth = 0;
  state.total = 0;
  state.hasMore = false;
  state.typeCounts = emptyCounts();
  state.loading = true;
  state.loadingMore = false;
  state.error = "";
  return { requestSeq: state.requestSeq, filterKey: reportFilterKey(state.filters), page: 1 };
}

export function beginReportRefresh<T extends { id: string }>(state: ReportPaginationState<T>): ReportPageToken {
  state.requestSeq += 1;
  state.loading = true;
  state.loadingMore = false;
  state.error = "";
  return {
    requestSeq: state.requestSeq,
    filterKey: reportFilterKey(state.filters),
    page: Math.max(1, state.loadedPageDepth),
  };
}

export function beginReportLoadMore<T extends { id: string }>(state: ReportPaginationState<T>): ReportPageToken | undefined {
  if (state.loading || state.loadingMore || !state.hasMore) return undefined;
  state.loadingMore = true;
  state.error = "";
  return {
    requestSeq: state.requestSeq,
    filterKey: reportFilterKey(state.filters),
    page: state.page + 1,
  };
}

export function isCurrentReportRequest<T extends { id: string }>(
  state: ReportPaginationState<T>,
  token: ReportPageToken,
) {
  return token.requestSeq === state.requestSeq && token.filterKey === reportFilterKey(state.filters);
}

export function mergeUniqueReports<T extends { id: string }>(current: T[], incoming: T[]) {
  const result = [...current];
  const positions = new Map(result.map((item, index) => [item.id, index]));
  for (const item of incoming) {
    const index = positions.get(item.id);
    if (index === undefined) {
      positions.set(item.id, result.length);
      result.push(item);
    } else {
      result[index] = item;
    }
  }
  return result;
}

export function resolveReportSelection<T extends { id: string }>(items: T[], selectedId: string) {
  if (selectedId && items.some((item) => item.id === selectedId)) return selectedId;
  return items[0]?.id ?? "";
}

export function applyReportPage<T extends { id: string }>(
  state: ReportPaginationState<T>,
  token: ReportPageToken,
  result: ReportPage<T>,
  replace: boolean,
) {
  if (!isCurrentReportRequest(state, token)) return false;
  state.items = replace ? mergeUniqueReports([], result.reports) : mergeUniqueReports(state.items, result.reports);
  state.page = result.page;
  state.loadedPageDepth = replace ? result.page : Math.max(state.loadedPageDepth, result.page);
  state.pageSize = result.pageSize;
  state.total = result.total;
  state.hasMore = result.hasMore;
  state.typeCounts = result.typeCounts;
  state.loading = false;
  state.loadingMore = false;
  state.error = "";
  return true;
}

export function failReportPage<T extends { id: string }>(
  state: ReportPaginationState<T>,
  token: ReportPageToken,
  message: string,
) {
  if (!isCurrentReportRequest(state, token)) return false;
  state.loading = false;
  state.loadingMore = false;
  state.error = message;
  return true;
}
