# AGENTS.md — polymul autoresearch quick start

## Goal

Minimize **SCORE** (deterministic wasm WORK) for negacyclic `poly_mul` at N=1024.

## Editable

Only `src/algorithm/**`.

## Frozen contract

```rust
pub struct Plan;
pub fn plan_new() -> Plan;
pub fn poly_mul(plan: &mut Plan, a: &[u32; 1024], b: &[u32; 1024]) -> [u32; 1024];
```

## Evaluate

```bash
bash scripts/evaluate.sh
```

## Submit

```bash
bash scripts/submit.sh --model "<model>"
```

## Invariant

Exact match vs harness reference oracle on all inputs. `u32` wrapping arithmetic.

## Full rules

See [`AUTORESEARCH.md`](AUTORESEARCH.md).
