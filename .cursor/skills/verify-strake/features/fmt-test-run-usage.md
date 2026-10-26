# fmt, test, and run usage (and golden smoke)

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

- `verify.sh` still asserts **usage-only** for bare `fmt`, `test`, and `run` (exit `2`). Real `fmt`/`test` on goldens are in [golden-fixtures](golden-fixtures.md).
- `run` remains a usage stub on `main` (`lib.rs`); do not add golden `run` rows until execution ships.
