# Strake verification feature map

Index of user-facing surfaces to prove on the reference CLI. Each file is one feature; drive it via `scripts/verify.sh` or the steps in the feature doc.

| Feature | Main today | Extend verify when |
|---------|------------|-------------------|
| [cli-top-level-usage](cli-top-level-usage.md) | yes | — |
| [build-post-mvp](build-post-mvp.md) | yes | real `build` ships |
| [check-usage-contract](check-usage-contract.md) | yes (usage + `check` on goldens via smoke / `t4_golden`) | T8 blob checks change usage or new check-only goldens |
| [fmt-test-run-usage](fmt-test-run-usage.md) | usage + `fmt`/`test` on goldens (smoke + `golden_sweep`) | real `run` execution ships |
| [rust-test-suite](rust-test-suite.md) | yes (`golden_sweep`, T2–T6, S1–S5) | new integration test modules or sweep rules |
| [golden-fixtures](golden-fixtures.md) | smoke (T2–T5) + fmt idempotence | new CLI goldens on `main` (`golden/expect/` + optional `GOLDEN_SMOKE` row) |
| [gate-scripts](gate-scripts.md) | yes (`scripts/tests` via `verify.sh`) | ub-gates scope, mutator timeout, or `pr-body` contract changes |

Run all mapped features that apply: `.cursor/skills/verify-strake/scripts/verify.sh`.
