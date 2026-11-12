# Gate scripts

CI behavior the Strake CLI does not expose. `verify.sh` runs it so a gate change is visible to the same check as the CLI.

## Contract

- `python3 -m unittest discover -s scripts/tests` exits 0.
- Blocking `scripts/run_ub_gates_ci.sh` (`UB_GATES_ADVISORY=0`) fails a changed function over the crapper threshold, fails pre-existing debt whose score rose, and exits 0 when that debt is untouched. The untouched row is printed.
- A mutator budget timeout exits non-zero in blocking mode.
- Changed Python is measured with `coverage run` before the mutator. Blocking mode exits non-zero with `mutator_no_coverage` when a changed file has mutation sites and none are covered. A file whose sites are killed exits 0.
- `python3 scripts/check_pr_body.py <fixture>` exits 1 when a heading from `.github/pull_request_template.md` is missing or the section is empty, and exits 0 when every section has text.

## Drive

`.cursor/skills/verify-strake/scripts/verify.sh` runs `python3 -m unittest discover -s scripts/tests` and records `gate-scripts.log`.
