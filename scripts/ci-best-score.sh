#!/usr/bin/env bash
# Read the current best (lowest) SCORE from fixtures/baselines.tsv.
# FROZEN — do not edit as part of autoresearch.
set -euo pipefail

file="${1:-fixtures/baselines.tsv}"
if [[ ! -f "$file" ]]; then
  echo "ci-best-score: missing $file" >&2
  exit 1
fi

best="$(awk -F'\t' '
  /^#/ { next }
  NF < 2 { next }
  $1 == "algorithm" { next }
  {
    for (i = 2; i <= NF; i++) {
      if ($i ~ /^[0-9]+$/) {
        print $i
        break
      }
    }
  }
' "$file" | sort -n | head -1)"

# Prefer RESULTS.md current record when reading the default baseline file.
if [[ "$file" == "fixtures/baselines.tsv" && -f RESULTS.md ]]; then
  record="$(sed -n 's/^\*\*Current record: \([0-9][0-9]*\).*/\1/p' RESULTS.md | head -1)"
  if [[ -n "$record" ]]; then
    best="$record"
  fi
fi

if [[ -z "$best" ]]; then
  echo "ci-best-score: no record in $file" >&2
  exit 1
fi

echo "$best"
