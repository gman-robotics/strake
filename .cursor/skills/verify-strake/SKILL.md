---
name: verify-strake
description: >-
  End-to-end verification for the Strake reference CLI on main: build the
  `strake` binary, run doctor checks, drive user-facing commands (usage lines,
  post-MVP `build` stub), and run the Rust integration tests. Use before
  shipping CLI or golden fixture changes, or when CI verify job fails.
---

# verify-strake

Drive the **Strake reference toolchain** the way a user does: compile the CLI, invoke subcommands, and assert exit codes and stdio. On `main` today the language pipeline is stubbed; this skill proves the **shipped CLI contract** (usage, `build` stub, diag wiring) and the crate test suite. When `golden/*.strake` lands (e.g. T2 parse/fmt), extend `features/golden-fixtures.md` and the optional block in `scripts/verify.sh`.

## Launch

One-shot build (no long-lived server):

```bash
cargo build --locked -p strake
```

Binary path: `target/debug/strake` (release: `target/release/strake` if you passed `--release`).

Teardown: nothing to keep alive. If you started a background process for debugging, kill only that PID (never `pkill strake`).

## Doctor

Read-only “worth driving?” check:

```bash
.cursor/skills/verify-strake/scripts/doctor.sh
```

Passes when the binary exists, runs, and prints the top-level usage on empty argv (exit 2).

## Drive

Full product verification (build + doctor + CLI scenarios + `cargo test`):

```bash
.cursor/skills/verify-strake/scripts/verify.sh
```

Harness: subprocess calls to the built `strake` binary plus `cargo test --locked -p strake`. Selectors are **argv and exit codes**, not coordinates.

Optional golden pass (only when `golden/` exists at repo root):

- The script auto-runs `golden-smoke` cases listed in `features/golden-fixtures.md` if that directory is present.

## Evidence

Proof artifacts go under:

`.cursor/skills/verify-strake/evidence/<UTC-timestamp>/`

Each run writes:

- `summary.txt` — pass/fail and commands run
- `build-post-mvp.txt` — stdout/stderr/exit from `strake build`
- `check-usage.txt` — stderr from `strake check` (no path)
- `cargo-test.log` — tail of `cargo test`

Standards: exercise the real CLI entrypoints (`main` → `dispatch`), not internal `#[cfg(test)]` helpers alone; capture **action + outcome** (exit code and streams); side effects are limited to `target/` from build/test.

## Cleanup

`verify.sh` registers an `EXIT` trap that removes only **scratch dirs it created** under `/tmp/strake-verify-*`. Evidence under `.cursor/skills/verify-strake/evidence/` is **never** deleted by cleanup.

## Helpers

| Script | Purpose |
|--------|---------|
| `scripts/doctor.sh` | Read-only readiness |
| `scripts/verify.sh` | Full verification + evidence |

Both are executable; invoke exactly as shown above from the repository root.

## Maintenance

After new CLI behavior or `golden/` fixtures ship, update the feature map under `features/` and add cases to `verify.sh` (or a new `scripts/drive-<feature>.sh` if the flow grows). Periodic upkeep: `maintain-verification-skill` (pstack hub).
