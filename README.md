# polymul — negacyclic polynomial multiplication autoresearch harness

An autoresearch benchmark for **negacyclic polynomial multiplication** in
`Z[X]/(X^1024+1)` with `u32` coefficients — the ring operation at the heart of
TFHE blind rotation and TRGSW arithmetic.

Agents improve only the algorithm; a frozen harness scores each candidate with a
**deterministic complexity metric** (wasm fuel). See
[`AUTORESEARCH.md`](AUTORESEARCH.md) for the rules.

## Layout

```
src/algorithm/   EDITABLE — poly_mul implementation (Plan, FFT/NTT, etc.)
src/harness/     frozen   — reference oracle, fixtures, scoring
src/main.rs      frozen   — CLI
tests/           frozen   — correctness gate (synthetic, not corpus-tied)
fixtures/        frozen   — pair metadata + baselines
scripts/         frozen   — guard.sh, evaluate.sh, measure-complexity.sh
metrics/         frozen   — wasm fuel metering (outside algorithm)
```

## Usage

```bash
cargo build --release
./target/release/polymul eval
```

Grade a candidate locally (guard + tests + score):

```bash
bash scripts/evaluate.sh
```

Measure deterministic complexity only:

```bash
bash scripts/measure-complexity.sh
```

## Frozen contract

```rust
pub struct Plan;
pub fn plan_new() -> Plan;
pub fn poly_mul(plan: &mut Plan, a: &[u32; 1024], b: &[u32; 1024]) -> [u32; 1024];
```

`poly_mul` computes `a(X) * b(X) mod (X^1024+1)` with wrapping `u32` arithmetic.
`Plan` may hold precomputed twiddle factors; `plan_new()` amortizes setup across
calls — matching TFHE usage.

## Improving it

Edit only `src/algorithm/`, run `bash scripts/evaluate.sh`, keep changes that
lower **SCORE** (deterministic WORK) while all correctness tests pass. Details
in [`AUTORESEARCH.md`](AUTORESEARCH.md) and [`AGENTS.md`](AGENTS.md).
