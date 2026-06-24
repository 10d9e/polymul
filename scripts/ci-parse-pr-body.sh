#!/usr/bin/env bash
# Extract ## Model / ## Approach / ## Iteration notes from a PR body.
# FROZEN — do not edit as part of autoresearch.
set -euo pipefail

section="${1:-Approach}"
body="${2:-}"

if [[ -z "$body" ]]; then
  exit 0
fi

awk -v want="$section" '
  BEGIN { in_section=0 }
  /^## / {
    title=$0
    sub(/^##[[:space:]]+/, "", title)
    if (in_section) { exit }
    in_section = (tolower(title) == tolower(want))
    next
  }
  in_section { print }
' <<< "$body" | sed '/^[[:space:]]*$/d'
