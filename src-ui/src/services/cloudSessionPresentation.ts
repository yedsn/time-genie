import type { CloudSessionView } from "./cloudSessionPresentationTypes";

export function cloudSessionLabel(session?: CloudSessionView) {
  if (!session || session.status === "reauth_required") return "未登录 Supabase";
  if (session.status === "offline_saved") return "离线但保持登录";
  return session.email || session.userId || "已登录";
}

export function cloudSessionNeedsPassword(session?: CloudSessionView) {
  return !session || session.status === "reauth_required";
}
