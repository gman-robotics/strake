# Gate scripts

CI behavior the Strake CLI does not expose. `verify.sh` runs it so a gate change is visible to the same check as the CLI.

## Contract

- `python3 -m unittest discover -s scripts/tests` exits 0.
- Blocking `scripts/run_ub_gates_ci.sh` (`UB_GATES_ADVISORY=0`) fails a changed function whose CRAP is not under 10, fails pre-existing debt whose score rose, and exits 0 when that debt is untouched. The untouched row is printed. A new function at CRAP 10 fails, and so does one above 10. A new function under 10 does not.
- Base and head crapper rows join on file path and function name. The base score is a git worktree at the merge base. `UB_GATES_COVERAGE_CMD` runs there, and crapper reads that worktree's lcov. Head coverage is not reused. A failed base coverage command, or lcov that misses a base file, fails closed (`tool_error`, blocking). A missing, empty, or unreadable base score fails closed the same way, including a readable file that is not crapper entries (`align-base failed`). The threshold stays CRAP under 10. `--lines` and blocking mode are unchanged.
- A mutator budget timeout exits non-zero in blocking mode.
- Changed Python is measured with `coverage run` before crapper scores those sources, and that same report is what the mutator reads. The gate then runs `python3 scripts/ub_changed_scope.py lcov-fresh <lcov> <files...>`. That command exits nonzero when the lcov is missing, empty, or older than a scored source, or when a source is missing. Blocking mode exits non-zero with `tool_error` in those cases. A Rust lcov from `UB_GATES_COVERAGE_CMD` does not stand in for that file. Blocking mode exits non-zero with `mutator_no_coverage` when a changed file has mutation sites and none were killed or survived. Kills in another file do not cover it. An uncovered site on a changed line is that miss when no changed-line site was killed or survived. A file that killed or survived a mutant exits 0.
- The mutator logs `mutator N/A` and exits 0 only when every changed line in every gate-relevant file is test code: `is_test_path` (a `tests/` file and the other test paths) or a line inside a `#[cfg(test)]` item. Any other diff uses those two checks unchanged. A line the scan cannot classify fails closed. A failed `git diff` still exits non-zero. Thresholds, `--lines`, and blocking mode are unchanged.
- An empty or whitespace-only changed line outside a string is neither test nor production. A blank line inside a string stays production. A comment or doc line stays production. N/A still needs one real test line, so a diff of only blank lines does not get N/A.
- A backslash in a normal string or a byte string skips the next character, so an escaped quote does not end the string. An escaped backslash before the closing quote still ends it. Raw strings do not treat a backslash as an escape. A char literal `'"'` or `'\"'` is not a string opener.
- `python3 scripts/check_pr_body.py <fixture>` exits 1 when a heading from `.github/pull_request_template.md` is missing or the section is empty, and exits 0 when every section has text.

## Drive

`.cursor/skills/verify-strake/scripts/verify.sh` runs `python3 -m unittest discover -s scripts/tests` and records `gate-scripts.log`.
