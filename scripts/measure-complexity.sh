#!/usr/bin/env bash
# Deterministic, tamper-proof complexity metric (lower = less compute).
#
# Builds the wasm shim and wasmtime host meter — both OUTSIDE src/algorithm/,
# so a submission cannot alter the measurement — and prints WORK.
#
# FROZEN — not part of the editable algorithm surface.
set -euo pipefail
cd "$(dirname "$0")/.."

rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
( cd metrics && RUSTFLAGS="" cargo build --release --quiet -p polymul-wasm-meter --target wasm32-unknown-unknown )
( cd metrics && cargo build --release --quiet -p polymul-fuel-meter )

WASM=metrics/target/wasm32-unknown-unknown/release/polymul_wasm_meter.wasm
./metrics/target/release/polymul-fuel-meter "$WASM"
