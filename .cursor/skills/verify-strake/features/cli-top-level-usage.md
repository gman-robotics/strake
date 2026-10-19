# CLI top-level usage

## Sub-features

- Empty argv prints usage on stderr and exits 2.
- Unknown subcommand prints the same top-level usage.

## How to get to it (user POV)

Install or build the `strake` binary, then run `strake` with no arguments (or `strake not-a-command`).

## Driving it with verify.sh

`verify.sh` runs:

```bash
"$STRAKE_BIN" 
"$STRAKE_BIN" not-a-command
```

Expect exit code `2` and stderr containing `usage: strake check | fmt | test | run | build`.

## Gotchas

- stdout must be empty for the empty-argv case on current `main`.
- Do not confuse with per-subcommand usage (`check`, `run`, etc.).
