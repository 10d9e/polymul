#!/usr/bin/env bash
# Run evaluate and fail unless SCORE beats the record on the base branch.
# FROZEN — do not edit as part of autoresearch.
set -euo pipefail
cd "$(dirname "$0")/.."

baseline_file="fixtures/baselines.tsv"
if [[ -n "${GITHUB_BASE_SHA:-}" ]]; then
  git show "${GITHUB_BASE_SHA}:fixtures/baselines.tsv" > /tmp/base-baselines.tsv 2>/dev/null || true
  git show "${GITHUB_BASE_SHA}:RESULTS.md" > /tmp/base-results.md 2>/dev/null || true
  baseline_file="/tmp/base-baselines.tsv"
  if [[ -f /tmp/base-results.md ]]; then
    RESULTS_MD=/tmp/base-results.md
  fi
fi

if [[ -n "${RESULTS_MD:-}" && -f "$RESULTS_MD" ]]; then
  best="$(sed -n 's/^\*\*Current record: \([0-9][0-9]*\).*/\1/p' "$RESULTS_MD" | head -1)"
fi
if [[ -z "${best:-}" ]]; then
  best="$(bash scripts/ci-best-score.sh "$baseline_file")"
fi
echo "Current record SCORE: $best (lower is better)"

if ! bash scripts/evaluate.sh --no-guard 2>&1 | tee /tmp/polymul_eval.out; then
  echo "::error::evaluate.sh failed — candidate is INVALID"
  exit 1
fi

score="$(sed -n 's/^SCORE: \([0-9][0-9]*\).*/\1/p' /tmp/polymul_eval.out | tail -1)"
if [[ -z "$score" ]]; then
  echo "::error::No numeric SCORE in evaluate output"
  exit 1
fi

echo "Candidate SCORE: $score"

if (( score >= best )); then
  echo "::error::SCORE $score does not beat the current record $best (lower is better)"
  exit 1
fi

echo "SCORE gate passed: $score < $best"
if [[ -n "${GITHUB_ENV:-}" ]]; then
  printf 'CANDIDATE_SCORE=%s\n' "$score" >> "$GITHUB_ENV"
fi
