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
  storageProtocol: "direct_v2",
  cloudAccessState: "online_writable",
  cloudWritable: true,
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
assert.equal(cloudSaveStatusLabel(cloud), "已直接保存到 Supabase");
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

const temporaryOutput = resolve(".tmp-cloud-sync-presentation-test");
if (temporaryOutput.startsWith(resolve("."))) rmSync(temporaryOutput, { recursive: true, force: true });
console.log("cloud sync presentation verification passed");
