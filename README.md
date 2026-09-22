# Strake

Agent-first intermediate representation: token-light ISA, tests and diagnostics in the IR, one semantics, many runtimes.

Spec-first. Not JavaScriptCore / Bun bytecode.

## Locks (2026-09-21)

- Text: `.strake`
- Binary / version: `strake-1`
- CLI: `strake check | run | test | build`
- License: MIT OR Apache-2.0
- Memory: linear heap; no host-GC objects in v0
- Effects: explicit capability imports (WASI is a later host adapter)
- Semantics SoT: spec + reference interpreter
- First portable backend: Wasm
- First native backend: Cranelift (LLVM later)
- Next work: short spec, then interpreter. No runtime code in this commit.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
