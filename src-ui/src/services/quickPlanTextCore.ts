export type QuickPlanTask = {
  id: string;
  title: string;
  parentId?: string;
  version: number;
  directMinutes?: number;
  recurrence?: unknown;
};

export type QuickPlanBaselineTask = {
  taskId: string;
  expectedVersion: number;
  title: string;
  parentTaskId?: string;
  order: number;
  directMinutes: number;
  recurring: boolean;
};

export type QuickPlanDraftLine = {
  lineKey: string;
  taskId?: string;
  expectedVersion?: number;
  text: string;
  depth: 0 | 1;
};

export type QuickPlanLineError = {
  lineIndex: number;
  message: string;
};

export type QuickPlanFinalItem = {
  lineKey: string;
  taskId?: string;
  title: string;
  parentLineKey?: string;
  depth: 0 | 1;
};

export type QuickPlanChangeItem = {
  lineKey: string;
  taskId?: string;
  title: string;
  depth: 0 | 1;
};

export type QuickPlanChangeSummary = {
  finalItems: QuickPlanFinalItem[];
  added: QuickPlanChangeItem[];
  renamed: QuickPlanChangeItem[];
  moved: QuickPlanChangeItem[];
  deleted: QuickPlanChangeItem[];
  errors: QuickPlanLineError[];
  dirty: boolean;
};

export type QuickPlanBatchRequest = {
  subjectId: string;
  baselineTasks: Array<{ taskId: string; expectedVersion: number }>;
  finalItems: Array<{ lineKey: string; taskId?: string; title: string; parentLineKey?: string }>;
  confirmedDeleteTaskIds: string[];
  todayPlanChange?: {
    workDate: string;
    includeLineKeys: string[];
    excludeItems: Array<{ taskId: string; expectedVersion?: number }>;
  };
};

export function createQuickPlanBaseline(tasks: readonly QuickPlanTask[]): QuickPlanBaselineTask[] {
  return tasks
    .filter((task) => task.version > 0)
    .map((task, order) => ({
      taskId: task.id,
      expectedVersion: task.version,
      title: task.title,
      parentTaskId: task.parentId,
      order,
      directMinutes: task.directMinutes ?? 0,
      recurring: Boolean(task.recurrence),
    }));
}

export function createQuickPlanDraft(tasks: readonly QuickPlanTask[]): QuickPlanDraftLine[] {
  return tasks.map((task) => ({
    lineKey: `task:${task.id}`,
    taskId: task.version > 0 ? task.id : undefined,
    expectedVersion: task.version > 0 ? task.version : undefined,
    text: task.title,
    depth: task.parentId ? 1 : 0,
  }));
}

function indentation(raw: string): { depth: 0 | 1; title: string } | { error: string } {
  const prefix = raw.match(/^[\t ]*/)?.[0] ?? "";
  const depth = !prefix ? 0 as const : prefix === "\t" || prefix === "  " || prefix === "    " ? 1 as const : undefined;
  if (depth === undefined) return { error: "缩进只能为空、一个 Tab、两个空格或四个空格" };
  const content = raw.slice(prefix.length);
  if (!content.startsWith("- ")) return { error: "每一行事项必须以 '- ' 开头" };
  return { depth, title: content.slice(2).trim() };
}

export function parseQuickPlanText(text: string, keyPrefix = "new"): { lines: QuickPlanDraftLine[]; errors: QuickPlanLineError[] } {
  const lines: QuickPlanDraftLine[] = [];
  const errors: QuickPlanLineError[] = [];
  let rootSeen = false;
  text.replace(/\r\n?/g, "\n").split("\n").forEach((raw, lineIndex) => {
    if (!raw.trim() || raw.trim() === "-") return;
    const parsed = indentation(raw);
    if ("error" in parsed) {
      errors.push({ lineIndex, message: parsed.error });
      return;
    }
    if (parsed.depth === 1 && !rootSeen) {
      errors.push({ lineIndex, message: "二级事项前需要一个一级事项" });
    }
    if (parsed.depth === 0) rootSeen = true;
    lines.push({ lineKey: `${keyPrefix}:${lineIndex}`, text: parsed.title, depth: parsed.depth });
  });
  return { lines, errors };
}

export function serializeQuickPlanLines(lines: readonly QuickPlanDraftLine[]) {
  return lines.map((line) => `${line.depth ? "  " : ""}- ${line.text}`).join("\n");
}

export function reconcileQuickPlanText(
  previous: readonly QuickPlanDraftLine[],
  text: string,
  keyPrefix = "new",
): { lines: QuickPlanDraftLine[]; errors: QuickPlanLineError[] } {
  const parsed = parseQuickPlanText(text, keyPrefix);
  if (parsed.errors.length) return parsed;
  const next = parsed.lines.map((line) => ({ ...line }));
  const unused = new Set(previous.map((_, index) => index));

  for (const line of next) {
    const matchIndex = [...unused].find((index) => previous[index].depth === line.depth
      && previous[index].text === line.text) ?? -1;
    if (matchIndex < 0) continue;
    const source = previous[matchIndex];
    line.lineKey = source.lineKey;
    line.taskId = source.taskId;
    line.expectedVersion = source.expectedVersion;
    unused.delete(matchIndex);
  }
  return { lines: next, errors: [] };
}

export function removeQuickPlanSubtree(lines: readonly QuickPlanDraftLine[], index: number) {
  const count = lines[index]?.depth === 0
    ? 1 + lines.slice(index + 1).findIndex((line) => line.depth === 0)
    : 1;
  const normalizedCount = count <= 0 ? lines.length - index : count;
  return {
    lines: [...lines.slice(0, index), ...lines.slice(index + normalizedCount)],
    removed: lines.slice(index, index + normalizedCount),
  };
}

export function analyzeQuickPlanDraft(
  baseline: readonly QuickPlanBaselineTask[],
  draft: readonly QuickPlanDraftLine[],
): QuickPlanChangeSummary {
  const errors: QuickPlanLineError[] = [];
  const finalItems: QuickPlanFinalItem[] = [];
  const lineKeys = new Set<string>();
  const taskIds = new Set<string>();
  let parentLineKey: string | undefined;

  draft.forEach((line, lineIndex) => {
    const title = line.text.trim();
    if (!title) return;
    if (lineKeys.has(line.lineKey)) errors.push({ lineIndex, message: "事项行身份重复" });
    lineKeys.add(line.lineKey);
    if (line.taskId) {
      if (taskIds.has(line.taskId)) errors.push({ lineIndex, message: "同一事项不能出现两次" });
      taskIds.add(line.taskId);
    }
    if (line.depth === 1 && !parentLineKey) errors.push({ lineIndex, message: "二级事项前需要一个一级事项" });
    if (line.depth === 0) parentLineKey = line.lineKey;
    finalItems.push({
      lineKey: line.lineKey,
      taskId: line.taskId,
      title,
      parentLineKey: line.depth ? parentLineKey : undefined,
      depth: line.depth,
    });
  });

  const baselineById = new Map(baseline.map((task) => [task.taskId, task]));
  const finalTaskIdByLine = new Map(finalItems.filter((item) => item.taskId).map((item) => [item.lineKey, item.taskId!]));
  const finalByTaskId = new Map(finalItems.filter((item) => item.taskId).map((item) => [item.taskId!, item]));
  const finalOrderByParent = new Map<string, string[]>();
  for (const item of finalItems) {
    const parentTaskId = item.parentLineKey ? finalTaskIdByLine.get(item.parentLineKey) ?? item.parentLineKey : "root";
    const siblings = finalOrderByParent.get(parentTaskId) ?? [];
    if (item.taskId) siblings.push(item.taskId);
    finalOrderByParent.set(parentTaskId, siblings);
  }
  const baselineOrderByParent = new Map<string, string[]>();
  for (const task of baseline) {
    const key = task.parentTaskId ?? "root";
    const siblings = baselineOrderByParent.get(key) ?? [];
    siblings.push(task.taskId);
    baselineOrderByParent.set(key, siblings);
  }

  const added = finalItems.filter((item) => !item.taskId).map(toChangeItem);
  const renamed: QuickPlanChangeItem[] = [];
  const moved: QuickPlanChangeItem[] = [];
  for (const item of finalItems) {
    if (!item.taskId) continue;
    const source = baselineById.get(item.taskId);
    if (!source) {
      errors.push({ lineIndex: draft.findIndex((line) => line.lineKey === item.lineKey), message: "事项不在打开编辑器时的计划中" });
      continue;
    }
    if (source.title.trim() !== item.title) renamed.push(toChangeItem(item));
    const nextParent = item.parentLineKey ? finalTaskIdByLine.get(item.parentLineKey) ?? item.parentLineKey : undefined;
    const sourceSiblings = baselineOrderByParent.get(source.parentTaskId ?? "root") ?? [];
    const nextSiblings = finalOrderByParent.get(nextParent ?? "root") ?? [];
    if (source.parentTaskId !== nextParent || sourceSiblings.indexOf(item.taskId) !== nextSiblings.indexOf(item.taskId)) {
      moved.push(toChangeItem(item));
    }
  }
  const deleted = baseline
    .filter((task) => !finalByTaskId.has(task.taskId))
    .map((task) => ({ lineKey: `task:${task.taskId}`, taskId: task.taskId, title: task.title, depth: task.parentTaskId ? 1 as const : 0 as const }));

  const childrenByParent = new Map<string, QuickPlanFinalItem[]>();
  for (const item of finalItems) {
    if (!item.parentLineKey) continue;
    const children = childrenByParent.get(item.parentLineKey) ?? [];
    children.push(item);
    childrenByParent.set(item.parentLineKey, children);
  }
  for (const [parentKey, children] of childrenByParent) {
    if (!children.length) continue;
    const parent = finalItems.find((item) => item.lineKey === parentKey);
    const source = parent?.taskId ? baselineById.get(parent.taskId) : undefined;
    if (source?.recurring) errors.push({ lineIndex: draft.findIndex((line) => line.lineKey === parentKey), message: "重复事项不能成为父事项" });
    if ((source?.directMinutes ?? 0) > 0) errors.push({ lineIndex: draft.findIndex((line) => line.lineKey === parentKey), message: "有直接工时的事项不能成为父事项" });
  }

  return {
    finalItems, added, renamed, moved, deleted, errors,
    dirty: Boolean(added.length || renamed.length || moved.length || deleted.length),
  };
}

function toChangeItem(item: QuickPlanFinalItem): QuickPlanChangeItem {
  return { lineKey: item.lineKey, taskId: item.taskId, title: item.title, depth: item.depth };
}

export function buildQuickPlanBatchRequest(
  subjectId: string,
  baseline: readonly QuickPlanBaselineTask[],
  summary: QuickPlanChangeSummary,
): QuickPlanBatchRequest {
  return {
    subjectId,
    baselineTasks: baseline.map((task) => ({ taskId: task.taskId, expectedVersion: task.expectedVersion })),
    finalItems: summary.finalItems.map(({ lineKey, taskId, title, parentLineKey }) => ({ lineKey, taskId, title, parentLineKey })),
    confirmedDeleteTaskIds: summary.deleted.flatMap((item) => item.taskId ? [item.taskId] : []),
  };
}

export function mergeScopedQuickPlanDraft(
  fullDraft: readonly QuickPlanDraftLine[],
  fullBaseline: readonly QuickPlanBaselineTask[],
  scopedBaseline: readonly QuickPlanBaselineTask[],
  scopedSummary: QuickPlanChangeSummary,
) {
  const finalScopedIds = new Set(scopedSummary.finalItems.flatMap((item) => item.taskId ? [item.taskId] : []));
  const finalScopedRootIds = new Set(scopedSummary.finalItems.filter((item) => item.taskId && item.depth === 0).map((item) => item.taskId!));
  const fullById = new Map(fullBaseline.map((task) => [task.taskId, task]));
  const attachedExtras = new Map<string, QuickPlanDraftLine[]>();
  for (const line of fullDraft) {
    if (!line.taskId || finalScopedIds.has(line.taskId)) continue;
    const parentId = fullById.get(line.taskId)?.parentTaskId;
    if (!parentId || !finalScopedRootIds.has(parentId)) continue;
    const extras = attachedExtras.get(parentId) ?? [];
    extras.push({ ...line, depth: 1 });
    attachedExtras.set(parentId, extras);
  }
  const movedIds = new Set([
    ...finalScopedIds,
    ...[...attachedExtras.values()].flatMap((lines) => lines.flatMap((line) => line.taskId ? [line.taskId] : [])),
  ]);
  const anchorIndexes = fullDraft.flatMap((line, index) => line.taskId && movedIds.has(line.taskId) ? [index] : []);
  const insertAt = anchorIndexes.length ? Math.min(...anchorIndexes) : fullDraft.length;
  const remaining = fullDraft.filter((line) => !line.taskId || !movedIds.has(line.taskId));
  const normalized = scopedSummary.finalItems.map((item) => ({
    lineKey: item.lineKey,
    taskId: item.taskId,
    expectedVersion: item.taskId ? scopedBaseline.find((task) => task.taskId === item.taskId)?.expectedVersion : undefined,
    text: item.title,
    depth: item.depth,
  }));
  const scopedBlock: QuickPlanDraftLine[] = [];
  let currentRootTaskId: string | undefined;
  for (let index = 0; index < normalized.length; index += 1) {
    const line = normalized[index];
    scopedBlock.push(line);
    if (line.depth === 0) currentRootTaskId = line.taskId;
    const next = normalized[index + 1];
    if ((!next || next.depth === 0) && currentRootTaskId) scopedBlock.push(...(attachedExtras.get(currentRootTaskId) ?? []));
  }
  const boundedInsertAt = Math.min(insertAt, remaining.length);
  return [...remaining.slice(0, boundedInsertAt), ...scopedBlock, ...remaining.slice(boundedInsertAt)];
}
