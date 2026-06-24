#!/usr/bin/env bash
# Record a valid submission: append history entry + leaderboard row.
# FROZEN — do not edit as part of autoresearch.
#
# Usage:
#   bash scripts/record.sh --author @handle --model "codex 5.5" --note "what changed"
#   bash scripts/record.sh --ci --author @handle --model "..." --note "..." --diff-base HEAD~1
set -euo pipefail
cd "$(dirname "$0")/.."

author=""
model=""
note=""
attempts=""
ci_mode=0
diff_base="HEAD"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --author) author="${2:-}"; shift 2 ;;
    --model) model="${2:-}"; shift 2 ;;
    --note) note="${2:-}"; shift 2 ;;
    --attempts) attempts="${2:-}"; shift 2 ;;
    --ci) ci_mode=1; shift ;;
    --diff-base) diff_base="${2:-}"; shift 2 ;;
    -h|--help)
      sed -n '2,10p' "$0"
      exit 0
      ;;
    *) echo "record.sh: unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [[ -z "$note" ]]; then
  echo "record.sh: --note is required" >&2
  exit 2
fi

if (( ci_mode )) && [[ -z "$model" ]]; then
  echo "record.sh: --model is required in CI mode" >&2
  exit 2
fi

if [[ -z "$author" ]]; then
  author="$(git config --get github.user 2>/dev/null || true)"
fi
if [[ -z "$author" ]]; then
  author="$(git config user.name 2>/dev/null || echo unknown)"
fi
[[ "$author" != @* ]] && author="@${author}"

if (( ! ci_mode )); then
  bash scripts/guard.sh
fi

if [[ ! -x ./target/release/polymul ]]; then
  cargo build --release --quiet
fi

echo "== score (for snapshot) =="
eval_out="$(./target/release/polymul eval 2>&1)" || {
  echo "$eval_out"
  echo "record.sh: eval failed" >&2
  exit 1
}
echo "$eval_out"

if echo "$eval_out" | grep -q 'SCORE: INVALID'; then
  echo "record.sh: correctness failed — not recorded" >&2
  exit 1
fi

score="$(echo "$eval_out" | sed -n 's/^SCORE: \([0-9][0-9]*\).*/\1/p' | tail -1)"
if [[ -z "$score" ]]; then
  echo "record.sh: could not parse SCORE" >&2
  exit 1
fi

commit="$(git rev-parse --short HEAD)"
commit_full="$(git rev-parse HEAD)"
git_name="$(git config user.name 2>/dev/null || echo unknown)"
git_email="$(git config user.email 2>/dev/null || echo unknown)"
date_iso="$(date +%Y-%m-%d)"

diff_stat="$(git diff --stat "$diff_base" HEAD -- src/algorithm/ 2>/dev/null || true)"
[[ -z "$diff_stat" ]] && diff_stat="(no algorithm diff between ${diff_base} and HEAD)"

next=1
for f in history/entries/*.md; do
  [[ -e "$f" ]] || continue
  n="${f##*/}"
  n="${n%%-*}"
  n="${n#0}"; n="${n#0}"; n="${n#0}"
  if [[ "$n" =~ ^[0-9]+$ ]] && (( 10#$n >= next )); then
    next=$((10#$n + 1))
  fi
done
entry_id="$(printf '%04d' "$next")"

prev_score=""
while IFS= read -r line; do
  case "$line" in
    "| "[0-9]*) ;;
    *) continue ;;
  esac
  s="$(echo "$line" | awk -F'|' '{gsub(/ /,"",$5); print $5}')"
  [[ "$s" =~ ^[0-9]+$ ]] || continue
  if [[ -z "$prev_score" ]] || (( s < prev_score )); then
    prev_score="$s"
  fi
done < RESULTS.md

if [[ -n "$prev_score" ]]; then
  delta=$((score - prev_score))
  if (( delta < 0 )); then
    delta_str="${delta} (new record)"
    status="record"
  else
    delta_str="+${delta}"
    status="attempt"
  fi
else
  delta_str="— (first entry)"
  status="record"
fi

slug="$(echo "$author" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9@._-' | tr '@.' '-')"
slug="${slug:-unknown}"
entry_file="history/entries/${entry_id}-${slug}.md"
mkdir -p history/entries

{
  echo "# Entry ${entry_id} — SCORE ${score} (${delta_str})"
  echo
  echo "| Field | Value |"
  echo "|-------|-------|"
  echo "| Date | ${date_iso} |"
  echo "| Author | ${author} |"
  [[ -n "$model" ]] && echo "| Model | ${model} |"
  echo "| Git author | ${git_name} \<${git_email}\> |"
  echo "| Commit | \`${commit}\` (${commit_full}) |"
  echo "| SCORE | ${score} |"
  echo "| Δ vs previous record | ${delta_str} |"
  echo "| Status | ${status} |"
  echo
  echo "## Approach"
  echo
  echo "$note"
  echo
  if [[ -n "$attempts" ]]; then
    echo "## Iteration notes"
    echo
    echo "$attempts"
    echo
  fi
  echo "## Algorithm changes"
  echo
  echo '```'
  echo "$diff_stat"
  echo '```'
  echo
  echo "## Eval snapshot"
  echo
  echo '```'
  echo "$eval_out"
  echo '```'
} > "$entry_file"

short_note="$(echo "$note" | tr '\n' ' ' | sed 's/  */ /g' | cut -c1-80)"
[[ ${#note} -gt 80 ]] && short_note="${short_note}…"

row="| ${entry_id} | ${date_iso} | ${author} | ${score} | ${delta_str} | \`${commit}\` | [${entry_id}](history/entries/${entry_id}-${slug}.md) | ${short_note} |"

tmp_results="$(mktemp)"
awk -v row="$row" '
  { lines[NR] = $0; if ($0 ~ /^\|/) last = NR }
  END {
    for (i = 1; i <= NR; i++) {
      print lines[i]
      if (i == last) print row
    }
  }
' RESULTS.md > "$tmp_results" && mv "$tmp_results" RESULTS.md

if [[ "$status" == "record" ]]; then
  if grep -q '^\*\*Current record:' RESULTS.md; then
    sed -i.bak "s/^\*\*Current record:.*/\*\*Current record: ${score}\*\* (${author}, entry ${entry_id})/" RESULTS.md
    rm -f RESULTS.md.bak
  fi
  name="record_${entry_id}"
  printf '%s\t%s\t%s\n' "$name" "$score" "entry ${entry_id} by ${author}" >> fixtures/baselines.tsv
fi

echo
echo "Recorded entry ${entry_id} (${status}): SCORE ${score}"
echo "  history: ${entry_file}"
echo "  leaderboard: RESULTS.md"
