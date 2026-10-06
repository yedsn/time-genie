import assert from "node:assert/strict";
import path from "node:path";
import { pathToFileURL } from "node:url";

const modulePath = path.resolve(".tmp-unassigned-resolution-guard-test/unassignedResolutionGuard.js");
const { canSubmitUnassignedResolution } = await import(pathToFileURL(modulePath));

assert.equal(canSubmitUnassignedResolution("session-a", 3, "session-a", 3), true);
assert.equal(canSubmitUnassignedResolution("session-a", 3, "session-b", 1), false);
assert.equal(canSubmitUnassignedResolution("session-a", 3, "session-a", 4), false);
assert.equal(canSubmitUnassignedResolution("session-a", 3, "", 0), false);

console.log("Unassigned resolution guard verification passed.");
