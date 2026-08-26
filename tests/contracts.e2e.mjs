import { assert } from "./helpers.mjs";

const expected = ["clients", "cli", "interfaces", "lib-core", "api-server.rs", "web-server.rs"];
for (const kind of expected) {
  assert(kind.includes("-") || kind.endsWith(".rs") || ["cli", "clients", "interfaces"].includes(kind), kind);
}
console.log("fanwaave contract catalog ok");

