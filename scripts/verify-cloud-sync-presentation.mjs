import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-cloud-sync-presentation-test/cloudSyncPresentation.js");
const { cloudSyncPresentation, cloudSyncRetryFeedback } = await import(pathToFileURL(modulePath));

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
assert.equal(synced.label, "已同步");
assert.equal(synced.detail, "最近同步 time:1234");
assert.equal(synced.needsAttention, false);
assert.equal(synced.attentionLevel, "");

assert.deepEqual(cloudSyncRetryFeedback({ pushed: 2, pending: 0, conflicts: 0 }, { ...cloud }), {
  level: "success",
  message: "已同步 2 项修改",
});

assert.deepEqual(cloudSyncRetryFeedback({ pushed: 0, pending: 3, conflicts: 0 }, { ...cloud, pendingOperations: 3 }), {
  level: "warning",
  message: "本机修改已保存，仍有 3 项等待同步",
});

assert.deepEqual(cloudSyncRetryFeedback({ pushed: 1, pending: 0, conflicts: 1 }, { ...cloud, conflictCount: 1 }), {
  level: "warning",
  message: "同步遇到版本冲突，请选择要保留的版本",
});

const temporaryOutput = resolve(".tmp-cloud-sync-presentation-test");
if (temporaryOutput.startsWith(resolve("."))) rmSync(temporaryOutput, { recursive: true, force: true });
console.log("cloud sync presentation verification passed");
