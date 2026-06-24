#!/usr/bin/env bash
# Scorekeeper — SCORE phase. FROZEN — do not edit as part of autoresearch.
set -euo pipefail
cd "$(dirname "$0")/.."

OUT_DIR="${OUT_DIR:-ledger-out}"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
printf 'RECORD=0\n' > "$OUT_DIR/meta.env"

commit_msg="${GITHUB_EVENT_HEAD_COMMIT_MESSAGE:-$(git log -1 --format=%B)}"
if [[ "$commit_msg" == *"[skip ci]"* ]]; then
  echo "scorekeeper: skipping bot ledger commit"
  exit 0
fi

if ! git rev-parse HEAD~1 >/dev/null 2>&1; then
  echo "scorekeeper: no parent commit"
  exit 0
fi

algo_changed="$(git diff --name-only HEAD~1 HEAD -- src/algorithm/ || true)"
ledger_changed="$(git diff --name-only HEAD~1 HEAD -- RESULTS.md history/entries/ fixtures/baselines.tsv || true)"

if [[ -n "$ledger_changed" && -z "$algo_changed" ]]; then
  echo "INTEGRITY VIOLATION: ledger changed without algorithm update" >&2
  exit 1
fi
if [[ -n "$ledger_changed" && -n "$algo_changed" ]]; then
  echo "INTEGRITY VIOLATION: do not commit RESULTS.md or history/entries/ in PRs" >&2
  exit 1
fi
if [[ -z "$algo_changed" ]]; then
  echo "scorekeeper: no algorithm changes; nothing to record"
  exit 0
fi

echo "== algorithm changed =="
printf '  %s\n' $algo_changed

echo "== evaluate (authoritative score) =="
eval_log="$(mktemp)"
bash scripts/evaluate.sh --no-guard 2>&1 | tee "$eval_log"

score="$(sed -n 's/^SCORE: \([0-9][0-9]*\).*/\1/p' "$eval_log" | tail -1)"
best="$(bash scripts/ci-best-score.sh)"
if [[ -z "$score" ]]; then
  echo "scorekeeper: could not parse SCORE" >&2
  exit 1
fi
if (( score >= best )); then
  echo "INTEGRITY VIOLATION: SCORE $score does not beat record $best on main" >&2
  exit 1
fi
cp "$eval_log" /tmp/polymul_eval.out

author="@${GITHUB_ACTOR:-unknown}"
model=""
note=""
attempts=""
pr_body=""
if [[ -n "${GITHUB_REPOSITORY:-}" && -n "${GITHUB_SHA:-}" ]]; then
  pr_body="$(gh api "repos/${GITHUB_REPOSITORY}/commits/${GITHUB_SHA}/pulls" \
    --jq '.[0].body // empty' 2>/dev/null || true)"
  pr_author="$(gh api "repos/${GITHUB_REPOSITORY}/commits/${GITHUB_SHA}/pulls" \
    --jq '.[0].user.login // empty' 2>/dev/null || true)"
  [[ -n "$pr_author" ]] && author="@${pr_author}"
fi
if [[ -n "$pr_body" ]]; then
  model="$(bash scripts/ci-parse-pr-body.sh Model "$pr_body" || true)"
  note="$(bash scripts/ci-parse-pr-body.sh Approach "$pr_body" || true)"
  attempts="$(bash scripts/ci-parse-pr-body.sh "Iteration notes" "$pr_body" || true)"
fi
if [[ -z "$model" ]]; then
  echo "scorekeeper: missing ## Model in PR description" >&2
  exit 1
fi
[[ -z "$note" ]] && note="$(git log -1 --format=%B | sed '/^$/d' | head -5)"

record_args=(--ci --author "$author" --model "$model" --note "$note" --diff-base HEAD~1)
[[ -n "$attempts" ]] && record_args+=(--attempts "$attempts")

echo "== record submission =="
rec_out="$(bash scripts/record.sh "${record_args[@]}")"
echo "$rec_out"

entry_file="$(printf '%s\n' "$rec_out" | sed -n 's/^  history: //p' | tail -1)"
if [[ -z "$entry_file" || ! -f "$entry_file" ]]; then
  echo "scorekeeper: record.sh produced no entry" >&2
  exit 1
fi
entry_base="${entry_file##*/}"
entry_id="${entry_base%%-*}"

cp RESULTS.md "$OUT_DIR/RESULTS.md"
mkdir -p "$OUT_DIR/entries"
cp "$entry_file" "$OUT_DIR/entries/$entry_base"
cp fixtures/baselines.tsv "$OUT_DIR/baselines.tsv"
cat > "$OUT_DIR/meta.env" <<EOF
RECORD=1
ENTRY_ID=$entry_id
ENTRY_FILE=$entry_base
EOF

echo "scorekeeper(score): prepared entry $entry_id for publish"
