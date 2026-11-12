#!/usr/bin/env bash
# Copied from agent-bootstrap origin/main 9e06b310d6c3d0583b16c6aab423b12880be305e
# (743927a-era main, plus the two commits after it on origin/main).
# ub-mode gates for CI (GMA-410 report path, GMA-499 blocking).
# Uses the merge-base file list. Upstream `--changed` reads git status only.
# UB_GATES_ADVISORY defaults to 0 (blocking). UB_GATES_ADVISORY=1 reports and exits 0.
# run_ub_gates_report.sh sets UB_GATES_ADVISORY=1. A direct call does not.
# Blocking mode fails on skip (except skipped_no_scope), tool_error, findings in
# changed scope, a mutator budget timeout, uncovered mutation sites, and skipped_mutator.
# UB_GATES_SKIP_MUTATOR=1 and the CURSOR_AGENT auto-skip are skipped_mutator.
# They are never ran_clean. Blocking mode exits 1. Advisory mode records the
# same outcome and exits 0.
# A failed git diff --name-only is tool_error. A missing merge base is tool_error.
# The base crapper runs in a git worktree at the merge base. When
# UB_GATES_COVERAGE_CMD is set, that command runs in the worktree and crapper
# reads that lcov, never the head lcov. A failed base coverage command, or
# lcov that misses a base file, is tool_error. --no-coverage is not used.
# Base and head rows join on file path plus function name. A missing or
# unreadable base score is tool_error and blocks, including a readable file
# that is not crapper entries. Pass is CRAP under 10.
# --lines and blocking mode are unchanged.
# Base-ref fetch honors UB_GATES_BASE_REF, uses no depth limit, and unshallows
# a shallow clone. A mutator budget timeout fails the job in blocking mode.
# Advisory mode reports that timeout and still exits 0. The step summary's
# first line states which of those happened. Changed Python is measured with
# coverage.py before the mutator. Blocking mode fails when a changed file has
# mutation sites and none were killed or survived. Kills in another file do not
# cover it. An uncovered site on a changed line is that miss when no
# changed-line site was killed or survived. The mutator reports N/A and passes
# only when every changed line in every gate-relevant file is test code: an
# is_test_path (tests/ and the other test paths) or a line inside a #[cfg(test)]
# item. An empty or whitespace-only line outside a string is neither test nor
# production, and N/A still needs one real test line. Any other diff uses the per-file and uncovered-line checks above,
# unchanged. A scan that cannot classify a line fails closed. A diff failure
# still fails. .pyi and .d.ts are outside gate scope.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCOPE_PY="${SCRIPT_DIR}/ub_changed_scope.py"
ADVISORY="${UB_GATES_ADVISORY:-0}"
BASE_REF="${UB_GATES_BASE_REF:-origin/main}"
OUTCOME_FILE="${UB_GATES_OUTCOME_FILE:-.metrics/ub-gates-outcome.txt}"
DRYER_THRESHOLD="${UB_GATES_DRYER_THRESHOLD:-0.8}"
MUTATOR_BUDGET="${UB_GATES_MUTATOR_BUDGET_SECS:-1200}"

log() { printf '%s\n' "$*"; }

write_outcome() {
  mkdir -p "$(dirname "${OUTCOME_FILE}")"
  {
    printf 'outcome=%s\n' "$1"
    printf 'gate_miss=%s\n' "${GATE_MISS:-0}"
    printf 'gates_ran=%s\n' "${GATES_RAN:-0}"
    printf 'advisory=%s\n' "${ADVISORY}"
    if [[ -n "${SCOPE_COUNT:-}" ]]; then printf 'scope_files=%s\n' "${SCOPE_COUNT}"; fi
    if [[ -n "${CRAPPER_RC:-}" ]]; then printf 'crapper_exit=%s\n' "${CRAPPER_RC}"; fi
    if [[ -n "${CRAPPER_FINDINGS:-}" ]]; then printf 'crapper_findings=%s\n' "${CRAPPER_FINDINGS}"; fi
    if [[ -n "${CRAPPER_DEBT:-}" ]]; then printf 'crapper_debt=%s\n' "${CRAPPER_DEBT}"; fi
    if [[ -n "${DRYER_RC:-}" ]]; then printf 'dryer_exit=%s\n' "${DRYER_RC}"; fi
    if [[ -n "${DRYER_HIGH:-}" ]]; then printf 'dryer_candidates_ge_threshold=%s\n' "${DRYER_HIGH}"; fi
    if [[ -n "${DRYER_FINDINGS:-}" ]]; then printf 'dryer_findings=%s\n' "${DRYER_FINDINGS}"; fi
    if [[ -n "${DRYER_DEBT:-}" ]]; then printf 'dryer_debt=%s\n' "${DRYER_DEBT}"; fi
    if [[ -n "${MUTATOR_RC:-}" ]]; then printf 'mutator_exit=%s\n' "${MUTATOR_RC}"; fi
    if [[ -n "${MUTATOR_SKIPPED:-}" ]]; then printf 'mutator_skipped=%s\n' "${MUTATOR_SKIPPED}"; fi
    if [[ -n "${MUTATOR_TIMEOUT:-}" ]]; then printf 'mutator_timeout=%s\n' "${MUTATOR_TIMEOUT}"; fi
    if [[ -n "${MUTATOR_BUDGET_USED:-}" ]]; then printf 'mutator_budget_secs=%s\n' "${MUTATOR_BUDGET_USED}"; fi
    if [[ -n "${MUTATOR_SITES:-}" ]]; then printf 'mutator_sites=%s\n' "${MUTATOR_SITES}"; fi
    if [[ -n "${MUTATOR_COVERED:-}" ]]; then printf 'mutator_covered=%s\n' "${MUTATOR_COVERED}"; fi
    if [[ -n "${MUTATOR_KILLED:-}" ]]; then printf 'mutator_killed=%s\n' "${MUTATOR_KILLED}"; fi
    if [[ -n "${MUTATOR_SURVIVED:-}" ]]; then printf 'mutator_survived=%s\n' "${MUTATOR_SURVIVED}"; fi
    if [[ -n "${MUTATOR_UNCOVERED:-}" ]]; then printf 'mutator_uncovered=%s\n' "${MUTATOR_UNCOVERED}"; fi
    if [[ -n "${MUTATOR_NOT_RUN:-}" ]]; then printf 'mutator_not_run=%s\n' "${MUTATOR_NOT_RUN}"; fi
    if [[ -n "${MUTATOR_NA:-}" ]]; then printf 'mutator_na=%s\n' "${MUTATOR_NA}"; fi
    if [[ -n "${TOOL_ERROR:-}" ]]; then printf 'tool_error=%s\n' "${TOOL_ERROR}"; fi
  } >"${OUTCOME_FILE}"
}

emit_warning() {
  local msg="$1"
  log "ub-gates: WARNING: ${msg}"
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then
    local safe="${msg//'%'/'%25'}"
    safe="${safe//$'\n'/'%0A'}"
    safe="${safe//$'\r'/'%0D'}"
    echo "::warning title=ub-gates::${safe}"
  fi
}

append_step_summary() {
  [[ -n "${GITHUB_STEP_SUMMARY:-}" ]] || return 0
  mkdir -p "$(dirname "${GITHUB_STEP_SUMMARY}")" 2>/dev/null || return 0
  {
    if [[ "${MUTATOR_TIMEOUT:-0}" == "1" && "${ADVISORY}" == "1" ]]; then
      echo "mutator timeout: budget exhausted; non-blocking"
    fi
    if [[ "${MUTATOR_TIMEOUT:-0}" == "1" && "${ADVISORY}" != "1" ]]; then
      echo "mutator timeout: budget exhausted; blocking"
    fi
    echo "## ub-gates"
    echo ""
    if [[ "${OUTCOME:-}" == "skipped_mutator" && "${ADVISORY}" == "1" ]]; then
      echo "> **Advisory skip (not clean):** \`skipped_mutator\` — mutator did not run. This is not a clean result."
      echo ""
    fi
    if [[ "${OUTCOME:-}" == "skipped_mutator" && "${ADVISORY}" != "1" ]]; then
      echo "> **Blocking skip:** \`skipped_mutator\` — mutator did not run. This is not a clean result."
      echo ""
    fi
    if [[ "${OUTCOME:-}" == "skipped_tools_missing" && "${ADVISORY}" == "1" ]]; then
      echo "> **Neutral skip (not a gate pass):** \`skipped_tools_missing\` — crapper, dryer, and mutator were not on PATH. Install tools in CI is a follow-up ticket."
      echo ""
    fi
    if [[ "${OUTCOME:-}" == "skipped_tools_missing" && "${ADVISORY}" != "1" ]]; then
      echo "> **Blocking skip:** \`skipped_tools_missing\` — crapper, dryer, and mutator were not on PATH. The job fails."
      echo ""
    fi
    if [[ "${OUTCOME:-}" == "tool_error" && "${ADVISORY}" == "1" ]]; then
      echo "> **Advisory tool error (not clean, not a quality finding):** \`tool_error\` — crapper/dryer/mutator failed to run correctly. Review exit codes and metrics files; hand back to implementer."
      echo ""
    fi
    if [[ "${OUTCOME:-}" == "tool_error" && "${ADVISORY}" != "1" ]]; then
      echo "> **Blocking tool error:** \`tool_error\` — crapper/dryer/mutator failed to run correctly. The job fails."
      echo ""
    fi
    echo "| Field | Value |"
    echo "| --- | --- |"
    echo "| outcome | ${OUTCOME:-unknown} |"
    echo "| gates_ran | ${GATES_RAN:-0} |"
    echo "| gate_miss | ${GATE_MISS:-0} |"
    echo "| scope_files | ${SCOPE_COUNT:-0} |"
    if [[ -n "${CRAPPER_RC:-}" ]]; then echo "| crapper_exit | ${CRAPPER_RC} |"; fi
    if [[ -n "${CRAPPER_FINDINGS:-}" ]]; then echo "| crapper_findings | ${CRAPPER_FINDINGS} |"; fi
    if [[ -n "${CRAPPER_DEBT:-}" ]]; then echo "| crapper_debt | ${CRAPPER_DEBT} |"; fi
    if [[ -n "${DRYER_RC:-}" ]]; then echo "| dryer_exit | ${DRYER_RC} |"; fi
    if [[ -n "${DRYER_HIGH:-}" ]]; then echo "| dryer_candidates_ge_${DRYER_THRESHOLD} | ${DRYER_HIGH} |"; fi
    if [[ -n "${DRYER_FINDINGS:-}" ]]; then echo "| dryer_findings | ${DRYER_FINDINGS} |"; fi
    if [[ -n "${DRYER_DEBT:-}" ]]; then echo "| dryer_debt | ${DRYER_DEBT} |"; fi
    if [[ -n "${MUTATOR_RC:-}" ]]; then echo "| mutator_exit | ${MUTATOR_RC} |"; fi
    if [[ -n "${MUTATOR_TIMEOUT:-}" ]]; then echo "| mutator_timeout | ${MUTATOR_TIMEOUT} |"; fi
    if [[ -n "${MUTATOR_SITES:-}" ]]; then echo "| mutator_sites | ${MUTATOR_SITES} |"; fi
    if [[ -n "${MUTATOR_COVERED:-}" ]]; then echo "| mutator_covered | ${MUTATOR_COVERED} |"; fi
    if [[ -n "${MUTATOR_KILLED:-}" ]]; then echo "| mutator_killed | ${MUTATOR_KILLED} |"; fi
    if [[ -n "${MUTATOR_SURVIVED:-}" ]]; then echo "| mutator_survived | ${MUTATOR_SURVIVED} |"; fi
    if [[ -n "${MUTATOR_UNCOVERED:-}" ]]; then echo "| mutator_uncovered | ${MUTATOR_UNCOVERED} |"; fi
    if [[ -n "${MUTATOR_NOT_RUN:-}" ]]; then echo "| mutator_not_run | ${MUTATOR_NOT_RUN} |"; fi
    if [[ -n "${MUTATOR_NA:-}" ]]; then echo "| mutator_na | ${MUTATOR_NA} |"; fi
    echo ""
    echo "Outcome file: \`${OUTCOME_FILE}\`"
  } >>"${GITHUB_STEP_SUMMARY}"
}

exit_for_outcome() {
  local code=0
  case "${OUTCOME}" in
    skipped_no_scope|ran_clean)
      code=0
      ;;
    ran_with_findings)
      if [[ "${ADVISORY}" == "1" ]]; then
        log "ub-gates: advisory mode — findings reported; job succeeds"
        code=0
      else
        log "ub-gates: blocking mode — failing job on gate findings"
        code=1
      fi
      ;;
    tool_error)
      if [[ "${ADVISORY}" == "1" ]]; then
        log "ub-gates: advisory mode — tool_error reported; job succeeds"
        code=0
      else
        log "ub-gates: blocking mode — failing job on tool_error"
        code=1
      fi
      ;;
    skipped_tools_missing|skipped_no_base_ref|skipped_no_merge_base|skipped_not_git)
      if [[ "${ADVISORY}" == "1" ]]; then
        code=0
      else
        code=1
      fi
      ;;
    skipped_mutator)
      if [[ "${ADVISORY}" == "1" ]]; then
        log "ub-gates: advisory mode — mutator skip is not ran_clean; job succeeds"
        code=0
      else
        log "ub-gates: blocking mode — mutator skip is not ran_clean"
        code=1
      fi
      ;;
    mutator_timeout)
      log "ub-gates: blocking mode — mutator budget timeout fails the job"
      code=1
      ;;
    mutator_no_coverage)
      if [[ "${ADVISORY}" == "1" ]]; then
        log "ub-gates: advisory mode — mutator covered no sites; job succeeds"
        code=0
      else
        log "ub-gates: blocking mode — mutator covered no sites"
        code=1
      fi
      ;;
    *)
      code=1
      ;;
  esac
  write_outcome "${OUTCOME}"
  append_step_summary
  exit "${code}"
}

gate_relevant_file() {
  case "$1" in
    *.pyi|*.d.ts) return 1 ;;
    *.py|*.ts|*.tsx|*.mts|*.cts|*.js|*.jsx|*.mjs|*.cjs|*.go|*.rs|*.java|*.clj|*.cljs|*.cljc) return 0 ;;
    *) return 1 ;;
  esac
}

# test code is the oracle; mutator upstream skips it too.
is_test_path() {
  local path="$1"
  local base="${path##*/}"
  case "/${path}/" in
    */test/*|*/tests/*|*/spec/*|*/specs/*|*/testdata/*|*/__tests__/*|*/fixtures/*) return 0 ;;
  esac
  case "${base}" in
    test_*.py|*_test.py|*_test.go|*_test.rs|*Test.java) return 0 ;;
    *.test.ts|*.test.tsx|*.test.js|*.test.jsx|*.test.mts|*.test.cts|*.test.mjs|*.test.cjs) return 0 ;;
    *.spec.ts|*.spec.tsx|*.spec.js|*.spec.jsx|*.spec.mts|*.spec.cts|*.spec.mjs|*.spec.cjs) return 0 ;;
  esac
  return 1
}

collect_scope_files() {
  local merge_base="$1"
  local f list rc
  SCOPE_FILES=()
  SCOPE_DIFF_FAILED=0
  list="$(mktemp)"
  set +e
  git diff --name-only --diff-filter=ACMRT "${merge_base}" HEAD >"${list}" 2>/dev/null
  rc=$?
  set -e
  if [[ "${rc}" -ne 0 ]]; then
    rm -f "${list}"
    SCOPE_DIFF_FAILED=1
    SCOPE_COUNT=0
    return 0
  fi
  while IFS= read -r f; do
    [[ -z "${f}" ]] && continue
    [[ -f "${f}" ]] || continue
    gate_relevant_file "${f}" || continue
    is_test_path "${f}" && continue
    SCOPE_FILES+=("${f}")
  done <"${list}"
  rm -f "${list}"
  SCOPE_COUNT="${#SCOPE_FILES[@]}"
}

gate_bin() {
  command -v "$1" >/dev/null 2>&1 && echo "$1" && return 0
  if [[ -x "./$1" ]]; then
    echo "./$1"
    return 0
  fi
  return 1
}

lcov_ready() {
  python3 - <<'PY'
import pathlib
import sys

roots = (pathlib.Path("target/coverage"), pathlib.Path("coverage"))
for root in roots:
    if not root.exists():
        continue
    for path in root.rglob("lcov.info"):
        if path.is_file() and path.stat().st_size > 0:
            sys.exit(0)
sys.exit(1)
PY
}

apply_scope_filter() {
  local cmd="$1"
  local edn="$2"
  local thresh="$3"
  local out rc counts findings debt
  local -a flags
  flags=(--base "${MERGE_BASE}" --edn "${edn}" --threshold "${thresh}")
  if [[ "${cmd}" == "crap" && -n "${BASE_EDN:-}" ]]; then
    flags+=(--base-edn "${BASE_EDN}")
  fi
  if [[ "${cmd}" == "crap" && ${#COVERAGE_PREFIX[@]} -gt 0 ]]; then
    flags+=(--coverage-expected)
  fi
  out="$(mktemp)"
  set +e
  python3 "${SCOPE_PY}" "${cmd}" "${flags[@]}" >"${out}"
  rc=$?
  set -e
  if [[ "${rc}" -ne 0 ]]; then
    cat "${out}" >&2 || true
    rm -f "${out}"
    return "${rc}"
  fi
  python3 - "${out}" "${cmd}" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
kind = sys.argv[2]
for bucket in ("findings", "debt"):
    for row in payload[bucket]:
        if kind == "crap":
            print(
                "ub-gates: crapper %s %s %s crap=%s"
                % (bucket, row["namespace"], row["name"], row["crap"])
            )
        else:
            print("ub-gates: dryer %s score=%s" % (bucket, row["score"]))
PY
  counts="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1], encoding="utf-8")); print(len(d["findings"])); print(len(d["debt"]))' "${out}")"
  rm -f "${out}"
  findings="${counts%%$'\n'*}"
  debt="${counts##*$'\n'}"
  if [[ "${cmd}" == "crap" ]]; then
    CRAPPER_FINDINGS="${findings}"
    CRAPPER_DEBT="${debt}"
  else
    DRYER_FINDINGS="${findings}"
    DRYER_DEBT="${debt}"
  fi
  log "ub-gates: scope join ${cmd} findings=${findings} debt=${debt}"
  return 0
}

base_edn_readable() {
  local path="$1"
  [[ -f "${path}" && -r "${path}" && -s "${path}" ]]
}

drop_base_worktree() {
  local dest="$1"
  if [[ -z "${dest}" ]]; then
    return 0
  fi
  git worktree remove --force "${dest}" >/dev/null 2>&1 || rm -rf "${dest}"
  git worktree prune >/dev/null 2>&1 || true
}

add_base_worktree() {
  local dest="$1"
  local rc
  if [[ -e "${dest}" ]]; then
    rmdir "${dest}" 2>/dev/null || rm -rf "${dest}"
  fi
  set +e
  git worktree add --detach "${dest}" "${MERGE_BASE}"
  rc=$?
  set -e
  return "${rc}"
}

# Run UB_GATES_COVERAGE_CMD inside the base worktree. Head lcov is not copied.
# A failed command, or lcov that misses a base file, fails closed.
run_base_coverage() {
  local root="$1"
  local listing="$2"
  local rc
  if [[ -z "${UB_GATES_COVERAGE_CMD:-}" ]]; then
    return 0
  fi
  log "ub-gates: base coverage worktree ${root}"
  set +e
  (
    cd "${root}"
    bash -c "${UB_GATES_COVERAGE_CMD}"
  )
  rc=$?
  set -e
  if [[ "${rc}" -ne 0 ]]; then
    log "ub-gates: base coverage command failed"
    return 1
  fi
  python3 "${SCOPE_PY}" lcov-covers --root "${root}" --list "${listing}"
}

# Score merge-base files in a worktree at MERGE_BASE. Base coverage is the
# coverage command run in that worktree, never the head lcov. --no-coverage
# is not used. A missing or unreadable score fails closed.
write_base_crap_edn() {
  local base_root work file rc prc idx src manifest repo_root join_log status listing
  local -a base_files=()
  local -a base_args=()
  BASE_EDN=""
  status=0
  repo_root="$(pwd)"
  base_root="$(mktemp -d "${TMPDIR:-/tmp}/ub-crap-base.XXXXXX")"
  work="$(mktemp -d "${TMPDIR:-/tmp}/ub-crap-join.XXXXXX")"
  manifest="${work}/manifest"
  listing="${work}/files.txt"
  : >"${manifest}"
  : >"${listing}"
  for file in "${SCOPE_FILES[@]}"; do
    if git cat-file -e "${MERGE_BASE}:${file}" 2>/dev/null; then
      base_files+=("${file}")
      printf '%s\n' "${file}" >>"${listing}"
    fi
  done
  if [[ "${#base_files[@]}" -eq 0 ]]; then
    rm -rf "${base_root}" "${work}"
    return 0
  fi
  if ! add_base_worktree "${base_root}"; then
    rm -rf "${work}"
    return 1
  fi
  log "ub-gates: base worktree ${MERGE_BASE}"
  if ! run_base_coverage "${base_root}" "${listing}"; then
    status=1
  fi
  base_args=(--threshold 10)
  if [[ -n "${UB_GATES_COVERAGE_CMD:-}" ]]; then
    base_args+=(--use-existing-coverage)
  fi
  idx=0
  if [[ "${status}" -eq 0 ]]; then
    for file in "${base_files[@]}"; do
      log "ub-gates: base crapper ${base_args[*]} ${file}"
      set +e
      (
        cd "${base_root}"
        "${CRAPPER}" "${base_args[@]}" "${file}"
      )
      rc=$?
      set -e
      src="${base_root}/.metrics/crap.edn"
      if [[ "${rc}" -ne 0 && "${rc}" -ne 2 ]]; then
        status=1
        break
      fi
      if ! base_edn_readable "${src}"; then
        status=1
        break
      fi
      cp "${src}" "${work}/base-${idx}.edn" || status=1
      if [[ "${status}" -ne 0 ]]; then
        break
      fi
      # Head probe names the function in the real tree. Its metrics overwrite
      # .metrics/crap.edn; the saved head report is restored later.
      set +e
      "${CRAPPER}" --root "${repo_root}" --use-existing-coverage --threshold 10 "${file}"
      prc=$?
      set -e
      if [[ "${prc}" -eq 0 || "${prc}" -eq 2 ]] && base_edn_readable "${repo_root}/.metrics/crap.edn"; then
        cp "${repo_root}/.metrics/crap.edn" "${work}/head-${idx}.edn" || status=1
      else
        status=1
      fi
      if [[ "${status}" -ne 0 ]]; then
        break
      fi
      printf '%s\t%s\t%s\n' "${file}" "${work}/base-${idx}.edn" "${work}/head-${idx}.edn" >>"${manifest}"
      idx=$((idx + 1))
    done
  fi
  if [[ "${status}" -eq 0 ]]; then
    mkdir -p .metrics
    set +e
    join_log="$(python3 "${SCOPE_PY}" align-base --manifest "${manifest}" --dest ".metrics/crap-base.edn")"
    rc=$?
    set -e
    if [[ "${rc}" -ne 0 ]]; then
      status=1
    elif ! base_edn_readable ".metrics/crap-base.edn"; then
      status=1
    elif [[ -n "${join_log}" ]]; then
      printf '%s\n' "${join_log}"
    fi
  fi
  drop_base_worktree "${base_root}"
  rm -rf "${work}"
  if [[ "${status}" -ne 0 ]]; then
    BASE_EDN=""
    return 1
  fi
  BASE_EDN=".metrics/crap-base.edn"
  base_edn_readable "${BASE_EDN}"
}

score_base_crapper() {
  local saved="" status=0
  if [[ -f .metrics/crap.edn ]]; then
    saved="$(mktemp)"
    cp .metrics/crap.edn "${saved}" || return 1
  fi
  if ! write_base_crap_edn; then
    status=1
  fi
  if [[ -n "${saved}" ]]; then
    mkdir -p .metrics
    cp "${saved}" .metrics/crap.edn || status=1
    rm -f "${saved}"
  fi
  return "${status}"
}

run_crapper() {
  local args=(--threshold 10)
  BASE_EDN=""
  BASE_SCORE_FAILED=0
  if [[ ${#COVERAGE_PREFIX[@]} -gt 0 ]]; then
    args+=("${COVERAGE_PREFIX[@]}")
  fi
  args+=("${SCOPE_FILES[@]}")
  set +e
  "${CRAPPER}" "${args[@]}"
  CRAPPER_RC=$?
  set -e
  if ! score_base_crapper; then
    BASE_SCORE_FAILED=1
    emit_warning "tool_error — base crapper score failed"
  fi
}

run_dryer() {
  rm -f .metrics/dry.edn
  set +e
  "${DRYER}" --threshold "${DRYER_THRESHOLD}" "${SCOPE_FILES[@]}"
  DRYER_RC=$?
  set -e
}

note_mutator_rc() {
  local rc="$1"
  if [[ "${rc}" -eq 1 || "${rc}" -eq 2 ]]; then
    MUTATOR_RC="${rc}"
    TOOL_ERROR=1
    return 1
  fi
  if [[ "${rc}" -eq 3 ]]; then
    MUTATOR_RC=3
    return 0
  fi
  if [[ "${rc}" -ne 0 ]]; then
    MUTATOR_RC="${rc}"
    TOOL_ERROR=1
    return 1
  fi
  return 0
}

mark_mutator_timeout() {
  MUTATOR_TIMEOUT=1
  if [[ "${ADVISORY}" == "1" ]]; then
    emit_warning "mutator budget exhausted; remaining files skipped; non-blocking"
    return 0
  fi
  emit_warning "mutator budget exhausted; remaining files skipped; blocking"
}

scope_has_python() {
  local file
  for file in "${SCOPE_FILES[@]}"; do
    case "${file}" in
      *.py) return 0 ;;
    esac
  done
  return 1
}

run_python_coverage() {
  local rc rcfile
  if ! scope_has_python; then
    return 0
  fi
  if [[ ! -d scripts/tests ]]; then
    return 0
  fi
  if ! command -v coverage >/dev/null 2>&1; then
    return 1
  fi
  rcfile="$(mktemp)"
  cat >"${rcfile}" <<EOF
[run]
patch = subprocess
parallel = true
source = ${SCRIPT_DIR}
omit = ${SCRIPT_DIR}/tests/*
EOF
  mkdir -p target/coverage/python
  set +e
  coverage run --rcfile "${rcfile}" -m unittest discover -s scripts/tests
  rc=$?
  if [[ "${rc}" -eq 0 ]]; then
    coverage combine --rcfile "${rcfile}"
    rc=$?
  fi
  if [[ "${rc}" -eq 0 ]]; then
    coverage lcov --rcfile "${rcfile}" -o target/coverage/python/lcov.info
    rc=$?
  fi
  set -e
  rm -f "${rcfile}"
  if [[ "${rc}" -ne 0 || ! -s target/coverage/python/lcov.info ]]; then
    return 1
  fi
  if [[ ${#COVERAGE_PREFIX[@]} -eq 0 ]]; then
    COVERAGE_PREFIX=(--use-existing-coverage)
  fi
  log "ub-gates: python coverage target/coverage/python/lcov.info"
  return 0
}

mutator_test_command() {
  case "$1" in
    *.py)
      if [[ -d scripts/tests ]]; then
        printf '%s\n' "python3 -m unittest discover -s scripts/tests"
        return 0
      fi
      ;;
  esac
  if [[ -n "${UB_GATES_MUTATOR_TEST_CMD:-}" ]]; then
    printf '%s\n' "${UB_GATES_MUTATOR_TEST_CMD}"
  fi
}

read_mutator_counts() {
  local counts
  MUTATOR_SITES=0
  MUTATOR_KILLED=0
  MUTATOR_SURVIVED=0
  MUTATOR_UNCOVERED=0
  MUTATOR_COVERED=0
  MUTATOR_NOT_RUN=0
  MUTATOR_NO_COVERAGE=0
  MUTATOR_NA=0
  if [[ -d .metrics/mutate ]]; then
    set +e
    changed_json="$(python3 "${SCOPE_PY}" lines --base "${MERGE_BASE}" 2>/dev/null)"
    diff_rc=$?
    set -e
    if [[ "${diff_rc}" -ne 0 ]]; then
      changed_json=""
      log "ub-gates: mutator changed-line diff failed; failing closed"
    fi
    counts="$(
      UB_CHANGED_JSON="${changed_json}" \
      UB_DIFF_OK="$([[ "${diff_rc}" -eq 0 ]] && printf 1 || printf 0)" \
      UB_MUTATOR_STATUS="${MUTATOR_STATUS_LOG:-.metrics/mutator-status.txt}" \
      python3 - <<'PY'
import json
import os
import re
import sys
from pathlib import Path

def totals(text: str) -> tuple[int, int, int, int]:
    def total(name: str) -> int:
        return sum(int(item) for item in re.findall(rf":{name}\s+(\d+)", text))
    return total("sites"), total("killed"), total("survived"), total("uncovered")

def status_lines(path: Path) -> dict[str, dict[str, set[int]]]:
    found = {"UNCOVERED": {}, "KILLED": {}, "SURVIVED": {}}
    if not path.is_file():
        return found
    pattern = re.compile(r"^(UNCOVERED|KILLED|SURVIVED)\s+(\S+):(\d+)\b")
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        match = pattern.match(line.strip())
        if match is None:
            continue
        kind, source, number = match.group(1), match.group(2), int(match.group(3))
        found[kind].setdefault(source, set()).add(number)
    return found

def hits(bucket: dict[str, set[int]], source: str, lines: list[int]) -> bool:
    return bool(bucket.get(source, set()) & set(lines))

def source_of(text: str) -> str:
    match = re.search(r':source\s+"((?:\\.|[^"\\])*)"', text)
    if match is None:
        return ""
    return match.group(1).replace('\\"', '"').replace("\\\\", "\\")

def _gate_relevant(path: str) -> bool:
    if path.endswith(".pyi") or path.endswith(".d.ts"):
        return False
    return path.endswith((
        ".py", ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs",
        ".go", ".rs", ".java", ".clj", ".cljs", ".cljc",
    ))

def _is_test_path(path: str) -> bool:
    # Same rules as bash is_test_path. A tests/ file is test code.
    base = path.rsplit("/", 1)[-1]
    wrapped = "/" + path.strip("/") + "/"
    dirs = ("/test/", "/tests/", "/spec/", "/specs/", "/testdata/", "/__tests__/", "/fixtures/")
    if any(part in wrapped for part in dirs):
        return True
    if base.startswith("test_") and base.endswith(".py"):
        return True
    if base.endswith(("_test.py", "_test.go", "_test.rs", "Test.java")):
        return True
    test_suffixes = (
        ".test.ts", ".test.tsx", ".test.js", ".test.jsx", ".test.mts", ".test.cts",
        ".test.mjs", ".test.cjs", ".spec.ts", ".spec.tsx", ".spec.js", ".spec.jsx",
        ".spec.mts", ".spec.cts", ".spec.mjs", ".spec.cjs",
    )
    return base.endswith(test_suffixes)

def _blank_rust(text: str):
    # Hide comments and strings so a #[cfg(test)] inside one is not an item.
    # None means the scan did not finish; the caller fails closed.
    # The string line set is the spans this scan already blanks.
    chars = list(text)
    i = 0
    n = len(chars)
    string_lines: set[int] = set()

    def spaces(j: int, stop: int) -> None:
        while j < stop:
            if chars[j] != "\n":
                chars[j] = " "
            j += 1

    while i < n:
        c = chars[i]
        nxt = chars[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            j = i
            while j < n and chars[j] != "\n":
                chars[j] = " "
                j += 1
            i = j
            continue
        if c == "/" and nxt == "*":
            depth = 1
            chars[i] = chars[i + 1] = " "
            i += 2
            while i < n and depth:
                if chars[i] == "/" and i + 1 < n and chars[i + 1] == "*":
                    depth += 1
                    chars[i] = chars[i + 1] = " "
                    i += 2
                    continue
                if chars[i] == "*" and i + 1 < n and chars[i + 1] == "/":
                    depth -= 1
                    chars[i] = chars[i + 1] = " "
                    i += 2
                    continue
                if chars[i] != "\n":
                    chars[i] = " "
                i += 1
            if depth:
                return None
            continue
        if c == "'" and nxt == '"' and i + 2 < n and chars[i + 2] == "'":
            i += 3
            continue
        if c == "'" and nxt == "\\" and i + 3 < n and chars[i + 2] == '"' and chars[i + 3] == "'":
            i += 4
            continue
        raw = False
        if c in {"r", "b"} and nxt in {'"', "#"}:
            raw = c == "r" or (c == "b" and nxt == '"')
            if c == "b" and nxt == "r":
                raw = True
        if c == "b" and nxt == "r" and i + 2 < n and chars[i + 2] in {'"', "#"}:
            chars[i] = chars[i + 1] = " "
            i += 2
            c = chars[i]
            nxt = chars[i + 1] if i + 1 < n else ""
            raw = True
        if raw or c == '"':
            start = i
            j = i + (0 if c == '"' and not raw else 1)
            if raw and c != '"':
                j = i
            hashes = 0
            if raw:
                while j < n and chars[j] == "#":
                    hashes += 1
                    j += 1
                if j >= n or chars[j] != '"':
                    i += 1
                    continue
            else:
                j = i
            j += 1
            closer = '"' + ("#" * hashes)
            prev = text[i - 1] if i else ""
            honor_escape = (not raw) and c == '"' and (prev == "b" or prev not in {"r", "c", "#"})
            while j < n and not text.startswith(closer, j):
                if honor_escape and text.startswith("\\", j):
                    j += 2
                    continue
                j += 1
            if j >= n:
                return None
            stop = j + len(closer)
            spaces(start, stop)
            if start < stop:
                first = text.count("\n", 0, start) + 1
                last = text.count("\n", 0, stop - 1) + 1
                string_lines.update(range(first, last + 1))
            i = stop
            continue
        i += 1
    return "".join(chars), string_lines

def _cfg_test_lines(text: str):
    blanked = _blank_rust(text)
    if blanked is None:
        return None
    masked, string_lines = blanked
    found = set()
    for match in re.finditer(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]", masked):
        cursor = match.end()
        while True:
            while cursor < len(masked) and masked[cursor].isspace():
                cursor += 1
            if cursor >= len(masked) or masked[cursor] != "#":
                break
            nxt = cursor + 1
            while nxt < len(masked) and masked[nxt].isspace():
                nxt += 1
            if nxt >= len(masked) or masked[nxt] != "[":
                break
            depth = 0
            while nxt < len(masked):
                if masked[nxt] == "[":
                    depth += 1
                elif masked[nxt] == "]":
                    depth -= 1
                    if depth == 0:
                        nxt += 1
                        break
                nxt += 1
            else:
                return None
            cursor = nxt
        depth = 0
        seen_brace = False
        end = cursor
        while end < len(masked):
            char = masked[end]
            if char in "([{":
                depth += 1
                if char == "{":
                    seen_brace = True
            elif char in ")]}":
                depth = max(0, depth - 1)
                if depth == 0 and seen_brace and char == "}":
                    end += 1
                    break
            elif char == ";" and depth == 0 and not seen_brace:
                end += 1
                break
            end += 1
        else:
            return None
        first = text.count("\n", 0, match.start()) + 1
        last = text.count("\n", 0, max(match.start(), end - 1)) + 1
        found.update(range(first, last + 1))
    return found, string_lines

def _neutral_line(source: str, number: int, string_lines: set[int]) -> bool:
    # Whitespace-only, and outside every string span the scan already found.
    rows = source.splitlines()
    if number < 1 or number > len(rows):
        return False
    if rows[number - 1].strip() != "":
        return False
    return number not in string_lines

def _changed_lines_are_test(changed: dict) -> bool:
    # True only when every changed line of every gate-relevant file is test
    # code or a neutral blank. One production line, or a scan we cannot
    # finish, is false. At least one real test line is required.
    saw = False
    for path, lines in changed.items():
        if not isinstance(path, str) or not _gate_relevant(path):
            continue
        if not isinstance(lines, list):
            return False
        if _is_test_path(path):
            if lines:
                saw = True
            continue
        if not path.endswith(".rs"):
            if lines:
                return False
            continue
        try:
            source = Path(path).read_text(encoding="utf-8")
        except OSError:
            return False
        scanned = _cfg_test_lines(source)
        if scanned is None:
            return False
        spans, string_lines = scanned
        for number in lines:
            if not isinstance(number, int):
                return False
            if _neutral_line(source, number, string_lines):
                continue
            if number not in spans:
                return False
            saw = True
    return saw

sites = killed = survived = uncovered = 0
bare = False
na = False
reason = ""
root = Path(".metrics/mutate")
by_source: dict[str, list[int]] = {}
for path in sorted(root.rglob("*.edn")):
    text = path.read_text(encoding="utf-8", errors="replace")
    file_sites, file_killed, file_survived, file_uncovered = totals(text)
    src = source_of(text) or path.as_posix()
    bucket = by_source.setdefault(src, [0, 0, 0])
    bucket[0] += file_sites
    bucket[1] += file_killed
    bucket[2] += file_survived
    sites += file_sites
    killed += file_killed
    survived += file_survived
    uncovered += file_uncovered

def mark_uncovered_files(suffix: str) -> None:
    global bare, reason
    for src, (file_sites, file_killed, file_survived) in by_source.items():
        if file_sites > 0 and file_killed + file_survived == 0:
            bare = True
            reason = f"{src} has mutation sites and killed + survived == 0{suffix}"

if os.environ.get("UB_DIFF_OK") != "1":
    mark_uncovered_files(" and the diff was unavailable")
else:
    try:
        changed = json.loads(os.environ.get("UB_CHANGED_JSON") or "{}")
    except json.JSONDecodeError as exc:
        print(f"ub-gates: mutator changed-line json failed ({exc}); failing closed", file=sys.stderr)
        changed = None
    if not isinstance(changed, dict):
        mark_uncovered_files(" and the diff was unreadable")
    else:
        if _changed_lines_are_test(changed):
            na = True
        else:
            mark_uncovered_files("")
            reported = status_lines(Path(os.environ["UB_MUTATOR_STATUS"]))
            for source, lines in changed.items():
                if not isinstance(lines, list):
                    continue
                numbers = [int(number) for number in lines]
                uncovered_hit = hits(reported["UNCOVERED"], source, numbers)
                ran = hits(reported["KILLED"], source, numbers) or hits(reported["SURVIVED"], source, numbers)
                if uncovered_hit and not ran:
                    bare = True
                    reason = f"{source} uncovered on a changed line and no changed-line mutant ran"
if bare:
    print("bare")
    if reason:
        print(f"ub-gates: mutator no-coverage: {reason}", file=sys.stderr)
elif na:
    print("na")
print(sites)
print(killed)
print(survived)
print(uncovered)
PY
    )"
    if [[ "${counts%%$'\n'*}" == "bare" ]]; then
      MUTATOR_NO_COVERAGE=1
      counts="${counts#*$'\n'}"
    elif [[ "${counts%%$'\n'*}" == "na" ]]; then
      MUTATOR_NA=1
      counts="${counts#*$'\n'}"
    fi
    MUTATOR_SITES="${counts%%$'\n'*}"
    counts="${counts#*$'\n'}"
    MUTATOR_KILLED="${counts%%$'\n'*}"
    counts="${counts#*$'\n'}"
    MUTATOR_SURVIVED="${counts%%$'\n'*}"
    MUTATOR_UNCOVERED="${counts##*$'\n'}"
    MUTATOR_COVERED=$((MUTATOR_KILLED + MUTATOR_SURVIVED))
    MUTATOR_NOT_RUN=$((MUTATOR_SITES - MUTATOR_COVERED - MUTATOR_UNCOVERED))
  fi
  log "ub-gates: mutator sites=${MUTATOR_SITES} covered=${MUTATOR_COVERED} killed=${MUTATOR_KILLED} survived=${MUTATOR_SURVIVED} uncovered=${MUTATOR_UNCOVERED} not_run=${MUTATOR_NOT_RUN}"
  if [[ "${MUTATOR_NA}" == "1" ]]; then
    log "ub-gates: mutator N/A (0 production mutation sites)"
  fi
}

run_mutator() {
  local deadline file lines left rc args test_cmd
  MUTATOR_RC=0
  MUTATOR_TIMEOUT=0
  MUTATOR_BUDGET_USED="${MUTATOR_BUDGET}"
  MUTATOR_STATUS_LOG=".metrics/mutator-status.txt"
  mkdir -p .metrics
  : > "${MUTATOR_STATUS_LOG}"
  log "ub-gates: mutator budget ${MUTATOR_BUDGET}s"
  deadline=$((SECONDS + MUTATOR_BUDGET))
  for file in "${SCOPE_FILES[@]}"; do
    if (( SECONDS >= deadline )); then
      mark_mutator_timeout
      return 0
    fi
    lines="$(python3 "${SCOPE_PY}" lines --base "${MERGE_BASE}" --file "${file}")"
    if [[ -z "${lines}" ]]; then
      continue
    fi
    left=$((deadline - SECONDS))
    if (( left < 1 )); then
      mark_mutator_timeout
      return 0
    fi
    args=(--lines "${lines}")
    test_cmd="$(mutator_test_command "${file}")"
    if [[ -n "${test_cmd}" ]]; then
      args+=(--test-command "${test_cmd}")
    fi
    if [[ ${#COVERAGE_PREFIX[@]} -gt 0 ]]; then
      args+=("${COVERAGE_PREFIX[@]}")
    fi
    args+=("${file}")
    set +e
    timeout "${left}" "${MUTATOR}" "${args[@]}" >"${MUTATOR_STATUS_LOG}.part" 2>"${MUTATOR_STATUS_LOG}.err"
    rc=$?
    set -e
    if [[ -f "${MUTATOR_STATUS_LOG}.part" ]]; then
      cat "${MUTATOR_STATUS_LOG}.part"
      cat "${MUTATOR_STATUS_LOG}.part" >> "${MUTATOR_STATUS_LOG}"
    fi
    if [[ -f "${MUTATOR_STATUS_LOG}.err" ]]; then
      cat "${MUTATOR_STATUS_LOG}.err" >&2
    fi
    rm -f "${MUTATOR_STATUS_LOG}.part" "${MUTATOR_STATUS_LOG}.err"
    if [[ "${rc}" -eq 124 || "${rc}" -eq 137 ]]; then
      mark_mutator_timeout
      return 0
    fi
    note_mutator_rc "${rc}" || return 0
  done
}

classify_crapper() {
  CRAPPER_FINDINGS=0
  CRAPPER_DEBT=0
  if [[ ! -f .metrics/crap.edn ]]; then
    return 1
  fi
  apply_scope_filter crap .metrics/crap.edn 10 || return 1
  if [[ "${CRAPPER_RC}" -ne 0 && "${CRAPPER_RC}" -ne 2 ]]; then
    return 1
  fi
  return 0
}

classify_dryer() {
  DRYER_FINDINGS=0
  DRYER_DEBT=0
  DRYER_HIGH=0
  # 0 is the only dryer success code. Exit 1, 2, and every other code are
  # tool_error even when .metrics/dry.edn exists.
  if [[ "${DRYER_RC}" -ne 0 ]]; then
    return 1
  fi
  if [[ ! -f .metrics/dry.edn ]]; then
    return 1
  fi
  apply_scope_filter dry .metrics/dry.edn "${DRYER_THRESHOLD}" || return 1
  DRYER_HIGH="${DRYER_FINDINGS}"
  return 0
}

GATE_MISS=0
GATES_RAN=0
OUTCOME=""
TOOL_ERROR=0
COVERAGE_PREFIX=()
FINDING=0

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  OUTCOME="skipped_not_git"
  GATE_MISS=1
  exit_for_outcome
fi

base_remote_name() {
  if [[ "${BASE_REF}" == */* ]]; then
    printf '%s\n' "${BASE_REF%%/*}"
  else
    printf '%s\n' "origin"
  fi
}

base_branch_name() {
  if [[ "${BASE_REF}" == */* ]]; then
    printf '%s\n' "${BASE_REF#*/}"
  else
    printf '%s\n' "${BASE_REF}"
  fi
}

repo_is_shallow() {
  [[ "$(git rev-parse --is-shallow-repository 2>/dev/null || echo false)" == "true" ]]
}

fetch_base_ref() {
  local remote branch
  remote="$(base_remote_name)"
  branch="$(base_branch_name)"
  if repo_is_shallow; then
    git fetch --unshallow "${remote}" 2>/dev/null || git fetch --unshallow 2>/dev/null || true
  fi
  if ! git rev-parse --verify "${BASE_REF}^{commit}" >/dev/null 2>&1; then
    git fetch "${remote}" "+refs/heads/${branch}:refs/remotes/${remote}/${branch}" 2>/dev/null || true
  fi
}

fetch_base_ref

if ! git rev-parse --verify "${BASE_REF}^{commit}" >/dev/null 2>&1; then
  OUTCOME="skipped_no_base_ref"
  GATE_MISS=1
  exit_for_outcome
fi

MERGE_BASE="$(git merge-base HEAD "${BASE_REF}" 2>/dev/null || echo "")"
if [[ -z "${MERGE_BASE}" ]]; then
  OUTCOME="tool_error"
  GATE_MISS=1
  TOOL_ERROR=1
  emit_warning "tool_error — no merge base between HEAD and ${BASE_REF}"
  exit_for_outcome
fi

collect_scope_files "${MERGE_BASE}"
if [[ "${SCOPE_DIFF_FAILED}" -eq 1 ]]; then
  OUTCOME="tool_error"
  GATE_MISS=1
  TOOL_ERROR=1
  emit_warning "tool_error — git diff --name-only failed"
  exit_for_outcome
fi
if [[ "${SCOPE_COUNT}" -eq 0 ]]; then
  OUTCOME="skipped_no_scope"
  GATE_MISS=0
  GATES_RAN=0
  log "ub-gates: no gate-relevant files in ${MERGE_BASE}...HEAD — skipped (OK)"
  exit_for_outcome
fi

SKIP_MUTATOR="${UB_GATES_SKIP_MUTATOR:-0}"
if [[ "${SKIP_MUTATOR}" != "1" && -n "${CURSOR_AGENT:-}" && -z "${GITHUB_ACTIONS:-}" ]]; then
  SKIP_MUTATOR=1
  log "ub-gates: skipping mutator (cloud agent harness; run on laptop or CI)"
fi

CRAPPER="$(gate_bin crapper || true)"
DRYER="$(gate_bin dryer || true)"
MUTATOR=""
if [[ "${SKIP_MUTATOR}" != "1" ]]; then
  MUTATOR="$(gate_bin mutator || true)"
fi

need_mutator=1
if [[ "${SKIP_MUTATOR}" == "1" ]]; then
  need_mutator=0
fi

if [[ -z "${CRAPPER}" || -z "${DRYER}" || ( "${need_mutator}" -eq 1 && -z "${MUTATOR}" ) ]]; then
  OUTCOME="skipped_tools_missing"
  GATE_MISS=1
  GATES_RAN=0
  emit_warning "skipped_tools_missing — crapper, dryer, and mutator not on PATH (${SCOPE_COUNT} gate-relevant file(s) in scope). This is not a gate pass; install tools in CI is a follow-up ticket."
  log "ub-gates: scope had ${SCOPE_COUNT} file(s); see skills/ub-mode/references/gates-by-language.md"
  exit_for_outcome
fi

if [[ -n "${UB_GATES_COVERAGE_CMD:-}" ]]; then
  set +e
  bash -c "${UB_GATES_COVERAGE_CMD}"
  cov_rc=$?
  set -e
  if [[ "${cov_rc}" -ne 0 ]]; then
    OUTCOME="tool_error"
    GATE_MISS=1
    TOOL_ERROR=1
    GATES_RAN=0
    emit_warning "tool_error — UB_GATES_COVERAGE_CMD failed"
    exit_for_outcome
  fi
  if ! lcov_ready; then
    OUTCOME="tool_error"
    GATE_MISS=1
    TOOL_ERROR=1
    GATES_RAN=0
    emit_warning "tool_error — coverage command exited 0 but lcov is missing or empty"
    exit_for_outcome
  fi
  COVERAGE_PREFIX=(--use-existing-coverage)
fi

log "ub-gates: running on ${SCOPE_COUNT} file(s) from ${MERGE_BASE}...HEAD (advisory=${ADVISORY})"
log "ub-gates: ${SCOPE_FILES[*]}"

run_crapper
run_dryer
MUTATOR_SKIPPED=0
MUTATOR_TIMEOUT=0
MUTATOR_NO_COVERAGE=0
PYTHON_COVERAGE_FAILED=0
if [[ "${SKIP_MUTATOR}" == "1" ]]; then
  MUTATOR_SKIPPED=1
  MUTATOR_RC=""
else
  if ! run_python_coverage; then
    PYTHON_COVERAGE_FAILED=1
    emit_warning "tool_error — python coverage for changed .py files failed"
  fi
  run_mutator
  read_mutator_counts
fi

if [[ -n "${MUTATOR_RC}" ]]; then
  log "ub-gates: crapper exit ${CRAPPER_RC}; dryer exit ${DRYER_RC}; mutator exit ${MUTATOR_RC}"
else
  log "ub-gates: crapper exit ${CRAPPER_RC}; dryer exit ${DRYER_RC}; mutator skipped"
fi

if ! classify_crapper; then
  TOOL_ERROR=1
fi
if ! classify_dryer; then
  TOOL_ERROR=1
fi
if [[ "${BASE_SCORE_FAILED:-0}" -eq 1 ]]; then
  TOOL_ERROR=1
fi
if [[ "${PYTHON_COVERAGE_FAILED:-0}" -eq 1 ]]; then
  TOOL_ERROR=1
fi

if [[ "${TOOL_ERROR}" -eq 0 ]]; then
  if [[ "${CRAPPER_FINDINGS}" -gt 0 ]]; then FINDING=1; fi
  if [[ "${DRYER_FINDINGS}" -gt 0 ]]; then FINDING=1; fi
  if [[ -n "${MUTATOR_RC}" && "${MUTATOR_RC}" -eq 3 ]]; then FINDING=1; fi
fi

GATES_RAN=1
if [[ "${TOOL_ERROR}" -eq 1 ]]; then
  OUTCOME="tool_error"
  GATE_MISS=1
  emit_warning "tool_error — dryer/mutator tool failure or missing .metrics/dry.edn after dryer; not ran_clean."
elif [[ "${MUTATOR_NO_COVERAGE:-0}" == "1" ]]; then
  OUTCOME="mutator_no_coverage"
  GATE_MISS=1
elif [[ "${MUTATOR_TIMEOUT:-0}" == "1" && "${ADVISORY}" != "1" ]]; then
  OUTCOME="mutator_timeout"
  GATE_MISS=1
elif [[ "${FINDING}" -eq 1 ]]; then
  OUTCOME="ran_with_findings"
elif [[ "${MUTATOR_SKIPPED}" == "1" ]]; then
  OUTCOME="skipped_mutator"
  GATE_MISS=1
else
  OUTCOME="ran_clean"
fi

exit_for_outcome
