//! Algorithm entry point.
//!
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │ FROZEN CONTRACT — do NOT change these signatures:                   │
//! │     pub struct Plan;                                                │
//! │     pub fn plan_new() -> Plan;                                      │
//! │     pub fn poly_mul(plan: &mut Plan, a: &[u32; 1024],              │
//! │                     b: &[u32; 1024]) -> [u32; 1024];                │
//! │ The bodies, and everything in this directory, are yours to improve. │
//! │ Invariant: exact negacyclic product mod (X^1024+1) in u32.          │
//! └─────────────────────────────────────────────────────────────────────┘

/// Opaque plan for precomputed twiddle tables, scratch buffers, etc.
pub struct Plan;

/// Create a plan (may precompute FFT/NTT tables).
pub fn plan_new() -> Plan {
    Plan
}

/// Negacyclic polynomial multiplication: a(X) * b(X) mod (X^1024+1).
pub fn poly_mul(plan: &mut Plan, a: &[u32; 1024], b: &[u32; 1024]) -> [u32; 1024] {
  let _ = plan;
  const N: usize = 1024;
  let mut res = [0u32; N];

  for i in 0..N {
    for j in 0..N {
      let prod = a[i].wrapping_mul(b[j]);
      if i + j < N {
        res[i + j] = res[i + j].wrapping_add(prod);
      } else {
        res[i + j - N] = res[i + j - N].wrapping_sub(prod);
      }
    }
  }

  res
}
