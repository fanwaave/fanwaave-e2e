// Cross-repository smoke for the exact public AttoLang candidate commits.
// Only authoritative JSON Schema and public RPC policy may define the contract.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";

const root = process.cwd();
const clients = resolve(root, process.env.FANWAAVE_CLIENTS_CHECKOUT || "external/fanwaave-clients");
const interfaces = resolve(root, process.env.FANWAAVE_INTERFACES_CHECKOUT || "external/fanwaave-interfaces");
const readJson = (base, path) => JSON.parse(readFileSync(join(base, path), "utf8"));

const binding = readJson(clients, "clients/atto/contract-binding.v1.json");
const rpc = readJson(clients, "rpc/public-projection.v1.json");
const projection = readJson(interfaces, "atto/notification-dispatch.v1.json");
const schema = readJson(interfaces, "schema/v1/notificationdispatch.json");
const { validate_atto_binding } = await import(pathToFileURL(join(clients, "scripts/atto-admission.mjs")).href);
const { validate_atto_projection } = await import(pathToFileURL(join(interfaces, "scripts/check-atto-contracts.mjs")).href);

assert.equal(validate_atto_binding(binding, rpc), true);
assert.equal(validate_atto_projection(projection, schema), true);
assert.equal(binding.contractAuthority, "fanwaave/fanwaave-interfaces");
assert.equal(projection.language, binding.language);
assert.equal(projection.admission, binding.admission);
assert.equal(binding.transportImplemented, false, "cross-repo admission must not pretend a transport exists");
assert.equal(projection.generated, false, "must not misrepresent hand-authored code as generated");
assert.equal(rpc.rules.allowAdminScope, false);
assert.equal(rpc.rules.allowAudienceWidening, false);
assert.equal(schema.properties.revision.const, "fanwaave-0002");

// Negative controls ensure the peer verifiers reject cross-boundary policy widening.
assert.throws(() => validate_atto_binding({...binding, scope:"admin"}, rpc), /rejected/);
assert.throws(() => validate_atto_projection({...projection, fields:{...projection.fields, revision:{const:"forged"}}}, schema), /rejected/);
console.log("Fanwaave exact-head AttoLang public contract admission passed");
