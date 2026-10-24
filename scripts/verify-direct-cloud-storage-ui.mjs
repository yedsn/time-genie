import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const workbench = fs.readFileSync(path.join(root, "src-ui/src/views/MainWorkbench.vue"), "utf8");
const snapshots = fs.readFileSync(path.join(root, "src-ui/src/components/CloudSnapshotsSettings.vue"), "utf8");
const presentation = fs.readFileSync(path.join(root, "src-ui/src/services/cloudSyncPresentation.ts"), "utf8");
const service = fs.readFileSync(path.join(root, "src-ui/src/services/tauri.ts"), "utf8");
const rust = fs.readFileSync(path.join(root, "src-tauri/src/supabase.rs"), "utf8");
const lib = fs.readFileSync(path.join(root, "src-tauri/src/lib.rs"), "utf8");
const schemaIntegration = fs.readFileSync(path.join(root, "supabase/schema_integration_test.sql"), "utf8");

for (const expected of [
  "本地复制到云端", "云端复制到本地", "预览替换影响",
  "完整替换云端", "创建备份并替换本地", "不会删除本地源数据",
  "不会删除云端源数据", "不会自动合并", "不会建立后续同步",
  "保持当前模式", "使用本地存储", "使用 Supabase 云端",
  "cloudBusinessReadOnly", "cloudReadOnlyReason", "data-cloud-readonly-banner",
  "rejectCloudReadonlyMutation", "if (rejectCloudReadonlyMutation()) return;",
  `:disabled="cloudBusinessReadOnly && store.activePage !== 'settings'"`,
]) {
  if (!workbench.includes(expected)) throw new Error(`直连云端设置界面缺少：${expected}`);
}
for (const expected of [
  "async function handleCloudSnapshotRestored()",
  "store.clearCloudBusinessMemory();",
  "store.loadWorkspaceData()",
  "store.loadTimeData()",
  "store.loadReports()",
]) {
  if (!workbench.includes(expected)) throw new Error(`直连云端恢复后重新查询缺少：${expected}`);
}
for (const expected of [
  "restoreCommitted = result.restored;",
  "重新读取云端权威数据",
]) {
  if (!snapshots.includes(expected)) throw new Error(`直连云端恢复提示缺少：${expected}`);
}
for (const expected of [
  "stageLocalDataCopyToCloud", "replaceCloudWithStagedLocalData",
  "replaceLocalWithCloudDataCopy", "setCloudStorageMode", "storage_mode_set_cloud",
]) {
  if (!service.includes(expected) && !rust.includes(expected) && !lib.includes(expected)) {
    throw new Error(`直连云端复制/切换服务缺少：${expected}`);
  }
}
for (const forbidden of [
  "cloudQueueDialogOpen", "cloudConflictDialogOpen", "resetLocalCacheDialogOpen",
  "listCloudSyncQueue", "listCloudSyncConflicts", "resetLocalCacheFromCloud",
  "resetLegacyCloudAndUseAuthority", "resetLegacyCloudToAuthority",
  "storage-protocol-upgrade-section", "storageState?.conflictCount",
  `title="同步队列"`, `title="处理云端同步冲突"`, `title="重置本地数据"`,
]) {
  if (workbench.includes(forbidden)) throw new Error(`前端仍包含旧同步入口或对话框：${forbidden}`);
}
for (const expected of [
  `v-if="directCloudEnabled" class="secondary-button compact"`,
  "data-direct-cloud-refresh",
  "刷新云端数据",
]) {
  if (!workbench.includes(expected)) throw new Error(`direct_v2 设置页能力边界缺少：${expected}`);
}
for (const expected of [
  "cloudStatePillTitle", "点击查看云端状态", "cloudDockWorkingLabel",
  "刷新中",
  "云端离线，只能查看已加载数据；不会保存到本机等待同步",
  "登录已失效，请重新登录 Supabase",
  "主体设置已直接保存到 Supabase",
  "保存后会直接提交到 Supabase，不会写入本机等待同步",
]) {
  if (!workbench.includes(expected)) throw new Error(`direct_v2 主工作台语义缺少：${expected}`);
}
for (const expected of [
  "已直接保存到 Supabase",
  "数据已在其他设备变化，正在重新查询最新云端数据",
  "云端离线，只能查看已加载数据；不会保存到本机等待同步",
  "登录已失效，请重新登录 Supabase",
]) {
  if (!presentation.includes(expected)) throw new Error(`direct_v2 状态或提交提示缺少：${expected}`);
}
for (const expected of [
  "仍运行旧同步版本的设备", "legacy sync write was accepted after direct protocol upgrade",
  "legacy incremental pull was accepted after direct protocol upgrade",
  "legacy tracking lease was accepted after direct protocol upgrade",
  "upgraded second device could not read the latest direct cloud data",
]) {
  if (!schemaIntegration.includes(expected)) throw new Error(`旧设备拒绝验收缺少：${expected}`);
}
console.log("Direct cloud storage UI verification passed.");
