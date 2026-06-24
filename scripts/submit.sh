#!/usr/bin/env bash
# Submit a candidate: evaluate locally, push branch, open PR, wait for CI.
#
# Usage:
#   bash scripts/submit.sh --model "opus 4.8"
#
# Options:
#   --model    <name>   AI model used (required by CI)
#   --title    <text>   PR title
#   --approach <text>   ## Approach body
#   --notes    <text>   ## Iteration notes
#   --commit   <msg>    Commit uncommitted src/algorithm/ changes first
#   --no-wait           Create PR without waiting for merge
#   --yes               Skip confirmation prompts
set -euo pipefail
cd "$(dirname "$0")/.."

MODEL="${POLYMUL_MODEL:-}"
TITLE=""
APPROACH=""
NOTES=""
COMMIT_MSG=""
WAIT=1
ASSUME_YES=0

die()  { echo "submit: $*" >&2; exit 1; }
info() { echo "==> $*"; }

usage() { awk 'NR==1{next} /^#/{sub(/^# ?/,""); print; next} {exit}' "$0"; exit 0; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --model)    MODEL="${2:?}"; shift 2;;
    --title)    TITLE="${2:?}"; shift 2;;
    --approach) APPROACH="${2:?}"; shift 2;;
    --notes)    NOTES="${2:?}"; shift 2;;
    --commit)   COMMIT_MSG="${2:?}"; shift 2;;
    --no-wait)  WAIT=0; shift;;
    --yes|-y)   ASSUME_YES=1; shift;;
    -h|--help)  usage;;
    *) die "unknown option: $1";;
  esac
done

confirm() {
  [[ "$ASSUME_YES" == 1 ]] && return 0
  [[ -t 0 ]] || die "non-interactive; pass --yes"
  local reply
  read -r -p "$1 [y/N] " reply
  [[ "$reply" =~ ^[Yy]$ ]]
}

command -v git >/dev/null || die "git not found"
command -v gh  >/dev/null || die "gh not found — https://cli.github.com"
git rev-parse --git-dir >/dev/null 2>&1 || die "not a git repo"

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
case "$BRANCH" in
  main|master|HEAD) die "create a feature branch: git checkout -b improve/<name>";;
esac

info "checking GitHub authentication"
if ! gh auth status >/dev/null 2>&1; then
  [[ -t 0 ]] || die "run 'gh auth login' first"
  gh auth login || die "gh auth login failed"
fi

if ! git diff --quiet || ! git diff --cached --quiet || \
   [[ -n "$(git ls-files --others --exclude-standard)" ]]; then
  outside="$( { git diff --name-only HEAD; git ls-files --others --exclude-standard; } \
    | sort -u | grep -v '^src/algorithm/' || true)"
  [[ -n "$outside" ]] && die $'uncommitted changes outside src/algorithm/:\n'"$outside"

  echo "Uncommitted changes in src/algorithm/:"
  git status --short -- src/algorithm/
  [[ -z "$COMMIT_MSG" ]] && { read -r -p "Commit message: " COMMIT_MSG; }
  [[ -n "$COMMIT_MSG" ]] || die "pass --commit or commit first"
  git add -A -- src/algorithm/
  git commit -m "$COMMIT_MSG"
fi

git rev-parse --verify -q origin/main >/dev/null 2>&1 || git fetch origin main --quiet
[[ -z "$(git rev-list origin/main..HEAD)" ]] && die "no commits ahead of origin/main"

info "evaluating candidate (guard + tests + score)"
eval_out="$(mktemp)"
if ! bash scripts/evaluate.sh | tee "$eval_out"; then
  die "local evaluation failed"
fi
SCORE="$(grep -oE 'SCORE:[[:space:]]*[0-9]+' "$eval_out" | grep -oE '[0-9]+' | tail -1 || true)"
RECORD="$(bash scripts/ci-best-score.sh 2>/dev/null || true)"
if [[ -n "$SCORE" && -n "$RECORD" ]]; then
  if (( SCORE >= RECORD )); then
    die "local SCORE $SCORE does not beat record $RECORD — improve before submitting"
  fi
  info "local SCORE $SCORE beats record $RECORD"
fi

if [[ -z "$MODEL" ]]; then
  [[ -t 0 ]] || die "--model is required"
  read -r -p "AI model used: " MODEL
  [[ -n "$MODEL" ]] || die "--model is required"
fi

commit_subjects="$(git log --reverse --format='%s' origin/main..HEAD)"
commit_bodies="$(git log --reverse --format='%s%n%b' origin/main..HEAD)"
[[ -z "$TITLE" ]]    && TITLE="$(echo "$commit_subjects" | head -1)"
[[ -z "$TITLE" ]]    && TITLE="$BRANCH"
[[ -z "$APPROACH" ]] && APPROACH="$commit_bodies"

body_file="$(mktemp)"
{
  echo "## Model"
  echo; echo "$MODEL"; echo
  echo "## Approach"
  echo; echo "$APPROACH"
  if [[ -n "$NOTES" ]]; then
    echo; echo "## Iteration notes"; echo; echo "$NOTES"
  fi
  echo; echo "## Validation"
  echo
  echo '`bash scripts/evaluate.sh` passed locally; only `src/algorithm/` changed.'
  [[ -n "$SCORE" ]] && echo "Local SCORE: \`$SCORE\` (CI recomputes after merge)."
} > "$body_file"

echo; echo "Branch: $BRANCH"; echo "Title: $TITLE"; echo "Model: $MODEL"
echo "----- PR body -----"; cat "$body_file"; echo "-------------------"
confirm "push '$BRANCH' and open PR?" || die "aborted"

info "pushing $BRANCH"
git push -u origin "$BRANCH"

PR="$(gh pr list --head "$BRANCH" --state open --json number --jq '.[0].number // empty')"
if [[ -n "$PR" ]]; then
  info "updating PR #$PR"
  gh pr edit "$PR" --title "$TITLE" --body-file "$body_file" >/dev/null
  gh pr close "$PR" >/dev/null && gh pr reopen "$PR" >/dev/null
else
  url="$(gh pr create --base main --head "$BRANCH" --title "$TITLE" --body-file "$body_file")"
  PR="$(basename "$url")"
  info "opened PR #$PR — $url"
fi

[[ "$WAIT" == 0 ]] && { info "PR #$PR created (--no-wait)"; exit 0; }

info "waiting for CI on PR #$PR"
checks_deadline=$(( $(date +%s) + 180 ))
until gh pr checks "$PR" >/dev/null 2>&1; do
  [[ "$(date +%s)" -gt "$checks_deadline" ]] && die "no CI checks after 3m"
  sleep 5
done
if ! gh pr checks "$PR" --watch --fail-fast; then
  die "CI failed for PR #$PR"
fi
info "CI passed; waiting for auto-merge"

deadline=$(( $(date +%s) + 600 ))
while :; do
  state="$(gh pr view "$PR" --json state --jq .state)"
  case "$state" in
    MERGED) break;;
    CLOSED) die "PR #$PR closed without merging";;
  esac
  [[ "$(date +%s)" -gt "$deadline" ]] && die "timed out waiting for merge"
  sleep 6
done

merge_sha="$(gh pr view "$PR" --json mergeCommit --jq '.mergeCommit.oid')"
info "PR #$PR merged ($merge_sha)"

info "waiting for Scorekeeper to record entry…"
score_deadline=$(( $(date +%s) + 300 ))
while [[ "$(date +%s)" -lt "$score_deadline" ]]; do
  git fetch origin main --quiet || true
  if git show origin/main:RESULTS.md 2>/dev/null | grep -q "Current record:"; then
    latest_row="$(git show origin/main:RESULTS.md | grep -E '^\| [0-9]' | tail -1)"
    if echo "$latest_row" | grep -q "${merge_sha:0:7}"; then
      echo; echo "Recorded: $latest_row"
      git show origin/main:RESULTS.md | grep 'Current record:' | head -1
      break
    fi
  fi
  sleep 8
done

info "Done. git checkout main && git pull"
