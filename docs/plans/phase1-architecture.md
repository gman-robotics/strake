# Phase 1 MVP architecture

Plan only. No build until Tom approves this pass. Source of truth is docs tip `6341a16`: SPEC.md, LOWERING.md, README.md, `errors/catalog.json`. Fog #1–#6, two-layer A1–A8, P1–P13, L1–L15, R1–R4, and D1–D7 stay locked. **Strake-A** and **strake-1** are provisional labels (SPEC "Why two layers", LOWERING header, D6). Roles stay: agents write A (`.strake`); backends eat the SSA ISA; the reference interpreter runs that ISA.

MVP is parse, check, and fmt of A; lower to the ISA; interpret it; run `golden/`; JSON diagnostics from the catalog (SPEC "MVP"). CLI is `check` (including `--fn`), `fmt`, `run`, and `test`. `build` exits 2 with `build is post-MVP` and no catalog code (SPEC "MVP", "Modules and CLI"; README "Locks").

## Synthesis

Base shape: one crate, five public functions, opcode enum private to the interpreter, re-checked against `6341a16`. Two companion sketches produced no file (one seat could not start, one timed out). The 2026-10-12 r1 draft was input, not the base.

Rejected shapes:

- A public opcode module and an ISA text dump. Callers would hand-write the ISA (SPEC "Banned"). Invariant 6 would fight a dump whose point is `%n`.
- A second evaluator that runs A directly. Invariant 0 would become a sync job (SPEC "Why two layers").
- A multi-crate workspace. SPEC "Layout" and "Stamped product locks" are one crate, `strake`. Extra crates would publish the types this shape hides.
- Topic modules (control, values, limits) crossing the pipeline. Each pipeline stage already owns a different type. A second cut splits those owners.

Adjusted after the spec check: the first golden is T3; page count follows Q2 (directive, else the import, else 16); user `trap` stays Q6 because the prelude card lists it; seams are Rust visibility.

## 1. Architecture

### Module layout

```
crates/strake/
  Cargo.toml
  src/lib.rs                pub check, fmt, test, run, build
  src/main.rs               argv only
  src/body/mod.rs           one compile function
  src/body/diag.rs          catalog kernel
  src/body/syntax.rs        private CST, parse, fmt
  src/body/check.rs         binding table and types
  src/body/lower.rs         desugar into builder calls
  src/body/host.rs          EmptyHost, crate-private
  src/body/interp/mod.rs    heap, fuel, frames, exec
  src/body/interp/op.rs     private opcode enum
  src/body/interp/ir.rs     Builder and Program
```

The repo root is a Cargo workspace whose only member is `crates/strake/`, the layout in SPEC "Layout". Provisional label strings live in one private const. Pipeline types are named `Program` and sketch, so a rename does not move code.

Caller view, which the signatures serve:

```
strake check [--draft] [--fn NAME] <file|dir>
strake fmt <file>
strake test [--draft] <file>
strake run --fn NAME [--draft] <file>     # entry shape is Q8
strake build                              # exit 2, plain text, no catalog code
```

`check`, `fmt`, `test`, `run`, and `build` return `Exit { code, stdout, stderr }`. `Opts` is `draft` plus `only_fn` on `check`. That is the whole public surface. `check <dir>` loads each `src/**/*.strake` as its own module and does not link them (SPEC "Modules and CLI"). `--fn` keeps that export plus same-file tests whose calls are only to it (SPEC "Incremental").

### Dependency Rule

Dependencies point inward:

```
main, host                         outer circle
syntax → check → lower → ir → interp
diag + errors/catalog.json         inner kernel
```

`syntax` may name `check` and `diag`. `check` may name `lower` and `diag`. `lower` may name the builder. The builder fills `Program` and may name the private opcode enum, so `ir` sits inside `interp` and the dependency still points inward. `interp` may name `diag`. `diag` names nobody in the pipeline. `main` calls the five functions only. A host may enter through `Machine::call` and may not name the CST, the sketch, or an opcode.

`compile` is the one pipeline function. Any check diagnostic returns before lower (SPEC "Holes": a ship hole is rejected before lower; minimum golden 3 does not run). A clean `check` lowers and drops the `Program`, so "if A cannot lower, it is not legal A" (LOWERING "What lowering is not") is the same function `test` and `run` use. Fmt reprints the CST and does not check: an `E_SHADOW` file stays illegal and still has a format fixed point (SPEC "Control", LOWERING "L6").

### Information hiding

| Seam | Owner | What a wrong import cannot do |
|---|---|---|
| CST | `syntax` | Interpreter cannot name it. `exec` takes `&Program` and a host. |
| Binding table | `check` | One L6 table. The sketch moves once into `emit` and is dropped. |
| Opcodes | `interp/op.rs` | `lower` has no opcode name. It calls builder methods. |
| `Program` fields | `interp/ir.rs` | Callers see `exports()`, derived from the records `exec` uses. |
| `Diag` | `diag` | Span bytes and rendered `want`/`got`. No vreg field. |
| Host | `host.rs` | Not on the crate API. The CLI uses `EmptyHost`. |

The builder's control surface is `if_`, `loop_`, and `match_`. `br`, `br_if`, `br_table`, and `phi` are private inside those methods. There is no `emit_br`. Join values are the only names that leave an arm (lowerer invariant 1). `finish` derives predecessors from terminators, so the edge list has one writer (invariant 2).

Screened against shallow modules, pass-through stages, split owners, and two ways to do one job. The five functions hide desugar, fuel order, the heap, and the catalog. There is one heap, one status function, and one fault-to-code map. A runs only as `interp(lower(A))`. The CLI loads `.strake` text.

### Data model

The sketch is structured A after checking: binds, `if`, `loop`, `for`, `match`, `assert.eq`, `hole`, `ret`, calls, prelude ops, with source names and spans. `emit` numbers vregs in domination order. Parameters are `%0..%k-1` (LOWERING "Names → SSA"). Names are dropped with the sketch.

`Program` is functions of blocks of `Inst { op, dst, span }`. `dst` is the vreg a producing instruction defines. The opcode enum is the closed card (LOWERING "Opcode sketch"): `br br_if br_table call ret phi trap`; `add sub mul div eq ne lt le gt ge`; the value-heap ops; `load store`; `const.i32 const.i64 const.f32 const.f64 const.bool const.unit`. One `add`, with the type on the operand (`NumTy`), matching "types already on the operands". `br_if` is the two-target form (LOWERING "if").

`trap`'s immediate is only what the lowerer plants: `Hole` for a draft hole (LOWERING "Holes") and `Lower` for the `for` desugar's `Err` arm (LOWERING "for x in xs"). `E_FUEL`, `E_STACK`, `E_DIV0`, `E_OOB`, and instantiate-time `E_IMPORT` are faults raised by `exec`, so the lowerer cannot emit `trap E_FUEL`. User-written `trap` is Q6.

`assert.eq` is not an opcode (SPEC "Tests"). LOWERING "assert.eq desugar" writes `diag`, and the opcode card has no `diag`. LOWERING "Calls, tests, properties" says the test export carries a diag sidecar. Reading that keeps the card closed: the mismatch path is a `ret` whose kind is `TestFail { want, got }` with both operands vregs, evaluated when that `ret` runs. `ret 0` and an explicit `ret 1` are value returns. Explicit `ret 1` is CLI 1 and is not `E_TEST`. `ret 77` in the test or property body is `Refused`. A callee that returns i64 77 is an ordinary value (SPEC "Tests"). Confirming this reading is Q1.

The value heap is one append-only `Vec`. A handle is the index. `cons`, records, and sums store edges only to a strictly smaller index, so a cycle cannot be built. `E_CYCLE` is reserved and has no constructor (catalog; SPEC "Value heap"; LOWERING "Values in strake-1"). v0 does not reclaim. Host OOM aborts the process. `list.get` and `bytes.get` out of range return `Err` (tag 1, `unit` payload), and those functions do not raise `E_OOB` (invariant 3). `ok` is tag 0. A dataless tag carries `unit` (SPEC "Types (v0)"). Scalars are immediates. Floats are stored as bits. Region bytes are a separate `Option<Vec<u8>>`, present only after `import region mem : region`. Lowering does not allocate a region for lists (LOWERING "Regions", invariant 4).

`not` on `bool` is sugar for `eq` against `false` and is not an opcode (SPEC "Prelude"). `for` lowers as the normative index loop: `list.len` once, header phi from 0, `Err` arm `trap Lower`, one binding of the user name (LOWERING "for x in xs"). No constant folding (LOWERING "What lowering is not", L15).

### Invariant 0 and the lowerer invariants

Meaning is `interp(lower(A))` (SPEC "Why two layers"; LOWERING invariant 0). `test` and `run` call `exec` on `compile`'s `Program` only. Goldens are `.strake` files driven by the binary. Structural checks sit in `interp`'s own tests, which can see opcodes. The CLI never prints a vreg.

| Inv | Where it lives | Check |
|---|---|---|
| 0 `interp(lower(A))` | `test` / `run` only | black-box exit and stderr on the thin-slice file, then the minimum list |
| 1 joins are phis | `if_` / `loop_` / `match_` | unit: a name used out of its arm fails in `finish`; golden 4 |
| 2 structured jumps | no public `br`; `finish` checks targets | unit walk of every terminator |
| 3 list/bytes OOB is `Err` | result sum; `E_OOB` unreachable there | unit; golden 5 has no `E_OOB` |
| 4 region | missing import is check `E_REGION` and does not lower; OOB is `E_OOB` from `load`/`store`, i128 `offset + width` | unit both ways |
| 5 golden answers | asserts in the file, plus expected exit and diag code | SPEC "Minimum goldens" via `strake test` / `strake check` |
| 6 A names | span bytes; `Diag` has no vreg | `E_SHADOW` and `E_TEST` stderr contain the A name and no `%` digit |

A checked sketch that cannot lower aborts. That is a bug, not a new catalog code. `E_LOWER` stays the `for` assertion.

### Diagnostic spans

Each instruction carries the byte span of the A construct that emitted it. Hygienic temps from `for` and `forall` carry that statement's span (LOWERING "for x in xs", invariant 6). Faults and `E_TEST` copy the span onto the diagnostic. The catalog `msg` is copied verbatim; instance detail is `want`, `got`, and optional `patch` (SPEC "Agent-native mechanics"). `want`/`got` render at the mismatch `ret`: scalars as literals, floats as hex bit patterns, compounds as `handle:<id>`, import payloads not walked (catalog `render`). The JSON writer has no `%` conversion. The span object itself is Q7. Traces stay inside the capabilities the module imported (SPEC "Capability-scoped traces").

`Code` is generated from the catalog's non-reserved codes. `E_HOLE` and `E_IMPORT` each leave as one kind string, chosen by the site (catalog `kind_note`). A test `include_str!`s `errors/catalog.json` and fails if a message drifts, if `E_CYCLE` becomes constructable, or if an emitted `kind` is an array.

### Exit-code precedence (D4)

Implemented only as `diag::status`, whose comment is the catalog sentence (catalog `cli.precedence`, SPEC "Tests", D4):

```
any trap -> 2; else any 77 -> 77; else any nonzero test -> 1; else 0
```

`status` folds every test and property export of the module. A trap is never rewritten to `E_TEST` or 77. A nonzero return other than the refused `ret` becomes 1, including an explicit `ret 2`. `build` does not call `status`. Unit rows: trap with 77 → 2; 77 with a nonzero test → 77; nonzero → 1; all zero → 0. Exits for `check`, `fmt`, and usage errors are Q7.

### Fuel, stack, wrap

Fuel (SPEC "Fuel", LOWERING "Fuel and call entry order"). Every executed instruction costs 1, including `const`, `phi`, `br`, `list.cons`, and `call`. An import costs `1 + fuel_cost`. Fuel is checked before the instruction; on a shortfall, `E_FUEL` and the instruction does not run. Defaults `fuel 100000` and `pages 16`. One module directive may override each. Not per function.

Call entry, in this order:

1. Unresolved import → `E_IMPORT`. No push, no charge.
2. Else if the push would make frame 1025 → `E_STACK`. No charge.
3. Else charge, then push.

Frame 1024 is legal. Frame 1025 traps. Fuel does not replace the cap (SPEC "Stack", R4). An import occupies one frame until it returns. A re-entrant host→guest call pushes another guest frame through the same `call`. Instantiate, before any export, traps `E_IMPORT` for a missing host binding or `fuel_cost < 0` and does not charge (SPEC "Traps and region", LOWERING "Imports"). The unknown-callee case of `E_IMPORT` is a check and does not lower.

Frames are an explicit vector, so a depth-1024 test can run under debug assertions. Whether host entry itself is frame 1 is Q8. An A-level fuel golden expects `E_FUEL`, not a remaining-fuel integer, because phi and branch instructions are part of the cost and the spec does not freeze that total.

Integers `add` / `sub` / `mul` wrap, Wasm-style (SPEC "Arithmetic", R2): `wrapping_add`, `wrapping_sub`, `wrapping_mul` on i32 and i64. Integer `div` truncates toward zero. Divisor 0, and signed `MIN / -1` on i32 and i64, trap `E_DIV0`. Float division by zero is IEEE and is not `E_DIV0`. Float `eq` / `ne` compare bits (`+0` differs from `-0`; identical NaN bits compare equal). Ordered compares are IEEE, and any NaN yields false (SPEC "Float compare"). `golden/` stays i64 and bool (R3). `eq` on list, record, or sum is `E_TYPE`. Bytes `eq` is bytewise.

Region check, in i128: `offset < 0` or `offset + width > pages * 65536` traps `E_OOB`. Widths are 4, 8, and 1. Unaligned access is legal. `load` / `store` of `unit`, list, record, or sum is `E_TYPE` (SPEC "Traps and region").

Forall span is computed in i128 with no wrapping subtract. `HI < LO`, or span `≤ 0`, or span `> i64::MAX`, is check `E_FORALL`. Bounds are i64 literals or module-level integer literal bindings. No iteration cap; a legal span runs until module fuel traps (SPEC "Forall span", L11). No default-suite forall has a span above that module's fuel.

### Dependencies

Phase 1 links **`blake3` only**, and only when the hash / `use` ticket lands. SPEC "Delimiters" and "Packages" define the content hash as blake3 of the formatted UTF-8, with no formatter version mixed in. The standard library does JSON, argv, spans, and the heap. A parser generator, serde, clap, a snapshot crate, or a tokenizer would add a second error type or a second diagnostic shape. `cl100k_base` is named and no numeric budget is stated (SPEC "Agent-native mechanics"), so there is nothing to enforce.

## 2. Questions for Tom

Each default is what the tickets implement until Tom answers. None of these reopen a lock. SPEC and LOWERING stay untouched.

1. **Sidecar vs `diag`.** LOWERING "assert.eq desugar" writes `diag E_TEST`. LOWERING "Opcode sketch" has no `diag`. LOWERING "Calls, tests, properties" says a diag sidecar. **Default:** `TestFail { want, got }` on the mismatch `ret`; both are vregs; `diag` is not an opcode. Question for Tom.

2. **Header, `fuel`, `pages`.** The worked example starts with `strake 1` (SPEC "Layer A") and never says the header is mandatory. SPEC "Fuel" allows one directive each for `fuel` and `pages`. LOWERING "Regions" also lets the region import state a page count. No grammar, and no precedence. **Default:** header required, else `E_PARSE`; at most one `fuel` and one `pages` after it; the directive wins, else the import's count, else 16. The worked example is a fmt fixed point. Question for Tom.

3. **Literals and `assert.eq` order.** SPEC "ANF" allows literals. LOWERING "L6" writes `acc0 = 0` with no literal type. Implicit coercion is banned (SPEC "Banned"). The example `assert.eq r 14` is not tied to `want` / `got`. `eq` on `unit`, and ordered compares on `bool`, are unstated. **Default:** a literal on a typed op takes that op's type; a bare integer binding is i64; source order is got, then want; `eq` / `ne` on `unit` is `E_TYPE`; ordered compares are numeric only. Question for Tom.

4. **Tag spelling and `_`.** SPEC "Types (v0)" and "Prelude" spell `ok` / `err`. LOWERING "match" and the `for` desugar spell `Ok` / `Err` and `Err _`. D7 records both as non-blocking nits. **Default:** A spelling is `ok` / `err`; `_` is a match-arm discard and is not an L6 binding site. Question for Tom.

5. **Match-arm join.** `E_IF_PHI` is defined for `if` (SPEC "Control", catalog). LOWERING "match" still draws a join phi and names no code when one arm lacks the value. **Default:** arms may share one join name; a missing arm value is `E_MATCH`. Question for Tom.

6. **Forms the card names and does not spell.** Prelude lists `trap`, `load`, `store`, `{ ... }`, `x.field`, and imports (SPEC "Prelude", "Traps and region"; LOWERING "Values", "Imports") without operand grammar. **Default:** `x = load T base addr`; `store T base addr val`; `call path(args)`; `{ field: operand, ... }`; `x.field`; field and user-sum indexes are declaration order; at most one region import. User `trap` is not in the thin slice. Question for Tom: does a user `trap` exist, and which code may it carry? It must not be a second way to raise `E_FUEL`.

7. **Span JSON and exits outside the test channel.** Catalog `diag.fields` lists `span` and does not define it. `cli.precedence` covers tests (SPEC "Tests"). `check` and `fmt` exits are unstated, as are stdout vs stderr. **Default:** JSON Lines on stderr, one object, catalog field order, absent optionals omitted; `span` is `{"file","start","end"}` half-open byte offsets; `check` exits 1 on any check diagnostic else 0; `fmt` exits 1 on `E_PARSE` else 0 and writes text to stdout; usage errors exit 2, plain text, no catalog code. Question for Tom.

8. **`run`, fuel scope, entry frame.** SPEC "MVP" names `run` and defines no entry. Fuel is module-level (SPEC "Fuel"). Minimum golden 8 ("the 1025th call") does not say whether host entry occupies a frame or is a charged `call`. **Default:** `run --fn NAME` runs that export and prints one scalar (`handle:<id>` for a compound); each test, property, and run gets a fresh machine (fuel restored, empty heap, empty region); host entry pushes frame 1 and charges 1 through `call`. Question for Tom.

9. **Later-ticket gaps, one default each.** These do not block the thin slice. Question for Tom.
   - **`use` bytes.** SPEC "Packages" and `E_USE` require a blob and a blake3 match, and name no path. Multi-file is out (SPEC "Banned"). **Default:** a `use` line is check `E_USE` (missing) and does not lower. Fmt still hashes its stdout.
   - **Property export and list `forall`.** LOWERING names `test.name` only. SPEC "Tests" allows a list of literals and shows syntax only for `LO..HI`. **Default:** export `property.<name>`; list form is `forall x: T in NAME` on a module-level list literal, else `E_FORALL`.
   - **Test calls `priv`.** SPEC "Tests" and P13 say tests call exports only. A `priv fn` is still a local fn, so `E_IMPORT` does not fit. **Default:** a direct call from a test or property to `priv` is `E_TYPE`.

## 3. Phase 1 tickets

Eight PRs. T3 is the thin slice: parse, lower, and interpret one golden. T4–T6 fan out from it. T7–T8 finish the prelude card and minimum goldens 1–10. Size: **S** one module, **M** one stage, **L** a stage plus the goldens it unlocks. A checked failure does not enter `exec`. Each golden source is a fmt fixed point.

### T1. Diagnostic kernel and `build`

**Size** S. **Depends** nothing.
**Goal.** `Code`, `Kind`, `status`, the JSON writer, the catalog include test, and the `build` stub.
**Files.** Workspace `Cargo.toml` (no third-party deps yet), `crates/strake/src/lib.rs`, `main.rs`, `body/mod.rs`, `body/diag.rs`.
**Acceptance.** Precedence rows in §1, including trap+77, 77+nonzero, and `ret 2` → 1. `E_CYCLE` is not constructable. Every other catalog `msg` matches. An emitted `E_HOLE` kind is one string. `strake build` prints `build is post-MVP`, exits 2, and the stderr has no `"code"`.
**Verification.** Unit tests on `status` and the include. Black-box `strake build`.

### T2. Parse and fmt

**Size** M. **Depends** T1.
**Goal.** One private CST. Parse and fmt are its two consumers. Later tickets add forms to this parser.
**Files.** `body/syntax.rs`, fmt path on `body`.
**Acceptance.** `fn add2` and `test add2.basic` from SPEC "Layer A" parse and format idempotently at two-space indent (SPEC "Delimiters"). `if`, `for`, and `forall` parse, so later fixtures are the same parser. `E_PARSE` for a nested-call operand, `assert.eq` outside `test`/`property`, a user `%` name, odd-length hex, and a keyword off the prelude card. Indent is not syntax. Integer literals lex as i128; `-9223372036854775808` is a legal i64 literal; a forall bound that does not fit i64 is passed to check. Header per Q2.
**Verification.** Unit parser. Black-box `strake fmt` byte compare.

### T3. Thin slice: lower and interpret `add2.basic`

**Size** L. **Depends** T2.
**Goal.** The straight-line checker, the builder, private opcodes, the heap spine, `call` with the full entry order, `if_` so the success path of `assert.eq` lowers, and `strake test` of one file.
**Files.** `body/check.rs` (accept path), `body/lower.rs`, `body/interp/mod.rs`, `body/interp/op.rs`, `body/interp/ir.rs`, `body/host.rs`, `golden/add2_basic.strake`.
**Acceptance.** The file is `add2` plus `test add2.basic` only (the property waits for T7). It parses, checks, lowers, and runs. `strake test` exits 0 and stderr is empty. Invariant 0 for this file. A two-arm if-join exits 0 (minimum golden 4, success half). `Ret` kinds and both trap immediates exist. `lower` cannot name `br`. `call` implements import, then stack, then fuel.
**Verification.** Unit: exec result 14, terminator walk. Black-box `strake test golden/add2_basic.strake`.

### T4. Checks that block lowering

**Size** L. **Depends** T3.
**Goal.** Extend the T3 checker with L6, if-joins, the ship hole, unknown callees, directory selection, and `--fn`.
**Files.** `body/check.rs`, `golden/shadow.strake`, `golden/if_phi.strake`, `golden/hole_ship.strake`.
**Acceptance.** Minimum golden 3: second binding → `E_SHADOW` kind `check`, exit 1 under Q7, `exec` not entered. Outer name assigned in an arm, two sibling `for` binders, a loop param assigned in the body, a forall binder rebound → `E_SHADOW`. Minimum golden 4, failure half: missing if-arm → `E_IF_PHI`, no lower. `eq` on a list → `E_TYPE`. Unknown callee → `E_IMPORT` kind `check`, no lower. Ship hole → `E_HOLE` kind `check`, no lower (golden 10, ship half). `check <dir>` reads only `src/**/*.strake`. `--fn` keeps a test whose calls are only that export and skips a test that also calls a second export.
**Verification.** Unit codes. Black-box `strake check` on `golden/shadow.strake`.

### T5. Test channel

**Size** M. **Depends** T3.
**Goal.** Mismatch sidecar, refusal, precedence across exports, spans.
**Files.** assert/ret lowering, diag render, `golden/test_channel.strake`.
**Acceptance.** Minimum golden 9: `assert.eq` mismatch exits 1, kind `result`, code `E_TEST`, scalar `want`/`got` per Q3. `ret 77` in the test exits 77, `E_REFUSED`. A callee returning i64 77 does not refuse. A trap inside a test stays exit 2. Stderr's span slice is the A text and contains no `%` digit (invariant 6). Fall-off inserts `ret 0`.
**Verification.** Unit `status` and render. Black-box `strake test golden/test_channel.strake`.

### T6. Wrap, div0, fuel, stack

**Size** M. **Depends** T3.
**Goal.** Arithmetic and the limit goldens on the T3 machine.
**Files.** interp arith and fuel, the `fuel` directive in the T2 parser, `golden/wrap.strake`, `golden/div0.strake`, `golden/fuel.strake`, `golden/stack.strake`.
**Acceptance.** Golden 1: i64 add/sub/mul wrap, exit 0. Golden 2: divisor 0 and i64 `MIN / -1` → `E_DIV0` kind `trap`, exit 2; i32 unit cases too. Float `/0` does not trap (unit). Golden 7: module `fuel` override → `E_FUEL`, and that instruction did not run. Golden 8: the call that would make frame 1025 → `E_STACK`, fuel unchanged, entry frame per Q8. Unresolved import beats stack and does not charge (unit). Phi costs 1 (unit).
**Verification.** Unit call-entry table. Black-box `strake test` on the four files.

### T7. Lists, match, forall

**Size** L. **Depends** T4, T6.
**Goal.** Normative `for`, sums, `E_MATCH`, `E_FORALL`, property loops.
**Files.** check and lower for those forms, `golden/for_list.strake`, `golden/forall.strake`.
**Acceptance.** Golden 5: `for` over a list, and `list.get` out of range matched as `Err`, exit 0, no `E_OOB`. The desugar's `Err` arm is `trap Lower`. Golden 6: `HI < LO` → `E_FORALL`, no lower; a small inclusive property exits 0 with span ≤ fuel. Non-exhaustive match → `E_MATCH`. The first failing assert stops the property. No iterator value in the opcode enum. i128 span cases: `LO = 0`, `HI = i64::MAX` is `E_FORALL` (span does not fit i64); `LO = 1`, `HI = i64::MAX` is legal and fuel-bounded.
**Verification.** Unit: OOB result tag, desugar shape. Black-box `strake test` and `strake check`.

### T8. Rest of the card, draft holes, suite

**Size** L. **Depends** T5, T7.
**Goal.** Records, bytes, region, imports, hash, draft holes, `run` per Q8, and one CLI sweep of minimum goldens 1–10.
**Files.** check/lower/interp for those forms, `blake3` on the crate, `golden/hole_draft.strake`, one region file, one import file.
**Acceptance.** Golden 10, draft half: `--draft` reaches `trap Hole` and exits 2, kind `trap`. `bytes.get` out of range is `Err`. Record project uses declaration index. `load`/`store` of a list → `E_TYPE`. No region import → `E_REGION`, no lower. Region OOB, including a negative offset, → `E_OOB` via the i128 check; an unaligned i64 store is legal. Missing host and negative `fuel_cost` → instantiate `E_IMPORT` kind `trap`, no charge. `use` → `E_USE`, no lower (Q9). blake3 of fmt stdout has no version salt. The sweep runs goldens 1–10. No forall span exceeds that file's fuel (SPEC "Forall span", "Minimum goldens").
**Verification.** Unit bounds and instantiate. Black-box `strake check`, `strake test`, and `strake run` on the new files, then the suite.

## 4. Risks, and what Phase 1 is not

- README "Locks" and the SPEC status line still say engineering is gated. Tom's 2026-10-12 lift covers this pass and these ticket drafts. This PR does not edit those files.
- A fuel golden is a tripwire (override below the run), not a frozen instruction total.
- `E_LOWER` is unreachable when `list.len` matches the spine. Test `trap Lower` through the builder, not a lying list.
- A huge `pages` or a huge legal `forall` can abort the process. That matches SPEC "Value heap" and the fuel-only forall rule (L11).
- The i64 minimum literal is a lexer edge: sign plus digits in i128, then a range check.
- `Sketch` and `Builder` are visible inside `body`. Opcode privacy is what stops `lower` from spelling `br`. A later Wasm reader of `Program` is a module under `interp`, decided in that phase.
- Q7 and Q8 move expect files and the stack golden. Tickets cite them so the default is visible.

**Not in Phase 1.** Wasm, Cranelift, LLVM, self-host, multi-file modules, the GBNF engine, `grammar/strake.gbnf`, `llms.txt`, `strake fix`, WASI, a heap cap, `E_HEAP`, emitting `E_CYCLE`, maps, iterators, constant folding, an ISA text format, a second human diagnostic renderer, a token-budget enforcer (SPEC "MVP", "Deferrals", "Banned"). `build` exists only as the post-MVP stub.
