# Rust test suite

## Sub-features

- Library unit tests (`body::diag`, `lib` CLI helpers).
- Integration tests: `check_usage_sync`, `cli_build`, `status_guard`.
- Golden CLI sweep (T1): `golden_sweep` — every `golden/*.strake` with `golden/expect/<stem>.txt` or `UNIT_ONLY`.
- Tier goldens: `t2_golden`, `t3_golden`, `t4_golden`, `t5_golden` (test channel), `t6_golden` (wrap/div0/fuel/stack/not).
- Carry-plan / lowering: `s1_ssa_golden`, `s2_carry_plan_smoke`, `s2_loop_golden`, `s3_match`, `s4_lists_for_forall`, `s5_join_bool`.
- `s5_join_bool` drives GMA-423 in process: shared match join `ret u` (exit 0), `match_join.strake` still exit 3, and bool loop `break`, both-arms join, and swap `continue`.
- `s4_lists_for_forall` also drives the GMA-534 CLI rows: `for` over a module list, `forall` binder type, binder scope, `break`/`continue` inside `for`, and an out-of-range module `i64` bind. `for_body_jump_to_exit_returns_that_value` runs a `for` through the builder and interpreter and checks the returned value. `if_arm_forall` asserts the bound (`assert.eq n 0` on `0..0`). SPEC.md "Forall span" says `LO..HI` is inclusive and `span = i128(HI) - i128(LO) + 1`, so `0..0` runs once. The same source with `assert.eq n 1` is `E_TEST` with `got` `0`. `forall_rev_check_does_not_enter_exec` runs `strake::test` in process: check `E_FORALL` leaves the exec counter at 0, and `forall_ok` then moves it by one. Nothing reads `STRAKE_EXEC_COUNTER`.

## How to get to it (user POV)

From the repo root, run `cargo test` for the strake crate (CI runs the whole workspace).

## Driving it with verify.sh

```bash
cargo test --locked -p strake
```

Expect all tests passed; log captured under evidence.

## Gotchas

- Workspace has a single member today; `cargo test --locked` at root is equivalent.
- `golden_sweep` covers more CLI cases than `verify.sh` smoke; keep smoke rows representative when adding T7+ behavior on `main`.
- New goldens should get `golden/expect/` entries (or `UNIT_ONLY`) per `golden_sweep.rs` comments — update this doc when new test modules appear.
