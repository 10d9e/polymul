<!-- Prefer `bash scripts/submit.sh --model "<model>"` — it fills this template,
     runs the checks, opens the PR, and waits for CI to land it. -->

## Summary

<!-- One paragraph: what you changed and why. -->

## Model

<!-- REQUIRED: which AI model assisted (e.g. "opus 4.8", "codex 5.5"). -->

## Approach

<!-- REQUIRED: what you changed and why you expected lower SCORE. CI copies this into history/entries/. -->

## Iteration notes

<!-- Optional: what you tried and reverted. -->

## Checklist

- [ ] Only `src/algorithm/` changed — **no** `RESULTS.md`, `history/entries/`, or `fixtures/baselines.tsv`
- [ ] **`## Model`** filled in
- [ ] Local SCORE **beats** the current record (lower is better)
- [ ] **Verify PR** passes → auto-merges → **Scorekeeper** records the ledger

## Local score (informational)

<!-- Optional: paste evaluate output. CI score is authoritative. -->
