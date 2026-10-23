import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relativePath) => fs.readFileSync(path.join(root, relativePath), "utf8");
const docs = {
  readme: read("README.md"),
  protocol: read("docs/direct-cloud-storage-protocol.md"),
  data: read("docs/20260924_timegenie_数据结构设计.md"),
  api: read("docs/20260924_timegenie_后端接口设计.md"),
  restore: read("docs/cloud-workspace-restore.md"),
};
const schema = read("supabase/schema.sql");
const protocolPatch = read("supabase/20261022f_timegenie_direct_cloud_storage_protocol.sql");
const snapshotPatch = read("supabase/20261018c_timegenie_workspace_snapshots.sql");
const restorePatch = read("supabase/20261018e_timegenie_workspace_restore.sql");
const schemaSource = [schema, protocolPatch, snapshotPatch, restorePatch].join("\n");

const allDocs = Object.values(docs).join("\n");
for (const expected of [
  "direct_v2", "sync_v1", "CLOUD_PROTOCOL_UNSUPPORTED",
  "workspace_restore_prepare", "workspace_restore_commit",
  "紧急只读", "不重建 SQLite", "single-flight", "不执行每 5 秒业务轮询",
]) {
  if (!allDocs.includes(expected)) throw new Error(`直连云端文档缺少：${expected}`);
}

for (const rpc of [
  "storage_protocol_get",
  "storage_protocol_upgrade_preflight",
  "upgrade_storage_protocol",
  "direct_cloud_workbench_bootstrap",
  "workspace_snapshot_create",
  "workspace_restore_prepare",
  "workspace_restore_commit",
]) {
  if (!allDocs.includes(rpc)) throw new Error(`文档未声明 RPC：${rpc}`);
  if (!schemaSource.includes(rpc)) throw new Error(`文档 RPC 未在 Schema/增量 SQL 中找到：${rpc}`);
}

for (const expected of [
  "查询、快照与导出可用",
  "绝不回退到旧同步队列",
]) {
  if (!docs.readme.includes(expected)) throw new Error(`用户说明缺少：${expected}`);
}
for (const expected of [
  "清空当前进程内的领域数据",
  "不会下载完整快照或重建 SQLite 业务缓存",
]) {
  if (!docs.restore.includes(expected)) throw new Error(`恢复手册缺少：${expected}`);
}
for (const expected of [
  "紧急只读开关",
  "不得重新启用 `sync_v1` outbox",
  "test:direct-cloud-interaction-performance",
]) {
  if (!docs.protocol.includes(expected)) throw new Error(`运维说明缺少：${expected}`);
}

console.log("Direct cloud documentation verification passed.");
