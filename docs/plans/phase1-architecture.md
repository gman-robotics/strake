# Phase 1 MVP architecture

Plan only. No build until Tom approves this pass. Source of truth is docs tip `6341a16`: SPEC.md, LOWERING.md, README.md, `errors/catalog.json`. Fog #1–#6, two-layer A1–A8, P1–P13, L1–L15, R1–R4, and D1–D7 stay locked. **Strake-A** and **strake-1** are provisional labels (SPEC "Why two layers", LOWERING header, D6). Roles stay: agents write A (`.strake`); backends eat the SSA ISA; the reference interpreter runs that ISA.

MVP is parse, check, and fmt of A; lower to the ISA; interpret it; run `golden/`; JSON diagnostics from the catalog (SPEC "MVP"). CLI is `check` (including `--fn`), `fmt`, `run`, and `test`. `build` exits 2 with `build is post-MVP` and no catalog code (SPEC "MVP", "Modules and CLI"; README "Locks").

## Synthesis

Base shape: one crate, five public functions, opcode enum private to the interpreter. The 2026-10-12 r1 draft was input, not the base. This revision follows the PR #2 challenge at `dba6d23`: the assert sidecar is the locked reading; three catalog codes are no longer used for a different fault; the ten minimum goldens name their files and exits.

Rejected shapes: a public opcode module (SPEC "Banned"); an evaluator of A beside `interp(lower(A))`; a multi-crate workspace (SPEC "Layout" is one crate); topic modules that split a stage's type.

## 1. Architecture

### Module layout

```
crates/strake/
  Cargo.toml
  src/lib.rs                pub check, fmt, test, run, build
  src/main.rs               argv only
  src/body/mod.rs           compile sequences the stages
  src/body/diag.rs          catalog kernel
  src/body/syntax.rs        CST, parse, fmt; imports diag only
  src/body/check.rs         binding table and types; may name the CST
  src/body/lower.rs         desugar into builder calls
  src/body/host.rs          EmptyHost, crate-private
  src/body/interp/mod.rs    Machine, heap, fuel, frames, exec
  src/body/interp/op.rs     private opcode enum
  src/body/interp/ir.rs     Builder and Program
```

The repo root is a Cargo workspace whose only member is `crates/strake/` (SPEC "Layout"). Provisional label strings live in one private const.

```
strake check [--draft] [--fn NAME] <file|dir>
strake fmt <file>
strake test [--draft] <file>
strake run --fn NAME [--draft] <file>
strake build
```

The five functions return `Exit { code, stdout, stderr }`. `Opts` carries `draft` and, on `check`, `only_fn`. That is the public surface. `check <dir>` loads each `src/**/*.strake` as its own module and does not link them (SPEC "Modules and CLI", R1). `--fn` keeps that export plus same-file tests whose direct calls are only to it (SPEC "Incremental"). A test that reaches another export or a `priv fn` only through a helper is Q15.

### Dependency Rule

`body::compile` sequences the stages. Source dependencies:

```
main, host                         outer circle
syntax → diag
check  → syntax (the CST), diag
lower  → ir builder
ir     → interp (Op private to interp)
diag + errors/catalog.json         inner kernel
```

`syntax` names only `diag`. `check` may name the CST and does not name `lower`. `compile` passes the CST into `check` and, when check is clean, passes the sketch into `lower`. `lower` calls builder methods and does not see `Op`. The builder fills `Program` inside `interp`. `exec` takes `&Program` and a host. `diag` names nobody in the pipeline. `main` calls the five functions only. A host enters through `Machine::call` and does not name the CST, the sketch, or an opcode.

Any check diagnostic returns before lower (SPEC "Holes"; minimum golden 3 does not run). A clean `check` lowers and drops the `Program` (LOWERING "What lowering is not"). Fmt reprints the CST and does not check, so an `E_SHADOW` file stays illegal and still has a format fixed point (SPEC "Control", LOWERING "L6").

### Information hiding

| Seam | Owner | Boundary |
|---|---|---|
| CST | `syntax`, visible to `check` and `compile` | `exec` does not take it. `syntax` does not name `check`. |
| Binding table | `check` | One L6 table. `compile` moves the sketch into `lower`. |
| Opcodes | `interp/op.rs` | `lower` has no opcode name. It calls `if_`, `loop_`, and `match_`. |
| `Program` fields | `interp/ir.rs` | Callers see `exports()`, the same records `exec` uses. |
| `Diag` | `diag` | Span bytes and rendered `want`/`got`. No vreg field. |
| Host | `host.rs` | Not on the crate API. The CLI uses `EmptyHost`. |

`br`, `br_if`, `br_table`, and `phi` are private inside `if_`, `loop_`, and `match_`. There is no `emit_br`. Join values are the only names that leave an arm (lowerer invariant 1). `finish` derives predecessors from terminators (invariant 2). One heap, one `status`, one fault-to-code map. A runs only as `interp(lower(A))`.

### Data model

The sketch is structured A after checking: binds, `if`, `loop`, `for`, `match`, `assert.eq`, `hole`, `ret`, calls, prelude ops, with source names and spans. `emit` numbers vregs in domination order. Parameters are `%0..%k-1` (LOWERING "Names → SSA").

`Program` is functions of blocks of `Inst { op, dst, span }`. The opcode enum is the closed card (LOWERING "Opcode sketch", L13): `br br_if br_table call ret phi trap`; `add sub mul div eq ne lt le gt ge`; the value-heap ops; `load store`; `const.i32 const.i64 const.f32 const.f64 const.bool const.unit`. One `add`, type on the operand. `br_if` is the two-target form (LOWERING "if").

**Sidecar, locked.** `diag` is not an opcode. L13 desugars `assert.eq` before emit. LOWERING "Calls, tests, properties" gives the `test.name` export a diag sidecar. SPEC "Tests" is the effect: `E_TEST` with `want`/`got` and `ret 1`. The `diag` line in the desugar listing is that effect. The mismatch `ret` is `TestFail { want, got }`, both vregs, read when the `ret` runs. An explicit `ret 1` is a value return: CLI 1, not `E_TEST`. `ret 77` in the test or property body is `Refused`. A callee that returns i64 77 is an ordinary value.

The lowerer plants `trap` immediates `Hole` (draft hole) and `Lower` (`for` desugar `Err` arm). `E_FUEL`, `E_STACK`, `E_DIV0`, `E_OOB`, and instantiate-time `E_IMPORT` are `exec` faults, so a `trap` immediate is not a second way to raise them (SPEC "Fuel"). A user `trap` exists (A7, SPEC "Prelude"). Which immediates it may carry is Q7.

The value heap is one append-only `Vec`. Handles are indices. Edges point at a strictly smaller index, so a cycle cannot be built. `E_CYCLE` has no constructor (catalog; SPEC "Value heap"). v0 does not reclaim. Host OOM aborts the process. `list.get` and `bytes.get` out of range return `Err` (tag 1, `unit` payload) and do not raise `E_OOB`. `ok` is tag 0. Scalars are immediates. Floats are stored as bits. Region bytes are a separate `Option<Vec<u8>>`, present only after `import region mem : region`. Lowering does not allocate a region for lists (invariant 4).

`not` on `bool` is sugar for `eq` against `false` (SPEC "Prelude"). `for` lowers as the normative index loop (LOWERING "for x in xs"). `loop` / `break` / `continue` lower as one header, `br exit`, and `br header` (LOWERING "loop / break / continue", L7). No constant folding (L15).

Call and `assert.eq` literals take the parameter or operand type. `call add2(3, 4)` is legal on that rule (SPEC "ANF", the worked example, SPEC "Banned" coercion). That is not a question.

### Invariant 0 and the lowerer invariants

Meaning is `interp(lower(A))` (SPEC "Why two layers"; LOWERING invariant 0). `test` and `run` call `exec` on `compile`'s `Program` only. The minimum list is the table in §3. Structural checks sit inside `interp`, which can see opcodes. The CLI prints no vreg.

| Inv | Where it lives | Check |
|---|---|---|
| 0 | `test` / `run` only | §3 commands |
| 1 joins are phis | `if_` / `loop_` / `match_` | `golden/if_join.strake`; unit out-of-arm name fails in `finish` |
| 2 structured jumps | no `emit_br`; `finish` checks targets | unit walk of every terminator |
| 3 list/bytes OOB is `Err` | result sum | `golden/for_list.strake` has no `E_OOB` |
| 4 region | missing import is check `E_REGION`; OOB is `E_OOB` from `load`/`store`, i128 | `golden/region_miss.strake`, `golden/region_oob.strake` |
| 5 golden answers | §3 table | each file's command, exit, and diag |
| 6 A names | span bytes; `Diag` has no vreg | `E_SHADOW` and `E_TEST` stderr contain the A name and no `%` digit |

A checked sketch that cannot lower, or a `finish` that rejects a `Program`, is a bug. It exits **70**, plain text, no catalog code, and does not call `status`. It is not exit 2. The number is Q18.

### Diagnostic spans

Each instruction carries the byte span of the A construct that emitted it. Hygienic temps carry the `for` or `forall` span (invariant 6). Faults and `E_TEST` copy that span. `msg` is the catalog string; detail is `want`, `got`, and optional `patch` (SPEC "Agent-native mechanics"). Render: scalars as literals, floats as hex bit patterns, compounds as `handle:<id>`, import payloads not walked (catalog `render`). The JSON writer has no `%` conversion. The span object is Q8. One diagnostic is one JSON object on one line. Traces stay inside the capabilities the module imported (SPEC "Capability-scoped traces").

`Code` comes from the catalog's non-reserved codes. `E_HOLE` and `E_IMPORT` each leave as one kind string (catalog `kind_note`). An include test fails if a message drifts, if `E_CYCLE` becomes constructable, or if an emitted `kind` is an array.

### Exit codes

`diag::status` is the test channel only. Its comment is the catalog sentence (catalog `cli.precedence`, SPEC "Tests", D4):

```
any trap -> 2; else any 77 -> 77; else any nonzero test -> 1; else 0
```

`status` folds test and property exports. A trap is never rewritten to `E_TEST` or 77. An explicit `ret 2` in a test becomes 1. `build` does not call `status`. Unit rows: trap with 77 → 2; 77 with a nonzero test → 77; nonzero → 1; all zero → 0.

`run` does not call `status`. A normal return prints one scalar (`handle:<id>` for a compound) and exits 0, including a return of i64 77 or 2. A trap on `run` exits 2 with that trap code. When check fails first, `test` and `run` return the check exit (1 under Q8), do not call `status`, and do not enter `exec`. `check` and `fmt` exits, and usage exit 2 with no catalog code, are Q8.

### Fuel, stack, wrap

Every executed instruction costs 1, including `const`, `phi`, `br`, `list.cons`, and `call`. An import costs `1 + fuel_cost`. Fuel is checked before the instruction; on a shortfall, `E_FUEL` and the instruction does not run (SPEC "Fuel"). Defaults `fuel 100000` and `pages 16`. One module directive may override each (SPEC "Fuel": at most one of each). Not per function. The code for a second directive is Q3.

Call entry: unresolved import → `E_IMPORT`, no push, no charge; else frame 1025 → `E_STACK`, no charge; else charge, then push. Frame 1024 is legal. Fuel does not replace the cap (R4). An import occupies one frame until it returns. Re-entrant host→guest uses the same `call`. Instantiate, before any export, traps `E_IMPORT` for a missing host binding or `fuel_cost < 0` and does not charge. An unknown callee is check `E_IMPORT` and does not lower.

Frames are an explicit vector. The Q9 default, written into `golden/stack.strake`: host entry is call 1 and frame 1; calls 2..1024 fill frames 2..1024; call 1025 traps `E_STACK` and does not charge. Each test, property, and `run` gets a fresh machine. A fuel golden expects `E_FUEL`, not a remaining-fuel integer.

Integers `add` / `sub` / `mul` wrap (R2). Integer `div` truncates toward zero. Divisor 0, and signed `MIN / -1` on i32 and i64, trap `E_DIV0`. Float division by zero is IEEE. Float `eq` / `ne` compare bits. Ordered compares are IEEE; any NaN yields false (SPEC "Float compare"). `golden/` stays i64 and bool (R3). `eq` on list, record, or sum is `E_TYPE`. Bytes `eq` is bytewise.

Region check, in i128: `offset < 0` or `offset + width > pages * 65536` traps `E_OOB`. Widths are 4, 8, and 1. Unaligned access is legal (SPEC "Traps and region").

Forall span is i128, no wrapping subtract. `HI < LO`, span `≤ 0`, or span `> i64::MAX` is check `E_FORALL` (SPEC "Forall span", L11). `LO = 0`, `HI = i64::MAX` does not fit. `LO = 1`, `HI = i64::MAX` fits and is check-accepted in a unit test only. It is not a `strake test` file (SPEC "Forall span", "Minimum goldens").

### Dependencies

Phase 1 links **`blake3` only**, on the hash / `use` ticket. The hash is blake3 of the formatted UTF-8, with no formatter version (SPEC "Delimiters", "Packages"). The standard library does JSON, argv, spans, and the heap. No parser generator, serde, clap, snapshot crate, or tokenizer. `cl100k_base` has no numeric budget (SPEC "Agent-native mechanics").

## 2. Questions for Tom

A default is what the tickets implement until Tom answers. None of these reopen a lock. Closed, and not questions: the assert sidecar (L13); at most one `fuel` and one `pages` (SPEC "Fuel"); call and `assert.eq` literals take the operand type (SPEC "ANF"); a present `use` blob whose blake3 matches is legal, and a missing or mismatched blob is `E_USE` (SPEC "Packages"); tests call exports only (P13); `priv fn` is omitted from exports (R1); a user `trap` exists (A7); `run` exits as in §1.

1. **Header.** The worked example starts with `strake 1` and does not say it is the only legal header (SPEC "Layer A"). **Default:** header required, else `E_PARSE`. Question for Tom.
2. **Page precedence.** SPEC "Traps and region" lets the import state a page count, else 16. SPEC "Fuel" lets one directive override. **Default:** directive, else the import, else 16. Question for Tom.
3. **Second directive.** At most one `fuel` and one `pages` is locked. The catalog code is not. **Default:** reject the second directive, do not lower, and emit no catalog code until Tom names one. Question for Tom.
4. **Bare bindings and compare leftovers.** `assert.eq r 14` does not label `want`/`got`. **Default:** got, then want, so that example is got `r`, want `14`. A bare integer binding (`acc0 = 0`) is i64. `eq` / `ne` on `unit`, and ordered compares on `bool`, are `E_TYPE` ("compare family"). Question for Tom. Literal typing on `call` and `assert.eq` is already decided.
5. **Tag spelling and `_`.** SPEC "Prelude" spells `ok` / `err`. The `for` desugar spells `Ok` / `Err` and `Err _` (D7). **Default:** A spelling is `ok` / `err`. P6: the other spelling is `E_PARSE`. `_` is a match-arm discard and is not an L6 site. Question for Tom.
6. **Match-arm join value.** Arms may share one join name, the same shape as `if`. A missing arm value is a different fault from a non-exhaustive sum (`E_MATCH`, catalog "match is not exhaustive") and from `E_IF_PHI` (a name missing after `if`). **No code is emitted until Tom names one.** Question for Tom.
7. **User `trap` immediates.** The form exists (A7). **Default:** it lowers to the `trap` opcode and cannot carry `E_FUEL`, `E_STACK`, `E_DIV0`, `E_OOB`, or `E_IMPORT`. Which immediates it may carry is open. Question for Tom.
8. **Span JSON and check/fmt/usage exits.** Catalog lists `span` and does not define it. **Default:** JSON Lines on stderr, one object per line, catalog field order, absent optionals omitted; `span` is `{"file","start","end"}` half-open bytes; `check` exits 1 on any check diagnostic else 0; `fmt` exits 1 on `E_PARSE` else 0 and writes text to stdout; usage errors exit 2, plain text, no catalog code. Question for Tom.
9. **Entry frame and fuel scope.** SPEC "MVP" names `run` and defines no entry. **Default:** each test, property, and `run` gets a fresh machine; host entry is call 1 and frame 1. `golden/stack.strake` uses that count. `run`'s exit rule is §1, not this question. Question for Tom.
10. **`use` blob root.** A present blob whose blake3 matches the formatted text is legal. A missing blob or a mismatched hash is `E_USE` (SPEC "Packages"). Only the directory that holds the blob is open. **Default:** T8 passes that directory with `--blob-root`. Question for Tom.
11. **Property export name.** LOWERING names `test.name` only. **Default:** `property.<name>`. Question for Tom.
12. **List `forall` syntax.** SPEC "Tests" allows a finite list of literals and shows syntax only for `LO..HI`. **Default shape:** `forall x: T in NAME` on a module-level list literal. A form that is not the chosen syntax does not emit `E_FORALL` (that message is span empty, reversed, not an i64, or bounds not static). **No code until Tom answers.** Question for Tom.
13. **Test calls `priv`.** The prohibition is P13. `E_IMPORT` does not fit (a `priv fn` is a local fn). `E_TYPE` does not fit (catalog: operand type, arity, compare family, or `eq` on list/record/sum). **No code until Tom answers.** The call is rejected and does not lower. Question for Tom.
14. **`check --draft` on a hole.** Ship `check` emits `E_HOLE` kind `check` and does not lower (SPEC "Holes"). Draft execution traps. **Default:** `check --draft` does not emit check `E_HOLE`; it lowers and drops, and does not exec. Question for Tom.
15. **`--fn` through a helper.** Direct calls are SPEC "Incremental". A test that reaches a second export or a `priv fn` only through a local helper is unspecified. **Default:** the helper counts; that test is not selected. Question for Tom.
16. **Module-level integer bindings.** Forall bounds may be module-level integer literal bindings (SPEC "Forall span"). The syntax is unstated. **Default:** `name = <i64 literal>` at module scope. T7's running forall uses range literals until this is confirmed. Question for Tom.
17. **Region bytes.** Bool `load` of a byte other than 0 or 1, and the bytes of a fresh region, are unstated. `E_OOB` is the address range (catalog), so it is not the bool-byte fault. **Default:** a fresh region is zero-filled. A bool load of any other byte emits no catalog code until Tom names one. T8's region files load bytes they stored. Question for Tom.
18. **Lowerer abort.** §1 names exit 70 so a lowerer bug is not exit 2. Question for Tom: is 70 the number?

## 3. Minimum goldens

Each source is a fmt fixed point. `want` / `got` are omitted when the diag has none. Check failures are `strake test` so "no run" is the claim under test: exit 1, and a unit counter in `exec` stays 0.

| # | File | Command | Exit | Diag |
|---|---|---|---|---|
| 1 | `golden/wrap.strake` | `strake test golden/wrap.strake` | 0 | none. `add` of `9223372036854775807` and `1` equals `-9223372036854775808`; `sub` of `-9223372036854775808` and `1` equals `9223372036854775807`; `mul` of `9223372036854775807` and `2` equals `-2` |
| 2 | `golden/div0.strake` | `strake test golden/div0.strake` | 2 | two lines, each `E_DIV0` kind `trap`: divisor 0, and i64 `MIN / -1` |
| 3 | `golden/shadow.strake` | `strake test golden/shadow.strake` | 1 | `E_SHADOW` kind `check`. `exec` not entered |
| 4 | `golden/if_join.strake` | `strake test golden/if_join.strake` | 0 | none. Both arms bind one user name; that name is used after `if` |
| 4 | `golden/if_phi.strake` | `strake test golden/if_phi.strake` | 1 | `E_IF_PHI` kind `check`. `exec` not entered |
| 5 | `golden/for_list.strake` | `strake test golden/for_list.strake` | 0 | none. `list.get` out of range is `Err`. Stderr has no `E_OOB` |
| 6 | `golden/forall_rev.strake` | `strake test golden/forall_rev.strake` | 1 | `E_FORALL` kind `check`, `HI < LO`. No lower. `exec` not entered |
| 6 | `golden/forall_ok.strake` | `strake test golden/forall_ok.strake` | 0 | none. Inclusive span ≤ default fuel `100000` |
| 7 | `golden/fuel.strake` | `strake test golden/fuel.strake` | 2 | `E_FUEL` kind `trap`. The next instruction is a host import and the host records zero calls |
| 8 | `golden/stack.strake` | `strake test golden/stack.strake` | 2 | `E_STACK` kind `trap`. Entry is call 1; calls 2..1024 fill frames 2..1024; call 1025 traps; fuel unchanged |
| 9 | `golden/test_fail.strake` | `strake test golden/test_fail.strake` | 1 | `E_TEST` kind `result`, `want` `14`, `got` `16` (`assert.eq r 14`, `r` is 16) |
| 9 | `golden/refused.strake` | `strake test golden/refused.strake` | 77 | `E_REFUSED` kind `result` |
| 10 | `golden/hole_ship.strake` | `strake test golden/hole_ship.strake` | 1 | `E_HOLE` kind `check`. `exec` not entered |
| 10 | `golden/hole_draft.strake` | `strake test --draft golden/hole_draft.strake` | 2 | `E_HOLE` kind `trap` |

`golden/callee_77.strake`: `strake test`, exit 0, no `E_REFUSED`. The test calls an export that returns i64 77 and then falls off the end (`ret 0`).

Unit only, not a golden file: `LO = 1`, `HI = i64::MAX` check-accepts and is not executed. `LO = 0`, `HI = i64::MAX` is check `E_FORALL`. i32 `MIN / -1` and divisor 0 are unit `E_DIV0`. Float `/0` does not trap (unit).

## 4. Phase 1 tickets

Eight PRs. T3 is the thin slice. T4–T6 fan out. T7–T8 finish the card and the §3 sweep. Size: **S** one module, **M** one stage, **L** a stage plus its goldens.

### T1. Diagnostic kernel and `build`

**Size** S. **Depends** nothing.
**Goal.** `Code`, `Kind`, `status`, the JSON writer, the catalog include, and `build`.
**Files.** Workspace `Cargo.toml`, `src/lib.rs`, `main.rs`, `body/mod.rs`, `body/diag.rs`.
**Acceptance.** Precedence rows in §1, including trap+77, 77+nonzero, and `ret 2` → 1. `status` is not used by `run`. `E_CYCLE` is not constructable. Every other catalog `msg` matches. An emitted `E_HOLE` kind is one string. `strake build` prints `build is post-MVP`, exits 2, and stderr has no `"code"`.
**Verification.** Unit tests on `status` and the include. Black-box `strake build`.

### T2. Parse and fmt

**Size** M. **Depends** T1.
**Goal.** One CST. Parse and fmt are its consumers. `syntax` imports `diag` only.
**Files.** `body/syntax.rs`.
**Acceptance.** `fn add2` and `test add2.basic` parse and format idempotently at two-space indent. `if`, `loop`, `for`, `forall`, and `not` parse. `E_PARSE` for a nested-call operand, `assert.eq` outside `test`/`property`, a user `%` name, odd-length hex, and `and` / `or` / `rem` / bitwise / casts (SPEC "Arithmetic" closed absence). Indent is not syntax. Integer literals lex as i128; `-9223372036854775808` is a legal i64 literal. Header per Q1. A second `fuel` or `pages` is rejected with no catalog code (Q3).
**Verification.** Unit parser. Black-box `strake fmt` byte compare on the worked-example slice.

### T3. Thin slice: `add2.basic`

**Size** M. **Depends** T2.
**Goal.** Accept-path check, lower, and interpret one file. `compile` sequences CST → check → lower → exec.
**Files.** `body/check.rs`, `body/lower.rs`, `body/interp/mod.rs`, `body/interp/op.rs`, `body/interp/ir.rs`, `body/host.rs`, `golden/add2_basic.strake`.
**Acceptance.** The file is `add2` plus `test add2.basic` only. `call add2(3, 4)` typechecks because `3` and `4` take the parameter type. `strake test golden/add2_basic.strake` exits 0 and stderr is empty. Unit exec result is 14. `lower` cannot name `br`.
**Verification.** Unit result 14. Black-box `strake test golden/add2_basic.strake`.

### T4. Checks that block lowering

**Size** L. **Depends** T3.
**Goal.** L6, if-join failure, the ship hole, unknown callees, `priv` exports, directory selection, `--fn`.
**Files.** `body/check.rs`, `golden/shadow.strake`, `golden/if_phi.strake`, `golden/hole_ship.strake`, `golden/priv.strake`.
**Acceptance.** §3 rows 3, 4 (failure), and 10 (ship), each via `strake test`, exit 1, `exec` counter 0. Also `E_SHADOW` for an outer name assigned in an arm, two sibling `for` binders, a loop param assigned in the body, and a forall binder rebound. `eq` on a list → `E_TYPE`. Unknown callee → `E_IMPORT` kind `check`, no lower. `golden/priv.strake`: `strake check`, exit 0, and `exports()` omits the `priv fn` (R1). A test that calls `priv` does not lower and emits no catalog code (Q13). `check <dir>` reads only `src/**/*.strake`. `--fn` keeps a test whose direct calls are only that export.
**Verification.** Unit exec counter is 0 on the three `strake test` failures. Black-box:

```
strake test golden/shadow.strake
strake test golden/if_phi.strake
strake test golden/hole_ship.strake
strake check golden/priv.strake
```

### T5. Test channel

**Size** M. **Depends** T3.
**Goal.** Mismatch sidecar, refusal, spans.
**Files.** assert/ret lowering, `golden/test_fail.strake`, `golden/refused.strake`, `golden/callee_77.strake`.
**Acceptance.** §3 rows 9. `golden/callee_77.strake` exits 0 with no `E_REFUSED`. Fall-off inserts `ret 0`. Stderr for `E_TEST` slices the A name and contains no `%` digit.
**Verification.** Unit render. Black-box `strake test` on those three files.

### T6. Wrap, div0, fuel, stack, `not`

**Size** M. **Depends** T3.
**Goal.** The arithmetic and limit goldens, and `not` sugar.
**Files.** interp arith and fuel, `golden/wrap.strake`, `golden/div0.strake`, `golden/fuel.strake`, `golden/stack.strake`, `golden/not_bool.strake`.
**Acceptance.** §3 rows 1, 2, 7, and 8. `golden/not_bool.strake`: `strake test`, exit 0; `not true` lowers to `eq bool` against `false` and the test expects false. Unresolved import beats stack and does not charge (unit). Phi costs 1 (unit).
**Verification.** Unit call-entry table. Black-box `strake test` on the five files.

### T7. Lists, loop, match, forall

**Size** L. **Depends** T4.
**Goal.** Normative `for`, `loop` / `break` / `continue`, sums, `E_MATCH`, `E_FORALL`. The small property uses default fuel, so this ticket does not wait on T6.
**Files.** `golden/for_list.strake`, `golden/forall_rev.strake`, `golden/forall_ok.strake`, `golden/loop.strake`, `golden/loop_arity.strake`.
**Acceptance.** §3 rows 5 and 6. `golden/loop.strake`: `strake test`, exit 0; a loop parameter, `break`, and `continue` that names that parameter once (L7). `golden/loop_arity.strake`: `strake test`, exit 1, `E_TYPE` kind `check` (continue must name each loop parameter once), `exec` not entered. Non-exhaustive match → `E_MATCH`. A match arm that lacks the join value emits no catalog code (Q6). The desugar's `Err` arm is `trap Lower`, executed through the builder in a unit test, not by a lying list. No iterator value. Unit: `LO = 1`, `HI = i64::MAX` is accepted and not run; `LO = 0`, `HI = i64::MAX` is `E_FORALL`.
**Verification.** Unit OOB tag 1, desugar shape, and the `trap Lower` builder case. Black-box `strake test` on the five files.

### T8. Rest of the card and the sweep

**Size** L. **Depends** T5, T7.
**Goal.** Records, bytes, region, imports, `use`, draft holes, user `trap`, `run`, and the §3 sweep.
**Files.** `blake3` on the crate; `golden/hole_draft.strake`, `golden/region_oob.strake`, `golden/region_miss.strake`, `golden/region_unaligned.strake`, `golden/import_miss.strake`, `golden/use_ok.strake`, `golden/use_bad.strake`, `golden/bytes_oob.strake`, `golden/load_type.strake`, `golden/trap_user.strake`.
**Acceptance.** §3 row 10 draft. `golden/region_oob.strake`: `strake test`, exit 2, `E_OOB` kind `trap` (negative offset). `golden/region_miss.strake`: `strake test`, exit 1, `E_REGION` kind `check`, `exec` not entered. `golden/region_unaligned.strake`: `strake test`, exit 0, unaligned i64 store. `golden/import_miss.strake`: `strake test`, exit 2, `E_IMPORT` kind `trap`, host recorded zero calls. `golden/use_ok.strake`: `strake check --blob-root <dir> golden/use_ok.strake`, exit 0, blob bytes equal blake3 of the formatted file. `golden/use_bad.strake`: same command with a missing blob and again with a mismatched blob; each exits 1, `E_USE` kind `check`, `exec` not entered. `golden/bytes_oob.strake`: `strake test`, exit 0, `bytes.get` out of range is `Err`. `golden/load_type.strake`: `strake test`, exit 1, `E_TYPE` kind `check` for `load` of a list. `golden/trap_user.strake`: the form parses and lowers to `trap`; the immediate is not an `exec` fault (Q7). `strake run --fn add2 golden/add2_basic.strake` prints `14` and exits 0. The sweep is every §3 command.
**Verification.** Unit instantiate and i128 bounds. Black-box the commands above, then the §3 table.

## 5. Risks, and what Phase 1 is not

- README and the SPEC status line still say engineering is gated. This PR does not edit them.
- A fuel golden is a tripwire. `golden/fuel.strake` shows the missed instruction by a host counter, not by a leftover fuel total.
- `E_LOWER` is unreachable for a well-formed list. T7's builder test is the acceptance.
- A huge `pages` or a huge legal `forall` can abort the process (SPEC "Value heap", L11).
- The i64 minimum literal is lexed as sign plus digits in i128, then range-checked.
- Q8, Q9, and Q18 move exits. The §3 table cites the defaults.
- `Sketch` and `Builder` are visible inside `body`. Opcode privacy is what stops `lower` from spelling `br`.

**Not in Phase 1.** Wasm, Cranelift, LLVM, self-host, multi-file modules, the GBNF engine, `grammar/strake.gbnf`, `llms.txt`, `strake fix`, WASI, a heap cap, `E_HEAP`, emitting `E_CYCLE`, maps, iterators, constant folding, an ISA text format, a second diagnostic renderer, a token-budget enforcer (SPEC "MVP", "Deferrals", "Banned"). `build` is only the post-MVP stub.
