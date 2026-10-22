import type { CloudSessionView } from "./cloudSessionPresentationTypes";

export function cloudSessionLabel(session?: CloudSessionView) {
  if (!session || session.status === "reauth_required") return "未登录 Supabase";
  if (session.status === "offline_saved") return "离线但保持登录";
  return session.email || session.userId || "已登录";
}

export function cloudSessionNeedsPassword(session?: CloudSessionView) {
  return !session || session.status === "reauth_required";
}

export function cloudSessionErrorNeedsRecovery(error: unknown) {
  const message = error instanceof Error ? error.message : String(error ?? "");
  return /(?:AUTH_REQUIRED|AUTH_INVALID|AUTH_FAILED|AUTH_RETRY_REQUIRED|AUTH_BLOCKED|AUTH_UNVERIFIED|DEVICE_REVOKED|CREDENTIAL_UNAVAILABLE)\s*:/.test(message);
}
