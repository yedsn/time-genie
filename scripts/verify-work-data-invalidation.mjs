import assert from "node:assert/strict";
import { decideWorkDataInvalidation } from "../.tmp-direct-cloud-performance-test/workDataInvalidationCore.js";

const context = { currentWindowLabel: "main", workspaceEpoch: 3, appliedChangeSeq: 20, knownOperationIds: new Set(["local-op"]) };
assert.equal(decideWorkDataInvalidation({ sourceWindowLabel: "main", domains: ["tasks"] }, context).action, "ignore");
assert.equal(decideWorkDataInvalidation({ sourceWindowLabel: "hover", domains: ["tasks"] }, context).action, "refresh");
assert.equal(decideWorkDataInvalidation({ changeSeq: 20, domains: ["tasks"] }, context).action, "ignore");
assert.equal(decideWorkDataInvalidation({ changeSeq: 21, domains: ["tasks"] }, context).action, "refresh");
assert.equal(decideWorkDataInvalidation({ workspaceEpoch: 4, changeSeq: 1, domains: ["tasks"] }, context).action, "reset");
assert.deepEqual(decideWorkDataInvalidation({ operationId: "local-op", coveredDomains: ["tasks"], domains: ["tasks", "reports"] }, context).domains, ["reports"]);
console.log("work data invalidation verification passed");
