# Strake v0 spec

Status: locks as of 2026-09-21 (Tom accepted authoring-layer + these detail locks).
Not JSC / Bun bytecode.
Public: https://github.com/gman-robotics/strake
License: MIT OR Apache-2.0

## Why two layers

`strake-1` is the portable ISA so Cranelift / Wasm / later LLVM can do chipset work.
Agents author **Strake-A**. The toolchain lowers A → `strake-1`. One semantics.

```
intent / stories     host artifact (not in the ISA)
Strake-A  .strake    named values, sums, list, result, if/loop/for/match, property tests
strake-1             SSA + optional regions; what backends eat
interp / Wasm / Cranelift
```

A is defined by this spec plus the lowering in [LOWERING.md](LOWERING.md). Same answers from A and from the `strake-1` it becomes.

---

## Stamped product locks

- Names: `.strake` (A), `strake-1` (ISA), CLI `strake check | run | test | build`, crate `strake`
- SoT: this spec + LOWERING.md + reference interpreter of `strake-1`
- Effects: capability imports; WASI adapter later
- First portable backend: Wasm; first native: Cranelift
- Tests + JSON diagnostics first-class
- `strake fmt` mandatory before hash

---

## Layer A

Named values. Compiler assigns SSA ids at lower time.

```
strake 1

type pair { a: i64, b: i64 }

fn add2(a: i64, b: i64) -> i64
  x = add i64 a b
  y = mul i64 x 2
  ret y

test add2.basic
  r = call add2(3, 4)
  assert.eq r 14

property add2.zero
  forall n: i64 in -10..10
    assert.eq call add2(n, 0) mul i64 n 2
```

### Types (v0)

- Scalars: `i32 i64 f32 f64 bool`
- `bytes`
- Records: `type name { field: T, ... }`
- Sums: `type name { Tag T | Tag2 U | Nil }` including `result T E` = `Ok T | Err E`
- `list T` — **in v0**, immutable, finite, ordered

No strings distinct from `bytes`. No maps. No iterators as objects.

### Lists

```
xs = list.cons 1 (list.cons 2 (list.empty i64))
n  = list.len xs
r  = list.get xs 0          # result i64 unit   Err on OOB
for x in xs
  ...
```

- `list.empty T`, `list.cons T (list T) -> list T`, `list.len -> i64`
- `list.get` returns `result T unit` (OOB is `Err`, not trap)
- `for x in xs` is A syntax. Lowers to index loop. No extra iterator type.
- No in-place update. Build a new list or use a region.

### Errors

- `ok T` / `err E` values; `match` them
- `trap` = death (OOB **store**, fuel, missing import). Not recoverable failure.

### Control

`if`, `loop` + one `break`/`continue`, `for x in xs`, `match` on sums. No unstructured CFG in A.

### Holes

`x = hole T` in `--draft` only.

### Tests

Exports only. Property `forall` over finite integer ranges or finite lists of literals. Stories/Gherkin stay outside the file.

### Packages

`use add2@blake3:<hex>` after `fmt`.

### Incremental

`strake check --fn add2` — that export + tests that only call it.

### Prelude (frozen v0) — complete card

Arithmetic / compare: `add sub mul div eq ne lt le gt ge`
Control: `call ret if loop break continue for match`
Sums: `ok err`
Lists: `list.empty list.cons list.len list.get`
Records: field construct `{ ... }` and project `x.field`
Regions: `load store` (only inside a region import)
Other: `trap assert.eq hole`

If it is not on this card, it is not v0.

### Fuel

Defaults `fuel 100000`, `pages 16`. Trap on overflow. Not per-function unless a test says so.

---

## Layer strake-1

SSA `%n`, blocks, phis, import table, optional linear regions, closed opcode enum.
Backends consume this. Agents do not write it.

See [LOWERING.md](LOWERING.md).

---

## Agent-native mechanics

- `grammar/strake.gbnf` for A (prefix-stable)
- One spelling; formatter rejects aliases
- Token budget: `cl100k_base`
- Diags: stable `code` + `want`/`got` + optional `patch`
- Capability-scoped traces
- Model/tools = `import`, not `llm`
- Refusal: `ret 77`
- Pack: SPEC.md, LOWERING.md, GBNF, `errors/catalog.json`, `llms.txt`

---

## Banned

`eval`; indent-syntax; second dialect; Unicode opcodes; Gherkin in ISA; implicit coercion; host GC values; hand-written `strake-1`; maps/iterators in v0.

## Veto

Specify, check, or localize a failure — or wait.

## Next artifacts (not interpreter yet)

1. Opcode table in LOWERING.md (first cut is there)
2. `grammar/strake.gbnf`
3. `errors/catalog.json`
4. Gated: interp + lowerer vs golden `.strake`
