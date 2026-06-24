#!/usr/bin/env bash
# After a winning push to main, append the new record to fixtures/baselines.tsv.
# FROZEN — do not edit as part of autoresearch.
set -euo pipefail
cd "$(dirname "$0")/.."

score="${CANDIDATE_SCORE:-}"
if [[ -z "$score" ]]; then
  score="$(sed -n 's/^SCORE: \([0-9][0-9]*\).*/\1/p' /tmp/polymul_eval.out 2>/dev/null | tail -1)"
fi
if [[ -z "$score" ]]; then
  echo "ci-update-baseline: missing CANDIDATE_SCORE" >&2
  exit 1
fi

best="$(bash scripts/ci-best-score.sh fixtures/baselines.tsv)"
if (( score >= best )); then
  echo "ci-update-baseline: SCORE $score does not beat record $best; skip update"
  exit 0
fi

short_sha="$(git rev-parse --short HEAD)"
name="record_${short_sha}"
notes="CI record on $(date -u +%Y-%m-%dT%H:%MZ)"

printf '%s\t%s\t%s\n' "$name" "$score" "$notes" >> fixtures/baselines.tsv

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git add fixtures/baselines.tsv
git commit -m "$(cat <<EOF
Record SCORE $score on main.

[skip ci]
EOF
)"
git push

echo "Updated baselines.tsv with SCORE $score"
