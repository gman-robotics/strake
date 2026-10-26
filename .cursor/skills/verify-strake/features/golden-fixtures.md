# golden fixtures

## Sub-features

- `golden/*.strake` oracles for parse, fmt, check, test (T2–T4 on `main`).
- Black-box coverage also lives in `crates/strake/tests/t2_golden.rs`, `t3_golden.rs`, and `t4_golden.rs`; this feature is the **CLI smoke** slice in `verify.sh`.

## How to get to it (user POV)

From the repo root after building `strake`:

```bash
strake test golden/add2_basic.strake
strake check golden/priv.strake
strake fmt golden/add2_worked.strake
```

## Driving it with verify.sh

When `golden/` exists, `verify.sh` runs `GOLDEN_SMOKE` (exit + subcommand args) and a fmt idempotence check on `golden/add2_worked.strake`. Add rows when new goldens ship on `main` (do not enable T5-only fixtures until that code merges).

## Gotchas

- Do not commit verification edits to `golden/*.strake` in verify-skill PRs; only extend this map and `verify.sh` when files are on your branch's base.
- `fmt` prints formatted source on stdout; the smoke check diffs stdout against the committed golden bytes (does not rewrite files).
- Codeless rejections (e.g. `fuel_twice.strake`) exit `3` with plain stderr — covered in crate tests, not the minimal smoke list.
