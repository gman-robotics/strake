# Strake verification feature map

Index of user-facing surfaces to prove on the reference CLI. Each file is one feature; drive it via `scripts/verify.sh` or the steps in the feature doc.

| Feature | Main today | Extend when `golden/` lands |
|---------|------------|-----------------------------|
| [cli-top-level-usage](cli-top-level-usage.md) | yes | — |
| [build-post-mvp](build-post-mvp.md) | yes | — |
| [check-usage-contract](check-usage-contract.md) | yes | add real `check` on files |
| [fmt-test-run-usage](fmt-test-run-usage.md) | usage only | wire real commands |
| [rust-test-suite](rust-test-suite.md) | yes | add golden black-box rows |
| [golden-fixtures](golden-fixtures.md) | smoke (T2–T4) | extend with new goldens |

Run all mapped features that apply: `.cursor/skills/verify-strake/scripts/verify.sh`.
