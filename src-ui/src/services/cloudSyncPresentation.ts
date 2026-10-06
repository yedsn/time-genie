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
      label: state.pendingOperations > 0
        ? `${state.conflictCount} 项冲突，${state.pendingOperations} 项待处理`
        : `${state.conflictCount} 项冲突`,
      detail: state.pendingOperations > 0
        ? `本机修改已保存；冲突阻塞了后续 ${state.pendingOperations} 项同步，请先处理冲突`
        : "本机修改已保存，需处理冲突后继续同步",
      needsAttention: true,
      attentionLevel: "critical",
    };
  }

  if (state.pendingOperations > 0) {
    return {
      label: state.online ? `${state.pendingOperations} 项等待同步` : `${state.pendingOperations} 项等待连接`,
      detail: state.online
        ? state.lastError
          ? `本机修改已保存，等待同步到云端；上次同步失败：${state.lastError}`
          : "本机修改已保存，等待同步到云端"
        : state.lastError
          ? `本机修改已保存，等待网络或登录状态恢复后自动同步；上次同步失败：${state.lastError}`
          : "本机修改已保存，等待网络或登录状态恢复后自动同步",
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
    label: "实时同步",
    detail: state.lastSyncedAt
      ? `已实时同步 · 最近更新 ${formatSyncTime(state.lastSyncedAt)}`
      : "实时同步已连接，云端变化会自动更新",
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
