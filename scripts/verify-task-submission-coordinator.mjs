import assert from "node:assert/strict";
import { TaskSubmissionCoordinator } from "../.tmp-task-submission-coordinator-test/taskSubmissionCoordinatorCore.js";
import { mergeAuthoritativeTaskWithLatestDraft } from "../.tmp-task-submission-coordinator-test/taskSaveStateCore.js";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => { resolve = resolvePromise; reject = rejectPromise; });
  return { promise, resolve, reject };
}

{
  const coordinator = new TaskSubmissionCoordinator(10_000);
  const submitted = [];
  const first = coordinator.schedule("task-1", { title: "初稿" }, async (snapshot) => { submitted.push(snapshot); return snapshot.title; });
  const latest = coordinator.schedule("task-1", { title: "最终", estimate: 30 }, async (snapshot) => { submitted.push(snapshot); return snapshot.title; });
  assert.equal(first, latest);
  await coordinator.flush("task-1");
  assert.equal(await latest, "最终");
  assert.deepEqual(submitted, [{ title: "最终", estimate: 30 }]);
}

{
  const submitted = { title: "提交时标题", plannedDate: "2026-11-05", estimateMinutes: 20, note: "旧备注", projectName: "项目 A" };
  const current = { title: "最后标题", plannedDate: "2026-11-06", estimateMinutes: 45, note: "新备注", projectName: "项目 B" };
  const authoritative = { ...submitted, id: "task-1", version: 8 };
  const merged = mergeAuthoritativeTaskWithLatestDraft(authoritative, current, submitted);
  assert.deepEqual({ title: merged.title, plannedDate: merged.plannedDate, estimateMinutes: merged.estimateMinutes, note: merged.note, projectName: merged.projectName }, current);
  assert.equal(mergeAuthoritativeTaskWithLatestDraft(authoritative, submitted, submitted), authoritative);
}

{
  const coordinator = new TaskSubmissionCoordinator(10_000);
  const firstGate = deferred();
  const calls = [];
  let active = 0;
  let maxActive = 0;
  const first = coordinator.schedule("task-2", { title: "第一次" }, async (snapshot) => {
    active += 1;
    maxActive = Math.max(maxActive, active);
    calls.push(snapshot.title);
    await firstGate.promise;
    active -= 1;
    return snapshot.title;
  });
  const flushing = coordinator.flush("task-2");
  await Promise.resolve();
  const trailing = coordinator.schedule("task-2", { title: "第二次" }, async (snapshot) => {
    active += 1;
    maxActive = Math.max(maxActive, active);
    calls.push(snapshot.title);
    active -= 1;
    return snapshot.title;
  });
  firstGate.resolve();
  assert.equal(await flushing, "第二次");
  assert.equal(await first, "第二次");
  assert.equal(await trailing, "第二次");
  assert.deepEqual(calls, ["第一次", "第二次"]);
  assert.equal(maxActive, 1);
}

{
  const coordinator = new TaskSubmissionCoordinator(10_000);
  const gates = [deferred(), deferred()];
  let active = 0;
  let maxActive = 0;
  for (const [index, taskId] of ["task-a", "task-b"].entries()) {
    coordinator.schedule(taskId, taskId, async () => {
      active += 1;
      maxActive = Math.max(maxActive, active);
      await gates[index].promise;
      active -= 1;
      return taskId;
    });
  }
  const flushes = [coordinator.flush("task-a"), coordinator.flush("task-b")];
  await Promise.resolve();
  assert.equal(maxActive, 2);
  gates.forEach((gate) => gate.resolve());
  await Promise.all(flushes);
}

{
  const coordinator = new TaskSubmissionCoordinator(10_000);
  const createGate = deferred();
  const calls = [];
  const created = coordinator.schedule("draft-1", { title: "新事项" }, async (snapshot, context) => {
    calls.push([context.taskId, snapshot.title]);
    await createGate.promise;
    context.rebindTaskId("saved-1");
    return context.taskId;
  });
  const flushing = coordinator.flush("draft-1");
  await Promise.resolve();
  coordinator.schedule("draft-1", { title: "新事项已修改" }, async (snapshot, context) => {
    calls.push([context.taskId, snapshot.title]);
    return context.taskId;
  });
  createGate.resolve();
  assert.equal(await flushing, "saved-1");
  assert.equal(await created, "saved-1");
  assert.deepEqual(calls, [["draft-1", "新事项"], ["saved-1", "新事项已修改"]]);
  assert.equal(coordinator.hasPending("draft-1"), false);
  assert.equal(coordinator.hasPending("saved-1"), false);
}

{
  const coordinator = new TaskSubmissionCoordinator(10_000);
  const failure = new Error("保存失败");
  const failed = coordinator.schedule("task-error", 1, async () => { throw failure; });
  await assert.rejects(coordinator.flush("task-error"), failure);
  await assert.rejects(failed, failure);
  assert.equal(coordinator.hasPending("task-error"), false);
  const retried = coordinator.schedule("task-error", 2, async (value) => value);
  await coordinator.flush("task-error");
  assert.equal(await retried, 2);
}

console.log("task submission coordinator checks passed");
