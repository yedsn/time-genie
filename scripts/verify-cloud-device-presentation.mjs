import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const output = resolve(".tmp-cloud-device-presentation-test");
const modulePath = resolve(output, "cloudDevicePresentation.js");
const { cloudDeviceKind, cloudDeviceActionLabel, cloudDeviceActionDisabled, runConfirmedCloudAction } = await import(pathToFileURL(modulePath));

const current = { id: "a", deviceName: "电脑 A", platform: "windows", appVersion: "0.2.1", lastSeenAt: "2026-10-04T10:00:00Z", current: true };
const other = { ...current, id: "b", deviceName: "电脑 B", current: false };
const revoked = { ...other, revokedAt: "2026-10-04T11:00:00Z" };
assert.equal(cloudDeviceKind(current), "当前设备");
assert.equal(cloudDeviceKind(other), "windows");
assert.equal(cloudDeviceActionLabel(current), "退出");
assert.equal(cloudDeviceActionLabel(other), "撤销");
assert.equal(cloudDeviceActionDisabled(other, false), false);
assert.equal(cloudDeviceActionDisabled(revoked, false), true);
assert.equal(cloudDeviceActionDisabled(other, true), true);

let calls = 0;
const cancelled = await runConfirmedCloudAction(
  async () => { throw new Error("cancelled"); },
  async () => { calls += 1; return "called"; },
);
assert.equal(cancelled, undefined);
assert.equal(calls, 0);
const confirmed = await runConfirmedCloudAction(async () => true, async () => { calls += 1; return "done"; });
assert.equal(confirmed, "done");
assert.equal(calls, 1);

if (output.startsWith(resolve("."))) rmSync(output, { recursive: true, force: true });
console.log("cloud device presentation verification passed");
