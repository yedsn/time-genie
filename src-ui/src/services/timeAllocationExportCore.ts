export type TimeAllocationExportDraft = {
  startDate: string;
  endDate: string;
  includeSpecialTime: boolean;
};

export function currentMonthRange(workspaceToday: string): Pick<TimeAllocationExportDraft, "startDate" | "endDate"> {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(workspaceToday);
  if (!match) return { startDate: "", endDate: "" };
  const year = Number(match[1]);
  const month = Number(match[2]);
  const lastDay = new Date(Date.UTC(year, month, 0)).getUTCDate();
  return {
    startDate: `${match[1]}-${match[2]}-01`,
    endDate: `${match[1]}-${match[2]}-${String(lastDay).padStart(2, "0")}`,
  };
}

export function defaultExportDraft(workspaceToday: string): TimeAllocationExportDraft {
  return {
    ...currentMonthRange(workspaceToday),
    includeSpecialTime: false,
  };
}

export function exportRangeError(startDate: string, endDate: string): string {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(startDate) || !/^\d{4}-\d{2}-\d{2}$/.test(endDate)) {
    return "请选择完整的开始和结束日期";
  }
  return startDate > endDate ? "开始日期不能晚于结束日期" : "";
}

export function ensureXlsxExtension(path: string): string {
  return path.toLowerCase().endsWith(".xlsx") ? path : `${path}.xlsx`;
}

export class LatestRequestGuard {
  private value = 0;

  next(): number {
    this.value += 1;
    return this.value;
  }

  isCurrent(requestId: number): boolean {
    return requestId === this.value;
  }

  invalidate(): void {
    this.value += 1;
  }
}
