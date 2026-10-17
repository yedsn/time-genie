const EXECUTION_PANEL_SUBJECT_KEY = "timegenie:execution-panel-subject";

/**
 * 共享执行事项面板最近使用的主体，按设备保存，
 * 让连续处理同一主体的工作时不必每次重新切换。
 */
export function readRememberedPanelSubjectId(): string {
  try {
    return window.localStorage.getItem(EXECUTION_PANEL_SUBJECT_KEY) ?? "";
  } catch {
    return "";
  }
}

export function rememberPanelSubjectId(subjectId: string) {
  if (!subjectId) return;
  try {
    window.localStorage.setItem(EXECUTION_PANEL_SUBJECT_KEY, subjectId);
  } catch {
    // 存储不可用时只是不记住上次主体，不影响面板本身的切换与执行。
  }
}

