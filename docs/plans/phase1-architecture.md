# Phase 1 MVP architecture

Approved plan. Tom approved this pass on 2026-10-17 at 6:12 PM MT and locked every default in §2 (`Tom 2026-10-17, default`). **T1 is the first implementation ticket.** Source of truth is docs tip `6341a16`: SPEC.md, LOWERING.md, README.md, `errors/catalog.json`. Fog #1–#6, two-layer A1–A8, P1–P13, L1–L15, R1–R4, and D1–D7 stay locked. **Strake-A** and **strake-1** are provisional labels (SPEC "Why two layers", LOWERING header, D6). Roles stay: agents write A (`.strake`); backends eat the SSA ISA; the reference interpreter runs that ISA.

MVP is parse, check, and fmt of A; lower to the ISA; interpret it; run `golden/`; JSON diagnostics from the catalog (SPEC "MVP"). CLI is `check` (including `--fn`), `fmt`, `run`, and `test`. `build` exits 2 with `build is post-MVP` and no catalog code (SPEC "MVP", "Modules and CLI"; README "Locks").

## Synthesis

Base shape: one crate, five public functions, opcode enum private to the interpreter. The 2026-10-12 r1 draft was input, not the base. The PR #2 challenge at `dba6d23` locked the assert sidecar and stopped three catalog codes from naming a different fault. Tom's approval locks the nineteen defaults. Every behavior below is the approved default.

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

The five functions return `Exit { code, stdout, stderr }`. `Opts` carries `draft` and, on `check`, `only_fn`. That is the public surface. `check <dir>` loads each `src/**/*.strake` as its own module and does not link them (SPEC "Modules and CLI", R1). `--fn` keeps that export plus same-file tests whose direct calls are only to it (SPEC "Incremental"). A helper counts: a test that reaches another export or a `priv fn` only through a helper is not selected (§2.15).

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

Any check diagnostic returns before lower (SPEC "Holes"; minimum golden 3 does not run). A clean `check` lowers and drops the `Program` (LOWERING "What lowering is not"). `check --draft` on a hole does not emit check `E_HOLE`; it lowers, drops the `Program`, and does not exec (§2.14). Ship `check` still emits `E_HOLE` kind `check` and does not lower. Fmt reprints the CST and does not check, so an `E_SHADOW` file stays illegal and still has a format fixed point (SPEC "Control", LOWERING "L6"). A second well-formed `fuel` or `pages` directive is the same kind of file: `strake fmt` exits 0 and reprints it (§2.3, `golden/fuel_twice.strake`).

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

**Sidecar, locked.** `diag` is not an opcode. L13 desugars `assert.eq` before emit. LOWERING "Calls, tests, properties" gives the `test.name` export a diag sidecar. A property export is named `property.<name>` (§2.11). SPEC "Tests" is the effect: `E_TEST` with `want`/`got` and `ret 1`. The `diag` line in the desugar listing is that effect. The mismatch `ret` is `TestFail { want, got }`, both vregs, read when the `ret` runs. `assert.eq` order is got, then want (§2.4): `assert.eq r 14` with `r` = 16 is want `14`, got `16`. An explicit `ret 1` is a value return: CLI 1, not `E_TEST`. `ret 77` in the test or property body is `Refused`. A callee that returns i64 77 is an ordinary value.

The lowerer plants `trap` immediates `Hole` (draft hole) and `Lower` (`for` desugar `Err` arm). `E_FUEL`, `E_STACK`, `E_DIV0`, `E_OOB`, and instantiate-time `E_IMPORT` are `exec` faults, so a `trap` immediate is not a second way to raise them (SPEC "Fuel"). A user `trap` exists (A7, SPEC "Prelude"). It lowers to opcode `trap` and cannot carry `E_FUEL`, `E_STACK`, `E_DIV0`, `E_OOB`, `E_IMPORT`, `E_HOLE`, or `E_LOWER` (§2.7). No positive immediate is part of this plan. `golden/trap_user.strake` is a unit lower check only: opcode `trap`, immediate outside that list, no CLI exit.

The value heap is one append-only `Vec`. Handles are indices. Edges point at a strictly smaller index, so a cycle cannot be built. `E_CYCLE` has no constructor (catalog; SPEC "Value heap"). v0 does not reclaim. Host OOM aborts the process. `list.get` and `bytes.get` out of range return `Err` (tag 1, `unit` payload) and do not raise `E_OOB`. `ok` is tag 0. Scalars are immediates. Floats are stored as bits. Region bytes are a separate `Option<Vec<u8>>`, present only after `import region mem : region`. Lowering does not allocate a region for lists (invariant 4). Agents write `ok` / `err`. The other spelling is `E_PARSE` (P6, §2.5). `_` is a match-arm discard and is not an L6 site.

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

A checked sketch that cannot lower, or a `finish` that rejects a `Program`, is a bug. It exits **70**, plain text, no catalog code, and does not call `status` (§2.18). It is not exit 2 and it is not exit 3.

### Diagnostic spans

Each instruction carries the byte span of the A construct that emitted it. Hygienic temps carry the `for` or `forall` span (invariant 6). Faults and `E_TEST` copy that span. `msg` is the catalog string; detail is `want`, `got`, and optional `patch` (SPEC "Agent-native mechanics"). Render: scalars as literals, floats as hex bit patterns, compounds as `handle:<id>`, import payloads not walked (catalog `render`). The JSON writer has no `%` conversion. `span` is `{"file","start","end"}`, half-open bytes (§2.8). One diagnostic is one JSON object on one line. Traces stay inside the capabilities the module imported (SPEC "Capability-scoped traces").

`Code` comes from the catalog's non-reserved codes. `E_HOLE` and `E_IMPORT` each leave as one kind string (catalog `kind_note`). An include test fails if a message drifts, if `E_CYCLE` becomes constructable, or if an emitted `kind` is an array.

### Exit codes

`diag::status` is the test channel only. Its comment is the catalog sentence (catalog `cli.precedence`, SPEC "Tests", D4):

```
any trap -> 2; else any 77 -> 77; else any nonzero test -> 1; else 0
```

`status` folds test and property exports. A trap is never rewritten to `E_TEST` or 77. An explicit `ret 2` in a test becomes 1. `build` does not call `status`. Unit rows: trap with 77 → 2; 77 with a nonzero test → 77; nonzero → 1; all zero → 0.

`run` does not call `status`. A normal return prints one scalar (`handle:<id>` for a compound) and exits 0, including a return of i64 77 or 2. A trap on `run` exits 2 with that trap code. When check fails first, `test` and `run` return that check exit: **1** when the failure is a catalog diagnostic, **3** when it is a codeless rejection. They do not call `status` and do not enter `exec`. `check` exits 1 on any catalog check diagnostic; else, on a codeless rejection, 3; else 0. `fmt` exits 1 on `E_PARSE` else 0 and writes text to stdout. A second well-formed `fuel` or `pages` directive is not `E_PARSE`, so `strake fmt` on that file exits 0 and reprints it. Usage errors exit 2, plain text, no catalog code (§2.8). `run --fn` takes a nullary export (§2.19). `golden/run_const.strake` is `fn answer() -> i64` / `ret 14`. `strake run --fn answer golden/run_const.strake` prints `14` and exits 0.

A rejection that has no catalog code is not a clean check and not a successful run. The else-0 arm of `check` does not apply. A second `fuel` or `pages` directive (§2.3), a match arm missing the join value (§2.6), a list-`forall` form other than `forall x: T in NAME` on a module-level list literal (§2.12), and a test that calls `priv` (§2.13) exit **3**, plain text, no `"code"`, do not call `status`, do not lower, and do not enter `exec`. A bool `load` of a byte other than 0 or 1 is the same stop once the load runs (§2.17): byte 0 is false, byte 1 is true, and any other byte does not become a bool. The process stops with exit 3, plain text, no catalog code, `status` not called, and does not return true. Exit 3 is not exit 70 and is not `E_PARSE`, `E_MATCH`, `E_FORALL`, `E_TYPE`, `E_IMPORT`, or `E_OOB`.

### Fuel, stack, wrap

Every executed instruction costs 1, including `const`, `phi`, `br`, `list.cons`, and `call`. An import costs `1 + fuel_cost`. Fuel is checked before the instruction; on a shortfall, `E_FUEL` and the instruction does not run (SPEC "Fuel"). Defaults `fuel 100000` and `pages 16`. One module directive may override each (SPEC "Fuel": at most one of each). Page count is the directive, else the region import's count, else 16 (§2.2). A second `fuel` or a second `pages` exits 3, plain text, no catalog code, and does not lower (§2.3). `strake fmt` on that file exits 0 and reprints it. Not per function.

Call entry: unresolved import → `E_IMPORT`, no push, no charge; else frame 1025 → `E_STACK`, no charge; else charge, then push. Frame 1024 is legal. Fuel does not replace the cap (R4). An import occupies one frame until it returns. Re-entrant host→guest uses the same `call`. Instantiate, before any export, traps `E_IMPORT` for a missing host binding or `fuel_cost < 0` and does not charge. An unknown callee is check `E_IMPORT` and does not lower.

Frames are an explicit vector. Host entry is call 1 and frame 1 (§2.9). `golden/stack.strake`: calls 2..1024 fill frames 2..1024; call 1025 traps `E_STACK` and does not charge. Each test, property, and `run` gets a fresh machine. A fuel golden expects `E_FUEL`, not a remaining-fuel integer.

Integers `add` / `sub` / `mul` wrap (R2). Integer `div` truncates toward zero. Divisor 0, and signed `MIN / -1` on i32 and i64, trap `E_DIV0`. Float division by zero is IEEE. Float `eq` / `ne` compare bits. Ordered compares are IEEE; any NaN yields false (SPEC "Float compare"). `golden/` stays i64 and bool (R3). `eq` on list, record, or sum is `E_TYPE`. Bytes `eq` is bytewise. A bare integer binding (`acc0 = 0`) is i64. `eq` / `ne` on `unit`, and ordered compares on `bool`, are `E_TYPE` ("compare family") (§2.4).

Region check, in i128: `offset < 0` or `offset + width > pages * 65536` traps `E_OOB`. Widths are 4, 8, and 1. Unaligned access is legal (SPEC "Traps and region"). A fresh region is zero-filled. Bool `load`: byte 0 is false, byte 1 is true, and any other byte stops with exit 3, plain text, no catalog code, `status` not called, and does not return true (§2.17).

Forall span is i128, no wrapping subtract. `HI < LO`, span `≤ 0`, or span `> i64::MAX` is check `E_FORALL` (SPEC "Forall span", L11). `LO = 0`, `HI = i64::MAX` does not fit. `LO = 1`, `HI = i64::MAX` fits and is check-accepted in a unit test only. It is not a `strake test` file (SPEC "Forall span", "Minimum goldens"). Module-level integer bindings are `name = <i64 literal>` (§2.16). `golden/forall_ok.strake` uses range literals. A list `forall` is `forall x: T in NAME` on a module-level list literal; any other list-forall form exits 3 and does not emit `E_FORALL` (§2.12, `golden/forall_list_bad.strake`).

### Dependencies

Phase 1 links **`blake3` only**, on the hash / `use` ticket. The hash is blake3 of the formatted UTF-8, with no formatter version (SPEC "Delimiters", "Packages"). The standard library does JSON, argv, spans, and the heap. No parser generator, serde, clap, snapshot crate, or tokenizer. `cl100k_base` has no numeric budget (SPEC "Agent-native mechanics"). The blob directory is `--blob-root` (§2.10).

## 2. Locked decisions

Tom approved each default on 2026-10-17 at 6:12 PM MT. Every item cites `Tom 2026-10-17, default`. None of these reopen a lock. Closed before this list: the assert sidecar (L13); at most one `fuel` and one `pages` (SPEC "Fuel"); call and `assert.eq` literals take the operand type (SPEC "ANF"); a present `use` blob whose blake3 matches is legal, and a missing or mismatched blob is `E_USE` (SPEC "Packages"); tests call exports only (P13); `priv fn` is omitted from exports (R1); a user `trap` exists (A7); `run` exits as in §1.

1. **Header.** Required. Anything else is `E_PARSE`. `Tom 2026-10-17, default`.
2. **Page precedence.** The directive, else the region import's count, else 16. `Tom 2026-10-17, default`.
3. **Second directive.** A second `fuel` or a second `pages` is the exit-3 rejection in §1. No catalog code. `golden/fuel_twice.strake` is the oracle. `strake fmt` on that file exits 0 and reprints it. `Tom 2026-10-17, default`.
4. **Bare bindings and compare leftovers.** `assert.eq` is got, then want, so `assert.eq r 14` with `r` = 16 is want `14`, got `16`. A bare integer binding (`acc0 = 0`) is i64. `eq` / `ne` on `unit`, and ordered compares on `bool`, are `E_TYPE` ("compare family"). `Tom 2026-10-17, default`.
5. **Tag spelling and `_`.** A spelling is `ok` / `err`. The other spelling is `E_PARSE` (P6). `_` is a match-arm discard and is not an L6 site. `Tom 2026-10-17, default`.
6. **Match-arm join value.** Arms may share one join name, the same shape as `if`. A missing arm value exits 3 (§1). It is not `E_MATCH` and it is not `E_IF_PHI`. `golden/match_join.strake` is the oracle. `Tom 2026-10-17, default`.
7. **User `trap` immediates.** Lowers to opcode `trap`. Cannot carry `E_FUEL`, `E_STACK`, `E_DIV0`, `E_OOB`, `E_IMPORT`, `E_HOLE`, or `E_LOWER`. No positive immediate is named. `golden/trap_user.strake` is a unit lower check only: opcode `trap`, immediate outside that list, no CLI exit. `Tom 2026-10-17, default`.
8. **Span JSON and check/fmt/usage exits.** JSON Lines on stderr, one object per line, catalog field order, absent optionals omitted. `span` is `{"file","start","end"}` half-open bytes. `check` exits 1 on any catalog check diagnostic; else, on a codeless rejection, exit 3; else 0. `fmt` exits 1 on `E_PARSE` else 0 and writes text to stdout. A second well-formed directive is not `E_PARSE`, so `fmt` exits 0 on it. Usage errors exit 2, plain text, no catalog code. `Tom 2026-10-17, default`.
9. **Entry frame.** Each test, property, and `run` gets a fresh machine. Host entry is call 1 and frame 1. `golden/stack.strake` uses that count. `Tom 2026-10-17, default`.
10. **`use` blob root.** T8 passes the blob directory with `--blob-root`. `Tom 2026-10-17, default`.
11. **Property export name.** `property.<name>`. `Tom 2026-10-17, default`.
12. **List `forall` syntax.** `forall x: T in NAME` where `NAME` is a module-level list literal. Any other list-forall form exits 3 and does not emit `E_FORALL`. `golden/forall_list_bad.strake` is the oracle. `Tom 2026-10-17, default`.
13. **Test calls `priv`.** `golden/priv_call.strake` exits 3, plain text, no catalog code, and does not lower. `golden/priv.strake` stays a clean export check. `Tom 2026-10-17, default`.
14. **`check --draft` on a hole.** Does not emit check `E_HOLE`. It lowers and drops, and does not exec. Ship `check` emits `E_HOLE` kind `check` and does not lower. `Tom 2026-10-17, default`.
15. **`--fn` through a helper.** The helper counts. A test that reaches a second export or a `priv fn` only through a local helper is not selected. `Tom 2026-10-17, default`.
16. **Module-level integer bindings.** `name = <i64 literal>` at module scope. `golden/forall_ok.strake` keeps range literals. `Tom 2026-10-17, default`.
17. **Region bytes.** A fresh region is zero-filled. Byte 0 loads as false and byte 1 as true. Any other byte does not become a bool: the run stops with exit 3, plain text, no catalog code, `status` not called, and does not return true. T8's region files load bytes they stored. `Tom 2026-10-17, default`.
18. **Lowerer abort.** Exit 70. Plain text, no catalog code, `status` not called. Not exit 2 and not exit 3. `Tom 2026-10-17, default`.
19. **`run` argument spelling.** `run --fn` takes a nullary export. `golden/run_const.strake` is `fn answer() -> i64` / `ret 14`. `strake run --fn answer golden/run_const.strake` prints `14` and exits 0. `Tom 2026-10-17, default`.

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
| 6 | `golden/forall_ok.strake` | `strake test golden/forall_ok.strake` | 0 | none. Inclusive span ≤ default fuel `100000`. Range literals |
| 7 | `golden/fuel.strake` | `strake test golden/fuel.strake` | 2 | `E_FUEL` kind `trap`. The file sets `fuel 0`, adds two i64 literals inside a test, and imports nothing. That program costs at least 1, so `EmptyHost` reaches `E_FUEL` |
| 8 | `golden/stack.strake` | `strake test golden/stack.strake` | 2 | `E_STACK` kind `trap`. Entry is call 1; calls 2..1024 fill frames 2..1024; call 1025 traps; fuel unchanged |
| 9 | `golden/test_fail.strake` | `strake test golden/test_fail.strake` | 1 | `E_TEST` kind `result`, `want` `14`, `got` `16` (`assert.eq r 14`, `r` is 16) |
| 9 | `golden/refused.strake` | `strake test golden/refused.strake` | 77 | `E_REFUSED` kind `result` |
| 10 | `golden/hole_ship.strake` | `strake test golden/hole_ship.strake` | 1 | `E_HOLE` kind `check`. `exec` not entered |
| 10 | `golden/hole_draft.strake` | `strake test --draft golden/hole_draft.strake` | 2 | `E_HOLE` kind `trap` |

`golden/callee_77.strake`: `strake test`, exit 0, no `E_REFUSED`. The test calls an export that returns i64 77 and then falls off the end (`ret 0`).

### Codeless rejection goldens

Stderr is plain text and has no `"code"`. The file does not lower. `exec` is not entered. `status` is not called. Exit is 3 on `check` and on `test`.

`golden/fuel_twice.strake` (second directive, §2.3). Input:

```
strake 1
fuel 1
fuel 2

fn answer() -> i64
  ret 1
end
```

`strake check golden/fuel_twice.strake` exits 3. `strake test golden/fuel_twice.strake` exits 3. `strake fmt golden/fuel_twice.strake` exits 0 and its stdout is that text (fmt fixed point). A second `pages` is the same check rejection; this file is the named oracle.

`golden/match_join.strake` (match arm missing the join, §2.6). Input:

```
strake 1

fn pick(r: result i64 unit) -> i64
  match r
    ok v ->
      out = v
    err _ ->
      skipped = 0
  end
  ret out
end

test match_join.miss
  ret 0
end
```

`out` is bound on the `ok` arm only and used after `match`. `strake test golden/match_join.strake` exits 3. The sum is exhaustive, so the diagnostic is not `E_MATCH`. The construct is `match`, so the diagnostic is not `E_IF_PHI`.

`golden/forall_list_bad.strake` (list forall not on a module-level name, §2.12). Input:

```
strake 1

property bad.list
  t0 = list.empty i64
  xs = list.cons 1 t0
  forall n: i64 in xs
    assert.eq n n
  end
end
```

`xs` is local to the property. `strake test golden/forall_list_bad.strake` exits 3. Stderr does not contain `E_FORALL`.

Unit only, not a golden file: `LO = 1`, `HI = i64::MAX` check-accepts and is not executed. `LO = 0`, `HI = i64::MAX` is check `E_FORALL`. i32 `MIN / -1` and divisor 0 are unit `E_DIV0`. Float `/0` does not trap (unit).

## 4. Phase 1 tickets

Eight PRs. **T1 is the first implementation ticket.** T3 is the thin slice. T4–T6 fan out. T7–T8 finish the card and the §3 sweep. Size: **S** one module, **M** one stage, **L** a stage plus its goldens.

### T1. Diagnostic kernel and `build`

**First implementation ticket. Size** S. **Depends** nothing.
**Goal.** `Code`, `Kind`, `status`, the JSON writer, the catalog include, and `build`.
**Files.** Workspace `Cargo.toml`, `src/lib.rs`, `main.rs`, `body/mod.rs`, `body/diag.rs`.
**Acceptance.** Precedence rows in §1, including trap+77, 77+nonzero, and `ret 2` → 1. `status` is not used by `run`. `E_CYCLE` is not constructable. Every other catalog `msg` matches. An emitted `E_HOLE` kind is one string. `span` is `{"file","start","end"}` half-open bytes, one JSON object per line, absent optionals omitted (§2.8). `strake build` prints `build is post-MVP`, exits 2, and stderr has no `"code"`.
**Verification.** Unit tests on `status` and the include. Black-box `strake build`.

### T2. Parse and fmt

**Size** M. **Depends** T1.
**Goal.** One CST. Parse and fmt are its consumers. `syntax` imports `diag` only.
**Files.** `body/syntax.rs`, `golden/fuel_twice.strake`.
**Acceptance.** `fn add2` and `test add2.basic` parse and format idempotently at two-space indent. `if`, `loop`, `for`, `forall`, and `not` parse. `E_PARSE` for a nested-call operand, `assert.eq` outside `test`/`property`, a user `%` name, odd-length hex, and `and` / `or` / `rem` / bitwise / casts (SPEC "Arithmetic" closed absence). Indent is not syntax. Integer literals lex as i128; `-9223372036854775808` is a legal i64 literal. The header is required, else `E_PARSE` (§2.1). `golden/fuel_twice.strake` is the §3 input (two `fuel` directives). `strake check` and `strake test` on it exit 3, plain text, no catalog code, and do not lower. `strake fmt` on it exits 0 and reprints the file.
**Verification.** Unit parser. Black-box `strake fmt` byte compare on the worked-example slice. Black-box `strake fmt`, `strake check`, and `strake test` on `golden/fuel_twice.strake`.

### T3. Thin slice: `add2.basic`

**Size** M. **Depends** T2.
**Goal.** Accept-path check, lower, and interpret one file. `compile` sequences CST → check → lower → exec.
**Files.** `body/check.rs`, `body/lower.rs`, `body/interp/mod.rs`, `body/interp/op.rs`, `body/interp/ir.rs`, `body/host.rs`, `golden/add2_basic.strake`.
**Acceptance.** The file is `add2` plus `test add2.basic` only. `call add2(3, 4)` typechecks because `3` and `4` take the parameter type. `strake test golden/add2_basic.strake` exits 0 and stderr is empty. Unit exec result is 14. `lower` cannot name `br`. A `finish` rejection returns exit 70, plain text, and does not call `status` (§2.18).
**Verification.** Unit result 14, and the abort unit above. Black-box `strake test golden/add2_basic.strake`.

### T4. Checks that block lowering

**Size** L. **Depends** T3.
**Goal.** L6, if-join failure, the ship hole, unknown callees, `priv` exports, directory selection, `--fn`.
**Files.** `body/check.rs`, `golden/shadow.strake`, `golden/if_phi.strake`, `golden/hole_ship.strake`, `golden/priv.strake`, `golden/priv_call.strake`.
**Acceptance.** §3 rows 3, 4 (failure), and 10 (ship), each via `strake test`, exit 1, `exec` counter 0. Also `E_SHADOW` for an outer name assigned in an arm, two sibling `for` binders, a loop param assigned in the body, and a forall binder rebound. `eq` on a list → `E_TYPE`. Unknown callee → `E_IMPORT` kind `check`, no lower. `golden/priv.strake`: `strake check`, exit 0, and `exports()` omits the `priv fn` (R1). `golden/priv_call.strake`: a test calls that `priv fn`; `strake test` exits 3, plain text, no catalog code, does not lower, `exec` counter 0 (§2.13). `check <dir>` reads only `src/**/*.strake`. `--fn` keeps a test whose direct calls are only that export. A test that reaches another export or a `priv fn` only through a helper is not selected (§2.15). `check --draft` on a hole lowers and drops, emits no check `E_HOLE`, and does not exec (§2.14).
**Verification.** Unit exec counter is 0 on the three catalog failures and on `priv_call`. Black-box:

```
strake test golden/shadow.strake
strake test golden/if_phi.strake
strake test golden/hole_ship.strake
strake check golden/priv.strake
strake test golden/priv_call.strake
```

### T5. Test channel

**Size** M. **Depends** T3.
**Goal.** Mismatch sidecar, refusal, spans.
**Files.** assert/ret lowering, `golden/test_fail.strake`, `golden/refused.strake`, `golden/callee_77.strake`.
**Acceptance.** §3 rows 9. `assert.eq` is got, then want (§2.4). `golden/callee_77.strake` exits 0 with no `E_REFUSED`. Fall-off inserts `ret 0`. Stderr for `E_TEST` slices the A name and contains no `%` digit. Property exports are named `property.<name>` (§2.11).
**Verification.** Unit render. Black-box `strake test` on those three files.

### T6. Wrap, div0, fuel, stack, `not`

**Size** M. **Depends** T3.
**Goal.** The arithmetic and limit goldens, and `not` sugar.
**Files.** interp arith and fuel, `golden/wrap.strake`, `golden/div0.strake`, `golden/fuel.strake`, `golden/stack.strake`, `golden/not_bool.strake`.
**Acceptance.** §3 rows 1, 2, 7, and 8. `golden/not_bool.strake`: `strake test`, exit 0; `not true` lowers to `eq bool` against `false` and the test expects false. Unresolved import beats stack and does not charge (unit). Phi costs 1 (unit). Host entry is call 1 and frame 1; each test, property, and `run` gets a fresh machine (§2.9). Unit, with a bound host: the next instruction would be an integer `div` by 0 and remaining fuel is 0; the diagnostic is `E_FUEL`, not `E_DIV0`, and the `div` does not execute.
**Verification.** Unit call-entry table, including that fuel-before-div case. Black-box `strake test` on the five files.

### T7. Lists, loop, match, forall

**Size** L. **Depends** T4.
**Goal.** Normative `for`, `loop` / `break` / `continue`, sums, `E_MATCH`, `E_FORALL`. The small property uses default fuel, so this ticket does not wait on T6.
**Files.** `golden/for_list.strake`, `golden/forall_rev.strake`, `golden/forall_ok.strake`, `golden/loop.strake`, `golden/loop_arity.strake`, `golden/match_join.strake`, `golden/forall_list_bad.strake`.
**Acceptance.** §3 rows 5 and 6. `golden/loop.strake`: `strake test`, exit 0; a loop parameter, `break`, and `continue` that names that parameter once (L7). `golden/loop_arity.strake`: `strake test`, exit 1, `E_TYPE` kind `check` (continue must name each loop parameter once), `exec` not entered. Non-exhaustive match → `E_MATCH`. `golden/match_join.strake` is the §3 input: `strake test` exits 3, plain text, no catalog code, `exec` not entered; not `E_MATCH` and not `E_IF_PHI`. `golden/forall_list_bad.strake` is the §3 input: `strake test` exits 3, plain text, no catalog code, stderr has no `E_FORALL`, `exec` not entered. `golden/forall_ok.strake` uses range literals. Module-level integer bindings are `name = <i64 literal>` (§2.16). The desugar's `Err` arm is `trap Lower`, executed through the builder in a unit test, not by a lying list. No iterator value. Unit: `LO = 1`, `HI = i64::MAX` is accepted and not run; `LO = 0`, `HI = i64::MAX` is `E_FORALL`.
**Verification.** Unit OOB tag 1, desugar shape, and the `trap Lower` builder case. Black-box `strake test` on the seven files.

### T8. Rest of the card and the sweep

**Size** L. **Depends** T5, T7.
**Goal.** Records, bytes, region, imports, `use`, draft holes, user `trap`, `run`, and the §3 sweep.
**Files.** `blake3` on the crate; `golden/hole_draft.strake`, `golden/region_oob.strake`, `golden/region_miss.strake`, `golden/region_unaligned.strake`, `golden/import_miss.strake`, `golden/use_ok.strake`, `golden/use_bad.strake`, `golden/bytes_oob.strake`, `golden/load_type.strake`, `golden/trap_user.strake`, `golden/run_const.strake`.
**Acceptance.** §3 row 10 draft. `golden/region_oob.strake`: `strake test`, exit 2, `E_OOB` kind `trap` (negative offset). `golden/region_miss.strake`: `strake test`, exit 1, `E_REGION` kind `check`, `exec` not entered. `golden/region_unaligned.strake`: `strake test`, exit 0, unaligned i64 store. Page count is the directive, else the region import's count, else 16 (§2.2). A fresh region is zero-filled; these region files load bytes they stored (§2.17). `golden/import_miss.strake`: `strake test`, exit 2, `E_IMPORT` kind `trap`, host recorded zero calls. `golden/use_ok.strake`: `strake check --blob-root <dir> golden/use_ok.strake`, exit 0, blob bytes equal blake3 of the formatted file. `golden/use_bad.strake`: same command with a missing blob and again with a mismatched blob; each exits 1, `E_USE` kind `check`, `exec` not entered. `golden/bytes_oob.strake`: `strake test`, exit 0, `bytes.get` out of range is `Err`. `golden/load_type.strake`: `strake test`, exit 1, `E_TYPE` kind `check` for `load` of a list. `golden/trap_user.strake` has no CLI command: a unit lower check emits opcode `trap` with an immediate outside the cannot-carry list (`E_FUEL`, `E_STACK`, `E_DIV0`, `E_OOB`, `E_IMPORT`, `E_HOLE`, `E_LOWER`), and there is no process exit (§2.7). `golden/run_const.strake`: `strake run --fn answer golden/run_const.strake` prints `14` and exits 0 (§2.19). The sweep is every §3 command.
**Verification.** Unit instantiate and i128 bounds. Black-box the commands above, then the §3 table.

## 5. Risks, and what Phase 1 is not

- README and the SPEC status line still say engineering is gated. This PR does not edit them.
- A fuel golden is a tripwire. `golden/fuel.strake` is `fuel 0` and no host import. The "instruction did not run" oracle is T6's unit: a would-be `E_DIV0` comes back as `E_FUEL`.
- `E_LOWER` is unreachable for a well-formed list. T7's builder test is the acceptance.
- A huge `pages` or a huge legal `forall` can abort the process (SPEC "Value heap", L11).
- The i64 minimum literal is lexed as sign plus digits in i128, then range-checked.
- `Sketch` and `Builder` are visible inside `body`. Opcode privacy is what stops `lower` from spelling `br`.

**Not in Phase 1.** Wasm, Cranelift, LLVM, self-host, multi-file modules, the GBNF engine, `grammar/strake.gbnf`, `llms.txt`, `strake fix`, WASI, a heap cap, `E_HEAP`, emitting `E_CYCLE`, maps, iterators, constant folding, an ISA text format, a second diagnostic renderer, a token-budget enforcer (SPEC "MVP", "Deferrals", "Banned"). `build` is only the post-MVP stub.
