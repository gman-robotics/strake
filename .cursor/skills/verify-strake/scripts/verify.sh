#!/usr/bin/env bash
# Build strake, drive CLI contracts on main, run crate tests, write evidence.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
cd "$ROOT"

SCRATCH=""
cleanup() {
  if [[ -n "$SCRATCH" && -d "$SCRATCH" ]]; then
    rm -rf "$SCRATCH"
  fi
}
trap cleanup EXIT

EVID_ROOT="$ROOT/.cursor/skills/verify-strake/evidence"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
EVID_DIR="$EVID_ROOT/$STAMP"
mkdir -p "$EVID_DIR"

SUMMARY="$EVID_DIR/summary.txt"
fail() {
  echo "FAIL: $*" | tee -a "$SUMMARY"
  exit 1
}
pass() {
  echo "PASS: $*" | tee -a "$SUMMARY"
}

echo "verify-strake run $STAMP" >"$SUMMARY"
echo "root=$ROOT" >>"$SUMMARY"

echo "== build ==" | tee -a "$SUMMARY"
cargo build --locked -p strake 2>&1 | tee "$EVID_DIR/cargo-build.log"
pass "cargo build"

BIN="${STRAKE_BIN:-$ROOT/target/debug/strake}"
export STRAKE_BIN="$BIN"

echo "== doctor ==" | tee -a "$SUMMARY"
"$ROOT/.cursor/skills/verify-strake/scripts/doctor.sh" 2>&1 | tee "$EVID_DIR/doctor.log"
pass "doctor"

run_case() {
  local name=$1
  shift
  local out="$EVID_DIR/$name.txt"
  set +e
  "$@" >"$out.stdout" 2>"$out.stderr"
  local code=$?
  set -e
  {
    echo "cmd: $*"
    echo "exit: $code"
    echo "--- stdout ---"
    cat "$out.stdout"
    echo "--- stderr ---"
    cat "$out.stderr"
  } >"$out"
  echo "$code"
}

assert_exit() {
  local name=$1
  local want=$2
  shift 2
  local got
  got="$(run_case "$name" "$@")"
  if [[ "$got" -ne "$want" ]]; then
    fail "$name expected exit $want got $got (see $EVID_DIR/$name.txt)"
  fi
  pass "$name exit $want"
}

echo "== cli-top-level-usage ==" | tee -a "$SUMMARY"
assert_exit "cli-empty" 2 "$BIN"
grep -q 'usage: strake check | fmt | test | run | build' "$EVID_DIR/cli-empty.txt" \
  || fail "cli-empty missing top-level usage"
pass "cli-empty usage text"

assert_exit "cli-unknown-cmd" 2 "$BIN" not-a-command
grep -q 'usage: strake check | fmt | test | run | build' "$EVID_DIR/cli-unknown-cmd.txt" \
  || fail "cli-unknown-cmd missing top-level usage"

echo "== build-post-mvp ==" | tee -a "$SUMMARY"
assert_exit "build-post-mvp" 2 "$BIN" build
diff -q <(printf 'build is post-MVP\n') "$EVID_DIR/build-post-mvp.txt.stdout" >/dev/null \
  || fail "build stdout mismatch"
[[ ! -s "$EVID_DIR/build-post-mvp.txt.stderr" ]] || fail "build stderr must be empty"

echo "== check-usage-contract ==" | tee -a "$SUMMARY"
assert_exit "check-usage" 2 "$BIN" check
grep -q '\[--blob-root DIR\]' "$EVID_DIR/check-usage.txt" || fail "check missing blob-root"
grep -q '<file|dir>' "$EVID_DIR/check-usage.txt" || fail "check missing file|dir"

echo "== fmt-test-run-usage ==" | tee -a "$SUMMARY"
assert_exit "fmt-usage" 2 "$BIN" fmt
grep -q 'usage: strake fmt <file>' "$EVID_DIR/fmt-usage.txt" || fail "fmt usage"
assert_exit "test-usage" 2 "$BIN" test
grep -q 'usage: strake test' "$EVID_DIR/test-usage.txt" || fail "test usage"
assert_exit "run-usage" 2 "$BIN" run
grep -q 'usage: strake run --fn NAME' "$EVID_DIR/run-usage.txt" || fail "run usage"

echo "== rust-test-suite ==" | tee -a "$SUMMARY"
set +e
cargo test --locked -p strake 2>&1 | tee "$EVID_DIR/cargo-test.log"
test_code=${PIPESTATUS[0]}
set -e
if [[ "$test_code" -ne 0 ]]; then
  fail "cargo test exited $test_code"
fi
pass "cargo test"

if [[ -d "$ROOT/golden" ]]; then
  echo "== golden-smoke (golden/ present) ==" | tee -a "$SUMMARY"
  # Extend GOLDEN_SMOKE as T2+ features land; each entry: "exit:cmd args..."
  # Each entry: "exit:subcommand args..." (strake binary is prepended).
  GOLDEN_SMOKE=(
    "0:test golden/add2_basic.strake"
    "0:check golden/priv.strake"
  )
  for entry in "${GOLDEN_SMOKE[@]}"; do
    want_exit="${entry%%:*}"
    cmd_rest="${entry#*:}"
    safe_name="golden-${cmd_rest// /-}"
    safe_name="${safe_name//\//-}"
    # shellcheck disable=SC2086
    assert_exit "$safe_name" "$want_exit" "$BIN" $cmd_rest
  done
  echo "== golden-fmt-idempotent ==" | tee -a "$SUMMARY"
  assert_exit "golden-fmt-add2-worked" 0 "$BIN" fmt golden/add2_worked.strake
  diff -q golden/add2_worked.strake "$EVID_DIR/golden-fmt-add2-worked.txt.stdout" >/dev/null \
    || fail "golden fmt add2_worked stdout mismatch"
  pass "golden fmt add2_worked idempotent"
else
  echo "golden/: absent — skipped (see features/golden-fixtures.md)" >>"$SUMMARY"
fi

SCRATCH="$(mktemp -d /tmp/strake-verify-XXXXXX)"
echo "scratch=$SCRATCH (removed on EXIT)" >>"$SUMMARY"

echo "verify-strake: all checks passed" | tee -a "$SUMMARY"
echo "evidence: $EVID_DIR"
