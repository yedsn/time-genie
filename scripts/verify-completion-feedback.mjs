import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-completion-feedback-test/completionFeedbackCore.js");
const {
  CompletionFeedbackGate,
  completionFeedbackMessage,
  decideCompletionFeedbackMode,
  runCompletionEffect,
  remainingOpenTaskCount,
} = await import(pathToFileURL(modulePath));

assert.equal(decideCompletionFeedbackMode({ completedCount: 0, enabled: true }), "none");
assert.equal(decideCompletionFeedbackMode({ completedCount: 1, enabled: false }), "none");
assert.equal(decideCompletionFeedbackMode({ completedCount: 1, enabled: true }), "normal");
assert.equal(decideCompletionFeedbackMode({ completedCount: 1, openBefore: 1, openAfter: 0, enabled: true }), "all-clear");
assert.equal(completionFeedbackMessage("normal", 1), "完成一项，继续保持");
assert.equal(completionFeedbackMessage("normal", 3), "漂亮！一次完成 3 项");
assert.equal(completionFeedbackMessage("all-clear", 1), "当前视图事项已全部完成");
assert.equal(remainingOpenTaskCount(1, 1), 0);
assert.equal(remainingOpenTaskCount(4, 2), 2);
assert.equal(remainingOpenTaskCount(undefined, 1), undefined);

const gate = new CompletionFeedbackGate(220);
assert.equal(gate.shouldLaunch("normal", 1_000), true);
assert.equal(gate.shouldLaunch("normal", 1_100), false);
assert.equal(gate.shouldLaunch("all-clear", 1_100), true);
assert.equal(gate.shouldLaunch("normal", 1_230), true);
assert.equal(gate.shouldLaunch("none", 2_000), false);
assert.equal(runCompletionEffect(() => undefined), true);
assert.equal(runCompletionEffect(() => { throw new Error("particle failure"); }), false);

const temporaryOutput = resolve(".tmp-completion-feedback-test");
if (temporaryOutput.startsWith(resolve("."))) rmSync(temporaryOutput, { recursive: true, force: true });
console.log("completion feedback verification passed");
