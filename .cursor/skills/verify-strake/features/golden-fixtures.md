# golden fixtures (extend when present)

## Sub-features

- Planned: `golden/*.strake` oracles for parse, check, fmt, test, run (see `docs/plans/phase1-architecture.md` minimum goldens).
- Not on `main` until T2+ lands; `verify.sh` skips this block if `golden/` is missing.

## How to get to it (user POV)

After fixtures exist at the repository root, run e.g. `strake fmt golden/fuel_twice.strake` per the architecture doc.

## Driving it with verify.sh

When `golden/` exists, `verify.sh` runs the smoke list in `GOLDEN_SMOKE` inside the script (start with fmt idempotence on any `golden/*.strake` once T2 adds files). Add one row per architecture table entry as features become implemented.

Example future row (do not enable until file exists):

```bash
strake fmt golden/fuel_twice.strake   # exit 0, stdout equals file bytes
```

## Gotchas

- Do not commit verification edits to `golden/*.strake` in the verify-skill PR; only extend this map and `verify.sh` when those files are on your branch's base.
- PR #5 (`tginter_ca/parse-fmt-b34d`) adds parse/fmt goldens; extend `GOLDEN_SMOKE` after merge without touching stubbed `crates/strake/src/**` in the verify PR.
