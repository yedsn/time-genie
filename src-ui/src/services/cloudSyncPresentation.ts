export type CloudSyncPresentationState = {
  mode: "local" | "cloud" | string;
  storageProtocol?: "direct_v2" | string;
  cloudAccessState?: string;
  cloudWritable?: boolean;
  readOnlyReason?: string;
  online: boolean;
  syncState?: string;
  syncing?: boolean;
  saving?: boolean;
  pendingOperations?: number;
  conflictCount?: number;
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

export type DirectCloudMutationFeedback = {
  level: "warning" | "error";
  message: string;
  refreshScope: "none" | "business" | "reset";
};

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error ?? "");
}

export function directCloudMutationFeedback(error: unknown): DirectCloudMutationFeedback {
  const message = errorMessage(error);
  if (/(?:VERSION_CONFLICT|ENTITY_DELETED)\s*:/.test(message)) {
    return {
      level: "warning",
      message: "数据已在其他设备变化，正在重新查询最新云端数据",
      refreshScope: "business",
    };
  }
  if (/WORKSPACE_EPOCH_STALEs*:/.test(message)) {
    return {
      level: "warning",
      message: "云端工作空间已重置，正在清除旧页面数据并重新加载",
      refreshScope: "reset",
    };
  }
  if (/(?:DEVICE_REVOKED|AUTH_REQUIRED|AUTH_INVALID|AUTH_FAILED|AUTH_RETRY_REQUIRED|AUTH_BLOCKED|AUTH_UNVERIFIED|CREDENTIAL_UNAVAILABLE)\s*:/.test(message)) {
    return {
      level: "error",
      message: "登录已失效，请重新登录 Supabase",
      refreshScope: "none",
    };
  }
  if (/(?:CLOUD_READ_ONLY|CLOUD_OFFLINE|NETWORK_UNAVAILABLE|REQUEST_TIMEOUT|SERVICE_UNAVAILABLE)\s*:/.test(message)) {
    return {
      level: "warning",
      message: "云端离线，只能查看已加载数据；不会保存到本机等待同步",
      refreshScope: "none",
    };
  }
  return {
    level: "error",
    message: message.replace(/^[A-Z][A-Z_]*:\s*/, "") || "云端保存失败，请重试查询最新数据",
    refreshScope: "none",
  };
}

export function cloudSaveStatusLabel(state: CloudSyncPresentationState | undefined): string {
  if (!state || state.mode !== "cloud") return "仅保存在本机";
  if (state.cloudAccessState === "connecting") return "正在连接云端";
  if (state.cloudAccessState === "online_writable" && state.cloudWritable) return "已直接保存到 Supabase";
  if (state.cloudAccessState === "reauth_required") return "需要重新登录";
  return "云端只读";
}

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

  const accessState = state.cloudAccessState ?? (state.online ? "connecting" : "offline_readonly");
  if (accessState === "online_writable" && state.cloudWritable) {
    return {
      label: "云端在线",
      detail: state.lastSyncedAt
        ? `已直接读取 Supabase · 最近刷新 ${formatSyncTime(state.lastSyncedAt)}`
        : "已直接连接 Supabase，修改会立即提交到云端",
      needsAttention: false,
      attentionLevel: "",
    };
  }
  if (accessState === "connecting") {
    return {
      label: "正在连接云端",
      detail: state.readOnlyReason || "正在验证会话并加载 Supabase 数据",
      needsAttention: false,
      attentionLevel: "",
    };
  }
  if (accessState === "reauth_required") {
    return {
      label: "需要重新登录",
      detail: state.readOnlyReason || "登录已失效，请重新登录 Supabase",
      needsAttention: true,
      attentionLevel: "critical",
    };
  }
  return {
    label: "云端只读",
    detail: state.readOnlyReason || "云端离线，只能查看已加载数据；不会保存到本机等待同步",
    needsAttention: true,
    attentionLevel: "offline",
  };
}
