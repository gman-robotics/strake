# Rust test suite

## Sub-features

- Library unit tests (`body::diag`, `lib` CLI helpers).
- Integration tests: `check_usage_sync`, `cli_build`, `status_guard`, `t2_golden`, `t3_golden`, `t4_golden`.

## How to get to it (user POV)

From the repo root, run `cargo test` for the strake crate (CI runs the whole workspace).

## Driving it with verify.sh

```bash
cargo test --locked -p strake
```

Expect all tests passed; log captured under evidence.

## Gotchas

- Workspace has a single member today; `cargo test --locked` at root is equivalent.
- Golden black-box tests exercise more cases than `verify.sh` smoke; keep both when adding T5+ behavior on `main`.
