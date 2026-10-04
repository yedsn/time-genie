import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { rmSync } from "node:fs";

const modulePath = resolve(".tmp-cloud-session-presentation-test/cloudSessionPresentation.js");
const { cloudSessionLabel, cloudSessionNeedsPassword } = await import(pathToFileURL(modulePath));

assert.equal(cloudSessionLabel(undefined), "未登录 Supabase");
assert.equal(cloudSessionNeedsPassword(undefined), true);
assert.equal(cloudSessionLabel({ signedIn: false, status: "reauth_required" }), "未登录 Supabase");
assert.equal(cloudSessionNeedsPassword({ signedIn: false, status: "reauth_required" }), true);
assert.equal(cloudSessionLabel({ signedIn: true, status: "offline_saved", email: "user@example.com" }), "离线但保持登录");
assert.equal(cloudSessionNeedsPassword({ signedIn: true, status: "offline_saved" }), false);
assert.equal(cloudSessionLabel({ signedIn: true, status: "authenticated", email: "user@example.com" }), "user@example.com");
assert.equal(cloudSessionNeedsPassword({ signedIn: true, status: "authenticated" }), false);

const temporaryOutput = resolve(".tmp-cloud-session-presentation-test");
if (temporaryOutput.startsWith(resolve("."))) rmSync(temporaryOutput, { recursive: true, force: true });
console.log("cloud session presentation verification passed");

