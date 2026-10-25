import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const modulePath = path.join(root, "src-tauri/src/direct_cloud_realtime.rs");
const cloudSyncPath = path.join(root, "src-tauri/src/cloud_sync.rs");
const frontendAppPath = path.join(root, "src-ui/src/App.vue");
const frontendApiPath = path.join(root, "src-ui/src/services/tauri.ts");
const source = fs.readFileSync(modulePath, "utf8");
const cloudSync = fs.readFileSync(cloudSyncPath, "utf8");
const frontendApp = fs.readFileSync(frontendAppPath, "utf8");
const frontendApi = fs.readFileSync(frontendApiPath, "utf8");

for (const expected of [
  "WorkspaceInvalidation", "realtime_workspace_invalidation", "realtime_refresh_domains",
  "realtime_change_seq", "realtime_url", "realtime_reconnect_delay",
  "REALTIME_CONNECT_TIMEOUT_SECONDS", "REALTIME_JOIN_TIMEOUT_SECONDS",
  "pending_heartbeat_ref", "Supabase Realtime 心跳响应超时",
]) {
  if (!source.includes(expected)) throw new Error(`直连 Realtime 模块缺少：${expected}`);
}
for (const forbidden of [
  "sync_outbox", "cloud_incremental_pull", "apply_incremental_entities",
  "cloud_snapshot_get", "resolve_conflict", "reset_local_cache",
]) {
  if (source.includes(forbidden)) throw new Error(`直连 Realtime 模块越界引用旧同步能力：${forbidden}`);
}
for (const moved of [
  "realtime_workspace_invalidation", "realtime_refresh_domains",
  "realtime_change_seq", "realtime_url", "realtime_reconnect_delay",
]) {
  if (cloudSync.includes(`fn ${moved}(`)) {
    throw new Error(`cloud_sync.rs 仍定义已拆出的职责：${moved}`);
  }
}
if (!cloudSync.includes("cloud_connection_wake") || !cloudSync.includes("notify_one")) {
  throw new Error("Realtime 重连缺少可保留的立即唤醒入口。");
}
if (!cloudSync.includes("realtimeConnected") || !cloudSync.includes("realtimeError")) {
  throw new Error("Realtime 状态未与云端 REST 读写状态分离。");
}
if (/"cloudAccessState"\s*:\s*"offline_readonly"/.test(cloudSync)) {
  throw new Error("Realtime 错误仍会把整个云端业务状态强制切为只读。");
}
if (!frontendApi.includes("wakeCloudConnection")
  || !frontendApp.includes('addEventListener("online", handleNetworkOnline)')) {
  throw new Error("前端未在网络恢复时唤醒 Supabase 重连。");
}
console.log("Direct cloud Realtime boundary verified.");
