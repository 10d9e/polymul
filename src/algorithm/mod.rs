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

/// Reduce a value in [0, 2p) into [0, p).
#[inline(always)]
fn red(a: u64, p: u64) -> u64 {
    if a >= p {
        a - p
    } else {
        a
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

/// Montgomery product (R = 2^32): returns a*b*R^{-1} mod p in [0,p), division-free.
/// `pinv = -p^{-1} mod 2^32`. Inputs a,b < 2p < 2^32, so a*b < 4p^2 < p*R.
#[inline(always)]
fn mont_mul(a: u64, b: u64, p: u64, pinv: u32) -> u64 {
    let t = a * b;
    let m = (t as u32).wrapping_mul(pinv) as u64; // (t mod R) * (-p^{-1}) mod R
    let r = (t + m * p) >> 32; // exact /R, result in [0,2p)
    if r >= p {
        r - p
    } else {
        r
    }
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
    let s3 = red2p(b + p2 - d, p);
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
    let s3 = red2p(b + p2 - d, p);
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

/// Fused last two forward DIF stages (half-block sizes 4 then 1) on one contiguous
/// 16-element tile held in registers — one memory pass instead of two. The first
/// fused stage uses 16th-root twiddles w^{64*g} (already in the `w` table); the
/// second is all trivial.
#[inline(always)]
fn dif4_last16(x: &mut [u64; N], w: &[u32; N], ws: &[u32; N], ic: u32, ics: u32, p: u64) {
    let p2 = p << 1;
    let mut i = 0;
    while i < N {
        let mut t = [0u64; 16];
        unsafe {
            for k in 0..16 {
                *t.get_unchecked_mut(k) = *x.get_unchecked(i + k);
            }
            // half-block size 4: groups (g, g+4, g+8, g+12), twiddle exponent e = 64*g.
            for g in 0..4 {
                let e = 64 * g;
                let (a, b, c, d) = (
                    *t.get_unchecked(g),
                    *t.get_unchecked(g + 4),
                    *t.get_unchecked(g + 8),
                    *t.get_unchecked(g + 12),
                );
                let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3(w, ws, e);
                let (y0, y1, y2, y3) =
                    r4_lazy(a, b, c, d, p, p2, ic, ics, e == 0, t1c, t1s, t2c, t2s, t3c, t3s);
                *t.get_unchecked_mut(g) = y0;
                *t.get_unchecked_mut(g + 4) = y1;
                *t.get_unchecked_mut(g + 8) = y2;
                *t.get_unchecked_mut(g + 12) = y3;
            }
            // half-block size 1: groups (4h, 4h+1, 4h+2, 4h+3), all trivial twiddles.
            for h in 0..4 {
                let b4 = 4 * h;
                let (y0, y1, y2, y3) = r4_lazy(
                    *t.get_unchecked(b4),
                    *t.get_unchecked(b4 + 1),
                    *t.get_unchecked(b4 + 2),
                    *t.get_unchecked(b4 + 3),
                    p,
                    p2,
                    ic,
                    ics,
                    true,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                );
                *t.get_unchecked_mut(b4) = y0;
                *t.get_unchecked_mut(b4 + 1) = y1;
                *t.get_unchecked_mut(b4 + 2) = y2;
                *t.get_unchecked_mut(b4 + 3) = y3;
            }
            for k in 0..16 {
                *x.get_unchecked_mut(i + k) = *t.get_unchecked(k);
            }
        }
        i += 16;
    }
}

/// Forward radix-4 DIF on the two multiply operands in lockstep, sharing the
/// stage twiddle loads and index arithmetic. The last two stages (which act on
/// contiguous 16-element blocks) are fused into a single register pass.
#[inline(always)]
fn dif4_2(xa: &mut [u64; N], xb: &mut [u64; N], w: &[u32; N], ws: &[u32; N], ic: u32, ics: u32, p: u64) {
    let mut len = N / 4;
    while len >= 16 {
        let step = N / (4 * len);
        let p2 = p << 1;
        let mut i = 0;
        while i < N {
            let mut e = 0usize;
            let mut j = 0;
            while j < len {
                unsafe {
                    let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3(w, ws, e);
                    r4_bfly(xa, i, j, len, p, p2, ic, ics, e, t1c, t1s, t2c, t2s, t3c, t3s);
                    r4_bfly(xb, i, j, len, p, p2, ic, ics, e, t1c, t1s, t2c, t2s, t3c, t3s);
                }
                e += step;
                j += 1;
            }
            i += 4 * len;
        }
        len >>= 2;
    }
    dif4_last16(xa, w, ws, ic, ics, p);
    dif4_last16(xb, w, ws, ic, ics, p);
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

/// One inverse radix-4 DIT butterfly on four values (lazy, in/out [0,2p)).
/// Applies the inverse twiddles to b,c,d then the inverse 4-point DFT (root J).
/// Returns outputs for offsets +0, +len, +2len, +3len. `triv` skips twiddles.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn r4_lazy_dit(
    a: u64, mut b: u64, mut c: u64, mut d: u64, p: u64, p2: u64, jc: u32, jcs: u32, triv: bool,
    t1c: u32, t1s: u32, t2c: u32, t2s: u32, t3c: u32, t3s: u32,
) -> (u64, u64, u64, u64) {
    if !triv {
        b = shoup_lazy(b, t1c, t1s, p);
        c = shoup_lazy(c, t2c, t2s, p);
        d = shoup_lazy(d, t3c, t3s, p);
    }
    let s0 = red2p(a + c, p);
    let s1 = red2p(a + p2 - c, p);
    let s2 = red2p(b + d, p);
    let s3 = red2p(b + p2 - d, p);
    let js3 = shoup_lazy(s3, jc, jcs, p);
    (
        red2p(s0 + s2, p),
        red2p(s1 + js3, p),
        red2p(s0 + p2 - s2, p),
        red2p(s1 + p2 - js3, p),
    )
}

/// Fused first two inverse DIT stages (half-block sizes 1 then 4) on one contiguous
/// 16-element tile in registers — one memory pass instead of two. The second fused
/// stage uses the inverse 16th-root twiddles iw^{64*g} (already in the `iw` table).
#[inline(always)]
fn dit4_first16(
    x: &mut [u64; N], fb: &[u64; N], pinv: u32, iw: &[u32; N], iws: &[u32; N], jc: u32, jcs: u32, p: u64,
) {
    let p2 = p << 1;
    let mut i = 0;
    while i < N {
        let mut t = [0u64; 16];
        unsafe {
            // Fold the spectral pointwise product into the first inverse stage's
            // load: x holds fa, multiply by fb (Montgomery) as the tile is read in.
            for k in 0..16 {
                *t.get_unchecked_mut(k) = mont_mul(*x.get_unchecked(i + k), *fb.get_unchecked(i + k), p, pinv);
            }
            // half-block size 1: groups (4h..4h+3), trivial twiddles.
            for h in 0..4 {
                let b4 = 4 * h;
                let (o0, o1, o2, o3) = r4_lazy_dit(
                    *t.get_unchecked(b4),
                    *t.get_unchecked(b4 + 1),
                    *t.get_unchecked(b4 + 2),
                    *t.get_unchecked(b4 + 3),
                    p, p2, jc, jcs, true, 0, 0, 0, 0, 0, 0,
                );
                *t.get_unchecked_mut(b4) = o0;
                *t.get_unchecked_mut(b4 + 1) = o1;
                *t.get_unchecked_mut(b4 + 2) = o2;
                *t.get_unchecked_mut(b4 + 3) = o3;
            }
            // half-block size 4: groups (g, g+4, g+8, g+12), twiddle exponent e = 64*g.
            for g in 0..4 {
                let e = 64 * g;
                let (it1c, it1s, it2c, it2s, it3c, it3s) = twiddles3(iw, iws, e);
                let (o0, o1, o2, o3) = r4_lazy_dit(
                    *t.get_unchecked(g),
                    *t.get_unchecked(g + 4),
                    *t.get_unchecked(g + 8),
                    *t.get_unchecked(g + 12),
                    p, p2, jc, jcs, e == 0, it1c, it1s, it2c, it2s, it3c, it3s,
                );
                *t.get_unchecked_mut(g) = o0;
                *t.get_unchecked_mut(g + 4) = o1;
                *t.get_unchecked_mut(g + 8) = o2;
                *t.get_unchecked_mut(g + 12) = o3;
            }
            for k in 0..16 {
                *x.get_unchecked_mut(i + k) = *t.get_unchecked(k);
            }
        }
        i += 16;
    }
}

/// Inverse radix-4 DIT, digit-reversed -> natural. Lazy: values in [0, 2p). The
/// first two stages (contiguous 16-element blocks) are fused into one register pass.
#[inline(always)]
fn dit4(
    x: &mut [u64; N], fb: &[u64; N], pinv: u32, iw: &[u32; N], iws: &[u32; N], jc: u32, jcs: u32, p: u64,
) {
    dit4_first16(x, fb, pinv, iw, iws, jc, jcs, p);
    let mut len = 16;
    while len < N {
        let step = N / (4 * len);
        let p2 = p << 1;
        let mut i = 0;
        while i < N {
            let mut e = 0usize;
            let mut j = 0;
            while j < len {
                unsafe {
                    let a = *x.get_unchecked(i + j);
                    let mut b = *x.get_unchecked(i + j + len);
                    let mut c = *x.get_unchecked(i + j + 2 * len);
                    let mut d = *x.get_unchecked(i + j + 3 * len);
                    if e != 0 {
                        b = shoup_lazy(b, *iw.get_unchecked(e), *iws.get_unchecked(e), p);
                        c = shoup_lazy(c, *iw.get_unchecked(2 * e), *iws.get_unchecked(2 * e), p);
                        d = shoup_lazy(d, *iw.get_unchecked(3 * e), *iws.get_unchecked(3 * e), p);
                    }
                    let s0 = red2p(a + c, p);
                    let s1 = red2p(a + p2 - c, p);
                    let s2 = red2p(b + d, p);
                    let s3 = red2p(b + p2 - d, p);
                    let js3 = shoup_lazy(s3, jc, jcs, p); // J * (b - d), in [0,2p)
                    *x.get_unchecked_mut(i + j) = red2p(s0 + s2, p);
                    *x.get_unchecked_mut(i + j + 2 * len) = red2p(s0 + p2 - s2, p);
                    *x.get_unchecked_mut(i + j + len) = red2p(s1 + js3, p);
                    *x.get_unchecked_mut(i + j + 3 * len) = red2p(s1 + p2 - js3, p);
                }
                e += step;
                j += 1;
            }
            i += 4 * len;
        }
        len <<= 2;
    }
}

/// Forward transform of both multiply operands together (shared twiddle loads).
#[inline(always)]
fn fwd2(a: &[u32; N], b: &[u32; N], t: &PrimeTables) -> ([u64; N], [u64; N]) {
    let p = t.p;
    let mut xa = [0u64; N];
    let mut xb = [0u64; N];
    unsafe {
        for j in 0..N {
            let psi = *t.psi.get_unchecked(j);
            let psis = *t.psis.get_unchecked(j);
            *xa.get_unchecked_mut(j) = shoup_lazy(*a.get_unchecked(j) as u64, psi, psis, p);
            *xb.get_unchecked_mut(j) = shoup_lazy(*b.get_unchecked(j) as u64, psi, psis, p);
        }
    }
    dif4_2(&mut xa, &mut xb, &t.w, &t.ws, t.w[N / 4], t.ws[N / 4], p);
    (xa, xb)
}

/// Inverse negacyclic transform of `fa` (the pointwise product is folded into the
/// first DIT stage's load with `fb`), then post-weight by psi^{-j}*N^{-1}.
#[inline(always)]
fn inv(mut fa: [u64; N], fb: &[u64; N], t: &PrimeTables) -> [u64; N] {
    let p = t.p;
    // J = w^{-N/4} lives at index N/4 of the inverse twiddle table.
    dit4(&mut fa, fb, t.pinv, &t.iw, &t.iws, t.iw[N / 4], t.iws[N / 4], p);
    unsafe {
        for j in 0..N {
            let xj = *fa.get_unchecked(j);
            *fa.get_unchecked_mut(j) =
                shoup(xj, *t.ipsi.get_unchecked(j), *t.ipsis.get_unchecked(j), p);
        }
    }
    fa
}

fn convolve_mod(t: &PrimeTables, a: &[u32; N], b: &[u32; N]) -> [u64; N] {
    // The spectral pointwise product (Montgomery, since the domain is in Montgomery
    // form) is folded into the inverse transform's first stage, so there is no
    // standalone pointwise pass here.
    let (fa, fb) = fwd2(a, b, t);
    inv(fa, &fb, t)
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
            // v1 = (r1 - v0) * inv(P0) mod P1, division-free via Shoup.
            let t1 = red(*r1.get_unchecked(j) + p1 - v0, p1);
            let v1 = shoup(t1, inv01, inv01_s, p1);
            // w = (v0 + P0*v1) mod P2; v2 = (r2 - w) * inv(P0*P1) mod P2.
            let term = shoup(red(v1, p2), p0_mod_p2, p0_mod_p2_s, p2);
            let w = red(red(v0, p2) + term, p2);
            let t2 = red(*r2.get_unchecked(j) + p2 - w, p2);
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
