import type { TaskRecord } from "./tauri";

export const TASK_PRIORITIES = [
  { value: "urgent", label: "紧急", className: "priority-urgent" },
  { value: "high", label: "高", className: "priority-high" },
  { value: "normal", label: "普通", className: "priority-normal" },
  { value: "low", label: "低", className: "priority-low" },
] as const satisfies ReadonlyArray<{ value: TaskRecord["priority"]; label: string; className: string }>;

export function taskPriorityMeta(priority?: TaskRecord["priority"]) {
  return TASK_PRIORITIES.find((item) => item.value === priority) ?? TASK_PRIORITIES[2];
}
