# Strake v0 spec

Status: locks as of 2026-09-21, revised after adversarial REVISE. Tom stamped SuperGrok
adjudication locks 1–18 (catalog, Layer A restore, L6, fuel/stack/div0/OOB/import,
test-fail channel, float compare, value heap, fuel schedule, closed ISA gaps, indent vs
listings, check/README/build, forall span, A ≡ strake-1, golden deferral, GBNF/llms.txt
deferral, E_CYCLE reserved, WasmGC dismissal). **Eng stays gated** until a written
re-review accepts this docs tip.
Not JSC / Bun bytecode.
Public: https://github.com/gman-robotics/strake
License: MIT OR Apache-2.0
Bootstrap compiler: Rust crate `strake`.

## Why two layers

Agents author **Strake-A** (agent ANF surface — file extension `.strake`; the name is
still provisional pending a later rename pass) — named values, sums, list, result,
if/loop/for/match, property tests. The toolchain lowers A to **`strake-1`** (portable
SSA ISA — also provisional) so Cranelift / Wasm / later LLVM can do chipset work. One
semantics.

```
intent / stories     host artifact (not in the ISA)
Strake-A  .strake    named values, sums, list, result, if/loop/for/match, property tests
strake-1             SSA + optional regions; what backends eat
interp / Wasm / Cranelift
```

A is defined by this spec plus the lowering in [LOWERING.md](LOWERING.md).

**Invariant 0:** same answers from A and from the `strake-1` it becomes. The meaning of
an A program is `interp(lower(A))`.

---

## MVP (gated eng when review accepts)

In scope: parse + check + fmt A; lower to `strake-1`; interpret `strake-1`; run
`golden/`; JSON diags from [errors/catalog.json](errors/catalog.json).

Out of scope for first implement: Wasm, Cranelift, self-host, multi-file packages,
GBNF-driven constrained decoding (the grammar and constrained-decoding engine both land
later together — see [Deferrals](#deferrals); no empty `grammar/strake.gbnf` ships as a
placeholder before then).

MVP implements `check`, `fmt`, `run`, `test` only. `strake build` is not an MVP command:
it exits **2** with message `build is post-MVP` and emits no catalog code. `strake fix`
is not an MVP command either — see [Deferrals](#deferrals); diags may still carry
`patch`.

---

## Layout

Compiler repo, target layout (this docs tip ships `SPEC.md LOWERING.md README.md
LICENSE-MIT LICENSE-APACHE errors/catalog.json` only; `grammar/strake.gbnf`, `llms.txt`,
`crates/strake/`, and `golden/` are deferred deliverables — see
[Deferrals](#deferrals) — and do not exist as empty placeholders in the meantime):

```
SPEC.md LOWERING.md README.md LICENSE-MIT LICENSE-APACHE
grammar/strake.gbnf
errors/catalog.json
llms.txt
crates/strake/
golden/
```

Strake package (agents): `src/*.strake`. Tests live in the same file as exports.
`tests/` only if a file is too large. `strake check <dir>` loads `src/**/*.strake`.

v0 module = **one file**. Every `fn` is exported unless marked `priv` (`priv fn` is the
export suppressor).

---

## Stamped product locks

- Names: `.strake` (A), `strake-1` (ISA), CLI `strake check | run | test | build`, crate `strake`
- SoT: this spec + LOWERING.md + catalog + reference interpreter of `strake-1`
- Effects: capability imports; WASI adapter later
- First portable backend: Wasm; first native: Cranelift — **after** interp+golden
- Tests + JSON diagnostics first-class
- `strake fmt` mandatory before hash

---

## Layer A

Named values. Compiler assigns SSA ids at lower time.

**ANF.** Operands of ops, calls, and `assert.eq` are names or literals. A nested call as
an operand is `E_PARSE`. Every function has a return type; a function with nothing to
return uses `unit` and `ret unit`.

**Delimiters.** Blocks close with `end`. Indent is not syntax — the parser ignores
leading whitespace. `strake fmt` emits two-space indent and that output is stable for
the life of v0. The content hash is blake3 of the formatted UTF-8, with no formatter
version mixed in. The worked example immediately below is a complete, legal A module.
Short listings elsewhere, such as the Lists example under "Lists", are body fragments —
legal-ANF statement sequences (no nested-call operands, `end`-delimited, no `#`/`...`
placeholders standing in for statements) that would need a surrounding `fn` to run, not
full modules. Other code fences in this spec and in LOWERING.md are illustrative only —
some elide a body with `...` or `...body...` for exposition, and LOWERING.md's
post-arrow listings show `strake-1` (the lowered ISA), not A — and are not independently
legal A unless stated otherwise.

```
strake 1

type pair { a: i64, b: i64 }

fn add2(a: i64, b: i64) -> i64
  x = add i64 a b
  y = mul i64 x 2
  ret y
end

test add2.basic
  r = call add2(3, 4)
  assert.eq r 14
end

property add2.zero
  forall n: i64 in -10..10
    r = call add2(n, 0)
    w = mul i64 n 2
    assert.eq r w
  end
end
```

### Types (v0)

- Scalars: `i32 i64 f32 f64 bool`
- `unit` — one value, written `unit`; the return type of side-effect-only functions (`ret unit`)
- `bytes` — literal is even-length hex, e.g. `x"dead"`
- Records: `type name { field: T, ... }`
- Sums: `type name { Tag T | Tag2 U | Nil }` including `result T E` = `Ok T | Err E` (`ok` is tag 0, `err` is tag 1; a dataless tag carries `unit`)
- `list T` — **in v0**, immutable, finite, ordered

No strings distinct from `bytes`. No maps. No iterators as objects.

### Lists

```
t0 = list.empty i64
t1 = list.cons 2 t0
xs = list.cons 1 t1
n  = list.len xs
r  = list.get xs 0
for x in xs
  y = add i64 x 0
end
```

`r` has type `result i64 unit` (`Err` on OOB, never a trap). This listing is ANF: every
operand of `list.cons`/`list.get` is a name or literal, never a nested call.

- `list.empty T -> list T`, `list.cons h t -> list T`, `list.len -> i64`
- `list.get -> result T unit` (OOB is `Err`, never a trap, never `E_OOB`)
- `for x in xs` is A syntax. Lowers to a hygienic index loop (see LOWERING.md lock 7).
  No extra iterator type.
- No in-place update. Build a new list or use a region.

### Bytes

- `bytes.new hex -> bytes` — `hex` is an even-length hex literal payload, the same
  grammar as the `x"dead"` literal (not an SSA name); the result is a `bytes` handle.
- `bytes.len -> i64`
- `bytes.get -> result i64 unit` (byte value 0..255; OOB is `Err`, never `E_OOB`)

### Errors

- `ok T` / `err E` values; `match` them. `list.get` and `bytes.get` OOB are `Err`
  results, not traps — recoverable failure is `result`, never a trap.
- `trap` = death: `E_FUEL`, `E_STACK`, `E_DIV0`, `E_OOB`, `E_IMPORT` (instantiate-time:
  missing host binding, or negative `fuel_cost` — the unresolved-callee case of
  `E_IMPORT` is a check, not a trap; see "Traps and region" below), `E_HOLE` (`--draft`
  execution reaches a hole — the ship-check case of `E_HOLE` is a check, not a trap),
  `E_LOWER`. Not recoverable failure.

### Control

`if`, `loop` + one `break`/`continue`, `for x in xs`, `match` on sums. No unstructured
CFG in A.

**L6 — one binding site per name.** A source name has one binding site per function. A
second `name =` for the same name is check `E_SHADOW`. The formatter must not rename a
rebind into legality (legality cannot depend on `fmt`). Binding sites are parameters,
`name =`, `for` binders, match-arm binders, and loop parameters. Loop-carried values are
header phis written as loop parameters, not as assignments — `continue` supplies
back-edge operands and is not itself a binding. Both `if` arms may contribute the same
join name; that is one binding site. A join name used after `if` and missing on one arm
is `E_IF_PHI`. A name bound outside an arm and assigned inside an arm is `E_SHADOW`. The
`for` desugar uses hygienic `%`-prefixed temporaries (exempt from `E_SHADOW`) and does
not assign the user binder twice — see LOWERING.md for the exact desugar. The `%`
prefix is reserved for the lowerer: user source may not spell a `%`-prefixed name
(check `E_PARSE`); those temps are exempt from `E_SHADOW` because they are not source
names at all.

### Holes

`x = hole T` in `--draft` only. Ship check rejects a hole before lower (check
`E_HOLE`). `--draft` execution traps on a hole (trap `E_HOLE`).

### Tests

Exports only. `assert.eq` is not an opcode; it is legal only inside `test` and
`property` (elsewhere `E_PARSE`). Desugar: compare with `eq`; on mismatch emit diag
`E_TEST` (`want`/`got`) and `ret 1` — it does not trap. The lowerer inserts `ret 0` at
the end of a test/property that falls off the end. Explicit `ret 77` in a test or
property is result `E_REFUSED` (CLI 77) — a callee that merely *returns* the i64 `77`
does not refuse. A trap inside a test stays that trap (CLI 2) and is never rewritten to
`E_TEST` or 77.

CLI status: any trap → 2; else any 77 → 77; else any nonzero test → 1; else 0. Status 77
is reserved for `E_REFUSED`.

Property `forall` is over finite integer ranges or finite lists of literals. The first
failing assert returns immediately. Stories/Gherkin stay outside the file.

### Packages

`use name@blake3:<hex>` after `fmt`. Check `E_USE` if the blob is missing or the hash
does not equal blake3 of the formatted text.

### Incremental

`strake check --fn add2` — that export plus tests in the same file that only call it.

### Prelude (frozen v0) — complete card

Arithmetic / compare: `add sub mul div eq ne lt le gt ge`
Control: `call ret if loop break continue for match`
Sums: `ok err`
Lists: `list.empty list.cons list.len list.get`
Bytes: `bytes.new bytes.len bytes.get`
Records: field construct `{ ... }` and project `x.field`
Regions: `load store` (only inside a region import)
Other: `trap assert.eq hole`

`not` on `bool` is A sugar for `eq bool x false` and is not itself an opcode.
Bool combinators `and`/`or` are not v0 — use `if`.

If it is not on this card, it is not v0.

### Arithmetic

- `add sub mul` on integers **wrap** (two's complement), same as Wasm.
- Integer `div` is signed, truncates toward zero (Wasm `div_s`). Divisor `0`, or signed
  `MIN / -1` on `i32`/`i64`, traps `E_DIV0`.
- Float `add`/`sub`/`mul`/`div` are IEEE 754. Float division by zero is IEEE (±inf /
  NaN) and is **not** `E_DIV0`.
- v0 has no `rem`, `mod`, bitwise ops (`and`/`or`/`xor`/`shl`/`shr`), or casts, and none
  of those desugar into loops. This is a closed absence, not a smaller card waiting to
  grow.

### Float compare

| op | f32 / f64 semantics |
|---|---|
| `eq` / `ne` / `assert.eq` | Bitwise: same bits, including NaN payload; `+0` and `-0` differ. |
| `lt` / `le` / `gt` / `ge` | IEEE 754 ordered: any NaN → false; `+0` and `-0` compare equal. |

`assert.eq` desugars to the bitwise `eq`. Wasm later: bitwise `eq` is reinterpret-to-int
then integer `eq`, not `f32.eq`/`f64.eq`; ordered compares lower to Wasm `f*.lt` /
`f*.le` / etc. v0 `golden/` stays `i64`/`bool` only, so NaN is not a suite problem.

`eq` is not defined on `list`, `record`, or `sum` values (`E_TYPE`). Bytes `eq` is
bytewise content equality.

### Traps and region

- `E_FUEL` trap — remaining fuel is less than the next instruction's cost.
- `E_STACK` trap — the call would make frame 1025.
- `E_DIV0` trap — integer divisor 0, or signed `MIN / -1` on `i32`/`i64`.
- `E_OOB` trap — region address out of bounds on `load`/`store`.
- `E_REGION` check — `load`/`store` used and the module has no
  `import region mem : region`.
- `E_IMPORT` check — callee is neither a local `fn` nor an `import`; **trap** —
  instantiate-time host binding missing, or `fuel_cost < 0`.
- `E_HOLE` check — ship module has a hole; **trap** — `--draft` execution reaches a hole.
- `E_LOWER` trap — the `for` desugar observed `list.get` `Err` while index < len (a
  lowering assertion; unreachable in a well-formed `for` loop).

These codes are not interchangeable. `list.get`/`bytes.get` OOB is `Err`, never `E_OOB`.

Region: only exists as capability `import region mem : region` (no bare `region`
grant). Page size is 65536 bytes. Default `pages 16` (1048576 bytes) unless the import
states another page count. Addresses are little-endian byte offsets. The range check
uses i128 `offset + width`; `offset < 0` or `offset + width > pages * 65536` is `E_OOB`.
Widths: `i32`/`f32` = 4, `i64`/`f64` = 8, `bool` = 1. Unaligned access is legal.
`load`/`store` of `unit`, lists, records, or sums is `E_TYPE`.

### Fuel

Defaults `fuel 100000`, `pages 16`. One module-level directive may override each. Not
per-function.

Each executed `strake-1` instruction costs 1, including `const`, `phi`, `br`,
`list.cons`, and `call`. An import call costs `1 + fuel_cost`, where `fuel_cost` is a
host `i64`, default 0, must be ≥ 0. Fuel is checked *before* the instruction; if
remaining fuel is less than the cost, trap `E_FUEL` and do not execute.

Call entry order: unresolved import → `E_IMPORT`, do not push or charge; else depth
would exceed 1024 → `E_STACK`, do not charge; else charge, then push. Frame 1024 is
legal; frame 1025 traps. A host import occupies one frame until it returns. Re-entrant
host→guest calls push more guest frames.

### Forall span

`LO..HI` is inclusive. Check-time, in **i128**: if `HI < LO`, or
`span = i128(HI) - i128(LO) + 1` is ≤ 0 or `> 9223372036854775807`, check `E_FORALL`.
Do not use wrapping `sub`. `LO` and `HI` are `i64` literals or module-level integer
literal bindings; otherwise `E_FORALL`. No iteration cap — the runtime bound remains
module fuel only. No v0 golden may use a `forall` whose static span exceeds that
module's fuel; an `E_FUEL` golden sets an explicit module `fuel N` lower than the
default.

### Value heap

**v0 leaks.** The value heap is append-only and is not reclaimed: no GC, refcount,
arena, or drop. Handles are indices, not host GC objects. A host OOM is a process
crash, not an ISA trap. `pages` sizes the region only; the value heap has no byte cap in
v0 (see [Deferrals](#deferrals)). `eq` is not defined on lists, records, or sums
(`E_TYPE`); bytes `eq` is bytewise content equality.

### Stack

Max call depth **1024**. Overflow (frame 1025): trap `E_STACK`. Fuel does not replace
this.

---

## Layer strake-1

SSA `%n`, blocks, phis, import table, optional linear regions, closed opcode enum.
Backends consume this. Agents do not write it.

See [LOWERING.md](LOWERING.md).

---

## Modules and CLI

A module is one file. `strake check <dir>` checks each `src/**/*.strake` file as its own
module — it does **not** link them. `strake check <file>` checks that file.
`strake check --fn name` checks that export plus tests in the same file that only call
it. Cross-file resolution is `use name@blake3:<hex>` after `fmt`, or check `E_USE` if the
blob is missing or the hash mismatches. The CLI name `build` stays in the product lock;
in MVP `strake build` exits 2 with message `build is post-MVP` and no catalog code. MVP
implements `check`, `fmt`, `run`, `test` only. `strake fix` is not an MVP command; diags
may still carry `patch`.

---

## Agent-native mechanics

- `grammar/strake.gbnf` for A (prefix-stable) — lands together with the
  constrained-decoding engine, not before; see [Deferrals](#deferrals)
- One spelling; formatter rejects aliases
- Token budget: `cl100k_base`
- Diags: stable `code` + `kind` + `msg` + `span`, optional `want`/`got`/`patch`.
  `want`/`got` render scalars only (floats as hex bit patterns); compounds render
  `handle:<id>`; import payloads are not deep-printed. `kind` is a single string on
  every emitted diagnostic (`"check"`, `"trap"`, or `"result"`), never an array. The
  catalog lists two possible kinds for `E_HOLE` and `E_IMPORT` because each code can
  fire in either context (see [errors/catalog.json](errors/catalog.json)); a given
  emitted instance picks the one that actually happened.
- Model/tools = `import`, not `llm`
- Refusal: `ret 77` (result `E_REFUSED`, CLI 77)
- Pack: SPEC.md, LOWERING.md, GBNF, `errors/catalog.json`, `llms.txt`

**P11 — capability-scoped traces.** Diagnostic and trace payloads are scoped to the
capabilities the failing module actually imported. The default next-turn payload
carries no host secrets.

---

## Banned

`eval`; indent-syntax; second dialect; Unicode opcodes; Gherkin in ISA; implicit
coercion; host GC values; hand-written `strake-1`; maps/iterators; multi-file modules in
v0.

## Veto

Specify, check, or localize a failure — or wait.

---

## Minimum goldens

Text-only for now (see [Deferrals](#deferrals) — `golden/` files land with eng). The
default suite must cover at least:

1. `i64` add/sub/mul wrap
2. div-by-zero and `i64 MIN / -1` → `E_DIV0`
3. a second assignment to a name → `E_SHADOW`, no run
4. `if`-join success, and a missing-arm case → `E_IF_PHI`
5. `for` over a list plus `list.get` OOB = `Err` (result, not trap)
6. `HI < LO` → `E_FORALL`, and a small `forall` that passes
7. a module `fuel` override that exhausts → `E_FUEL`
8. the 1025th call → `E_STACK`
9. `assert.eq` fail returns 1 with `E_TEST`; `ret 77` returns 77 with `E_REFUSED`
10. a ship hole is check `E_HOLE`; a `--draft` hole traps `E_HOLE`

No `forall` in the default suite has a static span that exceeds that module's fuel.

---

## Deferrals

- `grammar/strake.gbnf` and `llms.txt` are deferred until constrained decoding lands.
  They are pack members (P8) but not blockers for the semantics re-review that gates the
  interpreter. Normative syntax until then is this spec. Do not ship empty grammar files
  as placeholders.
- `golden/` fixture files are deferred until eng; the minimum list above is locked now
  as text.
- `strake fix` is deferred; diags may still carry `patch` ahead of the command existing.
- There is no value-heap byte cap (`heap_bytes`) and no `E_HEAP` code in v0.

## Next

1. Re-review of this docs tip (no eng implement until written accept)
2. `golden/` fixtures, `grammar/strake.gbnf`, and `llms.txt` as review/eng deliverables
3. Then gated: reference interp + lowerer vs `golden/`
