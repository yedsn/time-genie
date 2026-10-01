import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-allocation-math-test/allocationMath.js");
const {
  normalizeAllocationMinutes,
  allocationMaximum,
  rebalanceAllocationMinutes,
} = await import(pathToFileURL(modulePath));

assert.equal(normalizeAllocationMinutes("12"), 12);
assert.equal(normalizeAllocationMinutes("-3"), 0);
assert.equal(normalizeAllocationMinutes("invalid"), 0);
assert.equal(allocationMaximum([30, 0], 1, 30), 30);
assert.equal(allocationMaximum([10, 20, 0], 2, 30), 20);

assert.deepEqual(rebalanceAllocationMinutes([30, 0], 1, 10, 30), [20, 10]);
assert.deepEqual(rebalanceAllocationMinutes([30, 0], 1, 40, 30), [0, 30]);
assert.deepEqual(rebalanceAllocationMinutes([10, 20, 0], 2, 15, 30), [10, 5, 15]);
assert.deepEqual(rebalanceAllocationMinutes([10, 20, 0], 2, 50, 30), [10, 0, 20]);
assert.deepEqual(rebalanceAllocationMinutes([20, 10], 0, 15, 30), [15, 15]);
assert.deepEqual(rebalanceAllocationMinutes([20, 10, 0], 0, 25, 30), [25, 5, 0]);
assert.deepEqual(rebalanceAllocationMinutes([30], 0, 40, 30), [30]);

const temporaryOutput = resolve(".tmp-allocation-math-test");
if (temporaryOutput.startsWith(resolve("."))) rmSync(temporaryOutput, { recursive: true, force: true });
console.log("allocation math verification passed");
