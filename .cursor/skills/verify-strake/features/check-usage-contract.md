# check usage contract

## Sub-features

- `strake check` without a path prints check-specific usage.
- Usage documents `[--blob-root DIR]` for upcoming T8 blob checks.

## How to get to it (user POV)

Run `strake check` with no file or directory argument.

## Driving it with verify.sh

```bash
"$STRAKE_BIN" check
```

Expect exit `2` and stderr containing `[--blob-root DIR]` and `<file|dir>`.

## Gotchas

- On current `main`, `check` does not parse `.strake` files yet; only usage is verified.
- After T2+, add driving `strake check golden/...` rows to `golden-fixtures.md`.
