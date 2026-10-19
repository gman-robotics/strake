# Rust test suite

## Sub-features

- Library unit tests (`body::diag`, `lib` stubs).
- Integration tests: `cli_build`, `status_guard`.

## How to get to it (user POV)

From the repo root, run `cargo test` for the strake crate (CI runs the whole workspace).

## Driving it with verify.sh

```bash
cargo test --locked -p strake
```

Expect all tests passed; log captured under evidence.

## Gotchas

- Workspace has a single member today; `cargo test --locked` at root is equivalent.
- Golden black-box tests will live alongside these when fixtures exist.
