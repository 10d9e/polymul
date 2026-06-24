# AUTORESEARCH — rules for improving polymul

You are an automated research agent. Your job is to **minimize deterministic
complexity** of negacyclic polynomial multiplication at N=1024 by editing the
algorithm, while a frozen harness measures you. Read this whole file before
changing anything.

## The objective

Minimize **SCORE = WORK** (deterministic wasm fuel over the fixed fixture
corpus), as reported by:

```
bash scripts/evaluate.sh
```

Lower SCORE is better. The baseline is in `fixtures/baselines.tsv`.

Measure WORK directly:

```
bash scripts/measure-complexity.sh
```

WORK is deterministic wasm fuel — the count of executed operators while
running `bench_polymul` on fixed fixture pairs. Lower is faster. The metric
lives outside `src/algorithm/` and cannot be gamed.

## The one hard invariant (non-negotiable)

The algorithm must produce **exact** negacyclic products for every input:

```
poly_mul(plan, a, b) == reference_poly_mul(a, b)    for all a, b
```

Coefficient arithmetic is `u32` wrapping (TFHE torus semantics). No tolerance,
no rounding — bit-identical outputs.

This is enforced by `tests/correctness.rs` (synthetic inputs) and the harness
fixture corpus. A candidate that fails any comparison is **INVALID** and scores
nothing.

## What you MAY edit

**Only files under `src/algorithm/`.** That is the entire mutable surface:

- `src/algorithm/mod.rs` — entry point + `Plan`
- You may **add new files/modules** under `src/algorithm/`.

You may implement any algorithm: naive, Karatsuba, NTT, negacyclic FFT, SIMD
intrinsics (via `std::arch`), precomputed roots in `Plan`, etc.

### The three frozen signatures

Inside `src/algorithm/mod.rs`, these must remain **character-for-character**
intact (bodies are yours):

```rust
pub struct Plan;

pub fn plan_new() -> Plan;

pub fn poly_mul(plan: &mut Plan, a: &[u32; 1024], b: &[u32; 1024]) -> [u32; 1024];
```

## What you MUST NOT touch (frozen)

Everything else, including:

- `src/main.rs`, `src/lib.rs`
- `src/harness/**`  (reference oracle, fixtures, scoring)
- `tests/**`        (the correctness gate)
- `fixtures/**`
- `scripts/**`
- `metrics/**`
- `Cargo.toml`      (no new dependencies — std-only Rust in algorithm)
- `AUTORESEARCH.md`

The boundary is enforced by `scripts/guard.sh` (local) and `scripts/guard-pr.sh`
(CI): only `src/algorithm/` may change in a submission.

**CI score gate.** Pull requests to `main` run `.github/workflows/score-gate.yml`.
If `src/algorithm/` changed, the candidate must produce a **strictly lower**
SCORE than the current record in `fixtures/baselines.tsv`. Otherwise the check
fails and the change cannot land (enable branch protection on **Score gate**).
Do not commit `fixtures/baselines.tsv` — CI appends new records after a winning
merge.

## Anti-cheat rules

1. **No embedding fixture data.** Do not bake fixture bytes or hashes into the
   algorithm. No detecting specific pairs and special-casing them.
2. **No side channels.** `plan_new` and `poly_mul` may use **only** their
   arguments and internal state derived from `plan_new`. No reading files, no
   network, no clock, no environment.
3. **Determinism.** No unseeded RNG, no thread-timing-dependent behavior.
4. **Generality over held-out tests.** Correctness tests use data *not* in the
   scored corpus.

## Per-iteration workflow

1. Edit only `src/algorithm/`.
2. Run the gate + scorer:
   ```
   bash scripts/evaluate.sh
   ```
   A candidate is **accepted** only if: the boundary guard passes, the build
   succeeds, all correctness tests pass, and it prints a numeric `SCORE:`.
3. If the new SCORE is lower than your best, keep the change. Otherwise revert
   (`git checkout -- src/algorithm`).
4. Occasionally run `cargo test` (debug build) to catch overflow bugs. Use
   `wrapping_*` ops for coefficient arithmetic.

## Research leads (roughly by expected payoff)

- **Negacyclic NTT / FFT** — O(N log N); precompute twiddle factors in `Plan`.
  This is the standard TFHE approach and the largest asymptotic win.
- **Karatsuba / Toom-Cook** — intermediate step before full NTT.
- **SIMD** — vectorize butterfly operations or schoolbook inner loops.
- **Memory layout** — cache-friendly transposes, in-place butterflies.
- **Lazy reduction** — defer modular reduction in NTT stages where safe.

Good luck. Make the number smaller.
