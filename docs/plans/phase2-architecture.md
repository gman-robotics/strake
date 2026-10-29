# Phase 2 architecture: close the MVP

Draft for Tom. Base: strake `origin/main` `c51e91f` and `docs/plans/phase1-architecture.md` (approved 2026-10-17). Ticket: GMA-475. This plan changes no code. Piper files tickets only after Tom approves.

## 1. Goals and non-goals

**Goals.**

1. Finish the Phase 1 scope that main does not have yet: `for`, `forall`, lists, bytes, records, region, imports, `use`, draft holes, user `trap`, and `run`.
2. Make every minimum golden in Phase 1 §3 and every T7/T8 file exist and pass in CI.
3. Keep every Phase 1 lock. Phase 1 §2 defaults 1–19 stay as they are.
4. Close with one sweep test that runs the full §3 table, so "MVP done" is a test result, not a claim.

**Non-goals.** Wasm, Cranelift, LLVM, self-host, multi-file link, `grammar/strake.gbnf`, `llms.txt`, `strake fix`, WASI, `E_HEAP`, a heap cap, maps, iterators, constant folding, an ISA text format, and a real `build`. These stay deferred (SPEC "MVP", "Deferrals"; Phase 1 §5). Phase 2 does not change SPEC.md, LOWERING.md, or `errors/catalog.json` semantics. It does not move the CI toolchain off Rust 1.83.0.

## 2. Current state (evidence)

Phase 1 tickets T1–T6 merged (PRs #3, #5, #7, #8, #9, #11). Later work added an SSA verifier (#13), carry-plan loops and if joins (#15), sums and `match` (#16), i32/i64 split (#17), `%`-name parse fixes (#18, #19), and the 1.83 clippy fix (#20).

**Delivered.**

- CLI `check`, `fmt`, `test`, `build` stub: `crates/strake/src/lib.rs:162`, `:188`, `:205`, `:253`; argv in `crates/strake/src/main.rs`.
- One CST, parse, fmt, codeless rejection: `crates/strake/src/body/syntax.rs` (2982 lines; `codeless_rejection` at `:322`).
- Check, including `E_SHADOW`, `E_IF_PHI`, `E_HOLE`, unknown callee `E_IMPORT`, `priv`: `crates/strake/src/body/check.rs`.
- Lower plus a carry plan for `if` / `loop` / `match` joins: `crates/strake/src/body/lower.rs`, `crates/strake/src/body/carry.rs` (`carry.rs` is new; Phase 1 §1 module list does not name it).
- Interpreter with fuel, stack, div0, wrap, sums: `crates/strake/src/body/interp/mod.rs`, `op.rs`, `ir.rs`.
- Tests: 14 files in `crates/strake/tests/`, about 250 `#[test]` items. CI (`.github/workflows/ci.yml`) runs fmt, clippy `-D warnings`, `cargo test --locked`, and `.cursor/skills/verify-strake/scripts/verify.sh`, all on Rust 1.83.0.

**Gaps (checked on main `c51e91f`).**

| Gap | Evidence |
|---|---|
| `run` is a stub. `strake run --fn add2 golden/add2_basic.strake` prints usage and exits 2. | `lib.rs:248` |
| Opcode card has no heap ops and no `load`/`store`. Only sum ops exist (`SumNew`, `SumTag`, `SumPayload`). | `interp/op.rs` |
| Unknown ops fall to a codeless fault `unimplemented op`. | `interp/mod.rs:455-460` |
| `lower` has no `for` or `forall` lowering. `Stmt::For` / `Stmt::Forall` appear only in `stmt_span`. | `lower.rs:387-388` |
| `TrapImm::User` maps to `E_TYPE`. Phase 1 §2.7 says a user `trap` cannot carry a catalog fault code; mapping it to `E_TYPE` needs a decision. | `interp/mod.rs:451` |
| `--blob-root` parses but `use` is not checked against blobs. No `blake3` dependency. | `main.rs:55`, `crates/strake/Cargo.toml` |
| 15 required goldens do not exist: `for_list`, `forall_rev`, `forall_ok`, `forall_list_bad`, `hole_draft`, `region_oob`, `region_miss`, `region_unaligned`, `import_miss`, `use_ok`, `use_bad`, `bytes_oob`, `load_type`, `trap_user`, `run_const`. | `golden/` (101 files) |
| README still says "Eng is gated", and SPEC "Next" still lists the re-review as step 1. | `README.md`, `SPEC.md:427-431` |

So Phase 1 T7 is partly done (loop, match), and T8 is not started.

## 3. Design

### Usage sketch (caller first)

```
strake test golden/for_list.strake            # exit 0, no E_OOB
strake test golden/forall_rev.strake          # exit 1, E_FORALL check, exec not entered
strake test --draft golden/hole_draft.strake  # exit 2, E_HOLE trap
strake test golden/region_oob.strake          # exit 2, E_OOB trap
strake check --blob-root DIR golden/use_ok.strake   # exit 0
strake run --fn answer golden/run_const.strake      # stdout "14", exit 0
```

The public surface does not change: five functions returning `Exit`, plus `Opts`.

### Type and signature sketch

Bodies are `unimplemented!()` until each ticket fills them. Names are proposals.

```rust
// interp/op.rs  (private; closes the LOWERING "Opcode sketch" card)
pub(crate) enum Op {
    /* existing */ ..,
    ListEmpty, ListCons, ListLen, ListGet,      // ListGet -> sum (tag 0 ok, tag 1 err unit)
    BytesLit, BytesLen, BytesGet,               // BytesGet -> sum, never E_OOB
    RecNew, RecGet,
    Load(ScalarTy), Store(ScalarTy),            // region only; i128 bounds; E_OOB on fault
}

// interp/mod.rs
pub(crate) enum HeapVal { List(Vec<Val>), Bytes(Vec<u8>), Rec(Vec<Val>), Sum { tag: u8, payload: Val } }
struct Heap { cells: Vec<HeapVal> }             // append-only; edge to smaller index only
impl Heap { fn alloc(&mut self, v: HeapVal) -> Handle { unimplemented!() } }
struct Region { bytes: Option<Vec<u8>> }        // Some only after `import region mem : region`
impl Machine {
    fn instantiate(p: &Program, host: &dyn Host) -> Result<Machine, ExecFault> { unimplemented!() } // E_IMPORT before stack, no charge
    fn load(&self, ty: ScalarTy, addr: i128) -> Result<Val, ExecFault> { unimplemented!() }
    fn store(&mut self, ty: ScalarTy, addr: i128, v: Val) -> Result<(), ExecFault> { unimplemented!() }
}

// interp/ir.rs  (Builder: lower still cannot name Op or br)
impl Builder {
    fn for_(&mut self, xs: VReg, span: Span, body: impl FnOnce(&mut Self, VReg)) -> Result<(), LowerError>;
    fn forall_range(&mut self, lo: i64, hi: i64, span: Span, body: impl FnOnce(&mut Self, VReg)) -> Result<(), LowerError>;
    fn forall_list(&mut self, xs: VReg, span: Span, body: impl FnOnce(&mut Self, VReg)) -> Result<(), LowerError>;
    fn list_get(&mut self, xs: VReg, i: VReg, span: Span) -> VReg;  // and the other heap ops
    fn load(&mut self, ty: ScalarTy, at: VReg, span: Span) -> VReg;
    fn store(&mut self, ty: ScalarTy, at: VReg, v: VReg, span: Span);
}

// check.rs
fn check_forall_span(lo: i64, hi: i64, fuel: u64) -> Result<(), Diag>;   // E_FORALL; i128 math
fn check_region(module: &Module) -> Vec<Diag>;                           // E_REGION when load/store has no import
fn check_use(module: &Module, blob_root: Option<&Path>) -> Vec<Diag>;    // E_USE; blake3 of fmt output

// lib.rs
pub fn run(path: &str, fn_name: &str, opts: Opts) -> Exit;  // nullary export; stdout is the rendered value
```

`Opts` gains `blob_root: Option<PathBuf>`. That is an additive field on a public struct. It is the only public change.

### Module map

No new module. `carry.rs` stays as the join planner. Heap and region stay in `interp/mod.rs`; if that file grows past about 1500 lines, split into `interp/heap.rs` (private to `interp`). `for` and `forall` lower through builder methods, so the Phase 1 rule "lower cannot spell `br`" holds.

### Synthesis

Two shapes were compared.

- **A. Builder methods per construct** (chosen). `for_`, `forall_range`, `forall_list` own the index loop and the phis. `lower` passes closures. This keeps the opcode enum private and reuses the `loop_` carry plan.
- **B. Desugar `for`/`forall` to `loop` in the sketch, before lower.** Smaller lower, but hygienic temps then need A names in the sketch, and spans for `E_TEST` inside a `forall` would point at synthetic code. It also puts the `trap Lower` arm in a place no test can reach through the builder (Phase 1 T7 says the builder test is the acceptance).

A wins on interface depth: one call per construct, and the invariant-6 span rule lives in one place.

## 4. Risks and blast radius

**The one fact Phase 2 is safe because of:** every new construct enters through new `Op` variants and new builder methods; no existing opcode, builder method, or catalog code changes meaning. Level reached: 2 (pointed at the line). The existing ops are a closed match in `interp/mod.rs`, and the fallback arm at `:455` is the only place a new op lands today. Proof to reach level 4: T1 adds a test that runs every existing golden through `cargo test` before and after each ticket, with byte-equal stderr.

| Risk | Where it breaks | Likely / cost | Check |
|---|---|---|---|
| Heap edges allow a cycle | `Heap::alloc` | low / high (E_CYCLE has no constructor) | unit: every edge index < own index |
| `forall` span overflow | `check_forall_span` | med / med | unit `LO=0, HI=i64::MAX` is `E_FORALL`; `LO=1` accepted, not run |
| Region address math wraps | `load`/`store` | med / high | i128 math; unit negative offset and end-of-page |
| Fuel order changes | `exec` charge before op | low / high | existing T6 fuel-before-div unit stays green |
| New `Opts` field breaks callers | `main.rs`, tests | high / low | compile; `check_usage_sync.rs` |
| `blake3` crate does not build on Rust 1.83 MSRV | CI | med / med | pin a version with MSRV ≤ 1.83; CI is the gate. Adding a crate dependency is not a Mac install. |
| User `trap` code changes | `interp/mod.rs:451` | low / med | needs Tom's answer (Q1) before T6 |
| Huge `pages` aborts process | region alloc | known (Phase 1 §5) | no test; documented |

**Cleared.** `fmt` does not change (no new syntax is planned; `for`/`forall` already parse, `syntax.rs`). `status` precedence does not change (`tests/status_guard.rs`).

## 5. Open questions for Tom

1. **User `trap` code.** Main maps `TrapImm::User` to `E_TYPE`. Phase 1 §2.7 names no code for it. Keep `E_TYPE`, or make it codeless exit 2?
2. **`run` output.** Phase 1 §2.19 fixes the i64 case (`14`). Render other return types with the catalog `render` rule (floats as hex bits, compounds as `handle:<id>`)? Default: yes.
3. **`blake3` dependency.** Accept one new crate dependency for `use`, pinned to an MSRV ≤ 1.83 version? Default: yes.
4. **README and SPEC "Next".** Update the "Eng is gated" line in the closing ticket, or leave docs to a separate pass?
5. **`carry.rs`.** Add it to the Phase 1 module map as the join planner, or fold it into `lower.rs`?

## 6. Tickets

Every ticket: no installs, Rust 1.83.0, CI green, one high-effort review (cap 2).

### T1. Golden regression harness
**Scope.** A test that runs every `golden/*.strake` with its recorded command and compares exit and stderr bytes to a checked-in expectation. **Files.** `crates/strake/tests/golden_sweep.rs`, `golden/expect/*.txt`. **Done.** All 101 current files pass; deleting one expectation fails the test. **Size** S. **Depends** none.

### T2. Heap ops: lists, bytes, records
**Scope.** New `Op` variants, `Heap`, builder methods, check types. **Files.** `interp/op.rs`, `interp/mod.rs`, `interp/ir.rs`, `check.rs`, `lower.rs`, `golden/bytes_oob.strake`. **Done.** `bytes_oob` exits 0; unit: out-of-range `list.get` is tag 1; unit: no backward edge. **Size** L. **Depends** T1.

### T3. `for` lowering
**Scope.** `Builder::for_`, normative index loop, `trap Lower` arm. **Files.** `ir.rs`, `lower.rs`, `golden/for_list.strake`. **Done.** §3 row 5; builder unit runs the `trap Lower` arm. **Size** M. **Depends** T2.

### T4. `forall`
**Scope.** Range and module-list forms, `E_FORALL`, exit-3 bad form. **Files.** `check.rs`, `ir.rs`, `lower.rs`, `golden/forall_rev.strake`, `golden/forall_ok.strake`, `golden/forall_list_bad.strake`. **Done.** §3 row 6; `forall_list_bad` exits 3, no `E_FORALL`; i64 span units. **Size** M. **Depends** T3.

### T5. Region, `load`/`store`, imports
**Scope.** `Region`, `instantiate`, `E_REGION`, `E_OOB`, `E_IMPORT` trap, page precedence (§2.2), zero fill (§2.17), `load` type check. **Files.** `interp/*`, `check.rs`, `golden/region_oob.strake`, `region_miss`, `region_unaligned`, `import_miss`, `load_type`. **Done.** Phase 1 T8 rows for those five files. **Size** L. **Depends** T2.

### T6. Draft holes and user `trap`
**Scope.** `test --draft` runs to `E_HOLE` trap; user `trap` lowering per Q1. **Files.** `lower.rs`, `interp/mod.rs`, `golden/hole_draft.strake`, `golden/trap_user.strake`. **Done.** §3 row 10 draft; unit lower check for `trap_user`. **Size** S. **Depends** T1, Q1.

### T7. `use` and `--blob-root`
**Scope.** `Opts.blob_root`, `check_use`, `blake3`. **Files.** `lib.rs`, `main.rs`, `check.rs`, `Cargo.toml`, `Cargo.lock`, `golden/use_ok.strake`, `golden/use_bad.strake`. **Done.** Phase 1 T8 `use` rows. **Size** M. **Depends** T1, Q3.

### T8. `run`
**Scope.** Implement `lib::run`. **Files.** `lib.rs`, `golden/run_const.strake`, verify-strake feature doc. **Done.** `strake run --fn answer golden/run_const.strake` prints `14`, exits 0; non-nullary export is a usage error. **Size** S. **Depends** T1, Q2.

### T9. MVP sweep and docs
**Scope.** Extend T1 to the full Phase 1 §3 table; add verify-strake features; README/SPEC per Q4. **Files.** `tests/golden_sweep.rs`, `.cursor/skills/verify-strake/**`, `README.md`, `SPEC.md` "Next" only. **Done.** Every §3 command passes in CI; no `unimplemented op` reachable from any golden. **Size** M. **Depends** T2–T8.

Order: T1 → T2 → (T3 → T4) ∥ T5; T6, T7, T8 after T1 in any order; T9 last. With one laptop lane, run them in number order.
