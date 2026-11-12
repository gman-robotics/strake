# Strake

Scope is this repository. Do not change branch protection, secrets, or other repositories.

## Checks

These jobs block a pull request. A skip or an advisory result is not a pass.

- `fmt, clippy, test`
- `verify-strake skill`, which runs `.cursor/skills/verify-strake/scripts/verify.sh`
- `ub-gates`. Crapper fails a new or changed function with CRAP above 10, and a pre-existing over-threshold function whose score rose. Untouched pre-existing debt is reported and does not fail. Dryer fails changed code at similarity 0.8 or higher. A mutator timeout fails. A surviving mutant fails.
- `pr-body` runs `scripts/check_pr_body.py` on pull requests. Every heading in `.github/pull_request_template.md` must be present and non-empty.

## Evidence

The comment `prove-it-works evidence @<head SHA>` records the commands, the test count, the verify-strake tail, the ub-gates summary table, and a before/after for each behavior the diff changes.
