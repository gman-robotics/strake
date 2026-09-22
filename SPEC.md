# Strake v0 spec

Status: locks as of 2026-09-21 including pre-review stamps (layout, MVP, arith, stack, modules, catalog).
Not JSC / Bun bytecode.
Public: https://github.com/gman-robotics/strake
License: MIT OR Apache-2.0
Bootstrap compiler: Rust crate `strake`.

## Why two layers

`strake-1` is the portable ISA so Cranelift / Wasm / later LLVM can do chipset work.
Agents author **Strake-A**. The toolchain lowers A → `strake-1`. One semantics.

```
intent / stories     host artifact (not in the ISA)
Strake-A  .strake    named values, sums, list, result, if/loop/for/match, property tests
strake-1             SSA + optional regions; what backends eat
interp / Wasm / Cranelift
```

A is defined by this spec plus [LOWERING.md](LOWERING.md).

---

## MVP (gated eng when review accepts)

In scope: parse + check + fmt A; lower to `strake-1`; interpret `strake-1`; run `golden/`; JSON diags from [errors/catalog.json](errors/catalog.json).

Out of scope for first implement: Wasm, Cranelift, self-host, multi-file packages, GBNF-driven constrained decoding (file may exist; engine later).

---

## Layout

Compiler repo:

```
SPEC.md LOWERING.md README.md LICENSE-MIT LICENSE-APACHE
grammar/strake.gbnf
errors/catalog.json
llms.txt
crates/strake/
golden/
```

Strake package (agents): `src/*.strake`. Tests live in the same file as exports. `tests/` only if a file is too large. `strake check <dir>` loads `src/**/*.strake`.

v0 module = **one file**. Every `fn` is exported unless marked `priv`.

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

Named values. Compiler assigns SSA ids at lower time. No shadowing (`E_SHADOW`).

### Types (v0)

`i32 i64 f32 f64 bool`, `bytes`, records, sums (`result T E` = `Ok T | Err E`), `list T` immutable.
No distinct strings. No maps. No iterator objects.

### Lists

`list.empty` `list.cons` `list.len` `list.get` → `result T unit` (OOB = `Err`).
`for x in xs` desugars to an index loop.

### Arithmetic

- `add sub mul` on integers: **wrap** (two's complement), same as Wasm.
- Integer `div` by zero: **trap** `E_DIV0`.
- `eq`/`assert.eq` on floats: **bitwise**. v0 `golden/` uses `i64`/`bool` only so NaN is not a suite problem.

### Control / errors / holes / tests / packages / incremental / prelude / fuel

Unchanged from prior locks: `ok`/`err` + `match`; `trap` is death; holes draft-only; tests on exports; `forall` span must fit `i64` and `HI >= LO` else `E_FORALL`; runtime bound is **fuel only**; prelude card frozen; defaults `fuel 100000`, `pages 16`.

### Stack

Max call depth **1024**. Overflow: trap `E_STACK`. Fuel does not replace this.

---

## Layer strake-1

See [LOWERING.md](LOWERING.md).

---

## Agent-native mechanics

Prefix-stable GBNF for A; one spelling; token budget `cl100k_base`; diags use catalog codes + optional `patch`; capability-scoped traces; tools are `import`; refusal `ret 77`.

---

## Banned

`eval`; indent-syntax; second dialect; Unicode opcodes; Gherkin in ISA; implicit coercion; host GC values; hand-written `strake-1`; maps/iterators; multi-file modules in v0.

## Veto

Specify, check, or localize a failure — or wait.

## Next

1. Plan/system review (no Reed implement until written accept)
2. `grammar/strake.gbnf` + `llms.txt` as review deliverables if missing
3. Then gated: Rust interp + lowerer vs `golden/`
