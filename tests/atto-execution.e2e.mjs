// Exact-source AttoLang compiler conformance: compile both published .atto
// policy modules with a tiny executable witness, run guest Wasm and assert 42.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";

const root = process.cwd();
const clients = resolve(root, process.env.FANWAAVE_CLIENTS_CHECKOUT || "../external/fanwaave-clients");
const interfaces = resolve(root, process.env.FANWAAVE_INTERFACES_CHECKOUT || "../external/fanwaave-interfaces");
const attoc = resolve(root, process.env.ATTOC || "../external/atto-source/target/release/attoc");
const temp = mkdtempSync(join(tmpdir(), "fanwaave-atto-conformance-"));

const cases = [
  {
    name: "health-client",
    path: join(clients, "clients/atto/health.atto"),
    main: `pub fnc main(): int {
      if valid_health_status(200) then
        if accept_health_response(200, 1024, 1024) then
          if accept_health_response(401, 1, 1024) then
            return 1;
          fi
          if accept_health_response(200, 1025, 1024) then
            return 2;
          fi
          if accept_health_response(200, -1, 1024) then
            return 3;
          fi
          return 42;
        fi
      fi
      return 0;
    }`,
  },
  {
    name: "notification-interfaces",
    path: join(interfaces, "atto/notification-dispatch.atto"),
    main: `pub fnc main(): int {
      if valid_dispatch_id_length(8) then
        if valid_dispatch_id_length(512) then
          if valid_dispatch_id_length(7) then
            return 1;
          fi
          if valid_dispatch_id_length(513) then
            return 2;
          fi
          if valid_dispatch_shape(10, true, true) then
            if valid_dispatch_shape(10, false, true) then
              return 3;
            fi
            if valid_dispatch_shape(10, true, false) then
              return 4;
            fi
            return 42;
          fi
        fi
      fi
      return 0;
    }`,
  },
];

try {
  for (const entry of cases) {
    const input = join(temp, `${entry.name}.atto`);
    const output = join(temp, `${entry.name}.wasm`);
    writeFileSync(input, readFileSync(entry.path, "utf8") + "\n" + entry.main + "\n");
    const compile = spawnSync(attoc, [input, "-o", output], { encoding: "utf8" });
    assert.equal(compile.status, 0, `${entry.name}: Atto compilation failed: ${compile.stderr}`);
    const wasm = readFileSync(output);
    assert.equal(WebAssembly.validate(wasm), true, `${entry.name}: invalid guest Wasm`);
    const module = new WebAssembly.Module(wasm);
    assert.deepEqual(WebAssembly.Module.imports(module), [], `${entry.name}: unexpected host imports`);
    const instance = new WebAssembly.Instance(module);
    assert.equal(instance.exports.main(), 42n, `${entry.name}: guest predicate boundary regression`);
    console.log(`PASS ${entry.name}: native attoc -> guest WebAssembly -> main() 42`);
  }
} finally {
  rmSync(temp, { recursive: true, force: true });
}
