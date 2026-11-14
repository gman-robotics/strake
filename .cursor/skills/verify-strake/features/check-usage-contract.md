# check usage contract

## Sub-features

- `strake check` without a path prints check-specific usage.
- Usage documents `[--blob-root DIR]`; required when the module has `use` items (blob hash / missing blob → `E_USE`).

## How to get to it (user POV)

Run `strake check` with no file or directory argument.

## Driving it with verify.sh

```bash
"$STRAKE_BIN" check
```

Expect exit `2` and stderr containing `[--blob-root DIR]` and `<file|dir>`.

## Gotchas

- Usage-without-path is what `verify.sh` asserts directly; real `check` on files is covered by `golden-fixtures.md` smoke (`priv.strake`) and `t4_golden.rs`.
- Directory `check` walks only `src/**/*.strake` under the given path (see `t4_golden.rs`).
