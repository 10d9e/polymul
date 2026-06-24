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

// Negacyclic polynomial multiplication in Z[X]/(X^N+1), N=1024, coefficients
// reduced mod 2^32 (u32 wrapping torus semantics).
//
// Strategy: number-theoretic transform (NTT) over three NTT-friendly primes,
// each carrying a length-N negacyclic transform, then Garner CRT reconstruction
// back to the exact product mod 2^32.
//
// The true (signed) product coefficients satisfy |c_k| < 1024 * (2^32)^2 < 2^74,
// so the CRT modulus P0*P1*P2 ~= 2^89.7 > 2^75 recovers them exactly; we then
// reduce mod 2^32. Negacyclic wrap is handled by pre-weighting inputs with psi^j
// (psi a primitive 2N-th root) and post-weighting with psi^{-j}/N.
//
// Forward transform is decimation-in-frequency (natural -> bit-reversed) and the
// inverse is decimation-in-time (bit-reversed -> natural), so no explicit
// bit-reversal permutation is ever needed. All tables are precomputed in
// `plan_new`, which is free under the harness' differential fuel metric.

const N: usize = 1024;

// Three primes p with 2048 | (p-1) and primitive root 3; each p < 2^30 so that
// products of residues fit in u64.
const P0: u64 = 998244353;
const P1: u64 = 1004535809;
const P2: u64 = 985661441;
const GEN: u64 = 3; // primitive root for all three primes

/// Per-prime NTT tables.
struct PrimeTables {
    p: u64,
    psi: [u64; N],  // psi^j           (pre-weight)
    ipsi: [u64; N], // psi^{-j} * N^-1 (post-weight, folds inverse-transform scale)
    tw: [u64; N],   // forward (DIF) twiddles: tw[len + j] = w_N^{j * N/(2*len)}
    itw: [u64; N],  // inverse (DIT) twiddles: itw[len + j] = w_N^{-(j * N/(2*len))}
}

/// Opaque plan holding precomputed tables (built once in `plan_new`, free).
pub struct Plan {
    t: [PrimeTables; 3],
    // Garner CRT constants.
    inv_p0_mod_p1: u64,
    p0_mod_p2: u64,
    inv_m01_mod_p2: u64,
    p2_half: u64,
    // low-32-bit constants for mod-2^32 reconstruction.
    p0_lo: u32,
    m01_lo: u32,
    p_lo: u32,
}

#[inline(always)]
fn modpow(mut b: u64, mut e: u64, m: u64) -> u64 {
    let mut r = 1u64;
    b %= m;
    while e > 0 {
        if e & 1 == 1 {
            r = ((r as u128 * b as u128) % m as u128) as u64;
        }
        b = ((b as u128 * b as u128) % m as u128) as u64;
        e >>= 1;
    }
    r
}

#[inline(always)]
fn modinv(a: u64, m: u64) -> u64 {
    modpow(a, m - 2, m)
}

fn build_tables(p: u64) -> PrimeTables {
    // psi: primitive 2N-th root of unity; w = psi^2 is a primitive N-th root.
    let psi_root = modpow(GEN, (p - 1) / (2 * N as u64), p);
    let w_root = (psi_root as u128 * psi_root as u128 % p as u128) as u64;
    let wi_root = modinv(w_root, p);
    let ninv = modinv(N as u64, p);

    let mut psi = [0u64; N];
    let mut ipsi = [0u64; N];
    let psi_inv = modinv(psi_root, p);
    let mut acc = 1u64;
    let mut iacc = 1u64; // psi^{-j}
    for j in 0..N {
        psi[j] = acc;
        ipsi[j] = (iacc as u128 * ninv as u128 % p as u128) as u64;
        acc = (acc as u128 * psi_root as u128 % p as u128) as u64;
        iacc = (iacc as u128 * psi_inv as u128 % p as u128) as u64;
    }

    // Stage twiddles, indexed [len + j] where len is the butterfly half-size.
    let mut tw = [0u64; N];
    let mut itw = [0u64; N];
    let mut len = 1usize;
    while len < N {
        let step = (N / (2 * len)) as u64; // w_{2*len} = w_N^step
        let wr = modpow(w_root, step, p);
        let wir = modpow(wi_root, step, p);
        let mut wj = 1u64;
        let mut wij = 1u64;
        for j in 0..len {
            tw[len + j] = wj;
            itw[len + j] = wij;
            wj = (wj as u128 * wr as u128 % p as u128) as u64;
            wij = (wij as u128 * wir as u128 % p as u128) as u64;
        }
        len <<= 1;
    }

    PrimeTables { p, psi, ipsi, tw, itw }
}

/// Forward NTT, decimation-in-frequency (Gentleman–Sande).
/// Natural-order input -> bit-reversed-order output.
#[inline(always)]
fn ntt_dif(a: &mut [u64; N], t: &PrimeTables) {
    let p = t.p;
    let mut len = N / 2;
    while len >= 1 {
        let mut start = 0usize;
        while start < N {
            let base = len;
            for j in 0..len {
                unsafe {
                    let u = *a.get_unchecked(start + j);
                    let v = *a.get_unchecked(start + j + len);
                    let s = u + v;
                    *a.get_unchecked_mut(start + j) = if s >= p { s - p } else { s };
                    let d = u + p - v;
                    let d = if d >= p { d - p } else { d };
                    *a.get_unchecked_mut(start + j + len) = (d * *t.tw.get_unchecked(base + j)) % p;
                }
            }
            start += 2 * len;
        }
        len >>= 1;
    }
}

/// Inverse NTT, decimation-in-time (Cooley–Tukey).
/// Bit-reversed-order input -> natural-order output.
#[inline(always)]
fn intt_dit(a: &mut [u64; N], t: &PrimeTables) {
    let p = t.p;
    let mut len = 1usize;
    while len < N {
        let mut start = 0usize;
        while start < N {
            let base = len;
            for j in 0..len {
                unsafe {
                    let u = *a.get_unchecked(start + j);
                    let v = (*a.get_unchecked(start + j + len) * *t.itw.get_unchecked(base + j)) % p;
                    let s = u + v;
                    *a.get_unchecked_mut(start + j) = if s >= p { s - p } else { s };
                    let d = u + p - v;
                    *a.get_unchecked_mut(start + j + len) = if d >= p { d - p } else { d };
                }
            }
            start += 2 * len;
        }
        len <<= 1;
    }
}

/// Negacyclic product mod p, returned as residues in [0, p).
#[inline(always)]
fn convolve_mod(t: &PrimeTables, a: &[u32; N], b: &[u32; N]) -> [u64; N] {
    let p = t.p;
    let mut fa = [0u64; N];
    let mut fb = [0u64; N];
    unsafe {
        for j in 0..N {
            let psi = *t.psi.get_unchecked(j);
            *fa.get_unchecked_mut(j) = (*a.get_unchecked(j) as u64 * psi) % p;
            *fb.get_unchecked_mut(j) = (*b.get_unchecked(j) as u64 * psi) % p;
        }
    }
    ntt_dif(&mut fa, t);
    ntt_dif(&mut fb, t);
    unsafe {
        for j in 0..N {
            *fa.get_unchecked_mut(j) = (*fa.get_unchecked(j) * *fb.get_unchecked(j)) % p;
        }
    }
    intt_dit(&mut fa, t);
    unsafe {
        for j in 0..N {
            *fa.get_unchecked_mut(j) = (*fa.get_unchecked(j) * *t.ipsi.get_unchecked(j)) % p;
        }
    }
    fa
}

/// Create a plan (precomputes NTT tables — free under the fuel metric).
pub fn plan_new() -> Plan {
    let t = [build_tables(P0), build_tables(P1), build_tables(P2)];
    let m01 = P0 * P1; // < 2^60
    Plan {
        t,
        inv_p0_mod_p1: modinv(P0 % P1, P1),
        p0_mod_p2: P0 % P2,
        inv_m01_mod_p2: modinv(m01 % P2, P2),
        p2_half: P2 >> 1,
        p0_lo: P0 as u32,
        m01_lo: m01 as u32,
        p_lo: (P0 as u32).wrapping_mul(P1 as u32).wrapping_mul(P2 as u32),
    }
}

/// Negacyclic polynomial multiplication: a(X) * b(X) mod (X^1024+1).
pub fn poly_mul(plan: &mut Plan, a: &[u32; 1024], b: &[u32; 1024]) -> [u32; 1024] {
    let r0 = convolve_mod(&plan.t[0], a, b);
    let r1 = convolve_mod(&plan.t[1], a, b);
    let r2 = convolve_mod(&plan.t[2], a, b);

    let p1 = P1;
    let p2 = P2;
    let inv01 = plan.inv_p0_mod_p1;
    let p0_mod_p2 = plan.p0_mod_p2;
    let inv_m01 = plan.inv_m01_mod_p2;
    let p2_half = plan.p2_half;
    let p0_lo = plan.p0_lo;
    let m01_lo = plan.m01_lo;
    let p_lo = plan.p_lo;

    let mut res = [0u32; N];
    unsafe {
        for j in 0..N {
            let v0 = *r0.get_unchecked(j);
            // v1 = (r1 - v0) * inv(P0) mod P1
            let t1 = (*r1.get_unchecked(j) + p1 - v0 % p1) % p1;
            let v1 = (t1 * inv01) % p1;
            // w = (v0 + P0*v1) mod P2
            let w = (v0 % p2 + p0_mod_p2 * v1 % p2) % p2;
            let t2 = (*r2.get_unchecked(j) + p2 - w) % p2;
            let v2 = (t2 * inv_m01) % p2;

            // u = v0 + P0*v1 + P0*P1*v2 ; need u mod 2^32 and sign of (u - P/2).
            let lo = (v0 as u32)
                .wrapping_add(p0_lo.wrapping_mul(v1 as u32))
                .wrapping_add(m01_lo.wrapping_mul(v2 as u32));
            *res.get_unchecked_mut(j) = if v2 >= p2_half { lo.wrapping_sub(p_lo) } else { lo };
        }
    }
    res
}
