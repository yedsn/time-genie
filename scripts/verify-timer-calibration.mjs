import assert from "node:assert/strict";
import { shouldCalibrateTimer, TimerCalibrationSingleFlight } from "../.tmp-direct-cloud-performance-test/timerCalibrationCore.js";

assert.equal(shouldCalibrateTimer(60_000, 0, 5_000, true), true);
assert.equal(shouldCalibrateTimer(29_999, 0, 5_000, false), false);
assert.equal(shouldCalibrateTimer(30_000, 0, 5_000, false), true);
assert.equal(shouldCalibrateTimer(10_000, 9_000, 16_000, true), true);

let calls = 0;
let release;
const flight = new TimerCalibrationSingleFlight();
const first = flight.run(async () => {
  calls += 1;
  if (calls === 1) await new Promise((resolve) => { release = resolve; });
});
const second = flight.run(async () => { calls += 1; });
assert.equal(calls, 1);
release();
await Promise.all([first, second]);
assert.equal(calls, 2, "恢复期间重复信号最多追加一次校准");
console.log("timer calibration verification passed");
