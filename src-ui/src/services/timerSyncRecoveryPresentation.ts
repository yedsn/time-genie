export type TimerSyncRecoveryItem = {
  recoveryKind?: string;
  recoveryContext?: Record<string, unknown> | null;
};

function integer(value: unknown) {
  return typeof value === "number" && Number.isInteger(value) ? value : undefined;
}

export function timerSyncRecoveryLabel(item: TimerSyncRecoveryItem) {
  const labels: Record<string, string> = {
    historical_candidate: "等待自动恢复",
    rebased: "已自动压缩，等待保存",
    competing_edit: "需要处理的设备冲突",
  };
  return item.recoveryKind ? labels[item.recoveryKind] ?? item.recoveryKind : "";
}

export function timerSyncRecoveryDetail(item: TimerSyncRecoveryItem) {
  const context = item.recoveryContext;
  if (!context) return "";
  const message = typeof context.message === "string" ? context.message : "";
  const fromBaseVersion = integer(context.fromBaseVersion);
  const cloudVersion = integer(context.cloudVersion);
  const localFinalVersion = integer(context.localFinalVersion);
  const versions = [
    cloudVersion !== undefined ? `云端版本 ${cloudVersion}` : "",
    fromBaseVersion !== undefined && localFinalVersion !== undefined
      ? `原本地跨度 ${fromBaseVersion} → ${localFinalVersion}`
      : "",
  ].filter(Boolean).join(" · ");
  return [message, versions].filter(Boolean).join(" · ");
}
