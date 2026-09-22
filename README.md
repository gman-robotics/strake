# Strake

Agent-authored language that lowers to a portable ISA. One semantics, many runtimes.
Not JavaScriptCore / Bun bytecode.

**Spec:** [SPEC.md](SPEC.md) · **Lowering:** [LOWERING.md](LOWERING.md) · **Errors:**
[errors/catalog.json](errors/catalog.json)

Agents write **Strake-A** (agent ANF surface, provisional name — named values, sums,
`result`, structured control, property tests). Toolchains lower A to **`strake-1`**
(portable SSA ISA, provisional name — SSA + optional regions). Wasm and Cranelift eat
`strake-1` so existing chipset optimizers stay in play, but only **after** the
reference interpreter and `golden/` exist — Wasm/Cranelift are not part of the MVP.

## Locks (2026-09-21, revised after adversarial REVISE)

- Text: `.strake` (layer A)
- Ship ISA: `strake-1`
- CLI: `strake check | run | test | build` — MVP implements `check`, `fmt`, `run`,
  `test` only; `strake build` is not an MVP command and exits 2 with
  `build is post-MVP`
- `strake check <dir>` checks each `src/**/*.strake` file as its own module; it does
  **not** link files together
- License: MIT OR Apache-2.0
- Data: values first; linear memory is an opt-in region
- Effects: explicit capability imports (WASI is a later host adapter)
- Semantics SoT: [SPEC.md](SPEC.md) + [LOWERING.md](LOWERING.md) +
  [errors/catalog.json](errors/catalog.json) + reference interpreter of `strake-1`
- First portable backend: Wasm; first native backend: Cranelift (LLVM later) — both
  come after the interpreter and `golden/`, not before
- Agent-native rules: see SPEC.md

Eng is gated until a written re-review accepts the current docs tip.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
