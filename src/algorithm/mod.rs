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
// back to the exact product mod 2^32. |signed c_k| < 2^74 and P0*P1*P2 ~= 2^89.7,
// so the integer is recovered exactly, then reduced mod 2^32.
//
// DIVISION-FREE: every modular multiply by a precomputed constant (twiddles, the
// psi pre/post weights, and the Garner mixing constants) uses Shoup's method —
// one extra multiply + shift to get the quotient, then a single conditional
// subtraction — instead of a hardware `%`. Under a cost model where integer
// division is ~25x an add (as on real hardware), this is far cheaper than the
// `%`-reduction the NTT is usually written with. Sums are kept reduced in [0,p)
// with a conditional subtract rather than deferred, since a conditional subtract
// is ~2 ops versus a division's 25. Only the spectral pointwise product (both
// operands variable) and the per-input coefficient reduction use `%`.
//
// Forward transform is decimation-in-frequency (natural -> bit-reversed) and the
// inverse is decimation-in-time (bit-reversed -> natural), so no explicit
// bit-reversal permutation is ever needed. All tables are precomputed in
// `plan_new`, which is free under the harness' differential metric.

const N: usize = 1024;

// Three primes p with 2048 | (p-1) and primitive root 3; each p < 2^30 so that
// products of residues fit in u64.
const P0: u64 = 998244353;
const P1: u64 = 1004535809;
const P2: u64 = 985661441;
const GEN: u64 = 3; // primitive root for all three primes

/// Per-prime NTT tables. Each multiplier `c` is stored alongside its Shoup
/// constant `c' = floor(c * 2^32 / p)` so that `c * x mod p` needs no division.
struct PrimeTables {
    p: u64,
    psi: [u32; N],   // psi^j               (negacyclic pre-weight)
    psis: [u32; N],  //   "  Shoup constant
    ipsi: [u32; N],  // psi^{-j} * N^{-1}    (post-weight, folds inverse scale)
    ipsis: [u32; N], //   "  Shoup constant
    w: [u32; N],     // w^e   forward twiddle powers (w = psi^2, a primitive N-th root)
    ws: [u32; N],    //   "  Shoup constant
    iw: [u32; N],    // w^{-e} inverse twiddle powers
    iws: [u32; N],   //   "  Shoup constant
    pinv: u32,       // -p^{-1} mod 2^32, for the Montgomery pointwise product
}

/// Opaque plan holding precomputed tables (built once in `plan_new`, free).
pub struct Plan {
    t: [PrimeTables; 3],
    // Garner CRT mixing constants (+ their Shoup forms for division-free use).
    inv_p0_mod_p1: u32,
    inv_p0_mod_p1_s: u32,
    p0_mod_p2: u32,
    p0_mod_p2_s: u32,
    inv_m01_mod_p2: u32,
    inv_m01_mod_p2_s: u32,
    p2_half: u64,
    // low-32-bit constants for mod-2^32 reconstruction.
    p0_lo: u32,
    m01_lo: u32,
    p_lo: u32,
}

#[inline(always)]
fn modpow(b: u64, mut e: u64, m: u64) -> u64 {
    let mm = m as u128;
    let mut r = 1u128;
    let mut bb = b as u128 % mm;
    while e > 0 {
        if e & 1 == 1 {
            r = r * bb % mm;
        }
        bb = bb * bb % mm;
        e >>= 1;
    }
    r as u64
}

#[inline(always)]
fn modinv(a: u64, m: u64) -> u64 {
    modpow(a, m - 2, m)
}

/// Shoup constant for multiplying by `c` modulo `p` (p < 2^31): floor(c<<32 / p).
#[inline(always)]
fn shoup_const(c: u64, p: u64) -> u32 {
    (((c as u128) << 32) / p as u128) as u32
}

/// Division-free `x * c mod p`, where `cs = floor(c<<32 / p)` and `0 <= x,c < p < 2^31`.
#[inline(always)]
fn shoup(x: u64, c: u32, cs: u32, p: u64) -> u64 {
    let q = x.wrapping_mul(cs as u64) >> 32;
    let r = x.wrapping_mul(c as u64).wrapping_sub(q.wrapping_mul(p));
    if r >= p {
        r - p
    } else {
        r
    }
}

/// Lazy Shoup: `x * c (mod p)` left in [0, 2p) — no final conditional subtract.
/// Valid (result in [0,2p)) for ANY x < 2^32, because with the 2^32 Shoup constant
/// the quotient error is at most 1 (Harvey's trick). Since the butterflies keep
/// values in [0, 4p) < 2^32, inputs are always in range.
#[inline(always)]
fn shoup_lazy(x: u64, c: u32, cs: u32, p: u64) -> u64 {
    let q = x.wrapping_mul(cs as u64) >> 32;
    x.wrapping_mul(c as u64).wrapping_sub(q.wrapping_mul(p))
}

/// Reduce a value in [0, 4p) into [0, 2p).
#[inline(always)]
fn red2p(a: u64, p: u64) -> u64 {
    let t = p << 1;
    if a >= t {
        a - t
    } else {
        a
    }
}

/// Montgomery product (R = 2^32): returns a*b*R^{-1} mod p in [0,2p), division-free
/// (the final conditional subtraction is omitted — the consumer reduces lazily).
/// `pinv = -p^{-1} mod 2^32`. Inputs a,b < 2p < 2^32, so a*b < 4p^2 < p*R.
#[inline(always)]
fn mont_mul(a: u64, b: u64, p: u64, pinv: u32) -> u64 {
    let t = a * b;
    let m = (t as u32).wrapping_mul(pinv) as u64; // (t mod R) * (-p^{-1}) mod R
    (t + m * p) >> 32 // exact /R, result in [0,2p)
}

fn build_tables(p: u64) -> PrimeTables {
    let psi_root = modpow(GEN, (p - 1) / (2 * N as u64), p);
    let w_root = (psi_root as u128 * psi_root as u128 % p as u128) as u64;
    let psi_inv = modinv(psi_root, p);
    let w_inv = modinv(w_root, p);
    let ninv = modinv(N as u64, p);
    // Montgomery factor R = 2^32: the pre-weight bakes in R (so the spectral
    // domain is in Montgomery form) and the post-weight bakes in R^{-1}.
    let r_mod = ((1u128 << 32) % p as u128) as u64;
    let rinv = modinv(r_mod, p);
    // -p^{-1} mod 2^32 via Newton's iteration (1 -> 2 -> ... -> 32 correct bits).
    let mut inv = 1u32;
    for _ in 0..5 {
        inv = inv.wrapping_mul(2u32.wrapping_sub((p as u32).wrapping_mul(inv)));
    }

    let mut pt = PrimeTables {
        p,
        psi: [0; N],
        psis: [0; N],
        ipsi: [0; N],
        ipsis: [0; N],
        w: [0; N],
        ws: [0; N],
        iw: [0; N],
        iws: [0; N],
        pinv: inv.wrapping_neg(),
    };

    let mut acc = 1u64; // psi^j
    let mut iacc = 1u64; // psi^{-j}
    for j in 0..N {
        let pm = (acc as u128 * r_mod as u128 % p as u128) as u64; // psi^j * R  (Montgomery)
        pt.psi[j] = pm as u32;
        pt.psis[j] = shoup_const(pm, p);
        let ip = (iacc as u128 * ninv as u128 % p as u128) as u64; // psi^{-j} * N^{-1}
        let ipm = (ip as u128 * rinv as u128 % p as u128) as u64; //   * R^{-1} (de-Montgomery)
        pt.ipsi[j] = ipm as u32;
        pt.ipsis[j] = shoup_const(ipm, p);
        acc = (acc as u128 * psi_root as u128 % p as u128) as u64;
        iacc = (iacc as u128 * psi_inv as u128 % p as u128) as u64;
    }

    let mut wacc = 1u64; // w^e
    let mut iwacc = 1u64; // w^{-e}
    for e in 0..N {
        pt.w[e] = wacc as u32;
        pt.ws[e] = shoup_const(wacc, p);
        pt.iw[e] = iwacc as u32;
        pt.iws[e] = shoup_const(iwacc, p);
        wacc = (wacc as u128 * w_root as u128 % p as u128) as u64;
        iwacc = (iwacc as u128 * w_inv as u128 % p as u128) as u64;
    }
    pt
}

/// One forward radix-4 DIF butterfly on array `x` at base `i+j` (stride `len`),
/// given the three stage twiddles already loaded. Lazy: values in [0,2p),
/// intermediates in [0,4p). `e == 0` means trivial (unit) twiddles.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
unsafe fn r4_bfly(
    x: &mut [u64; N], i: usize, j: usize, len: usize, p: u64, p2: u64, ic: u32, ics: u32, e: usize,
    t1c: u32, t1s: u32, t2c: u32, t2s: u32, t3c: u32, t3s: u32,
) {
    let a = *x.get_unchecked(i + j);
    let b = *x.get_unchecked(i + j + len);
    let c = *x.get_unchecked(i + j + 2 * len);
    let d = *x.get_unchecked(i + j + 3 * len);
    let s0 = red2p(a + c, p);
    let s2 = red2p(b + d, p);
    let s1 = red2p(a + p2 - c, p);
    let s3 = b + p2 - d; // in [0,4p); feeds only the lazy Shoup, which tolerates < 2^32
    let is3 = shoup_lazy(s3, ic, ics, p); // I * (b - d), in [0,2p)
    let y0 = red2p(s0 + s2, p);
    let y2 = s0 + p2 - s2; // [0,4p)
    let y1 = s1 + is3; // [0,4p)
    let y3 = s1 + p2 - is3; // [0,4p)
    *x.get_unchecked_mut(i + j) = y0;
    if e == 0 {
        *x.get_unchecked_mut(i + j + len) = red2p(y1, p);
        *x.get_unchecked_mut(i + j + 2 * len) = red2p(y2, p);
        *x.get_unchecked_mut(i + j + 3 * len) = red2p(y3, p);
    } else {
        *x.get_unchecked_mut(i + j + len) = shoup_lazy(y1, t1c, t1s, p);
        *x.get_unchecked_mut(i + j + 2 * len) = shoup_lazy(y2, t2c, t2s, p);
        *x.get_unchecked_mut(i + j + 3 * len) = shoup_lazy(y3, t3c, t3s, p);
    }
}

/// One radix-4 DIF butterfly on four values (lazy: in [0,2p), out [0,2p)).
/// `triv` skips the (unit) stage twiddles.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn r4_lazy(
    a: u64, b: u64, c: u64, d: u64, p: u64, p2: u64, ic: u32, ics: u32, triv: bool,
    t1c: u32, t1s: u32, t2c: u32, t2s: u32, t3c: u32, t3s: u32,
) -> (u64, u64, u64, u64) {
    let s0 = red2p(a + c, p);
    let s2 = red2p(b + d, p);
    let s1 = red2p(a + p2 - c, p);
    let s3 = b + p2 - d; // in [0,4p); feeds only the lazy Shoup, which tolerates < 2^32
    let is3 = shoup_lazy(s3, ic, ics, p);
    let y0 = red2p(s0 + s2, p);
    let y2 = s0 + p2 - s2;
    let y1 = s1 + is3;
    let y3 = s1 + p2 - is3;
    if triv {
        (y0, red2p(y1, p), red2p(y2, p), red2p(y3, p))
    } else {
        (
            y0,
            shoup_lazy(y1, t1c, t1s, p),
            shoup_lazy(y2, t2c, t2s, p),
            shoup_lazy(y3, t3c, t3s, p),
        )
    }
}

/// Forward radix-4 DIF of both operands in lockstep, through the middle stages only
/// (the psi pre-weight is folded into the first stage's load). The last two stages are
/// completed in the fused `boundary` pass.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn dif4_2(
    a: &[u32; N], b: &[u32; N], xa: &mut [u64; N], xb: &mut [u64; N], psi: &[u32; N], psis: &[u32; N],
    w: &[u32; N], ws: &[u32; N], ic: u32, ics: u32, p: u64,
) {
    let p2 = p << 1;
    // First stage (half-block N/4) with the psi pre-weight folded into the load.
    let len0 = N / 4;
    let mut j = 0;
    while j < len0 {
        unsafe {
            let e = j; // step = 1
            let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3(w, ws, e);
            let pw = |src: &[u32; N], idx: usize| {
                shoup_lazy(*src.get_unchecked(idx) as u64, *psi.get_unchecked(idx), *psis.get_unchecked(idx), p)
            };
            let (y0, y1, y2, y3) = r4_lazy(
                pw(a, j), pw(a, j + len0), pw(a, j + 2 * len0), pw(a, j + 3 * len0), p, p2, ic, ics,
                e == 0, t1c, t1s, t2c, t2s, t3c, t3s,
            );
            *xa.get_unchecked_mut(j) = y0;
            *xa.get_unchecked_mut(j + len0) = y1;
            *xa.get_unchecked_mut(j + 2 * len0) = y2;
            *xa.get_unchecked_mut(j + 3 * len0) = y3;
            let (z0, z1, z2, z3) = r4_lazy(
                pw(b, j), pw(b, j + len0), pw(b, j + 2 * len0), pw(b, j + 3 * len0), p, p2, ic, ics,
                e == 0, t1c, t1s, t2c, t2s, t3c, t3s,
            );
            *xb.get_unchecked_mut(j) = z0;
            *xb.get_unchecked_mut(j + len0) = z1;
            *xb.get_unchecked_mut(j + 2 * len0) = z2;
            *xb.get_unchecked_mut(j + 3 * len0) = z3;
        }
        j += 1;
    }
    // Remaining strided stages (half-blocks 64, 16) on xa, xb.
    let mut len = N / 16;
    while len >= 16 {
        let step = N / (4 * len);
        let mut i = 0;
        while i < N {
            let mut e = 0usize;
            let mut jj = 0;
            while jj < len {
                unsafe {
                    let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3(w, ws, e);
                    r4_bfly(xa, i, jj, len, p, p2, ic, ics, e, t1c, t1s, t2c, t2s, t3c, t3s);
                    r4_bfly(xb, i, jj, len, p, p2, ic, ics, e, t1c, t1s, t2c, t2s, t3c, t3s);
                }
                e += step;
                jj += 1;
            }
            i += 4 * len;
        }
        len >>= 2;
    }
    // The last two DIF stages are completed in the fused `boundary` pass.
}

/// Load the three stage twiddles (w^e, w^{2e}, w^{3e}) and their Shoup constants.
#[inline(always)]
unsafe fn twiddles3(w: &[u32; N], ws: &[u32; N], e: usize) -> (u32, u32, u32, u32, u32, u32) {
    if e == 0 {
        (0, 0, 0, 0, 0, 0)
    } else {
        (
            *w.get_unchecked(e),
            *ws.get_unchecked(e),
            *w.get_unchecked(2 * e),
            *ws.get_unchecked(2 * e),
            *w.get_unchecked(3 * e),
            *ws.get_unchecked(3 * e),
        )
    }
}

/// One inverse radix-4 DIT butterfly, Harvey-lazy: inputs and outputs in [0,4p).
/// The twiddled inputs go through `shoup_lazy` (which tolerates < 2^32) so only the
/// untwiddled input `a` (and, for a trivial butterfly, b,c,d) needs reducing; the
/// four outputs stay unreduced in [0,4p). Outputs are for offsets +0,+len,+2len,+3len.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn r4_lazy_dit(
    a: u64, mut b: u64, mut c: u64, mut d: u64, p: u64, p2: u64, jc: u32, jcs: u32, triv: bool,
    t1c: u32, t1s: u32, t2c: u32, t2s: u32, t3c: u32, t3s: u32,
) -> (u64, u64, u64, u64) {
    let a = red2p(a, p); // [0,4p) -> [0,2p)
    if triv {
        b = red2p(b, p);
        c = red2p(c, p);
        d = red2p(d, p);
    } else {
        b = shoup_lazy(b, t1c, t1s, p); // [0,4p) input -> [0,2p)
        c = shoup_lazy(c, t2c, t2s, p);
        d = shoup_lazy(d, t3c, t3s, p);
    }
    let s0 = red2p(a + c, p);
    let s1 = red2p(a + p2 - c, p);
    let s2 = red2p(b + d, p);
    let s3 = b + p2 - d; // in [0,4p); feeds only the lazy Shoup
    let js3 = shoup_lazy(s3, jc, jcs, p);
    (s0 + s2, s1 + js3, s0 + p2 - s2, s1 + p2 - js3) // each in [0,4p), unreduced
}

/// Remaining inverse DIT stages (the first two are done by `boundary`): the middle
/// strided stages (half-blocks 16, 64) then the final stage (half-block 256) with the
/// psi^{-1}*N^{-1} post-weight folded into the output store. Values in [0,4p).
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn dit4_rest(
    x: &mut [u64; N], iw: &[u32; N], iws: &[u32; N], jc: u32, jcs: u32,
    ipsi: &[u32; N], ipsis: &[u32; N], p: u64,
) {
    let mut len = 16;
    while len < N / 4 {
        let step = N / (4 * len);
        let p2 = p << 1;
        let mut i = 0;
        while i < N {
            let mut e = 0usize;
            let mut j = 0;
            while j < len {
                unsafe {
                    let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3(iw, iws, e);
                    let (o0, o1, o2, o3) = r4_lazy_dit(
                        *x.get_unchecked(i + j),
                        *x.get_unchecked(i + j + len),
                        *x.get_unchecked(i + j + 2 * len),
                        *x.get_unchecked(i + j + 3 * len),
                        p, p2, jc, jcs, e == 0, t1c, t1s, t2c, t2s, t3c, t3s,
                    );
                    *x.get_unchecked_mut(i + j) = o0;
                    *x.get_unchecked_mut(i + j + len) = o1;
                    *x.get_unchecked_mut(i + j + 2 * len) = o2;
                    *x.get_unchecked_mut(i + j + 3 * len) = o3;
                }
                e += step;
                j += 1;
            }
            i += 4 * len;
        }
        len <<= 2;
    }
    // Final stage (half-block 256) with the psi^{-1}*N^{-1} post-weight folded into
    // the output store, so there is no standalone post-weight pass.
    let p2 = p << 1;
    let mut j = 0usize;
    while j < N / 4 {
        unsafe {
            let e = j; // step = 1 at the last stage
            let (it1c, it1s, it2c, it2s, it3c, it3s) = twiddles3(iw, iws, e);
            let (o0, o1, o2, o3) = r4_lazy_dit(
                *x.get_unchecked(j),
                *x.get_unchecked(j + N / 4),
                *x.get_unchecked(j + N / 2),
                *x.get_unchecked(j + 3 * N / 4),
                p, p2, jc, jcs, e == 0, it1c, it1s, it2c, it2s, it3c, it3s,
            );
            let pw = |o: u64, pos: usize| shoup(o, *ipsi.get_unchecked(pos), *ipsis.get_unchecked(pos), p);
            *x.get_unchecked_mut(j) = pw(o0, j);
            *x.get_unchecked_mut(j + N / 4) = pw(o1, j + N / 4);
            *x.get_unchecked_mut(j + N / 2) = pw(o2, j + N / 2);
            *x.get_unchecked_mut(j + 3 * N / 4) = pw(o3, j + 3 * N / 4);
        }
        j += 1;
    }
}

/// Forward transform of both multiply operands together (shared twiddle loads;
/// psi pre-weight folded into the first DIF stage).
#[inline(always)]
fn fwd2(a: &[u32; N], b: &[u32; N], t: &PrimeTables) -> ([u64; N], [u64; N]) {
    let p = t.p;
    let mut xa = [0u64; N];
    let mut xb = [0u64; N];
    dif4_2(a, b, &mut xa, &mut xb, &t.psi, &t.psis, &t.w, &t.ws, t.w[N / 4], t.ws[N / 4], p);
    (xa, xb)
}

// ---- Contiguous 16-element tile sub-stages (used by the fused boundary pass) ----

#[inline(always)]
unsafe fn dif_l4(t: &mut [u64; 16], w: &[u32; N], ws: &[u32; N], ic: u32, ics: u32, p: u64, p2: u64) {
    for g in 0..4 {
        let e = 64 * g;
        let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3(w, ws, e);
        let (y0, y1, y2, y3) = r4_lazy(
            *t.get_unchecked(g), *t.get_unchecked(g + 4), *t.get_unchecked(g + 8),
            *t.get_unchecked(g + 12), p, p2, ic, ics, e == 0, t1c, t1s, t2c, t2s, t3c, t3s,
        );
        *t.get_unchecked_mut(g) = y0;
        *t.get_unchecked_mut(g + 4) = y1;
        *t.get_unchecked_mut(g + 8) = y2;
        *t.get_unchecked_mut(g + 12) = y3;
    }
}

#[inline(always)]
unsafe fn dif_l1(t: &mut [u64; 16], ic: u32, ics: u32, p: u64, p2: u64) {
    for h in 0..4 {
        let b4 = 4 * h;
        let (y0, y1, y2, y3) = r4_lazy(
            *t.get_unchecked(b4), *t.get_unchecked(b4 + 1), *t.get_unchecked(b4 + 2),
            *t.get_unchecked(b4 + 3), p, p2, ic, ics, true, 0, 0, 0, 0, 0, 0,
        );
        *t.get_unchecked_mut(b4) = y0;
        *t.get_unchecked_mut(b4 + 1) = y1;
        *t.get_unchecked_mut(b4 + 2) = y2;
        *t.get_unchecked_mut(b4 + 3) = y3;
    }
}

/// First inverse sub-stage (trivial twiddles) specialized for inputs already in
/// [0,2p) (the Montgomery pointwise output), so the per-input reductions are skipped.
#[inline(always)]
unsafe fn dit_l1_in2p(t: &mut [u64; 16], jc: u32, jcs: u32, p: u64, p2: u64) {
    for h in 0..4 {
        let b4 = 4 * h;
        let a = *t.get_unchecked(b4);
        let b = *t.get_unchecked(b4 + 1);
        let c = *t.get_unchecked(b4 + 2);
        let d = *t.get_unchecked(b4 + 3);
        let s0 = red2p(a + c, p);
        let s1 = red2p(a + p2 - c, p);
        let s2 = red2p(b + d, p);
        let s3 = b + p2 - d;
        let js3 = shoup_lazy(s3, jc, jcs, p);
        *t.get_unchecked_mut(b4) = s0 + s2;
        *t.get_unchecked_mut(b4 + 1) = s1 + js3;
        *t.get_unchecked_mut(b4 + 2) = s0 + p2 - s2;
        *t.get_unchecked_mut(b4 + 3) = s1 + p2 - js3;
    }
}

#[inline(always)]
unsafe fn dit_l4(t: &mut [u64; 16], iw: &[u32; N], iws: &[u32; N], jc: u32, jcs: u32, p: u64, p2: u64) {
    for g in 0..4 {
        let e = 64 * g;
        let (it1c, it1s, it2c, it2s, it3c, it3s) = twiddles3(iw, iws, e);
        let (o0, o1, o2, o3) = r4_lazy_dit(
            *t.get_unchecked(g), *t.get_unchecked(g + 4), *t.get_unchecked(g + 8),
            *t.get_unchecked(g + 12), p, p2, jc, jcs, e == 0, it1c, it1s, it2c, it2s, it3c, it3s,
        );
        *t.get_unchecked_mut(g) = o0;
        *t.get_unchecked_mut(g + 4) = o1;
        *t.get_unchecked_mut(g + 8) = o2;
        *t.get_unchecked_mut(g + 12) = o3;
    }
}

/// Fused transform boundary on contiguous 16-element blocks: `xa`,`xb` hold the two
/// operands after the middle forward stages; per block this completes the forward
/// (last two DIF stages) for each, does the Montgomery pointwise product, and starts
/// the inverse (first two DIT stages) — entirely in registers, so the full forward
/// spectra never touch memory.
#[inline(always)]
fn boundary(xa: &[u64; N], xb: &[u64; N], out: &mut [u64; N], t: &PrimeTables) {
    let p = t.p;
    let p2 = p << 1;
    let (icf, icfs) = (t.w[N / 4], t.ws[N / 4]); // forward 4th root I
    let (jc, jcs) = (t.iw[N / 4], t.iws[N / 4]); // inverse 4th root J
    let pinv = t.pinv;
    let mut i = 0;
    while i < N {
        let mut ta = [0u64; 16];
        let mut tb = [0u64; 16];
        unsafe {
            for k in 0..16 {
                *ta.get_unchecked_mut(k) = *xa.get_unchecked(i + k);
                *tb.get_unchecked_mut(k) = *xb.get_unchecked(i + k);
            }
            dif_l4(&mut ta, &t.w, &t.ws, icf, icfs, p, p2);
            dif_l1(&mut ta, icf, icfs, p, p2);
            dif_l4(&mut tb, &t.w, &t.ws, icf, icfs, p, p2);
            dif_l1(&mut tb, icf, icfs, p, p2);
            let mut tc = [0u64; 16];
            for k in 0..16 {
                *tc.get_unchecked_mut(k) = mont_mul(*ta.get_unchecked(k), *tb.get_unchecked(k), p, pinv);
            }
            dit_l1_in2p(&mut tc, jc, jcs, p, p2);
            dit_l4(&mut tc, &t.iw, &t.iws, jc, jcs, p, p2);
            for k in 0..16 {
                *out.get_unchecked_mut(i + k) = *tc.get_unchecked(k);
            }
        }
        i += 16;
    }
}

fn convolve_mod(t: &PrimeTables, a: &[u32; N], b: &[u32; N]) -> [u64; N] {
    let (xa, xb) = fwd2(a, b, t); // forward through the middle stages
    let mut x = [0u64; N];
    boundary(&xa, &xb, &mut x, t); // last fwd stages + pointwise + first inv stages, fused
    dit4_rest(&mut x, &t.iw, &t.iws, t.iw[N / 4], t.iws[N / 4], &t.ipsi, &t.ipsis, t.p); // rest of inverse
    x
}

/// Create a plan (precomputes NTT tables — free under the metric).
pub fn plan_new() -> Plan {
    let t = [build_tables(P0), build_tables(P1), build_tables(P2)];
    let m01 = P0 * P1; // < 2^60
    let inv01 = modinv(P0 % P1, P1);
    let p0m2 = P0 % P2;
    let invm01 = modinv(m01 % P2, P2);
    Plan {
        t,
        inv_p0_mod_p1: inv01 as u32,
        inv_p0_mod_p1_s: shoup_const(inv01, P1),
        p0_mod_p2: p0m2 as u32,
        p0_mod_p2_s: shoup_const(p0m2, P2),
        inv_m01_mod_p2: invm01 as u32,
        inv_m01_mod_p2_s: shoup_const(invm01, P2),
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
    let inv01_s = plan.inv_p0_mod_p1_s;
    let p0_mod_p2 = plan.p0_mod_p2;
    let p0_mod_p2_s = plan.p0_mod_p2_s;
    let inv_m01 = plan.inv_m01_mod_p2;
    let inv_m01_s = plan.inv_m01_mod_p2_s;
    let p2_half = plan.p2_half;
    let p0_lo = plan.p0_lo;
    let m01_lo = plan.m01_lo;
    let p_lo = plan.p_lo;

    let mut res = [0u32; N];
    unsafe {
        for j in 0..N {
            let v0 = *r0.get_unchecked(j); // < P0 < P1
            // v1 = (r1 - v0) * inv(P0) mod P1. Shoup tolerates any input < 2^32, so
            // t1 (< 2*P1) and v1 (< P1, used mod P2) need no pre-reduction.
            let t1 = *r1.get_unchecked(j) + p1 - v0; // [0, 2*P1)
            let v1 = shoup(t1, inv01, inv01_s, p1);
            // w = (v0 + P0*v1) mod P2 kept lazy in [0, 3*P2); v2 = (r2 - w) * inv(P0*P1).
            let term = shoup(v1, p0_mod_p2, p0_mod_p2_s, p2); // (P0 mod P2)*v1 mod P2
            let w = v0 + term; // < P0 + P2 < 3*P2
            let t2 = *r2.get_unchecked(j) + 3 * p2 - w; // (0, 4*P2)
            let v2 = shoup(t2, inv_m01, inv_m01_s, p2);

            // u = v0 + P0*v1 + P0*P1*v2 ; need u mod 2^32 and sign of (u - P/2).
            let lo = (v0 as u32)
                .wrapping_add(p0_lo.wrapping_mul(v1 as u32))
                .wrapping_add(m01_lo.wrapping_mul(v2 as u32));
            *res.get_unchecked_mut(j) = if v2 >= p2_half { lo.wrapping_sub(p_lo) } else { lo };
        }
    }
    res
}
