export type CloudSyncPresentationState = {
  mode: "local" | "cloud" | string;
  online: boolean;
  syncState: "synced" | "pending" | "conflict" | "error" | string;
  pendingOperations: number;
  conflictCount: number;
  lastSyncedAt?: number;
  lastError?: string;
};

export type CloudSyncAttentionLevel = "" | "critical" | "pending" | "offline";

export type CloudSyncPresentation = {
  label: string;
  detail: string;
  needsAttention: boolean;
  attentionLevel: CloudSyncAttentionLevel;
};

export type CloudSyncPushResult = {
  pushed: number;
  pending: number;
  conflicts: number;
};

export type CloudSyncRetryFeedback = {
  level: "success" | "warning";
  message: string;
};

export function cloudSyncPresentation(
  state: CloudSyncPresentationState | undefined,
  formatSyncTime: (value: number) => string,
): CloudSyncPresentation {
  if (!state || state.mode !== "cloud") {
    return {
      label: "本地模式",
      detail: "",
      needsAttention: false,
      attentionLevel: "",
    };
  }

  if (state.conflictCount > 0) {
    return {
      label: `${state.conflictCount} 项冲突`,
      detail: "本机修改已保存，需处理冲突后继续同步",
      needsAttention: true,
      attentionLevel: "critical",
    };
  }

  if (state.pendingOperations > 0) {
    return {
      label: `${state.pendingOperations} 项等待同步`,
      detail: state.lastError
        ? `本机修改已保存，等待同步到云端；上次同步失败：${state.lastError}`
        : "本机修改已保存，等待同步到云端",
      needsAttention: true,
      attentionLevel: "pending",
    };
  }

  if (state.lastError || state.syncState === "error") {
    return {
      label: "同步失败",
      detail: state.lastError ? `同步失败：${state.lastError}` : "同步失败，请刷新状态后重试",
      needsAttention: true,
      attentionLevel: "critical",
    };
  }

  if (!state.online) {
    return {
      label: "云端离线",
      detail: "本机缓存可继续使用，恢复登录或网络后再同步",
      needsAttention: true,
      attentionLevel: "offline",
    };
  }

  return {
    label: "已同步",
    detail: state.lastSyncedAt
      ? `最近同步 ${formatSyncTime(state.lastSyncedAt)}`
      : "本机缓存已准备好，尚无云端同步记录",
    needsAttention: false,
    attentionLevel: "",
  };
}

export function cloudSyncRetryFeedback(
  result: CloudSyncPushResult,
  state: CloudSyncPresentationState | undefined,
): CloudSyncRetryFeedback {
  if (result.conflicts > 0 || (state?.conflictCount ?? 0) > 0) {
    return {
      level: "warning",
      message: "同步遇到版本冲突，请选择要保留的版本",
    };
  }

  const pending = Math.max(result.pending, state?.pendingOperations ?? 0);
  if (pending > 0) {
    return {
      level: "warning",
      message: `本机修改已保存，仍有 ${pending} 项等待同步`,
    };
  }

  return {
    level: "success",
    message: result.pushed > 0 ? `已同步 ${result.pushed} 项修改` : "当前没有待同步修改",
  };
}
