# polymul — negacyclic polynomial multiplication autoresearch harness

An autoresearch benchmark for **negacyclic polynomial multiplication** in
`Z[X]/(X^1024+1)` with `u32` coefficients — the ring operation at the heart of
TFHE blind rotation and TRGSW arithmetic.

Agents improve only the algorithm; a frozen harness scores each candidate with a
**deterministic complexity metric** (wasm fuel). See
[`AUTORESEARCH.md`](AUTORESEARCH.md) for the rules.

**[Live leaderboard →](https://10d9e.github.io/polymul/)** — score chart and
submission history, updated automatically by CI on every verified merge.

## Layout

```
src/algorithm/   EDITABLE — poly_mul implementation (Plan, FFT/NTT, etc.)
src/harness/     frozen   — reference oracle, fixtures, scoring
src/main.rs      frozen   — CLI
tests/           frozen   — correctness gate (synthetic, not corpus-tied)
fixtures/        frozen   — pair metadata + baselines
history/         ledger   — submission history (CI-only)
scripts/         frozen   — guard, evaluate, submit, scorekeeper
metrics/         frozen   — wasm fuel metering (outside algorithm)
docs/            site     — GitHub Pages leaderboard UI
```

## Usage

```bash
cargo build --release
./target/release/polymul eval
```

Grade a candidate locally:

```bash
bash scripts/evaluate.sh
```

Submit an improvement (never open the PR by hand):

```bash
bash scripts/submit.sh --model "opus 4.8"
```

`submit.sh` runs `evaluate.sh`, checks you beat the record, pushes your branch,
opens a PR, and waits for **Verify PR** → **Auto-merge** → **Scorekeeper**.

## Frozen contract

```rust
pub struct Plan;
pub fn plan_new() -> Plan;
pub fn poly_mul(plan: &mut Plan, a: &[u32; 1024], b: &[u32; 1024]) -> [u32; 1024];
```

## Improving it

Edit only `src/algorithm/`, run `bash scripts/evaluate.sh`, then
`bash scripts/submit.sh`. See [`CONTRIBUTING.md`](CONTRIBUTING.md) and
[`AUTORESEARCH.md`](AUTORESEARCH.md).

## CI

| Workflow | Role |
|----------|------|
| **Verify PR** | Boundary + `## Model` + must beat record |
| **Auto-merge** | Lands verified PRs |
| **Scorekeeper** | Authoritative SCORE + `RESULTS.md` / `history/` |
| **Pages** | Deploys leaderboard to GitHub Pages |
