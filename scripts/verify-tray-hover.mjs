import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { readFileSync, rmSync } from "node:fs";

const modulePath = resolve(".tmp-tray-hover-test/trayHoverCore.js");
const { createSingleFlight, trayIdlePresentation } = await import(pathToFileURL(modulePath));

assert.equal(trayIdlePresentation({ hasActiveEntry: true, unassignedSessionId: "u1", unassignedSeconds: 60 }), "timer");
assert.equal(trayIdlePresentation({ hasActiveEntry: false, unassignedSessionId: "u1", unassignedSeconds: 60 }), "unassigned");
assert.equal(trayIdlePresentation({ hasActiveEntry: false, unassignedSessionId: "", unassignedSeconds: 0 }), "idle");
assert.equal(trayIdlePresentation({ hasActiveEntry: false, unassignedSessionId: "u1", unassignedSeconds: 0 }), "idle");

const hoverPanelSource = readFileSync(resolve("src-ui/src/views/HoverPanel.vue"), "utf8");
const mainWorkbenchSource = readFileSync(resolve("src-ui/src/views/MainWorkbench.vue"), "utf8");
const timeWorkspaceSource = readFileSync(resolve("src-ui/src/components/TimeTrackingWorkspace.vue"), "utf8");
const taskSelectSource = readFileSync(resolve("src-ui/src/components/TaskSelectControl.vue"), "utf8");

assert.match(hoverPanelSource, /openMainTimerTaskSelection/);
assert.doesNotMatch(hoverPanelSource, /TaskSelectControl|selectedTaskId|setHoverTaskSelectionMode/);
assert.match(mainWorkbenchSource, /onOpenTimerTaskSelection/);
assert.ok(mainWorkbenchSource.includes("timeTrackingWorkspaceRef.value?.focusTaskSelector()"));
assert.ok(timeWorkspaceSource.includes("defineExpose({ focusTaskSelector })"));
assert.ok(taskSelectSource.includes("defineExpose({ commit: commitText, closeSuggestions: closeTaskSuggestions, focus: focusTaskInput })"));

let release;
let calls = 0;
const runOnce = createSingleFlight(async () => {
  calls += 1;
  if (calls === 1) await new Promise((resolvePromise) => { release = resolvePromise; });
  return calls;
});
const first = runOnce();
const second = runOnce();
assert.equal(first, second);
assert.equal(calls, 1);
release();
assert.equal(await first, 1);
assert.equal(await runOnce(), 2);

rmSync(resolve(".tmp-tray-hover-test"), { recursive: true, force: true });
console.log("tray hover verification passed");
