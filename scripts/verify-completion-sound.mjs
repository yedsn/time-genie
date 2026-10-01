import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { resolve } from "node:path";

const soundPath = resolve("src-ui/src/assets/audio/completion-success.wav");
assert.equal(existsSync(soundPath), true);
const soundBuffer = readFileSync(soundPath);
assert.ok(soundBuffer.length > 0);
assert.equal(soundBuffer.subarray(0, 4).toString("ascii"), "RIFF");
assert.equal(soundBuffer.subarray(8, 12).toString("ascii"), "WAVE");
assert.equal(createHash("sha256").update(soundBuffer).digest("hex"), "084b78463e30dae4aac5edbf9e9c93cdee5ece4a4a688352e6a2b6686f9b1f4a");
console.log("completion sound verification passed");
