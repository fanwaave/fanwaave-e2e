# fanwaave-e2e

End-to-end, integration, and contract tests for `fanwaave`. Sibling production checkouts live under `~/codes/fanwaave`.

## Contract workspace admission

`npm test` always validates the six-repository contract catalog and checks
manifest discovery against isolated fixtures. To additionally verify real
sibling repositories, set `FANWAAVE_WORKSPACE_ROOT` to a directory containing
the six `fanwaave-*` checkouts:

```sh
FANWAAVE_WORKSPACE_ROOT="$HOME/codes/fanwaave" npm test
```

The opt-in check fails closed if any expected `.zpkg.toml` manifest is
missing or is not a regular file. Without the environment variable, tests
do not assume the entire organization is checked out on a single runner.
