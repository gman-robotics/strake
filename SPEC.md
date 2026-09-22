# Strake v0 spec

Status: locks as of 2026-09-21 (Tom accepted the agent-authoring changes).
Not JSC / Bun bytecode.
Public: https://github.com/gman-robotics/strake
License: MIT OR Apache-2.0

## Why two layers

The portable agent ISA was never wrong. Existing compilers (Cranelift, later LLVM; Wasm) already know chipsets. That work stays on **`strake-1`**.

Agents do not author that ISA by hand. They author **Strake-A** (named-value ANF). The toolchain lowers A → `strake-1`. One semantics. Many runtimes. The ISA is the leverage point; A is the native surface.

```
intent / stories     host artifact (not in the ISA)
Strake-A  .strake    named values, sums, result, if/loop/match, property tests
strake-1             SSA + explicit regions; what backends eat
interp / Wasm / Cranelift
```

---

## Stamped product locks

- Names: `.strake` (A text), `strake-1` (ship ISA), CLI `strake check | run | test | build`, crate `strake`
- SoT: this spec + reference interpreter of **`strake-1`** (A is defined by its lowering)
- Effects: capability imports; WASI is a host adapter later
- First portable backend: Wasm; first native: Cranelift
- Tests + JSON diagnostics first-class
- `strake fmt` mandatory before hash

---

## Layer A — what agents write

Named values. Compiler assigns SSA ids.

```
strake 1
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

### Values first

Default data: scalars, records, sums, `result T E`, `bytes`.
Linear memory is an **opt-in region** (`region` / `buf`), not the universe. `alloc`/`load`/`store` exist only inside a region capability.

### Errors

- `ok T` / `err E` are values; `match` them.
- `trap` is process death (OOB, overflow-fuel, unimplemented host). Do not use `trap` for recoverable failure.

### Control

`if`, `loop` with single `break`/`continue`, `match` on sums. No raw unstructured CFG in A. Lowering may emit blocks/phis in `strake-1`.

### Holes

`x = hole i64` legal under `strake check --draft`. Illegal in ship modules.

### Tests

- Example tests call **exports only** (no SSA temps, no region offsets).
- Property tests (`forall` over finite ranges) are in the language.
- Gherkin / stories stay **above** the file. A host may generate `test` blocks from them. Not ISA syntax.

### Packages

`use add2@blake3:<hex>` (or equivalent content address after `fmt`). Files are a checkout convenience.

### Incremental unit

`strake check --fn add2` typechecks and runs tests that only reference that export.

### Prelude (frozen v0)

`add sub mul div eq ne lt le gt ge`, `call ret`, `ok err match`, `if loop break continue`, `load store` (region only), `trap`, `assert.eq`, `hole`.
If it is not on this card, it is not v0.

### Fuel

Module defaults: `fuel 100000`, `pages 16`. Overflow is a trap. Agents should not set these per function unless a test demands it.

---

## Layer `strake-1` — what compilers eat

- SSA (`%n`), explicit blocks, linear regions, import table, opcode enum.
- This is the portable ISA. Cranelift/Wasm/LLVM consume this, not A.
- Reference interpreter executes `strake-1` (or A after a specified lowering — same answers).
- Differential tests: interp vs Wasm must match on the golden suite.

---

## Agent-native mechanics (still in)

- Prefix-stable grammar: `grammar/strake.gbnf` for A.
- One spelling per construct. Aliases are formatter errors.
- Token budget tokenizer: `cl100k_base`.
- Diagnostics: stable `code` + `want`/`got` + optional `patch`. `strake fix` applies patches.
- Traces capability-scoped; no host secrets in the default next-turn payload.
- Model/tool use is `import`, not an `llm` keyword.
- Refusal: ship module may `ret 77` (author-refused). Grammar must not erase “no”.
- Language pack: `SPEC.md`, GBNF, `errors/catalog.json`, `llms.txt`.

---

## Banned

`eval`; indent-syntax; a second drifting human dialect; Unicode opcodes; Gherkin in the ISA; implicit coercion; host GC objects as values; asking agents to write `strake-1` by hand.

## Veto

Helps specify, check, or localize a failure — or it waits.

## Next (still not implement tonight)

1. Opcode table for `strake-1` + A lowering notes
2. `grammar/strake.gbnf`
3. `errors/catalog.json`
4. Gated: ref interpreter + A→ISA lowerer against golden files
