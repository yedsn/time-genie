export type TaskSelectionSubject = {
  id: string;
};

export type TaskSelectionTask = {
  id: string;
  subjectId: string;
  parentId?: string;
};

export type TaskSelectionRow<T extends TaskSelectionTask> = {
  task: T;
  depth: number;
};

export function resolveTaskSelectionSubject(
  subjects: TaskSelectionSubject[],
  selectedTask: TaskSelectionTask | undefined,
  workspaceSubjectId: string,
) {
  if (selectedTask && subjects.some((subject) => subject.id === selectedTask.subjectId)) {
    return selectedTask.subjectId;
  }
  if (subjects.some((subject) => subject.id === workspaceSubjectId)) return workspaceSubjectId;
  return subjects[0]?.id ?? "";
}

export function taskSelectionRows<T extends TaskSelectionTask>(tasks: T[], subjectId: string): TaskSelectionRow<T>[] {
  const subjectTasks = tasks.filter((task) => task.subjectId === subjectId);
  const taskIds = new Set(subjectTasks.map((task) => task.id));
  const childMap = new Map<string, T[]>();
  const roots: T[] = [];

  for (const task of subjectTasks) {
    if (task.parentId && taskIds.has(task.parentId)) {
      const children = childMap.get(task.parentId) ?? [];
      children.push(task);
      childMap.set(task.parentId, children);
    } else {
      roots.push(task);
    }
  }

  const rows: TaskSelectionRow<T>[] = [];
  const append = (task: T, depth: number) => {
    rows.push({ task, depth });
    for (const child of childMap.get(task.id) ?? []) append(child, depth + 1);
  };
  for (const root of roots) append(root, 0);
  return rows;
}
