# Entry 0001 — SCORE 319558256 (baseline)

| Field | Value |
|-------|-------|
| Date | 2026-06-24 |
| Author | @10d9e |
| Model | — |
| Git author | autoresearch |
| Commit | `3793fd8` |
| SCORE | 319558256 |
| Δ vs previous record | — (initial baseline) |
| Status | record |

## Approach

Initial autoresearch harness with naive O(N²) schoolbook negacyclic polynomial
multiplication in `Z[X]/(X^1024+1)` with `u32` wrapping coefficients. `Plan` is a
placeholder; all work happens in `poly_mul`.

## Algorithm changes

```
(none — starting point)
```

## Eval snapshot

```
SCORE: 319558256 (deterministic wasm WORK; lower is better)
```
