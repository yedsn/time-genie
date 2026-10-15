export type QuickEstimateTask = {
  id: string;
  parentId?: string;
  title: string;
  status: "open" | "done";
  recurrence?: unknown;
  todayEstimate?: number;
  estimate?: number;
  dailyEstimateVersion?: number;
  version: number;
};

export type DurationParseResult =
  | { ok: true; minutes?: number }
  | { ok: false; message: string };

export function parseQuickEstimateMinutes(value: string): DurationParseResult {
  const text = value.trim();
  if (!text) return { ok: true, minutes: undefined };
  if (!/^\d+$/.test(text)) return { ok: false, message: "请输入整数分钟，例如 30、60、90" };
  const minutes = Number(text);
  if (!Number.isFinite(minutes)) return { ok: false, message: "请输入有效的分钟数" };
  if (minutes > 1440) return { ok: false, message: "单项预计不能超过 1440 分钟" };
  return { ok: true, minutes: minutes || undefined };
}

export function adjustQuickEstimateMinutes(minutes: number | undefined, delta: number) {
  const adjusted = Math.min(1440, Math.max(0, (minutes ?? 0) + delta));
  return adjusted || undefined;
}

export function formatQuickEstimateHours(minutes?: number) {
  if (minutes === undefined) return "0 小时";
  const hours = minutes / 60;
  return `${Number.isInteger(hours) ? hours : Number(hours.toFixed(2))} 小时`;
}

export function parseQuickDuration(value: string): DurationParseResult {
  const text = value.trim().toLowerCase();
  if (!text) return { ok: true, minutes: undefined };
  const match = text.match(/^([0-9]+(?:\.[0-9]+)?)(h|m|min)?$/);
  if (!match) return { ok: false, message: "请输入小时或分钟，例如 0.5、1h、30m" };
  const amount = Number(match[1]);
  const unit = match[2];
  const minutes = Math.round(amount * (unit === "m" || unit === "min" ? 1 : 60));
  if (!Number.isFinite(minutes) || minutes <= 0) return { ok: false, message: "预计用时必须大于 0" };
  if (minutes > 1440) return { ok: false, message: "单项预计不能超过 24 小时" };
  return { ok: true, minutes };
}

export function isAllocatableTask<T extends QuickEstimateTask>(task: T, tasks: readonly T[]) {
  if (task.status !== "open") return false;
  return !tasks.some((candidate) => candidate.status === "open" && candidate.parentId === task.id);
}

export function quickEstimateRows<T extends QuickEstimateTask>(tasks: readonly T[]) {
  const active = tasks.filter((task) => task.status === "open");
  return active.map((task) => ({
    task,
    allocatable: isAllocatableTask(task, active),
    suggestion: task.todayEstimate === undefined ? task.estimate : undefined,
    initialMinutes: task.todayEstimate,
  }));
}

export type QuickEstimateDraft = {
  taskId: string;
  originalMinutes?: number;
  minutes?: number;
  text: string;
  expectedTaskVersion: number;
  expectedDailyEstimateVersion?: number;
};

export function changedEstimateDrafts(drafts: readonly QuickEstimateDraft[]) {
  return drafts.filter((draft) => draft.minutes !== draft.originalMinutes);
}

export function allocatedMinutes(drafts: readonly QuickEstimateDraft[]) {
  return drafts.reduce((sum, draft) => sum + (draft.minutes ?? 0), 0);
}

export function capacitySummary(allocated: number, capacity: number) {
  const difference = capacity - allocated;
  return {
    allocatedMinutes: allocated,
    capacityMinutes: capacity,
    remainingMinutes: Math.max(0, difference),
    overMinutes: Math.max(0, -difference),
  };
}

export function scheduleCapacity(segments: readonly { start: string; end: string }[], enabled = true) {
  if (!enabled) return 0;
  return segments.reduce((total, segment) => {
    const start = parseClock(segment.start);
    const end = parseClock(segment.end);
    return total + (start !== undefined && end !== undefined && end > start ? end - start : 0);
  }, 0);
}

export function weekdayIndexForDate(date: string, timezone = "UTC") {
  const instant = new Date(`${date}T12:00:00Z`);
  const short = new Intl.DateTimeFormat("en-US", { timeZone: timezone, weekday: "short" }).format(instant);
  return ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].indexOf(short);
}

function parseClock(value: string) {
  const match = value.match(/^(\d{1,2}):(\d{2})$/);
  if (!match) return undefined;
  const hours = Number(match[1]);
  const minutes = Number(match[2]);
  return hours >= 0 && hours <= 23 && minutes >= 0 && minutes <= 59 ? hours * 60 + minutes : undefined;
}
