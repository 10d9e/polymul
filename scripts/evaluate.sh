#!/usr/bin/env bash
# Evaluate one candidate: boundary guard -> correctness gate -> score.
# FROZEN — do not edit as part of autoresearch.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$PATH:/usr/bin"

if [[ "${1:-}" != "--no-guard" ]]; then
  echo "== boundary guard =="
  bash scripts/guard.sh
fi

echo "== correctness gate (correctness tests) =="
if ! cargo test --release >/tmp/polymul_test.log 2>&1; then
  echo "TESTS FAILED — candidate is INVALID:"
  tail -n 30 /tmp/polymul_test.log
  exit 1
fi
grep -E "test result" /tmp/polymul_test.log

echo "== build =="
cargo build --release --quiet

echo "== score =="
./target/release/polymul eval
