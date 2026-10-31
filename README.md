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

## Run the ub-gates report

The `ub-gates` job in `.github/workflows/ci.yml` scores the merge-base diff with crapper, dryer, and mutator. It sets advisory mode (`UB_GATES_ADVISORY=1`). Findings, tool errors, and a mutator timeout are reported and the job exits 0. Gate results do not fail the build. Install failures still fail the job: `uv`, `cargo install`, and `rustup component add llvm-tools-preview`.

The mutator budget is 20 minutes (`UB_GATES_MUTATOR_BUDGET_SECS=1200`) in advisory mode and in blocking mode. Changed-line scope (`--lines`) applies in advisory mode too. A timeout is reported and does not fail the job. The job `timeout-minutes` stays 45 so the budget and coverage fit in the job.

Rust coverage is `cargo llvm-cov --locked` at the version in `scripts/ub-tools.lock`. The command creates `target/coverage/rust` and writes `lcov.info` there. The mutator test command is `cargo test --locked`.

Before you open a pull request, run the gates on your machine in blocking mode. The container installs the tools. Nothing is installed on the host.

```bash
bash scripts/ub_gates_container.sh
```

`UB_GATES_ADVISORY` defaults to `0`, so a local run fails on findings in changed code. For a local report that exits 0, set `UB_GATES_ADVISORY=1`. The first line of output is the temp directory that received `.metrics`, including `ub-gates-outcome.txt`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
