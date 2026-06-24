//! WebAssembly measurement shim — lives OUTSIDE src/algorithm/, so a submission
//! cannot touch the measurement path. Runs bench_polymul on fixed fixture pairs;
//! the host fuel meter counts executed operators.

use polymul::algorithm::{plan_new, poly_mul};
use polymul::harness::fixtures::{self, NUM_PAIRS};

/// Run `n` poly_mul calls on the first `n` fixture pairs (capped at NUM_PAIRS).
/// Creates one warm Plan, then multiplies; returns XOR checksum of all outputs.
#[no_mangle]
pub extern "C" fn bench_polymul(n: u32) -> u32 {
    let n = (n as usize).min(NUM_PAIRS);
    let mut plan = plan_new();
    let mut acc = 0u32;

    for i in 0..n {
        let p = fixtures::pair(i);
        let out = poly_mul(&mut plan, &p.a, &p.b);
        acc ^= fixtures::checksum(&out);
    }

    acc
}
