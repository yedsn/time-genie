import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const workbench = fs.readFileSync(path.join(root, "src-ui/src/views/MainWorkbench.vue"), "utf8");
const snapshots = fs.readFileSync(path.join(root, "src-ui/src/components/CloudSnapshotsSettings.vue"), "utf8");
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
  "directCloudEnabled &&", "!directCloudEnabled && storageState.pendingOperations",
  "storageState?.mode === 'cloud' && !directCloudEnabled",
  "升级为 Supabase 直连云端", "工作空间切换后不能降回旧同步协议",
  "受影响的其他设备", "完成旧队列后升级", "救援后放弃旧修改",
  "CLIENT_UPGRADE_REQUIRED", "放弃旧修改并升级",
  "getStorageProtocolUpgradePreflight", "upgradeStorageProtocolWithDrainedQueue",
  "exportRescueDataBackup", "upgradeStorageProtocolAfterRescue",
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
if (!workbench.includes('v-if="!directCloudEnabled && storageState.conflictCount"')) {
  throw new Error("direct_v2 下仍会展示同步冲突入口");
}
if (!workbench.includes(`v-if="storageState?.mode === 'cloud' && !directCloudEnabled" class="storage-section local-cache-section"`)) {
  throw new Error("direct_v2 下仍会展示本地缓存重置入口");
}
for (const expected of [
  `v-if="directCloudEnabled" class="secondary-button compact"`,
  "data-direct-cloud-refresh",
  "刷新云端数据",
  `v-if="!directCloudEnabled && storageState.pendingOperations && !storageState.conflictCount"`,
  `v-if="!directCloudEnabled && storageState.conflictCount"`,
]) {
  if (!workbench.includes(expected)) throw new Error(`direct_v2 设置页能力边界缺少：${expected}`);
}
for (const expected of [
  "queue.total ? '完成旧队列后升级' : '旧队列为空，直接升级'",
  "protocolUpgradePreflight.blockedReasons",
  "protocolUpgradeConfirmation !== '放弃旧修改'",
]) {
  if (!workbench.includes(expected)) throw new Error(`协议升级路径自动化缺少：${expected}`);
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
