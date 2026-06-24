#!/usr/bin/env bash
# Scorekeeper — PUBLISH phase. FROZEN — do not edit as part of autoresearch.
set -euo pipefail
cd "$(dirname "$0")/.."

IN_DIR="${IN_DIR:-ledger-in}"
if [[ ! -f "$IN_DIR/meta.env" ]]; then
  echo "publish: no ledger artifact"
  exit 0
fi

# shellcheck disable=SC1090,SC1091
source "$IN_DIR/meta.env"
if [[ "${RECORD:-0}" != "1" ]]; then
  echo "publish: nothing to publish"
  exit 0
fi

if [[ ! "${ENTRY_ID:-}" =~ ^[0-9]{4}$ ]]; then
  echo "publish: bad ENTRY_ID" >&2
  exit 1
fi
case "${ENTRY_FILE:-}" in
  ""|*..*|*/*) echo "publish: bad ENTRY_FILE" >&2; exit 1 ;;
esac
if [[ ! -f "$IN_DIR/RESULTS.md" || ! -f "$IN_DIR/entries/$ENTRY_FILE" ]]; then
  echo "publish: missing ledger files" >&2
  exit 1
fi

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

for attempt in $(seq 1 8); do
  git fetch --quiet origin main
  git reset --quiet --hard origin/main
  cp "$IN_DIR/RESULTS.md" RESULTS.md
  cp "$IN_DIR/baselines.tsv" fixtures/baselines.tsv
  mkdir -p history/entries
  cp "$IN_DIR/entries/$ENTRY_FILE" "history/entries/$ENTRY_FILE"
  git add RESULTS.md fixtures/baselines.tsv "history/entries/$ENTRY_FILE"
  if git diff --staged --quiet; then
    echo "publish: ledger already current (entry ${ENTRY_ID})"
    exit 0
  fi
  git commit -q -m "$(cat <<EOF
ci: record submission ${ENTRY_ID} [skip ci]

Authoritative ledger update from verified evaluate on main.
EOF
)"
  if git push --quiet origin HEAD:main; then
    echo "publish: pushed entry ${ENTRY_ID} (attempt ${attempt})"
    exit 0
  fi
  echo "publish: push rejected; retrying (attempt ${attempt})"
  sleep $(( (RANDOM % 5) + 1 ))
done
echo "publish: failed after 8 attempts" >&2
exit 1
