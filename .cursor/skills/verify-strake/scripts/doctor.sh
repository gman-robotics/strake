#!/usr/bin/env bash
# Read-only: is the strake binary worth driving?
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
cd "$ROOT"

BIN="${STRAKE_BIN:-$ROOT/target/debug/strake}"

if [[ ! -x "$BIN" ]]; then
  echo "doctor: missing executable: $BIN (run cargo build --locked -p strake)" >&2
  exit 1
fi

out="$(mktemp)"
trap 'rm -f "$out"' EXIT

set +e
"$BIN" >"$out" 2>&1
code=$?
set -e

if [[ "$code" -ne 2 ]]; then
  echo "doctor: expected exit 2 from empty argv, got $code" >&2
  cat "$out" >&2
  exit 1
fi

if ! grep -q 'usage: strake check | fmt | test | run | build' "$out"; then
  echo "doctor: top-level usage not found on stderr/stdout" >&2
  cat "$out" >&2
  exit 1
fi

echo "doctor: ok ($BIN)"
