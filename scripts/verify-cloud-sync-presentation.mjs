import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-cloud-sync-presentation-test/cloudSyncPresentation.js");
const { cloudSaveStatusLabel, cloudSyncPresentation, directCloudMutationFeedback } = await import(pathToFileURL(modulePath));

const formatSyncTime = (value) => `time:${value}`;
const cloud = {
  mode: "cloud",
  online: true,
  syncState: "synced",
  pendingOperations: 0,
  conflictCount: 0,
};

assert.deepEqual(cloudSyncPresentation(undefined, formatSyncTime), {
  label: "本地模式",
  detail: "",
  needsAttention: false,
  attentionLevel: "",
});

assert.deepEqual(cloudSyncPresentation({ ...cloud, mode: "local" }, formatSyncTime), {
  label: "本地模式",
  detail: "",
  needsAttention: false,
  attentionLevel: "",
});

assert.equal(cloudSaveStatusLabel(undefined), "仅保存在本机");
assert.equal(cloudSaveStatusLabel({ ...cloud, mode: "local" }), "仅保存在本机");
assert.equal(cloudSaveStatusLabel({ ...cloud, pendingOperations: 3 }), "还有 3 项待保存");
assert.equal(cloudSaveStatusLabel({ ...cloud, conflictCount: 1 }), "还有 1 项待保存");
assert.equal(cloudSaveStatusLabel({ ...cloud, pendingOperations: 2, conflictCount: 1 }), "还有 3 项待保存");
assert.equal(cloudSaveStatusLabel({ ...cloud, lastError: "网络失败" }), "保存失败");
assert.equal(cloudSaveStatusLabel({ ...cloud, online: false }), "云端连接不可用");
assert.equal(cloudSaveStatusLabel(cloud), "所有更改均已保存");
assert.equal(cloudSaveStatusLabel({ ...cloud, syncing: true }), "所有更改均已保存");
assert.equal(cloudSaveStatusLabel({ ...cloud, syncing: true, pendingOperations: 3 }), "还有 3 项待保存");
assert.equal(cloudSaveStatusLabel({ ...cloud, storageProtocol: "direct_v2", cloudAccessState: "connecting", cloudWritable: false }), "正在连接云端");
assert.equal(cloudSaveStatusLabel({ ...cloud, storageProtocol: "direct_v2", cloudAccessState: "online_writable", cloudWritable: true }), "已直接保存到 Supabase");
assert.equal(cloudSaveStatusLabel({ ...cloud, storageProtocol: "direct_v2", cloudAccessState: "offline_readonly", cloudWritable: false }), "云端只读");
assert.equal(cloudSaveStatusLabel({ ...cloud, storageProtocol: "direct_v2", cloudAccessState: "reauth_required", cloudWritable: false }), "需要重新登录");

const directOnline = cloudSyncPresentation({
  ...cloud, storageProtocol: "direct_v2", cloudAccessState: "online_writable", cloudWritable: true, lastSyncedAt: 4321,
}, formatSyncTime);
assert.equal(directOnline.label, "云端在线");
assert.match(directOnline.detail, /最近刷新 time:4321/);
assert.equal(directOnline.needsAttention, false);

const directConnecting = cloudSyncPresentation({
  ...cloud, storageProtocol: "direct_v2", cloudAccessState: "connecting", cloudWritable: false,
}, formatSyncTime);
assert.equal(directConnecting.label, "正在连接云端");
assert.equal(directConnecting.needsAttention, false);

const directOffline = cloudSyncPresentation({
  ...cloud, storageProtocol: "direct_v2", cloudAccessState: "offline_readonly", cloudWritable: false, readOnlyReason: "网络不可达",
}, formatSyncTime);
assert.equal(directOffline.label, "云端只读");
assert.equal(directOffline.detail, "网络不可达");
assert.equal(directOffline.attentionLevel, "offline");

const directReauth = cloudSyncPresentation({
  ...cloud, storageProtocol: "direct_v2", cloudAccessState: "reauth_required", cloudWritable: false, readOnlyReason: "设备授权已撤销",
}, formatSyncTime);
assert.equal(directReauth.label, "需要重新登录");
assert.equal(directReauth.attentionLevel, "critical");

const directOfflineDefault = cloudSyncPresentation({
  ...cloud, storageProtocol: "direct_v2", cloudAccessState: "offline_readonly", cloudWritable: false,
}, formatSyncTime);
assert.equal(directOfflineDefault.detail, "云端离线，只能查看已加载数据；不会保存到本机等待同步");

const directReauthDefault = cloudSyncPresentation({
  ...cloud, storageProtocol: "direct_v2", cloudAccessState: "reauth_required", cloudWritable: false,
}, formatSyncTime);
assert.equal(directReauthDefault.detail, "登录已失效，请重新登录 Supabase");

assert.deepEqual(directCloudMutationFeedback(new Error("VERSION_CONFLICT: stale version")), {
  level: "warning",
  message: "数据已在其他设备变化，正在重新查询最新云端数据",
  refreshScope: "business",
});
assert.deepEqual(directCloudMutationFeedback("ENTITY_DELETED: task missing"), {
  level: "warning",
  message: "数据已在其他设备变化，正在重新查询最新云端数据",
  refreshScope: "business",
});
assert.deepEqual(directCloudMutationFeedback(new Error("WORKSPACE_EPOCH_STALE: server=2")), {
  level: "warning",
  message: "云端工作空间已重置，正在清除旧页面数据并重新加载",
  refreshScope: "reset",
});
assert.deepEqual(directCloudMutationFeedback(new Error("DEVICE_REVOKED: revoked")), {
  level: "error",
  message: "登录已失效，请重新登录 Supabase",
  refreshScope: "none",
});
assert.deepEqual(directCloudMutationFeedback(new Error("CLOUD_OFFLINE: offline")), {
  level: "warning",
  message: "云端离线，只能查看已加载数据；不会保存到本机等待同步",
  refreshScope: "none",
});

const syncing = cloudSyncPresentation({ ...cloud, syncing: true, pendingOperations: 3 }, formatSyncTime);
assert.equal(syncing.label, "还有 3 项待保存");
assert.equal(syncing.detail, "本机更改将在后台自动保存");
assert.equal(syncing.needsAttention, false);
assert.equal(syncing.attentionLevel, "");

const saving = cloudSyncPresentation({ ...cloud, syncing: true, saving: true, pendingOperations: 3 }, formatSyncTime);
assert.equal(saving.label, "还有 3 项待保存");
assert.equal(saving.detail, "正在后台保存本机更改");

const pulling = cloudSyncPresentation({ ...cloud, syncing: true, saving: false }, formatSyncTime);
assert.equal(pulling.label, "实时同步");
assert.match(pulling.detail, /实时同步已连接/);

const conflict = cloudSyncPresentation({ ...cloud, online: false, lastError: "网络失败", pendingOperations: 3, conflictCount: 2 }, formatSyncTime);
assert.equal(conflict.label, "2 项冲突，3 项待处理");
assert.equal(conflict.attentionLevel, "critical");
assert.equal(conflict.needsAttention, true);
assert.match(conflict.detail, /冲突阻塞了后续 3 项同步/);

const pendingWithError = cloudSyncPresentation({ ...cloud, online: false, lastError: "网络失败", pendingOperations: 3 }, formatSyncTime);
assert.equal(pendingWithError.label, "3 项等待连接");
assert.equal(pendingWithError.attentionLevel, "pending");
assert.match(pendingWithError.detail, /本机修改已保存/);
assert.match(pendingWithError.detail, /网络失败/);

const error = cloudSyncPresentation({ ...cloud, online: false, lastError: "网络失败" }, formatSyncTime);
assert.equal(error.label, "同步失败");
assert.equal(error.attentionLevel, "critical");
assert.match(error.detail, /网络失败/);

const stateError = cloudSyncPresentation({ ...cloud, syncState: "error" }, formatSyncTime);
assert.equal(stateError.label, "同步失败");
assert.equal(stateError.attentionLevel, "critical");

const pending = cloudSyncPresentation({ ...cloud, online: false, pendingOperations: 3 }, formatSyncTime);
assert.equal(pending.label, "3 项等待连接");
assert.equal(pending.attentionLevel, "pending");
assert.match(pending.detail, /等待网络或登录状态恢复/);

const offline = cloudSyncPresentation({ ...cloud, online: false }, formatSyncTime);
assert.equal(offline.label, "云端离线");
assert.equal(offline.attentionLevel, "offline");
assert.equal(offline.needsAttention, true);

const synced = cloudSyncPresentation({ ...cloud, lastSyncedAt: 1234 }, formatSyncTime);
assert.equal(synced.label, "实时同步");
assert.equal(synced.detail, "已实时同步 · 最近更新 time:1234");
assert.equal(synced.needsAttention, false);
assert.equal(synced.attentionLevel, "");

const temporaryOutput = resolve(".tmp-cloud-sync-presentation-test");
if (temporaryOutput.startsWith(resolve("."))) rmSync(temporaryOutput, { recursive: true, force: true });
console.log("cloud sync presentation verification passed");
