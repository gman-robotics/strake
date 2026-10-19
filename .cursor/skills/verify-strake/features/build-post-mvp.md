# build post-MVP stub

## Sub-features

- `strake build` is documented as post-MVP in README; MVP returns a fixed message on stdout.

## How to get to it (user POV)

Run `strake build` after building the CLI.

## Driving it with verify.sh

```bash
"$STRAKE_BIN" build
```

Expect exit `2`, stdout exactly `build is post-MVP\n`, stderr empty (no catalog JSON on stderr).

## Gotchas

- Black-box test `crates/strake/tests/cli_build.rs` mirrors this; keep verify and that test aligned.
- When real `build` ships, replace expectations in this file and in `verify.sh`.
