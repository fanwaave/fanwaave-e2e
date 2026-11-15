# fanwaave-e2e

End-to-end, integration, and contract tests for `fanwaave`. Sibling production checkouts live under `~/codes/fanwaave`.

## Staged AttoLang cross-repository admission

`.github/workflows/atto-cross-repo.yml` verifies the exact candidate commits of `fanwaave-clients` and `fanwaave-interfaces` on every relevant PR. `tests/atto-contracts.e2e.mjs` executes both repositories' public-contract verifiers together and rejects mismatched authority, admin-scope widening and forged revisions. The workflow uses only public repository checkouts, pinned to the exact SHA of the current drafts, and no cross-organization secrets.

To run the test locally, set `FANWAAVE_CLIENTS_CHECKOUT` and `FANWAAVE_INTERFACES_CHECKOUT` to directories containing those exact candidate trees, then run `node tests/atto-contracts.e2e.mjs`.

This is a **contract-admission test**, not an AttoLang compiler invocation, production HTTP transport test or end-to-end notification dispatch. Update the two pins only after independently verifying new PR heads.
