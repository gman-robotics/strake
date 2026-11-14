# golden fixtures

## Sub-features

- `golden/*.strake` oracles for parse, fmt, check, test (T2–T6 on `main`; S1–S4 lowering goldens included).
- Full matrix: `crates/strake/tests/golden_sweep.rs` runs every golden with a checked-in `golden/expect/<stem>.txt` (Phase 2 T1 harness).
- Targeted suites: `t2_golden.rs` … `t6_golden.rs`, plus S1–S5 modules (`s1_ssa_golden`, `s2_loop_golden`, `s3_match`, `s4_lists_for_forall`, `s5_join_bool`, …).
- Shared match join: `golden/match_join_shared.strake` (`strake test`, exit 0). Both arms bind `u` and `ret u` returns that join. `golden/match_join.strake` stays exit 3 when an arm does not bind the join.
- Bool loop params: `golden/loop_bool_break.strake`, `golden/loop_bool_both_arms.strake`, and `golden/loop_bool_swap.strake` (`strake test`, exit 0). A bool parameter lowers and runs like an i64 parameter, including a cross-parameter `continue`. SPEC.md L191 and LOWERING.md L6 do not limit loop parameters to i64.
- `golden/if_arm_forall.strake` (`strake test`, exit 0). The then-arm `forall` is `0..0` and `assert.eq n 0`, so exit 0 means the body saw the bound. The else arm is `trap`.
- Ordered compares: `golden/cmp_ord_i64.strake`, `golden/cmp_ord_f64.strake` (NaN and `-0.0` vs `0.0`), `golden/cmp_ord_narrow.strake` (`i32`/`f32`), and `golden/cmp_ord_f32_nan.strake` (`strake test`, exit 0). `golden/cmp_ord_forbid.strake` and `golden/cmp_ord_range.strake` are check `E_TYPE` (exit 1), not a lower abort.
- f32 arithmetic: `golden/f32_arith.strake` (add, sub, mul) and `golden/f32_div.strake` (`strake test`, exit 0). Division by zero is an infinity compare, not `E_DIV0`.
- Match binder payloads (GMA-619). One binder name keeps one payload type. `golden/match_binder_conflict.strake` is `result A B` with binder `p` on both arms (`strake check` exit 1, `E_TYPE`). `golden/match_binder_sum.strake` is the same clash on a sum (`strake check` exit 1, `E_TYPE`). `golden/match_binder_agree.strake` is `result i64 i64` with binder `p` on both arms (`strake test` exit 0, the ok arm returns 4 and the err arm returns 9).
- `use` blobs (GMA-290 T8). `golden/use_ok.strake` with `check --blob-root golden/blob` exits 0 when `golden/blob/pkg` bytes match the declared blake3. `golden/use_bad.strake` with `check --blob-root golden/blob-empty` exits 1, `E_USE` kind `check`. Files without `use` ignore `--blob-root` (smoke still includes `golden/priv.strake` without the flag).
- Checker type table (GMA-497 PR A). A repeated record name is `strake check` exit 1, `E_TYPE` (`golden/dup_record.strake`). One record still runs (`golden/record_project.strake`, `strake test` exit 0). `unit` matches only `unit` on a result side: `golden/result_unit_join.strake` is check exit 1, and `golden/result_i64_join.strake` is test exit 0 (arms return 4 and 5). An `if` assign joined with a loop exit uses the checked assign type: `golden/loop_exit_record.strake` and `golden/loop_exit_bytes.strake` are check exit 1, and `golden/loop_arm_join_true.strake` stays test exit 0. Match arms that deliver one name must agree: `golden/match_arm_records.strake` and `golden/match_arm_project.strake` are check exit 1, and `golden/match_arm_agree.strake` is test exit 0 (ok arm returns 1, err arm returns 3). `err unit unit` still fits a `result i64 unit` parameter; the unit fit is not a join of two result types.
- This feature is the **CLI smoke** slice in `verify.sh` (representative rows, not the full sweep).

## How to get to it (user POV)

From the repo root after building `strake`:

```bash
strake test golden/add2_basic.strake
strake check golden/priv.strake
strake fmt golden/add2_worked.strake
strake test golden/callee_77.strake
strake test golden/if_arm_forall.strake
```

## Driving it with verify.sh

When `golden/` exists, `verify.sh` runs `GOLDEN_SMOKE` (exit + subcommand args) and a fmt idempotence check on `golden/add2_worked.strake`. Add smoke rows when a new **user-facing CLI** golden lands on `main`; add or bless `golden/expect/` for every CLI oracle (see `golden_sweep.rs`).

## Gotchas

- Do not commit verification edits to `golden/*.strake` in verify-skill PRs; only extend this map and `verify.sh` when files are on your branch's base.
- `fmt` prints formatted source on stdout; the smoke check diffs stdout against the committed golden bytes (does not rewrite files).
- Codeless rejections (e.g. `fuel_twice.strake`) exit `3` with plain stderr — covered in crate tests and `golden_sweep`, not the minimal smoke list.
- T5 test channel (`callee_77`, `refused`, `test_fail`, …) is on `main`; smoke includes at least one passing T5 `test` row.
