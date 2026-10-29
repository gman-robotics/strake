# Phase 2 architecture: close the MVP

Draft for Tom. Base: strake `origin/main` `c51e91f` and `docs/plans/phase1-architecture.md` (approved 2026-10-17). Ticket: GMA-475. This plan changes no code. Piper files tickets only after Tom approves.

## 1. Goals and non-goals

**Goals.**

1. Finish the Phase 1 scope that main does not have yet: `for`, `forall`, lists, bytes, records, region, imports, `use`, draft holes, user `trap`, and `run`.
2. Make every minimum golden in Phase 1 §3 and every T7/T8 file exist and pass in CI.
3. Keep every Phase 1 lock. Phase 1 §2 defaults 1–19 stay as they are.
4. Close with one sweep test that runs the full §3 table, so "MVP done" is a test result, not a claim.

**Non-goals.** Wasm, Cranelift, LLVM, self-host, multi-file link, `grammar/strake.gbnf`, `llms.txt`, `strake fix`, WASI, `E_HEAP`, a heap cap, maps, iterators, constant folding, an ISA text format, and a real `build`. These stay deferred (SPEC "MVP", "Deferrals"; Phase 1 §5). Phase 2 does not change SPEC.md, LOWERING.md, or `errors/catalog.json` semantics (T9 may edit only the SPEC "Next" list, per Q4). It does not move the CI toolchain off Rust 1.83.0.

## 2. Current state (evidence)

Phase 1 tickets T1–T6 merged (PRs #3, #5, #7, #8, #9, #11). Later work added an SSA verifier (#13), carry-plan loops and if joins (#15), sums and `match` (#16), i32/i64 split (#17), `%`-name parse fixes (#18, #19), and the 1.83 clippy fix (#20).

**Delivered.**

- CLI `check`, `fmt`, `test`, `build` stub: `crates/strake/src/lib.rs:162`, `:188`, `:205`, `:253`; argv in `crates/strake/src/main.rs`.
- One CST, parse, fmt, codeless rejection: `crates/strake/src/body/syntax.rs` (2982 lines; `codeless_rejection` at `:322`).
- Check, including `E_SHADOW`, `E_IF_PHI`, `E_HOLE`, unknown callee `E_IMPORT`, `priv`: `crates/strake/src/body/check.rs`.
- Lower plus a carry plan for `if` / `loop` / `match` joins: `crates/strake/src/body/lower.rs`, `crates/strake/src/body/carry.rs` (`carry.rs` is new; Phase 1 §1 module list does not name it).
- Interpreter with fuel, stack, div0, wrap, sums: `crates/strake/src/body/interp/mod.rs`, `op.rs`, `ir.rs`.
- Tests: 12 files in `crates/strake/tests/`; about 250 `#[test]` items in the whole crate. CI (`.github/workflows/ci.yml`) runs fmt, clippy `-D warnings`, `cargo test --locked`, and `.cursor/skills/verify-strake/scripts/verify.sh`, all on Rust 1.83.0.

**Gaps (checked on main `c51e91f`).**

| Gap | Evidence |
|---|---|
| `run` is a stub. `strake run --fn add2 golden/add2_basic.strake` prints usage and exits 2. | `lib.rs:248` |
| Opcode card has control, arithmetic, compare and const ops, but no list/bytes/record ops and no `load`/`store`. The only heap ops are the three sum ops (`SumNew`, `SumTag`, `SumPayload`). | `interp/op.rs:7-32` |
| Unknown ops fall to a codeless fault `unimplemented op`. Existing card ops `Ne`, `Lt`, `Le`, `Gt`, `Ge`, `ConstI32`, `ConstF32`, `ConstUnit` already reach it. The normative `for` loop needs `ge` (LOWERING.md:128). | `interp/mod.rs:456`, `interp/op.rs:19-29` |
| `lower` has no `for` or `forall` lowering. `Stmt::For` / `Stmt::Forall` appear only in `stmt_span`. | `lower.rs:387-388` |
| `TrapImm::User` maps to `E_TYPE` at exec. Phase 1 §2.7 locks only the lower shape (opcode `trap`, immediate outside the seven named codes, unit check, no CLI exit), so `trap_user` needs no decision. | `interp/mod.rs:451` |
| `Opts.blob_root: Option<String>` exists and `main.rs` stores it, but `check` never reads it. No `blake3` dependency. | `lib.rs:21`, `main.rs:55-60`, `crates/strake/Cargo.toml` |
| Module-level `name = <i64 literal>` (Phase 1 §2.16) has no `Item` variant. | `syntax.rs:17-26` |
| Sums live in their own store `Machine.sums` with a `u32` tag. | `interp/mod.rs:29-44`, `:90` |
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
    BytesNew, BytesLen, BytesGet,               // LOWERING.md:223 `bytes.new`               // BytesGet -> sum, never E_OOB
    RecNew, RecGet,
    Load(ScalarTy), Store(ScalarTy),            // region only; i128 bounds; E_OOB on fault
}

// interp/mod.rs
pub(crate) enum HeapVal { List(Vec<Val>), Bytes(Vec<u8>), Rec(Vec<Val>) }  // sums stay in Machine.sums, unchanged
struct Heap { cells: Vec<HeapVal> }             // append-only; edge to smaller index only
impl Heap { fn alloc(&mut self, v: HeapVal) -> Handle { unimplemented!() } }
struct Region { bytes: Option<Vec<u8>> }        // Some only after `import region mem : region`
impl Machine {
    fn instantiate(p: &Program, host: &dyn Host) -> Result<Machine, ExecFault> { unimplemented!() } // E_IMPORT before stack, no charge
    fn load(&self, ty: ScalarTy, addr: i128) -> Result<Val, ExecFault> { unimplemented!() } // bool byte not 0/1 -> ExecFault::Halt
}
pub(crate) enum ExecFault {
    Trap { code: Code, span: Span },   // existing (interp/mod.rs:57-60)
    Codeless(String),                  // existing: trapped outcome, `status` gives exit 2
    Halt(String),                      // new: Phase 1 §2.17 stop; run_test_exports returns it; lib::test/run exit 3, plain text, before `status`
}
impl Machine {
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
fn check_forall_span(lo: i64, hi: i64) -> Result<(), Diag>;   // SPEC "Forall span": span = i128(HI) - i128(LO) + 1; E_FORALL if span <= 0 or span > i64::MAX. LO/HI are i64 literals or module-level integer bindings. Fuel traps at run time, not here.
// syntax.rs: today ForallBound::Range holds only i128 literals (syntax.rs:193-198, parse_forall :1406-1416), and an identifier becomes ForallBound::List.
// T4 changes it to: enum RangeEnd { Lit(i128), Name(String, Span) }; Range { span, ty, lo: RangeEnd, hi: RangeEnd }.
// check resolves Name to a module-level i64 binding before check_forall_span; an unknown name or a module list name keeps today's meaning.
fn check_region(module: &Module) -> Vec<Diag>;                           // E_REGION when load/store has no import
fn check_use(module: &Module, blob_root: Option<&Path>) -> Vec<Diag>;    // E_USE; blake3 of fmt output

// lib.rs
pub fn run(path: &str, fn_name: &str, opts: Opts) -> Exit;  // nullary export; stdout is the rendered value
```

`Opts.blob_root: Option<String>` already exists (`lib.rs:21`). Phase 2 reads it in `check`. The public surface does not change.

### Module map

No new module. `carry.rs` stays as the join planner. Heap and region stay in `interp/mod.rs`; if that file grows past about 1500 lines, split into `interp/heap.rs` (private to `interp`). `for` and `forall` lower through builder methods, so the Phase 1 rule "lower cannot spell `br`" holds.

### Synthesis

Two shapes were compared.

- **A. Builder methods per construct** (chosen). `for_`, `forall_range`, `forall_list` own the index loop and the phis. `lower` passes closures. This keeps the opcode enum private and reuses the loop carry plan (`lower_loop` and `carry::loop_site_by_span`, `lower.rs:604-617`).
- **B. Desugar `for`/`forall` to `loop` in the sketch, before lower.** Smaller lower, but hygienic temps then need A names in the sketch, and spans for `E_TEST` inside a `forall` would point at synthetic code. It also puts the `trap Lower` arm in a place no test can reach through the builder (Phase 1 T7 says the builder test is the acceptance).

A wins on interface depth: one call per construct, and the invariant-6 span rule lives in one place.

## 4. Risks and blast radius

**The one fact Phase 2 is safe because of:** every new construct enters through new `Op` variants and new builder methods; no existing opcode, builder method, sum store, or catalog code changes meaning. Level reached: 2 (pointed at the line). New and not-yet-run ops land in the wildcard arm at `interp/mod.rs:456`; T2/T3 fill arms there and do not edit the existing arms or `Machine.sums`. Proof to reach level 4: T1 adds a test that runs every existing golden through `cargo test` before and after each ticket, with byte-equal stderr.

| Risk | Where it breaks | Likely / cost | Check |
|---|---|---|---|
| Heap edges allow a cycle | `Heap::alloc` | low / high (E_CYCLE has no constructor) | unit: every edge index < own index |
| `forall` span overflow | `check_forall_span` | med / med | unit `LO=0, HI=i64::MAX` is `E_FORALL`; `LO=1` accepted, not run |
| Region address math wraps | `load`/`store` | med / high | i128 math; unit negative offset and end-of-page |
| Fuel order changes | `exec` charge before op | low / high | existing T6 fuel-before-div unit stays green |
| `check` starts reading `blob_root` | `lib.rs:162` | low / low | files without `use` give the same output (T1 harness) |
| Sum store changes by accident | `SumNew`/`SumTag`/`SumPayload` | low / high | `tests/s3_match.rs` stays green |
| `blake3` crate does not build on Rust 1.83 MSRV | CI | med / med | pin a version with MSRV ≤ 1.83; CI is the gate. Adding a crate dependency is not a Mac install. |
| User `trap` exec code | `interp/mod.rs:451` | low / low | no golden runs it (Phase 1 §2.7); Q1 is optional |
| Huge `pages` aborts process | region alloc | known (Phase 1 §5) | no test; documented |

**Changed, not cleared.** `fmt` changes in T4 only: one new module-level item, `name = <i64 literal>` (`syntax.rs:17-26` has no such `Item` variant). Check: every existing golden stays a fmt fixed point (T1 harness), plus a fmt fixed-point unit for the new item. `for`/`forall` already parse (`syntax.rs`).

**Cleared.** `status` precedence does not change (`tests/status_guard.rs`).

## 5. Open questions for Tom

1. **User `trap` at run time (optional).** Phase 1 §2.7 locks the lower shape only. Main maps an executed user trap to `E_TYPE`. Keep that? It does not block any ticket.
2. **`run` float output.** Phase 1 §2.19 fixes i64 (`14`) and compounds are `handle:<id>`. Print floats as hex bits (catalog `render`)? Default: yes.
3. **`blake3` dependency.** Accept one new crate dependency for `use`, pinned to an MSRV ≤ 1.83 version? Default: yes.
4. **README and SPEC "Next".** Update the "Eng is gated" line in the closing ticket, or leave docs to a separate pass?
5. **`carry.rs`.** Add it to the Phase 1 module map as the join planner, or fold it into `lower.rs`?

## 6. Tickets

Every ticket: no installs, Rust 1.83.0, CI green, one high-effort review (cap 2).

### T1. Golden regression harness
**Scope.** A test that runs each listed golden with its command and compares exit and stderr bytes to a checked-in expectation. A golden with no CLI oracle (for example `trap_user`, Phase 1 §2.7) is listed in a unit-only list in `golden_sweep.rs` and has no expectation file. **Files.** `crates/strake/tests/golden_sweep.rs`, `golden/expect/*.txt`. **Done.** All 101 current files pass; deleting one expectation fails the test; a golden with no expectation and no unit-only entry fails the test. **Rule for T2–T8.** Each ticket adds an expectation file for every golden it adds that has a CLI command, and adds unit-only goldens to the unit-only list instead. **Size** S. **Depends** none.

### T2. Heap ops: lists, bytes, records
**Scope.** New `Op` variants, `Heap`, builder methods, check types. **Files.** `interp/op.rs`, `interp/mod.rs`, `interp/ir.rs`, `check.rs`, `lower.rs`, `golden/bytes_oob.strake`. **Done.** `bytes_oob` exits 0; unit: out-of-range `list.get` is tag 1; unit: in-range `bytes.get` is tag 0 with an i64 in 0..255 (SPEC "Bytes"); unit: `RecNew` then `RecGet` returns each field; unit: no backward edge. **Size** L. **Depends** T1.

### T3. `for` lowering
**Scope.** `Builder::for_`, normative index loop, `trap Lower` arm. **Files.** `ir.rs`, `lower.rs`, `interp/mod.rs` (exec arms for `ne lt le gt ge`, `const.i32`, `const.f32`, `const.unit`), `golden/for_list.strake`. **Done.** §3 row 5; builder unit runs the `trap Lower` arm; unit: each existing card op executes. **Size** M. **Depends** T2.

### T4. `forall`
**Scope.** Range and module-list forms, `E_FORALL`, exit-3 bad form, and module-level `name = <i64 literal>` items (Phase 1 §2.16; new `Item` variant in `syntax.rs`, fmt fixed point), and `ForallBound::Range` ends that can name such a binding (`RangeEnd`, see sketch). **Files.** `syntax.rs`, `check.rs`, `ir.rs`, `lower.rs`, `golden/forall_rev.strake`, `golden/forall_ok.strake`, `golden/forall_list_bad.strake`. **Done.** §3 row 6; `forall_list_bad` exits 3, no `E_FORALL`; unit: `name = <i64 literal>` parses and is a fmt fixed point; unit: `parse_forall` gives `Range` with `RangeEnd::Name` when `LO`/`HI` is an identifier followed by `..`; unit: a `forall` whose `LO`/`HI` name such a binding checks like the literal; unit: `LO=0, HI=i64::MAX` is `E_FORALL` and `LO=1, HI=i64::MAX` is accepted and not run. **Size** M. **Depends** T3.

### T5. Region, `load`/`store`, imports
**Scope.** `Region`, `instantiate`, `E_REGION`, `E_OOB`, `E_IMPORT` trap, page precedence (§2.2), zero fill (§2.17), `load` type check. **Files.** `interp/*`, `lower.rs` (lower `Rhs::Load` / `Rhs::Store`, now the unsupported arm at `lower.rs:1244` and the property lowerer at `:1168`; apply `Item::Pages` and `ImportSig::Region`, which `lower.rs:196-208` skips today), `lib.rs` (map `ExecFault::Halt` to exit 3 before `status`, `lib.rs:232-234`), `check.rs`, `golden/region_oob.strake`, `region_miss`, `region_unaligned`, `import_miss`, `load_type`. **Done.** Phase 1 T8 rows for those five files. Unit through `lib::test`: a bool `load` of a byte other than 0 or 1 gives exit 3, plain text, no catalog code, `status` not called (§2.17). Unit: page count follows §2.2 (directive, else import count, else 16). **Size** L. **Depends** T2.

### T6. Draft holes and user `trap`
**Scope.** `test --draft` runs to `E_HOLE` trap; user `trap` lowers to opcode `trap` per Phase 1 §2.7. **Files.** `lower.rs`, `interp/mod.rs`, `golden/hole_draft.strake`, `golden/trap_user.strake`. **Done.** §3 row 10 draft; unit lower check for `trap_user`; `trap_user` is on the T1 unit-only list, with no expectation file. **Size** S. **Depends** T1.

### T7. `use` and `--blob-root`
**Scope.** Read the existing `Opts.blob_root` in `check_use`; `blake3`. **Files.** `lib.rs`, `main.rs`, `check.rs`, `Cargo.toml`, `Cargo.lock`, `golden/use_ok.strake`, `golden/use_bad.strake`. **Done.** Phase 1 T8 `use` rows. **Size** M. **Depends** T1, Q3.

### T8. `run`
**Scope.** Implement `lib::run`. **Files.** `lib.rs`, `golden/run_const.strake`, verify-strake feature doc. **Done.** `strake run --fn answer golden/run_const.strake` prints `14`, exits 0; non-nullary export is a usage error; unit through `lib::run`: an `ExecFault::Halt` (bad bool byte, §2.17) gives exit 3, plain text, and `pub fn run` still does not reference `status` (`tests/status_guard.rs:174-179`). Float rendering follows Q2 but does not block `run_const`. **Size** S. **Depends** T1, T5.

### T9. MVP sweep and docs
**Scope.** Confirm the T1 sweep covers the full Phase 1 §3 table; add verify-strake features; README/SPEC per Q4. **Files.** `tests/golden_sweep.rs`, `.cursor/skills/verify-strake/**`, `README.md`, `SPEC.md` "Next" only. **Done.** Every §3 command passes in CI; no `unimplemented op` reachable from any golden. **Size** M. **Depends** T2–T8.

Order: run T1 to T9 in number order, one at a time. The dependency graph allows more parallel work, but T3, T4, T5 and T6 all edit `lower.rs` and `interp/mod.rs`, so number order is the schedule. T8 depends on T5 for the `Halt` path.
