import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { readFileSync, rmSync } from "node:fs";
import { resolve } from "node:path";

const modulePath = resolve(".tmp-timer-sync-recovery-presentation-test/timerSyncRecoveryPresentation.js");
const { timerSyncRecoveryDetail, timerSyncRecoveryLabel } = await import(pathToFileURL(modulePath));

assert.equal(timerSyncRecoveryLabel({ recoveryKind: "historical_candidate" }), "等待自动恢复");
assert.equal(timerSyncRecoveryLabel({ recoveryKind: "rebased" }), "已自动压缩，等待保存");
assert.equal(timerSyncRecoveryLabel({ recoveryKind: "competing_edit" }), "需要处理的设备冲突");
assert.equal(timerSyncRecoveryDetail({
  recoveryContext: {
    message: "已按云端权威版本自动压缩本机计时链",
    fromBaseVersion: 4,
    cloudVersion: 6,
    localFinalVersion: 8,
  },
}), "已按云端权威版本自动压缩本机计时链 · 云端版本 6 · 原本地跨度 4 → 8");

const view = readFileSync(resolve("src-ui/src/views/MainWorkbench.vue"), "utf8");
for (const required of [
  "版本 {{ item.baseVersion ?? '新建' }} → {{ item.payloadVersion ?? '?' }}",
  "cloudQueueRecoveryDetail(item)",
  "conflict.conflictReason || conflict.error",
  "本机内容",
  "云端内容",
  "resolveAllConflicts('use_cloud')",
  "resolveAllConflicts('keep_local')",
  "onTimerFactFollowed((event)",
  "已在另一台设备保存，已跟随该结果",
  "timer-followed-message__link",
  'store.activePage = "timer"',
  "store.selectedEntryId = event.entityId",
  "duration: 5000",
  "showClose: true",
  '@start-timer="startTimerForTask"',
]) {
  assert.ok(view.includes(required), `missing timer sync presentation: ${required}`);
}

const startTimerHandler = view.slice(
  view.indexOf("async function startTimerForTask"),
  view.indexOf("async function goToPlanFromTimerDialog"),
);
assert.ok(startTimerHandler.includes("store.transitionTimer({ taskId: persistedTaskId })"));
assert.ok(!startTimerHandler.includes("openExecutionPanel"), "plan timer action must not open the execution dialog");

const planEditor = readFileSync(resolve("src-ui/src/components/PlanListEditor.vue"), "utf8");
assert.ok(planEditor.includes(`@click.stop="emit('startTimer', row.task.id)"`));

const styles = readFileSync(resolve("src-ui/src/styles.css"), "utf8");
for (const required of [
  ".timer-followed-message {",
  ".timer-followed-message__link {",
  ".timer-followed-message__link:focus-visible",
]) {
  assert.ok(styles.includes(required), `missing timer followed message style: ${required}`);
}

const temporaryOutput = resolve(".tmp-timer-sync-recovery-presentation-test");
if (temporaryOutput.startsWith(resolve("."))) rmSync(temporaryOutput, { recursive: true, force: true });
console.log("timer sync recovery presentation verification passed");
