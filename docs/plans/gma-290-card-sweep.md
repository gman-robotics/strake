# Finish the remaining strake card commands

GMA-290 still owes two commands on main `d90ade62`. `check` stores `--blob-root` and never reads it. `run` parses `--fn` and always prints usage.

Records, bytes, region, imports, draft holes, user `trap`, and the section 3 sweep already pass. This program does not rebuild them.

Two stacked PRs follow. `use-blob` is first. `run-const` follows. The operator lands both. This plan PR does not implement them, and it is never merged.

GMA-483, GMA-484, GMA-485, GMA-622, and GMA-623 stay unstarted.

## How to read this

One box is one unit of work. Every box names the evidence that checks it. A nested box is a sub-step of the box above it. Check a box only when its evidence exists, a file, a log line, a screenshot, a test run, or a SHA. The body is a how-to. The appendices explain and record.

The program runs `pstack/skills/poteto-mode/playbooks/autopilot-stack.md`. The operator lands `use-blob` and `run-const`. Nobody merges from this plan. GMA-483 and GMA-484 stay unstarted until the operator queues them, and this plan does not open stand-in tickets for the same work.

Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

## Program checklist

### Arm the program

- [ ] State the protocol and this plan to the operator, then stop. Start execution only on the operator's explicit go.
- [ ] Read these from trunk at program start. Re-read them at every tick.
  - [ ] `git show origin/main:pstack/skills/poteto-mode/playbooks/autopilot-stack.md`
  - [ ] `git show origin/main:pstack/skills/swarm/SKILL.md`
  - [ ] `git show origin/main:pstack/skills/control-cli/SKILL.md`
  - [ ] `git show origin/main:pstack/skills/poteto-mode/playbooks/opening-a-pr.md`
  - [ ] `git show origin/main:pstack/skills/principle-sequence-verifiable-units/SKILL.md`
  - [ ] `git show origin/main:pstack/skills/principle-subtract-before-you-add/SKILL.md`
  - [ ] `git show origin/main:pstack/skills/principle-test-behavior-not-implementation/SKILL.md`
- [ ] On the operator's go, arm the audit tick as `/loop 1h` with the tick prompt below. Never leave the cadence to memory.
- [ ] Use this tick prompt, verbatim. "Re-read the execution playbook from trunk. Audit the operation against it and fix drift in this tick. Probe every active lane and judge progress by side effects only. Stand down a stuck lane and dispatch its replacement now. Then post a short status message to the operator in chat only when the audit found a tracked change that no earlier status message reported, such as a PR opened, a code-ready head, a round launched or closed, a verdict, a merge, a stuck agent and the action taken, a blocker added or cleared, or a decision only the operator can make. Name every such change and nothing else. Do not repeat a table, the merged list, or an unchanged blocker. If the audit found none, end the turn with no reply text. Either way, log this tick's row in your decision trail. The row names the items reported, or none."
- [ ] On the operator's hold or stand-down, send every owner a zero-writes order at once.

### Spawn owners

- [ ] Spawn one owner per PR with the full lifecycle the execution playbook names.
- [ ] Follow this dependency graph. Start dependent work only after its parent is merge-ready, and base it on the parent branch because the execution playbook stacks.
  - [ ] `use-blob` is first. It branches from `main`.
  - [ ] `run-const` after `use-blob`.
- [ ] Hold the file boundaries. `use-blob` touches only `crates/strake/Cargo.toml`, `Cargo.lock`, `crates/strake/src/body/check.rs`, `crates/strake/src/lib.rs`, `golden/use_ok.strake`, `golden/use_bad.strake`, `golden/blob/**`, `golden/blob-bad/**`, `golden/blob-empty/**`, `golden/expect/use_ok.txt`, `golden/expect/use_bad.txt`, `crates/strake/tests/use_blob.rs`, and `.cursor/skills/verify-strake/**`. `run-const` touches only `crates/strake/src/lib.rs`, `golden/run_const.strake`, `golden/expect/run_const.txt`, `crates/strake/tests/run_const.rs`, and `.cursor/skills/verify-strake/**`.
- [ ] Hold the review gate. Neither PR is review-gated. The operator still lands them. Do not merge.

### PR mechanics, for every PR

- [ ] Resolve the forge once. Default to `gh`. If `command -v origin` succeeds and Origin can resolve the repository, use `origin pr` for every PR operation. Record any fallback to `gh`. Never require `gt`. On this Cursor run, use the built-in PR tool.
- [ ] Open the PR ready, never draft, per **Opening a PR**. Use the run's built-in PR tool when it has one, else `origin pr create --status open --base main` or `gh pr create --base main` according to the resolved forge. A stack child targets its parent branch.
- [ ] Run the repo's lint and typecheck once before the PR-facing push. Push with hooks on.
- [ ] Run `/deslop` before each commit and `/no-comments` before review.
- [ ] Triage every Bugbot and security-reviewer comment per `../references/bugbot-triage.md`.
- [ ] Rebase onto current trunk before the code-ready report and babysit. Keep that merge base in fix rounds. Rebase again only at merge prep, on a `git merge-tree` conflict with trunk, on a CI failure that comes from a change on trunk, or when a known concurrent change to the same files lands.

### Verdict and merge, for every PR

- [ ] A full swarm runs once, at the code-ready head. Another full swarm runs only for new behavior, a redesign, or a large new diff. A rebase, a merge-conflict resolution, or a minor fix gets a single verifier at the new head, and a wording-only change gets none. Run each full swarm per `pstack/skills/swarm/SKILL.md`. One gates lane. The ten live lanes from the PR's **Verify, live** block. The perf lane from its **Verify, perf** block. Two or more audit lanes, each with its own focus, that read the diff and the receipts and distrust the PR body. The root audits the receipts in the merge-ready report before the verdict.
- [ ] Judge every finding by the execution playbook's blocking rule. The verdict is clean when none blocks. A failure on trunk behavior in a lane marked `Regression lane against trunk.` is recorded against trunk and does not block the PR. A new head gets a fresh verdict, except for results that stay valid under the patch-id rule in `playbooks/shipping.md`.
- [ ] Stop at merge-ready under `playbooks/autopilot-stack.md`. The operator lands the contiguous verified run. The owner does not merge. A restack that leaves `git patch-id` unchanged keeps its verdict.

### Boot recipe, for every live lane

Each live lane runs on its own cloud VM at the PR head. Strake has no `control-cli` skill in this environment. Drive the `strake` binary the way `.cursor/skills/verify-strake/scripts/verify.sh` does, and save the terminal capture as the screenshot.

- [ ] `git fetch origin <head-branch> && git checkout <head SHA>`.
- [ ] Install Rust 1.83.0 and build `target/debug/strake` with `cargo build --locked -p strake`. Wait until the binary exists.
- [ ] Pass argv only. Read exit code, stdout, and stderr. Do not call a private helper.
- [ ] Save every screenshot to `/tmp/swarm-<pr-id>/worker-<n>/<slug>.png` and return the paths with the report.

## Check use blobs (use-blob)

**Depends on.** None.

**Files.**

- [ ] Edit `crates/strake/Cargo.toml`.
- [ ] Edit `Cargo.lock`.
- [ ] Edit `crates/strake/src/body/check.rs`.
- [ ] Edit `crates/strake/src/lib.rs`.
- [ ] Create `golden/use_ok.strake`.
- [ ] Create `golden/use_bad.strake`.
- [ ] Create `golden/blob/pkg`.
- [ ] Create `golden/blob-bad/pkg`.
- [ ] Create `golden/blob-empty/.gitkeep`.
- [ ] Create `golden/expect/use_ok.txt`.
- [ ] Create `golden/expect/use_bad.txt`.
- [ ] Create `crates/strake/tests/use_blob.rs`.
- [ ] Edit `.cursor/skills/verify-strake/scripts/verify.sh`.
- [ ] Edit `.cursor/skills/verify-strake/features/check-usage-contract.md`.
- [ ] Edit `.cursor/skills/verify-strake/features/golden-fixtures.md`.

**Build.**

- [ ] Add `blake3` to `crates/strake/Cargo.toml`. It is the only new dependency. Pin a release that `cargo test --locked` builds with Rust 1.83.0. If no release builds, stop and report. Do not add another crate.
- [ ] Pass `opts.blob_root` from `check` in `crates/strake/src/lib.rs` into the checker in `crates/strake/src/body/check.rs`. Leave `run` as the usage stub.
- [ ] For each `use` item, open only `<blob-root>/<name>` when `--blob-root` is set. Reject a name that contains a separator or `..` without opening a path outside that directory.
- [ ] Treat a missing flag, a missing file, and a blake3 mismatch as `E_USE` kind `check`. The hex on the `use` line must equal blake3 of the blob bytes. The blob bytes are the formatted package text, so that digest is also blake3 of the formatted package. Do not hash the file that contains the `use` line.
- [ ] Write `golden/use_ok.strake` as a fmt fixed point whose `use pkg@blake3:<hex>` names that digest. Write `golden/blob/pkg` as the formatted package. Write `golden/use_bad.strake` with a hex that is not the digest of `golden/blob-bad/pkg`.
- [ ] Add `golden/expect/use_ok.txt` with args `check --blob-root golden/blob golden/use_ok.strake` and exit 0. Add `golden/expect/use_bad.txt` with args `check --blob-root golden/blob-empty golden/use_bad.strake` and exit 1. The mismatch command cannot be a second expectation for the same stem, so `crates/strake/tests/use_blob.rs` runs it.
- [ ] Add both success and failure commands to `GOLDEN_SMOKE` in `.cursor/skills/verify-strake/scripts/verify.sh`. Update the two feature docs so the skill shows the real check.

**You see.**

- [ ] `strake check --blob-root golden/blob golden/use_ok.strake` exits 0 and stderr is empty.
- [ ] `strake check --blob-root golden/blob-empty golden/use_bad.strake` exits 1. Stderr is one JSON line with code `E_USE` and kind `check`.
- [ ] `strake check --blob-root golden/blob-bad golden/use_bad.strake` exits 1 with the same code and kind.
- [ ] `strake check golden/add2_basic.strake` still exits 0. No current golden contains a `use` line.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] `crates/strake/tests/use_blob.rs` runs the three `check` commands above and asserts the exit code and the diagnostic code and kind. It also asserts the hex equals blake3 of `golden/blob/pkg`. Run `cargo test --locked -p strake --test use_blob --test golden_sweep`.
- [ ] Run `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked -p strake`.
- [ ] Run `.cursor/skills/verify-strake/scripts/verify.sh`. Pass when the new `GOLDEN_SMOKE` rows match the exits above.
- [ ] Run `ub-gates` on the diff. Pass when crapper findings are 0, dryer findings are 0, and the mutator reports 0 survivors and 0 uncovered on changed lines. Do not change thresholds, `--lines`, skip rules, or blocking.
- [ ] Run `scripts/check_pr_body.py` on the PR body. Pass when it prints no missing or empty section.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.7-xhigh-fast` at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Run `strake check golden/add2_basic.strake` at trunk and at head. Trunk has no `use_ok.strake`, so record that and gate the add2 check plus the `use_ok` exit the user waits for. Save `add2-check.png`. Pass when both exits are 0 and both stderrs are empty.
- [ ] Lane 2. Run `strake check --blob-root golden/blob golden/use_ok.strake`. Save `use-ok.png`. Pass when the exit is 0 and stderr is empty.
- [ ] Lane 3. Run `strake check --blob-root golden/blob-empty golden/use_bad.strake`. Save `use-missing.png`. Pass when the exit is 1 and stderr contains `"E_USE"` and `"kind":"check"`.
- [ ] Lane 4. Run `strake check --blob-root golden/blob-bad golden/use_bad.strake`. Save `use-mismatch.png`. Pass when the exit is 1 and stderr contains `"E_USE"` and `"kind":"check"`.
- [ ] Lane 5. Hash `golden/blob/pkg` with blake3 and read the hex from `golden/use_ok.strake`. Save `use-digest.png`. Pass when the hex equals that digest and the blob bytes equal the formatted package text.
- [ ] Lane 6. Run `strake check --blob-root golden/blob golden/add2_basic.strake`. Save `check-no-use.png`. Pass when the exit is 0 and stderr is empty.
- [ ] Lane 7. Run `strake check golden/use_ok.strake` with no `--blob-root`. Save `use-no-root.png`. Pass when the exit is 1 and stderr contains `"E_USE"` and `"kind":"check"`.
- [ ] Lane 8. Place a copy of the good blob beside `golden/use_ok.strake` and point `--blob-root` at `golden/blob-empty`. Save `use-outside-root.png`. Pass when the exit is 1 and stderr contains `"E_USE"`.
- [ ] Lane 9. Run the missing-blob command again and read the kind field. Save `use-no-exec.png`. Pass when the kind is `check`, the exit is 1, and stderr contains no `"kind":"trap"`.
- [ ] Lane 10. Run `strake fmt golden/use_ok.strake` and `strake fmt golden/use_bad.strake`. Save `use-fmt.png`. Pass when each exit is 0 and each stdout equals the file bytes.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Wall clock of `strake check golden/add2_basic.strake` at trunk and at head, plus wall clock of `strake check --blob-root golden/blob golden/use_ok.strake` at head. Trunk has no `use_ok` command that checks a blob, so the added work is that one check and the user-visible end state is exit 0 with empty stderr.
- [ ] Probe. Time each command at trunk, then at head, then at trunk again. Both sides time the add2 check. Only head times `use_ok`.
- [ ] Baseline. Record the trunk add2 check time first.
- [ ] Rule. Fail if the head add2 check is more than 1 second slower than the trunk baseline. Fail if the `use_ok` check takes 5 seconds or longer. Do not compare the `use_ok` time with the trunk usage time.

**Review gate.** None. use-blob is not review-gated.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] Rebased onto current trunk after the verdict, patch-id unchanged.
- [ ] The operator lands this PR. The owner does not merge.

## Implement run (run-const)

**Depends on.** use-blob.

**Files.**

- [ ] Edit `crates/strake/src/lib.rs`.
- [ ] Create `golden/run_const.strake`.
- [ ] Create `golden/expect/run_const.txt`.
- [ ] Create `crates/strake/tests/run_const.rs`.
- [ ] Edit `.cursor/skills/verify-strake/scripts/verify.sh`.
- [ ] Edit `.cursor/skills/verify-strake/features/golden-fixtures.md`.

**Build.**

- [ ] Replace the body of `pub fn run` in `crates/strake/src/lib.rs`. Keep the signature. Do not add a module. `main.rs` already calls `run`.
- [ ] Compile the file the way `test` does, including `opts.draft`, and do not call `diag::status`. On a check diagnostic, return that check exit and do not execute. On a codeless rejection, return exit 3 and do not execute.
- [ ] Execute the named nullary export. Print one scalar and exit 0. A trap exits 2 with that trap code. A missing export, or an export with parameters, exits 2 with plain usage text and no catalog code.
- [ ] Write `golden/run_const.strake` as `fn answer() -> i64` and `ret 14`, a fmt fixed point. Add `golden/expect/run_const.txt` with args `run --fn answer golden/run_const.strake`, exit 0, and stdout `14\n`.
- [ ] Add `0:run --fn answer golden/run_const.strake` to `GOLDEN_SMOKE`. Drive the other exit rules from temp files in `crates/strake/tests/run_const.rs`, the same shape as `crates/strake/tests/t5_region.rs`. Do not add goldens the ticket does not name.

**You see.**

- [ ] `strake run --fn answer golden/run_const.strake` prints `14` and exits 0.
- [ ] `strake test golden/add2_basic.strake` still exits 0.
- [ ] `cargo test --locked -p strake --test status_guard` still passes, because `pub fn run` does not reference `status`.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] `crates/strake/tests/run_const.rs` runs the named command and asserts stdout `14\n` and exit 0. It also runs a trap, a check failure, and a non-nullary export from temp files. Run `cargo test --locked -p strake --test run_const --test golden_sweep --test status_guard`.
- [ ] Run `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked -p strake`.
- [ ] Run `.cursor/skills/verify-strake/scripts/verify.sh`. Pass when the new `GOLDEN_SMOKE` row exits 0 and stdout is `14\n`.
- [ ] Run `ub-gates` on the diff. Pass when crapper findings are 0, dryer findings are 0, and the mutator reports 0 survivors and 0 uncovered on changed lines. Do not change thresholds, `--lines`, skip rules, or blocking.
- [ ] Run `scripts/check_pr_body.py` on the PR body. Pass when it prints no missing or empty section.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.7-xhigh-fast` at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Run `strake run --fn answer golden/add2_basic.strake` at trunk and at head. Trunk lacks `run_const` and the stub prints usage, so record exit 2 there and gate stdout `14\n` on `golden/run_const.strake` at head. Save `run-trunk.png`. Pass when trunk exits 2 with usage text and head `run_const` exits 0 with stdout `14\n`.
- [ ] Lane 2. Run `strake run --fn answer golden/run_const.strake`. Save `run-const.png`. Pass when the exit is 0 and stdout is `14\n`.
- [ ] Lane 3. Read stdout of that command with no extra bytes. Save `run-stdout.png`. Pass when stdout is exactly `14\n` and stderr is empty.
- [ ] Lane 4. Run `strake run --fn stop` on a temp file whose body is `trap`. Save `run-trap.png`. Pass when the exit is 2 and stderr contains a catalog code.
- [ ] Lane 5. Run `strake run --fn answer` on a temp file that loads a list. Save `run-check.png`. Pass when the exit is 1, stderr contains `"E_TYPE"` and `"kind":"check"`, and stderr contains no `"kind":"trap"`.
- [ ] Lane 6. Run `strake run --fn add2 golden/add2_basic.strake`. Save `run-arity.png`. Pass when the exit is 2 and stderr has no `"code"`.
- [ ] Lane 7. Run `strake run --fn missing golden/run_const.strake`. Save `run-missing.png`. Pass when the exit is 2 and stderr has no `"code"`.
- [ ] Lane 8. Run `strake test golden/add2_basic.strake`. Save `test-add2.png`. Pass when the exit is 0 and stderr is empty.
- [ ] Lane 9. Run `strake check golden/priv.strake`. Save `check-priv.png`. Pass when the exit is 0 and stderr is empty.
- [ ] Lane 10. Run `strake fmt golden/run_const.strake`. Save `run-fmt.png`. Pass when the exit is 0 and stdout equals the file bytes.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Wall clock of `strake test golden/add2_basic.strake` at trunk and at head, plus wall clock of `strake run --fn answer golden/run_const.strake` at head. Trunk's `run` is the usage stub, so the added work is the real `run_const` command and the user-visible end state is stdout `14\n` with exit 0.
- [ ] Probe. Time the test command at trunk, then at head, then at trunk again. Time `run_const` only at head.
- [ ] Baseline. Record the trunk test time first.
- [ ] Rule. Fail if the head test is more than 1 second slower than the trunk baseline. Fail if `run_const` takes 5 seconds or longer. Do not compare `run_const` with the trunk usage stub.

**Review gate.** None. run-const is not review-gated.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] Rebased onto `use-blob` after the verdict, patch-id unchanged.
- [ ] The operator lands this PR after `use-blob`. The owner does not merge.

## Close the program

- [ ] Every box above is checked with its evidence.
- [ ] Reply to the operator with the report the execution playbook names.

## Appendix A. Prototype evidence

No prototype branch. The questions were facts on main `d90ade62`, read from the tree. Nothing behavioral was left that a sketch could settle.

Already passing, so they are not in either PR.

- Section 3 rows in `docs/plans/phase1-architecture.md` match `golden/expect/*.txt`, and `crates/strake/tests/golden_sweep.rs` runs each `args` line. That includes `wrap`, `div0`, `shadow`, `if_join`, `if_phi`, `for_list`, `forall_rev`, `forall_ok`, `fuel`, `stack`, `test_fail`, `refused`, `hole_ship`, `hole_draft` (`test --draft`, exit 2, `E_HOLE` kind `trap`), `callee_77`, `fuel_twice`, `match_join`, and `forall_list_bad`.
- `golden/region_oob.strake` is a negative `load` offset. Expectation exit 2, `E_OOB` kind `trap`. `region_range` in `crates/strake/src/body/interp/mod.rs` compares the address in `i128`. `t5_region_len_checks_bounds` covers the page-size product.
- `golden/region_miss.strake` exits 1 with `E_REGION` kind `check`. `check` returns before `run_test_exports`.
- `golden/region_unaligned.strake` stores `i64` at address 3, then loads that value, and exits 0. The load before the store expects 0.
- `region_pages` in `crates/strake/src/body/syntax.rs` is the `pages` directive, else 16, and only when `import region mem : region` is present. The import grammar has no count. `t5_region_pages_directive_else_16`, `page_directive_sizes_the_region`, and `default_region_is_16_pages` lock that.
- `golden/import_miss.strake` exits 2 with `E_IMPORT` kind `trap`. `t5_instantiate_missing_import_traps_before_any_call` asserts the host call count stays 0.
- `golden/bytes_oob.strake` exits 0. `bytes.get` out of range becomes `Err` and the test expects `-1`. In range, byte `0xff` is `255`.
- `golden/load_type.strake` is `load list i64` and exits 1 with `E_TYPE` kind `check`.
- `golden/trap_user.strake` is on `UNIT_ONLY` in `golden_sweep.rs`. `trap_user_lowers_to_trap_outside_reserved_codes` asserts opcode `trap` and immediate `TrapImm::User`, which is outside `Fuel`, `Stack`, `Div0`, `Oob`, `Import`, `Hole`, and `Lower`. There is no CLI expectation.
- Records already execute. `RecNew` and `RecGet` are in `crates/strake/src/body/interp/op.rs`. `GOLDEN_SMOKE` includes `golden/record_project.strake` at exit 0.
- `Opts.blob_root` is set in `crates/strake/src/main.rs` and ignored by `check` in `crates/strake/src/lib.rs`. `crates/strake/Cargo.toml` has no dependencies. `pub fn run` returns usage and exits 2. `golden/use_ok.strake`, `golden/use_bad.strake`, and `golden/run_const.strake` are absent.

Unproven until the operator says go. The blob path `<root>/<use name>` and the decision to hash the blob rather than the importer are choices, recorded in Appendix B. No run measured them.

## Appendix B. Alternatives rejected

One combined PR lost. `Cargo.lock` would hide the `run` diff, and each command has its own exit check. Sequence them so the first check is green before the second starts.

Hashing the importer lost. The hex sits on the `use` line inside that file, so blake3 of the file cannot equal the hex. The lock says a present blob whose blake3 matches is legal, and SPEC says that hash is blake3 of the formatted text. The blob is that formatted text. The hex records blake3 of the blob.

Storing only the 32-byte digest as the blob lost. That file's own blake3 would be a hash of the digest, which is not the digest the `use` line records.

Naming the blob by the hex lost. The `use` line already has a name. The checker opens `<blob-root>/<name>` and does not walk the directory.

Rebuilding the section 3 sweep lost. `golden_sweep` already runs the table. New expectations join that harness. `trap_user` stays on `UNIT_ONLY`.

Adding a count to `import region mem : region` lost. The grammar has no count, and `region_pages` already implements directive else 16.

Editing README or SPEC lost. GMA-290 does not ask for that, and GMA-485 owns the docs pass. Those files stay untouched.

## Appendix C. Risks

`blake3` may not build on Rust 1.83.0. The risk lands in `use-blob`. The owner pins a release that `cargo test --locked` builds. If none does, the owner stops. No second crate is allowed.

`check` starts reading `blob_root`. A file with no `use` must keep its exit. Lane 6 on `use-blob` watches `golden/add2_basic.strake`. No current file under `golden/` contains a `use` line, measured by search on `d90ade62`.

`run` might call `status`. `crates/strake/tests/status_guard.rs` already fails that. Lane 4 on `run-const` watches a trap exit of 2, which `status` would not be asked to rewrite.

The omitted `--blob-root` flag is `E_USE` because no directory is opened and the blob is missing. That is a reading of SPEC "Packages" plus section 2.10. It is lane 7. If the operator wants the flag to skip the check, say so before `use-blob` starts. The default stands until then.

Strake has no `pstack/` tree, and this environment has no `control-cli` skill. The `git show origin/main:pstack/...` lines fail here. The owner reads those playbooks from the poteto-mode skill checkout. Live lanes run the `strake` binary and save the terminal capture.

GMA-483 is the same `use` work. GMA-484 is the same `run` work. This plan does not start them and does not open a second pair. GMA-485, GMA-622, and GMA-623 stay unstarted. Gate thresholds, `--lines`, skip rules, and blocking stay as they are.

A huge `pages` value can still abort the process. Phase 1 section 5 already records that. Neither PR changes it.

## Appendix D. Links and reading list

Read before editing. `docs/plans/phase1-architecture.md` sections 2, 3, and 4. The file is frozen. `SPEC.md` sections "Packages" and "Modules and CLI". `crates/strake/src/lib.rs` for `Opts`, `check`, and `run`. `crates/strake/src/body/syntax.rs` for `parse_use` and `region_pages`. `crates/strake/tests/golden_sweep.rs` for the expectation format. `.cursor/skills/verify-strake/scripts/verify.sh` for `GOLDEN_SMOKE`.

`use-blob` reads the how skill before editing, to keep the check in `check.rs` and the directory on `Opts`. `run-const` does not need a second design pass. The stub and the caller are already in `lib.rs` and `main.rs`. Neither PR needs an interrogate pass. The architecture text is frozen.

The decision trail is this file. Commit it with the plan PR. Do not start the two execution PRs from that commit.
