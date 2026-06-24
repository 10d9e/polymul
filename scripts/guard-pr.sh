#!/usr/bin/env bash
# PR boundary guard: algorithm submissions may change ONLY src/algorithm/.
# Infra PRs may change ONLY .github/ or scripts/ (not combined with algorithm).
# FROZEN — do not edit as part of autoresearch.
set -euo pipefail
cd "$(dirname "$0")/.."

base="${1:-}"
if [[ -z "$base" ]]; then
  if [[ -n "${GITHUB_BASE_SHA:-}" ]]; then
    base="$GITHUB_BASE_SHA"
  elif git rev-parse origin/main >/dev/null 2>&1; then
    base="$(git merge-base HEAD origin/main)"
  else
    base="$(git rev-parse HEAD~1)"
  fi
fi

violations=()
has_algorithm=0
has_infra=0
while IFS= read -r f; do
  [[ -z "$f" ]] && continue
  case "$f" in
    src/algorithm/*) has_algorithm=1 ;;
    .github/*|scripts/*) has_infra=1 ;;
    *) violations+=("$f") ;;
  esac
done < <(git diff --name-only "$base"...HEAD)

if (( ${#violations[@]} )); then
  echo "PR BOUNDARY VIOLATION — algorithm submissions may only change src/algorithm/;"
  echo "infra PRs may only change .github/ or scripts/:"
  printf '  %s\n' "${violations[@]}"
  echo
  echo "Do not commit fixtures/baselines.tsv — CI records new records on merge."
  exit 1
fi

if (( has_algorithm && has_infra )); then
  echo "PR BOUNDARY VIOLATION — infra changes (.github/ or scripts/) may not be"
  echo "combined with a src/algorithm submission; submit them as separate PRs."
  exit 1
fi

if (( has_algorithm )); then
  if ! grep -q 'pub struct Plan;' src/algorithm/mod.rs \
    || ! grep -q 'pub fn plan_new() -> Plan;' src/algorithm/mod.rs \
    || ! grep -q 'pub fn poly_mul(plan: &mut Plan, a: &\[u32; 1024\], b: &\[u32; 1024\]) -> \[u32; 1024\]' src/algorithm/mod.rs; then
    echo "PR BOUNDARY VIOLATION — frozen Plan/plan_new/poly_mul signatures were changed."
    exit 1
  fi

  if grep -rqE '#\[\s*global_allocator\s*\]' src/algorithm/ 2>/dev/null; then
    echo "PR BOUNDARY VIOLATION — src/algorithm/ must not declare a #[global_allocator]"
    exit 1
  fi

  echo "PR boundary OK (only src/algorithm/ changed; contract intact)"
elif (( has_infra )); then
  echo "PR boundary OK (infra changes only)"
else
  echo "PR boundary OK (no algorithm or infra changes)"
fi
