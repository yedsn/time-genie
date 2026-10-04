import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-transient-state-guard-test/transientStateGuard.js");
const { TRANSIENT_MISSING_STATE_REFRESH_LIMIT, transientMissingStateResult } = await import(pathToFileURL(modulePath));

let missingRefreshes = 0;
for (let attempt = 1; attempt < TRANSIENT_MISSING_STATE_REFRESH_LIMIT; attempt += 1) {
  const result = transientMissingStateResult(true, false, missingRefreshes);
  missingRefreshes = result.missingRefreshes;
  assert.equal(result.preservePrevious, true, `第 ${attempt} 次临时空状态应保留上一状态`);
}

const confirmedMissing = transientMissingStateResult(true, false, missingRefreshes);
assert.equal(confirmedMissing.missingRefreshes, TRANSIENT_MISSING_STATE_REFRESH_LIMIT);
assert.equal(confirmedMissing.preservePrevious, false, "连续多次空状态后应允许清空");

assert.deepEqual(transientMissingStateResult(true, true, confirmedMissing.missingRefreshes), {
  missingRefreshes: 0,
  preservePrevious: false,
});

assert.deepEqual(transientMissingStateResult(false, false, 2), {
  missingRefreshes: 0,
  preservePrevious: false,
});

const temporaryOutput = resolve(".tmp-transient-state-guard-test");
if (temporaryOutput.startsWith(resolve("."))) rmSync(temporaryOutput, { recursive: true, force: true });
console.log("transient state guard verification passed");
