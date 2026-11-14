import { strict as nodeAssert } from "node:assert";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { contractRepositories, missingContractManifests } from "./contract-workspace.mjs";
import { assert } from "./helpers.mjs";

assert(contractRepositories.length === 6, "six contract participants expected");
assert(new Set(contractRepositories).size === contractRepositories.length, "duplicate contract repository");
for (const repo of contractRepositories) {
  assert(/^fanwaave-[a-z0-9.-]+$/.test(repo), `invalid contract repo ${repo}`);
}

// Exercise the workspace admission checker without requiring sibling checkouts.
const fixture = mkdtempSync(join(tmpdir(), "fanwaave-e2e-"));
try {
  for (const repo of contractRepositories) {
    mkdirSync(join(fixture, repo));
    writeFileSync(join(fixture, repo, ".zpkg.toml"), "");
  }
  nodeAssert.deepEqual(missingContractManifests(fixture), []);
  const absent = contractRepositories[0];
  rmSync(join(fixture, absent, ".zpkg.toml"));
  nodeAssert.deepEqual(missingContractManifests(fixture), [absent]);
  nodeAssert.throws(() => missingContractManifests(""), TypeError);
} finally {
  rmSync(fixture, { recursive: true, force: true });
}

// In a checked-out fanwaave workspace, run the real cross-repo admission gate.
if (process.env.FANWAAVE_WORKSPACE_ROOT) {
  const missing = missingContractManifests(process.env.FANWAAVE_WORKSPACE_ROOT);
  assert(missing.length === 0, `missing sibling package manifests: ${missing.join(", ")}`);
  console.log("fanwaave sibling contract manifests present");
}
console.log("fanwaave contract catalog ok");
