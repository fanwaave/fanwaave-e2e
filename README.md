# fanwaave-e2e

End-to-end, integration, and contract tests for `fanwaave`. Sibling production checkouts live under `~/codes/fanwaave`.

## Staged AttoLang cross-repository admission

`.github/workflows/atto-cross-repo.yml` verifies the exact candidate commits of `fanwaave-clients` and `fanwaave-interfaces` on every relevant PR. `tests/atto-contracts.e2e.mjs` executes both repositories' public-contract verifiers together and rejects mismatched authority, admin-scope widening and forged revisions. The workflow uses only public repository checkouts pinned to the exact candidate SHAs, with no cross-organization secrets.

The separate `atto-compiler` job pins `atto-lang/atto-source.rs` to `d06f76c3c5567b66168365caed11150ed6c95a3d`, builds its native `attoc`, appends executable witnesses to the actual `.atto` modules, compiles them and executes guest WebAssembly in Node. It asserts both positive and negative response/id boundary behavior. This is a real executable subset conformance check, **not** proof of a production HTTP transport, full JSON Schema validation or general AttoLang language parity.

To run the contract test locally, set `FANWAAVE_CLIENTS_CHECKOUT` and `FANWAAVE_INTERFACES_CHECKOUT` to directories containing the candidate trees and run `node tests/atto-contracts.e2e.mjs`. For executable conformance also set `ATTOC` to a compiled pinned native Atto compiler and run `node tests/atto-execution.e2e.mjs`.

Update all source pins only after independently verifying the new PR heads.
