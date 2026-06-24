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
    // Forward radix-4 (two fused DIF stages) twiddles, indexed [q + j] where q is
    // the quarter-block size. ta = w_{4q}^j, tb = ta*w_4, tc = ta^2.
    ta: [u64; N],
    tb: [u64; N],
    tc: [u64; N],
    // Inverse radix-4 (two fused DIT stages) twiddles, indexed [q + j].
    // ita = w_{4q}^{-j}, itb = w_{4q}^{-(j+q)}, itc = w_{2q}^{-j} = ita^2.
    ita: [u64; N],
    itb: [u64; N],
    itc: [u64; N],
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

    // Radix-4 twiddles: each fused pass combines two radix-2 stages, processing
    // quarter-blocks of size q for q in {N/4, N/16, ..., 1}.
    let i4 = modpow(w_root, (N / 4) as u64, p); // w_4 = primitive 4th root
    let i4i = modpow(wi_root, (N / 4) as u64, p); // w_4^{-1}
    let mut ta = [0u64; N];
    let mut tb = [0u64; N];
    let mut tc = [0u64; N];
    let mut ita = [0u64; N];
    let mut itb = [0u64; N];
    let mut itc = [0u64; N];
    let mut q = N / 4;
    loop {
        let wr = modpow(w_root, (N / (4 * q)) as u64, p); // w_{4q}
        let wir = modpow(wi_root, (N / (4 * q)) as u64, p); // w_{4q}^{-1}
        let mut a_j = 1u64; // w_{4q}^j
        let mut ai_j = 1u64; // w_{4q}^{-j}
        for j in 0..q {
            ta[q + j] = a_j;
            tb[q + j] = (a_j as u128 * i4 as u128 % p as u128) as u64;
            tc[q + j] = (a_j as u128 * a_j as u128 % p as u128) as u64;
            ita[q + j] = ai_j;
            itb[q + j] = (ai_j as u128 * i4i as u128 % p as u128) as u64;
            itc[q + j] = (ai_j as u128 * ai_j as u128 % p as u128) as u64;
            a_j = (a_j as u128 * wr as u128 % p as u128) as u64;
            ai_j = (ai_j as u128 * wir as u128 % p as u128) as u64;
        }
        if q == 1 {
            break;
        }
        q >>= 2;
    }

    PrimeTables { p, psi, ipsi, ta, tb, tc, ita, itb, itc }
}

/// One fused radix-4 DIF butterfly (two combined radix-2 DIF stages) on the four
/// values held at indices i0,i1,i2,i3 of `x`, using twiddles ta,tb,tc.
/// Equivalent to two consecutive radix-2 Gentleman–Sande stages, so the overall
/// permutation remains base-2 bit reversal.
#[inline(always)]
unsafe fn r4_dif(x: &mut [u64; N], i0: usize, i1: usize, i2: usize, i3: usize,
                 ta: u64, tb: u64, tc: u64, p: u64) {
    let x0 = *x.get_unchecked(i0);
    let x1 = *x.get_unchecked(i1);
    let x2 = *x.get_unchecked(i2);
    let x3 = *x.get_unchecked(i3);

    let s02 = { let s = x0 + x2; if s >= p { s - p } else { s } };
    let s13 = { let s = x1 + x3; if s >= p { s - p } else { s } };
    let d02 = { let d = x0 + p - x2; if d >= p { d - p } else { d } };
    let d13 = { let d = x1 + p - x3; if d >= p { d - p } else { d } };
    let m02 = (d02 * ta) % p;
    let m13 = (d13 * tb) % p;

    *x.get_unchecked_mut(i0) = { let s = s02 + s13; if s >= p { s - p } else { s } };
    *x.get_unchecked_mut(i1) = ({ let d = s02 + p - s13; if d >= p { d - p } else { d } } * tc) % p;
    *x.get_unchecked_mut(i2) = { let s = m02 + m13; if s >= p { s - p } else { s } };
    *x.get_unchecked_mut(i3) = ({ let d = m02 + p - m13; if d >= p { d - p } else { d } } * tc) % p;
}

/// First fused radix-4 DIF butterfly that also applies the negacyclic pre-weight
/// psi^i on the fly, reading the raw u32 input and writing the u64 transform
/// buffer. Folds the separate pre-weight pass into the first forward pass.
#[inline(always)]
unsafe fn r4_dif_pre(src: &[u32; N], psi: &[u64; N], dst: &mut [u64; N],
                     i0: usize, i1: usize, i2: usize, i3: usize,
                     ta: u64, tb: u64, tc: u64, p: u64) {
    let x0 = (*src.get_unchecked(i0) as u64 * *psi.get_unchecked(i0)) % p;
    let x1 = (*src.get_unchecked(i1) as u64 * *psi.get_unchecked(i1)) % p;
    let x2 = (*src.get_unchecked(i2) as u64 * *psi.get_unchecked(i2)) % p;
    let x3 = (*src.get_unchecked(i3) as u64 * *psi.get_unchecked(i3)) % p;

    let s02 = { let s = x0 + x2; if s >= p { s - p } else { s } };
    let s13 = { let s = x1 + x3; if s >= p { s - p } else { s } };
    let d02 = { let d = x0 + p - x2; if d >= p { d - p } else { d } };
    let d13 = { let d = x1 + p - x3; if d >= p { d - p } else { d } };
    let m02 = (d02 * ta) % p;
    let m13 = (d13 * tb) % p;

    *dst.get_unchecked_mut(i0) = { let s = s02 + s13; if s >= p { s - p } else { s } };
    *dst.get_unchecked_mut(i1) = ({ let d = s02 + p - s13; if d >= p { d - p } else { d } } * tc) % p;
    *dst.get_unchecked_mut(i2) = { let s = m02 + m13; if s >= p { s - p } else { s } };
    *dst.get_unchecked_mut(i3) = ({ let d = m02 + p - m13; if d >= p { d - p } else { d } } * tc) % p;
}

/// Forward NTT of both multiply operands in lockstep, using fused radix-4 passes
/// (5 passes instead of 10). The negacyclic pre-weight psi^i is folded into the
/// first pass, so the raw u32 inputs are consumed directly into the u64 buffers.
/// Natural-order input -> bit-reversed-order output.
#[inline(always)]
fn ntt_dif2(a: &[u32; N], b: &[u32; N], fa: &mut [u64; N], fb: &mut [u64; N], t: &PrimeTables) {
    let p = t.p;
    // First pass (q = N/4, single block) folds in the psi pre-weight.
    let q0 = N / 4;
    for j in 0..q0 {
        unsafe {
            let ta = *t.ta.get_unchecked(q0 + j);
            let tb = *t.tb.get_unchecked(q0 + j);
            let tc = *t.tc.get_unchecked(q0 + j);
            let i1 = j + q0;
            let i2 = i1 + q0;
            let i3 = i2 + q0;
            r4_dif_pre(a, &t.psi, fa, j, i1, i2, i3, ta, tb, tc, p);
            r4_dif_pre(b, &t.psi, fb, j, i1, i2, i3, ta, tb, tc, p);
        }
    }
    // Remaining passes operate in place on the u64 buffers.
    let mut q = q0 >> 2;
    loop {
        let mut start = 0usize;
        while start < N {
            for j in 0..q {
                unsafe {
                    let ta = *t.ta.get_unchecked(q + j);
                    let tb = *t.tb.get_unchecked(q + j);
                    let tc = *t.tc.get_unchecked(q + j);
                    let i0 = start + j;
                    let i1 = i0 + q;
                    let i2 = i1 + q;
                    let i3 = i2 + q;
                    r4_dif(fa, i0, i1, i2, i3, ta, tb, tc, p);
                    r4_dif(fb, i0, i1, i2, i3, ta, tb, tc, p);
                }
            }
            start += 4 * q;
        }
        if q == 1 {
            break;
        }
        q >>= 2;
    }
}

/// Inverse NTT using fused radix-4 DIT passes (5 passes instead of 10).
/// Bit-reversed-order input -> natural-order output. Inverse of `ntt_dif2`'s
/// per-array transform.
#[inline(always)]
fn intt_dit(a: &mut [u64; N], b: &[u64; N], t: &PrimeTables) {
    let p = t.p;
    // First pass (q = 1): fold in the pointwise product a[i] *= b[i].
    unsafe {
        let wa = *t.ita.get_unchecked(1);
        let wb = *t.itb.get_unchecked(1);
        let wc = *t.itc.get_unchecked(1);
        let mut start = 0usize;
        while start < N {
            let i0 = start;
            let i1 = start + 1;
            let i2 = start + 2;
            let i3 = start + 3;
            let x0 = (*a.get_unchecked(i0) * *b.get_unchecked(i0)) % p;
            let x1 = (*a.get_unchecked(i1) * *b.get_unchecked(i1)) % p;
            let x2 = (*a.get_unchecked(i2) * *b.get_unchecked(i2)) % p;
            let x3 = (*a.get_unchecked(i3) * *b.get_unchecked(i3)) % p;

            let v1 = (x1 * wc) % p;
            let v3 = (x3 * wc) % p;
            let p0 = { let s = x0 + v1; if s >= p { s - p } else { s } };
            let p1 = { let d = x0 + p - v1; if d >= p { d - p } else { d } };
            let p2 = { let s = x2 + v3; if s >= p { s - p } else { s } };
            let p3 = { let d = x2 + p - v3; if d >= p { d - p } else { d } };

            let va = (p2 * wa) % p;
            let vb = (p3 * wb) % p;
            *a.get_unchecked_mut(i0) = { let s = p0 + va; if s >= p { s - p } else { s } };
            *a.get_unchecked_mut(i2) = { let d = p0 + p - va; if d >= p { d - p } else { d } };
            *a.get_unchecked_mut(i1) = { let s = p1 + vb; if s >= p { s - p } else { s } };
            *a.get_unchecked_mut(i3) = { let d = p1 + p - vb; if d >= p { d - p } else { d } };
            start += 4;
        }
    }

    // Middle passes (q = 4, ..., N/16).
    let mut q = 4usize;
    while q < N / 4 {
        let mut start = 0usize;
        while start < N {
            for j in 0..q {
                unsafe {
                    let wa = *t.ita.get_unchecked(q + j);
                    let wb = *t.itb.get_unchecked(q + j);
                    let wc = *t.itc.get_unchecked(q + j);
                    let i0 = start + j;
                    let i1 = i0 + q;
                    let i2 = i1 + q;
                    let i3 = i2 + q;

                    let x0 = *a.get_unchecked(i0);
                    let x1 = *a.get_unchecked(i1);
                    let x2 = *a.get_unchecked(i2);
                    let x3 = *a.get_unchecked(i3);

                    let v1 = (x1 * wc) % p;
                    let v3 = (x3 * wc) % p;
                    let p0 = { let s = x0 + v1; if s >= p { s - p } else { s } };
                    let p1 = { let d = x0 + p - v1; if d >= p { d - p } else { d } };
                    let p2 = { let s = x2 + v3; if s >= p { s - p } else { s } };
                    let p3 = { let d = x2 + p - v3; if d >= p { d - p } else { d } };

                    let va = (p2 * wa) % p;
                    let vb = (p3 * wb) % p;
                    *a.get_unchecked_mut(i0) = { let s = p0 + va; if s >= p { s - p } else { s } };
                    *a.get_unchecked_mut(i2) = { let d = p0 + p - va; if d >= p { d - p } else { d } };
                    *a.get_unchecked_mut(i1) = { let s = p1 + vb; if s >= p { s - p } else { s } };
                    *a.get_unchecked_mut(i3) = { let d = p1 + p - vb; if d >= p { d - p } else { d } };
                }
            }
            start += 4 * q;
        }
        q <<= 2;
    }

    // Last pass (q = N/4, single block): fold the psi^{-j} * N^{-1} post-weight
    // into the four output stores.
    let q = N / 4;
    for j in 0..q {
        unsafe {
            let wa = *t.ita.get_unchecked(q + j);
            let wb = *t.itb.get_unchecked(q + j);
            let wc = *t.itc.get_unchecked(q + j);
            let i0 = j;
            let i1 = i0 + q;
            let i2 = i1 + q;
            let i3 = i2 + q;

            let x0 = *a.get_unchecked(i0);
            let x1 = *a.get_unchecked(i1);
            let x2 = *a.get_unchecked(i2);
            let x3 = *a.get_unchecked(i3);

            let v1 = (x1 * wc) % p;
            let v3 = (x3 * wc) % p;
            let p0 = { let s = x0 + v1; if s >= p { s - p } else { s } };
            let p1 = { let d = x0 + p - v1; if d >= p { d - p } else { d } };
            let p2 = { let s = x2 + v3; if s >= p { s - p } else { s } };
            let p3 = { let d = x2 + p - v3; if d >= p { d - p } else { d } };

            let va = (p2 * wa) % p;
            let vb = (p3 * wb) % p;
            let o0 = { let s = p0 + va; if s >= p { s - p } else { s } };
            let o2 = { let d = p0 + p - va; if d >= p { d - p } else { d } };
            let o1 = { let s = p1 + vb; if s >= p { s - p } else { s } };
            let o3 = { let d = p1 + p - vb; if d >= p { d - p } else { d } };
            *a.get_unchecked_mut(i0) = (o0 * *t.ipsi.get_unchecked(i0)) % p;
            *a.get_unchecked_mut(i2) = (o2 * *t.ipsi.get_unchecked(i2)) % p;
            *a.get_unchecked_mut(i1) = (o1 * *t.ipsi.get_unchecked(i1)) % p;
            *a.get_unchecked_mut(i3) = (o3 * *t.ipsi.get_unchecked(i3)) % p;
        }
    }
}

/// Negacyclic product mod p, returned as residues in [0, p).
#[inline(always)]
fn convolve_mod(t: &PrimeTables, a: &[u32; N], b: &[u32; N]) -> [u64; N] {
    let mut fa = [0u64; N];
    let mut fb = [0u64; N];
    ntt_dif2(a, b, &mut fa, &mut fb, t);
    intt_dit(&mut fa, &fb, t);
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
            // v1 = (r1 - v0) * inv(P0) mod P1.  v0 = r0[j] < P0 < P1, so v0 % P1 == v0.
            let t1 = (*r1.get_unchecked(j) + p1 - v0) % p1;
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
