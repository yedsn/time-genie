import assert from "node:assert/strict";
import { derivedUnassignedSeconds } from "../.tmp-unassigned-time-display-test/unassignedTimeDisplayCore.js";

assert.equal(derivedUnassignedSeconds(42, 10_000, 13_999), 45);
assert.equal(derivedUnassignedSeconds(42, 310_000, 313_999), 45);
assert.equal(derivedUnassignedSeconds(42, 10_000, 9_000), 42);
assert.equal(derivedUnassignedSeconds(0, 0, 100_000), 0);
assert.equal(derivedUnassignedSeconds(299, 10_000, 11_000), 300);

console.log("Unassigned time display verification passed.");
