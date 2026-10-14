import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-tray-hover-test/trayHoverCore.js");
const { createSingleFlight, reconcileTrayTaskSelection, trayIdlePresentation } = await import(pathToFileURL(modulePath));

const tasks = [
  { id: "parent", selectable: true },
  { id: "leaf", parentId: "parent", selectable: true },
  { id: "disabled", selectable: false },
];
assert.equal(reconcileTrayTaskSelection("leaf", tasks), "leaf");
assert.equal(reconcileTrayTaskSelection("parent", tasks), "");
assert.equal(reconcileTrayTaskSelection("disabled", tasks), "");
assert.equal(reconcileTrayTaskSelection("deleted", tasks), "");

assert.equal(trayIdlePresentation({ hasActiveEntry: true, unassignedSessionId: "u1", unassignedSeconds: 60 }), "timer");
assert.equal(trayIdlePresentation({ hasActiveEntry: false, unassignedSessionId: "u1", unassignedSeconds: 60 }), "unassigned");
assert.equal(trayIdlePresentation({ hasActiveEntry: false, unassignedSessionId: "", unassignedSeconds: 0 }), "idle");
assert.equal(trayIdlePresentation({ hasActiveEntry: false, unassignedSessionId: "u1", unassignedSeconds: 0 }), "idle");

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
