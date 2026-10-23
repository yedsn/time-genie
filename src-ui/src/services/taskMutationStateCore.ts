export type TaskIdentity = { id: string; parentId?: string; version: number };

export type ApplyCreatedTaskOptions<T extends TaskIdentity> = {
  tasks: T[];
  draftId: string;
  authoritative: T;
  selectedTaskId: string;
};

export function applyCreatedTask<T extends TaskIdentity>(options: ApplyCreatedTaskOptions<T>) {
  const { tasks, draftId, authoritative } = options;
  let draftIndex = tasks.findIndex((task) => task.id === draftId);
  const duplicateIndex = tasks.findIndex((task) => task.id === authoritative.id);
  if (duplicateIndex >= 0 && duplicateIndex !== draftIndex) {
    tasks.splice(duplicateIndex, 1);
    if (duplicateIndex < draftIndex) draftIndex -= 1;
  }
  if (draftIndex >= 0) tasks.splice(draftIndex, 1, authoritative);
  else tasks.push(authoritative);
  for (const task of tasks) if (task.parentId === draftId) task.parentId = authoritative.id;
  return options.selectedTaskId === draftId ? authoritative.id : options.selectedTaskId;
}

export function upsertAuthoritativeTask<T extends TaskIdentity>(tasks: T[], authoritative: T) {
  const index = tasks.findIndex((task) => task.id === authoritative.id);
  if (index >= 0) tasks.splice(index, 1, authoritative);
  else tasks.push(authoritative);
  return authoritative;
}

export function collectTaskSubtreeIds<T extends TaskIdentity>(tasks: readonly T[], rootId: string) {
  const ids = new Set([rootId]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const task of tasks) {
      if (task.parentId && ids.has(task.parentId) && !ids.has(task.id)) {
        ids.add(task.id);
        changed = true;
      }
    }
  }
  return ids;
}

export function removeTaskSubtree<T extends TaskIdentity>(tasks: T[], rootId: string) {
  const ids = collectTaskSubtreeIds(tasks, rootId);
  for (let index = tasks.length - 1; index >= 0; index -= 1) {
    if (ids.has(tasks[index].id)) tasks.splice(index, 1);
  }
  return ids;
}

export const taskMutationDomains = {
  create: { covered: ["tasks"], stale: ["todayPlan", "todayOverview"] },
  update: { covered: ["tasks"], stale: ["todayPlan", "todayOverview"] },
  delete: { covered: ["tasks"], stale: ["todayPlan", "completedTasks", "todayOverview"] },
  move: { covered: ["tasks"], stale: ["todayPlan"] },
  duplicate: { covered: ["tasks"], stale: ["todayPlan", "todayOverview"] },
  completion: { covered: ["tasks"], stale: ["todayPlan", "completedTasks", "todayOverview"] },
} as const;
