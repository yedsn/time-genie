import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-cloud-session-presentation-test/cloudSessionPresentation.js");
const { cloudSessionErrorNeedsRecovery, cloudSessionLabel, cloudSessionNeedsPassword } = await import(pathToFileURL(modulePath));

assert.equal(cloudSessionLabel(undefined), "未登录 Supabase");
assert.equal(cloudSessionNeedsPassword(undefined), true);
assert.equal(cloudSessionLabel({ signedIn: false, status: "reauth_required" }), "未登录 Supabase");
assert.equal(cloudSessionNeedsPassword({ signedIn: false, status: "reauth_required" }), true);
assert.equal(cloudSessionLabel({ signedIn: true, status: "offline_saved", email: "user@example.com" }), "离线但保持登录");
assert.equal(cloudSessionNeedsPassword({ signedIn: true, status: "offline_saved" }), false);
assert.equal(cloudSessionLabel({ signedIn: true, status: "authenticated", email: "user@example.com" }), "user@example.com");
assert.equal(cloudSessionNeedsPassword({ signedIn: true, status: "authenticated" }), false);
assert.equal(cloudSessionErrorNeedsRecovery("AUTH_REQUIRED: 云端会话不可用"), true);
assert.equal(cloudSessionErrorNeedsRecovery("KEEP_LOCAL_OVERWRITE_FAILED: 覆盖失败：AUTH_INVALID: 会话失效"), true);
assert.equal(cloudSessionErrorNeedsRecovery(new Error("AUTH_FAILED: token expired")), true);
assert.equal(cloudSessionErrorNeedsRecovery("NETWORK_ERROR: 暂时无法连接"), false);

const temporaryOutput = resolve(".tmp-cloud-session-presentation-test");
if (temporaryOutput.startsWith(resolve("."))) rmSync(temporaryOutput, { recursive: true, force: true });
console.log("cloud session presentation verification passed");
