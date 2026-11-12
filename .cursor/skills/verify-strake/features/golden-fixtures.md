# golden fixtures

## Sub-features

- `golden/*.strake` oracles for parse, fmt, check, test (T2–T6 on `main`; S1–S4 lowering goldens included).
- Full matrix: `crates/strake/tests/golden_sweep.rs` runs every golden with a checked-in `golden/expect/<stem>.txt` (Phase 2 T1 harness).
- Targeted suites: `t2_golden.rs` … `t6_golden.rs`, plus S1–S5 modules (`s1_ssa_golden`, `s2_loop_golden`, `s3_match`, `s4_lists_for_forall`, `s5_join_bool`, …).
- Shared match join: `golden/match_join_shared.strake` (`strake test`, exit 0). Both arms bind `u` and `ret u` returns that join. `golden/match_join.strake` stays exit 3 when an arm does not bind the join.
- Bool loop params: `golden/loop_bool_break.strake`, `golden/loop_bool_both_arms.strake`, and `golden/loop_bool_swap.strake` (`strake test`, exit 0). A bool parameter lowers and runs like an i64 parameter, including a cross-parameter `continue`. SPEC.md L191 and LOWERING.md L6 do not limit loop parameters to i64.
- This feature is the **CLI smoke** slice in `verify.sh` (representative rows, not the full sweep).

## How to get to it (user POV)

From the repo root after building `strake`:

```bash
strake test golden/add2_basic.strake
strake check golden/priv.strake
strake fmt golden/add2_worked.strake
strake test golden/callee_77.strake
```

## Driving it with verify.sh

When `golden/` exists, `verify.sh` runs `GOLDEN_SMOKE` (exit + subcommand args) and a fmt idempotence check on `golden/add2_worked.strake`. Add smoke rows when a new **user-facing CLI** golden lands on `main`; add or bless `golden/expect/` for every CLI oracle (see `golden_sweep.rs`).

## Gotchas

- Do not commit verification edits to `golden/*.strake` in verify-skill PRs; only extend this map and `verify.sh` when files are on your branch's base.
- `fmt` prints formatted source on stdout; the smoke check diffs stdout against the committed golden bytes (does not rewrite files).
- Codeless rejections (e.g. `fuel_twice.strake`) exit `3` with plain stderr — covered in crate tests and `golden_sweep`, not the minimal smoke list.
- T5 test channel (`callee_77`, `refused`, `test_fail`, …) is on `main`; smoke includes at least one passing T5 `test` row.
