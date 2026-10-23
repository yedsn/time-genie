import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const modulePath = path.join(root, "src-tauri/src/direct_cloud_realtime.rs");
const cloudSyncPath = path.join(root, "src-tauri/src/cloud_sync.rs");
const source = fs.readFileSync(modulePath, "utf8");
const cloudSync = fs.readFileSync(cloudSyncPath, "utf8");

for (const expected of [
  "WorkspaceInvalidation", "realtime_workspace_invalidation", "realtime_refresh_domains",
  "realtime_change_seq", "realtime_url", "realtime_reconnect_delay",
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
console.log("Direct cloud Realtime boundary verified.");
