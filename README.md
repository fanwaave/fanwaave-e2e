# fanwaave-e2e

End-to-end, integration, and contract tests for `fanwaave`. Sibling production checkouts live under `~/codes/fanwaave`.

## Staged AttoLang cross-repository admission

`.github/workflows/atto-cross-repo.yml` verifies the exact candidate commits of `fanwaave-clients` and `fanwaave-interfaces` on every relevant PR. `tests/atto-contracts.e2e.mjs` executes both repositories' public-contract verifiers together and rejects mismatched authority, admin-scope widening and forged revisions. It uses public repository checkouts pinned to exact candidate SHAs and no cross-organization secrets.

`tests/atto-execution.e2e.mjs` is an **optional executable conformance harness**. When a trusted operator supplies the compiled native `attoc` from reviewed `atto-lang/atto-source.rs` source, it appends executable witnesses to both real `.atto` modules and checks their generated guest WebAssembly, including boundary negatives. It has **not yet been run in CI**. The first CI attempt to fetch `atto-lang/atto-source.rs` failed at checkout because that repository is private and the fanwaave-e2e `GITHUB_TOKEN` has no cross-repository access. The failed cross-org checkout was removed rather than weakening credential/security policy. A future token-free, exact-tree mirror with blob verification is the required alternative before making compilation a required gate.

To run the policy check locally, set `FANWAAVE_CLIENTS_CHECKOUT` and `FANWAAVE_INTERFACES_CHECKOUT` to local candidate trees and run `node tests/atto-contracts.e2e.mjs`. For optional executable conformance also set `ATTOC` to a compiler executable built from an independently verified source SHA, then run `node tests/atto-execution.e2e.mjs`.

These are still not proof of production HTTP transports, full JSON Schema validation, or general AttoLang language parity. Update pinned SHAs only after independently verifying new PR heads.

## Rust-first cross-repository contract tests

The required new cross-repository conformance suite is now a small Rust crate at `tests/atto-contract-rs`. It tests the exact client and interface PR candidates, compares their authored JSON contract fields, and rejects negative authority/security changes. Run it with `cargo test --manifest-path tests/atto-contract-rs/Cargo.toml --all-targets`, setting the two checkout environment variables to the verified exact candidate trees. The existing Node-based contract smoke remains until cross-runner parity is reviewed. No Python scripts are introduced. Native .atto execution remains separately blocked on access to the private upstream compiler source.
