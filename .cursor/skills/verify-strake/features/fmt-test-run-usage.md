# fmt, test, and run usage stubs

## Sub-features

- `strake fmt` requires exactly one file path in usage.
- `strake test` accepts optional `--draft` and one file.
- `strake run` requires `--fn NAME` and one file.

## How to get to it (user POV)

Invoke each subcommand without the arguments the usage line requires.

## Driving it with verify.sh

```bash
"$STRAKE_BIN" fmt
"$STRAKE_BIN" test
"$STRAKE_BIN" run
```

Each expects exit `2` and a usage line on stderr matching `main.rs` dispatch.

## Gotchas

- `test` internally references `diag::status` for the status-guard test contract; that is not user-visible until real `test` runs.
- When parse/exec land, replace these checks with golden-driven flows.
