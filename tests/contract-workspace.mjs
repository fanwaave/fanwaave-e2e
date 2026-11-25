import { statSync } from "node:fs";
import { resolve, join } from "node:path";

export const contractRepositories = Object.freeze([
  "fanwaave-clients",
  "fanwaave-cli",
  "fanwaave-interfaces",
  "fanwaave-lib-core",
  "fanwaave-api-server.rs",
  "fanwaave-web-server.rs",
]);

/**
 * Validate the package manifest for each contract consumer, not just the
 * existence of a checkout directory. Returns a stable list of missing entries.
 */
export function missingContractManifests(workspaceRoot) {
  if (typeof workspaceRoot !== "string" || workspaceRoot.trim().length === 0) {
    throw new TypeError("workspaceRoot must be a non-empty directory");
  }
  const base = resolve(workspaceRoot);
  const missing = [];
  for (const repo of contractRepositories) {
    const manifest = join(base, repo, ".zpkg.toml");
    try {
      if (!statSync(manifest).isFile()) missing.push(repo);
    } catch (error) {
      if (error?.code === "ENOENT" || error?.code === "ENOTDIR") {
        missing.push(repo);
      } else {
        throw error;
      }
    }
  }
  return missing;
}
