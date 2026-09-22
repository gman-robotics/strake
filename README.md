# Strake

Agent-authored language that lowers to a portable ISA. One semantics, many runtimes.
Not JavaScriptCore / Bun bytecode.

**Spec:** [SPEC.md](SPEC.md)

Agents write **Strake-A** (named values, sums, `result`, structured control, property tests).
Toolchains lower A to **`strake-1`** (SSA + optional regions). Wasm and Cranelift eat `strake-1` so existing chipset optimizers stay in play.

## Locks (2026-09-21)

- Text: `.strake` (layer A)
- Ship ISA: `strake-1`
- CLI: `strake check | run | test | build`
- License: MIT OR Apache-2.0
- Data: values first; linear memory is an opt-in region
- Effects: explicit capability imports (WASI is a later host adapter)
- Semantics SoT: spec + reference interpreter of `strake-1`
- First portable backend: Wasm
- First native backend: Cranelift (LLVM later)
- Agent-native rules: see SPEC.md

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
