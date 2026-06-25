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
// `%`-reduction the NTT is usually written with.
//
// SIMD: the forward transform multiplies BOTH operands (a and b) by the same
// twiddles in lockstep, so a and b are packed into the two lanes of an i64x2
// vector and the whole forward NTT runs with v128 arithmetic — one SIMD multiply
// (charged like a scalar multiply) does both operands' modular products at once.
// The lane abstraction `L` has a real-`v128` implementation on wasm (where the
// metric runs) and a scalar two-`u64` fallback elsewhere (where correctness is
// checked); both compute bit-identical results, so the native correctness gate
// validates the exact algorithm the wasm build meters.
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

/// Vectorized Montgomery product, two independent products per i64x2 (lane k holds
/// `a_k*b_k*R^{-1} mod p` in [0,2p)). `maskv = splat(0xffffffff)`, `pinvv =
/// splat(-p^{-1} mod 2^32)`. Inputs a,b < 2p < 2^32 so a*b < 4p^2 < p*R.
#[inline(always)]
unsafe fn mont_mul_l(a: L, b: L, pv: L, pinvv: L, maskv: L) -> L {
    let t = a.mul(b); // low 64 of a*b (exact, < p*R)
    let tlo = t.and(maskv); // t mod R
    let m = tlo.mul(pinvv).and(maskv); // (t mod R) * (-p^{-1}) mod R
    t.add(m.mul(pv)).shr32() // exact /R, result in [0,2p)
}

// ---------------------------------------------------------------------------
// Two-lane vector abstraction `L`. Lane 0 carries operand `a`, lane 1 carries
// operand `b`; the forward NTT runs both in lockstep. On wasm it is a real
// `v128` (i64x2); elsewhere a scalar pair, so the native correctness gate runs
// the identical algorithm. Every method is `unsafe fn` so the (target-feature)
// wasm impl and the scalar impl share one call site; the wasm forward functions
// carry `#[target_feature(enable = "simd128")]` via `cfg_attr`.
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
struct L(v128);

#[cfg(target_arch = "wasm32")]
impl L {
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn splat(x: u64) -> L {
        L(u64x2_splat(x))
    }
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn new(a: u64, b: u64) -> L {
        L(u64x2(a, b))
    }
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn add(self, o: L) -> L {
        L(i64x2_add(self.0, o.0))
    }
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn sub(self, o: L) -> L {
        L(i64x2_sub(self.0, o.0))
    }
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn mul(self, o: L) -> L {
        L(i64x2_mul(self.0, o.0)) // low 64 bits per lane (exact for our < 2^64 products)
    }
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn shr32(self) -> L {
        L(u64x2_shr(self.0, 32))
    }
    /// Lanewise (a >= o) -> all-ones / all-zeros mask. Operands < 2^63 so signed
    /// `i64x2_ge` agrees with unsigned compare.
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn ge(self, o: L) -> L {
        L(i64x2_ge(self.0, o.0))
    }
    /// Lanewise mask ? t : f (mask all-ones / all-zeros).
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn select(mask: L, t: L, f: L) -> L {
        L(v128_bitselect(t.0, f.0, mask.0))
    }
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn lane0(self) -> u64 {
        u64x2_extract_lane::<0>(self.0)
    }
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn lane1(self) -> u64 {
        u64x2_extract_lane::<1>(self.0)
    }
    /// Load two adjacent u64 (positions p[0], p[1]) into lanes 0,1.
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn load(p: *const u64) -> L {
        L(v128_load(p as *const v128))
    }
    /// Store lanes 0,1 into two adjacent u64 (p[0], p[1]).
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn store(p: *mut u64, v: L) {
        v128_store(p as *mut v128, v.0);
    }
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn and(self, o: L) -> L {
        L(v128_and(self.0, o.0))
    }
    /// Deinterleave: returns (lane0 of x, lane0 of y) and (lane1 of x, lane1 of y).
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn unzip(x: L, y: L) -> (L, L) {
        (
            L(i64x2_shuffle::<0, 2>(x.0, y.0)),
            L(i64x2_shuffle::<1, 3>(x.0, y.0)),
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
struct L(u64, u64);

#[cfg(not(target_arch = "wasm32"))]
impl L {
    #[inline(always)]
    unsafe fn splat(x: u64) -> L {
        L(x, x)
    }
    #[inline(always)]
    unsafe fn new(a: u64, b: u64) -> L {
        L(a, b)
    }
    #[inline(always)]
    unsafe fn add(self, o: L) -> L {
        L(self.0.wrapping_add(o.0), self.1.wrapping_add(o.1))
    }
    #[inline(always)]
    unsafe fn sub(self, o: L) -> L {
        L(self.0.wrapping_sub(o.0), self.1.wrapping_sub(o.1))
    }
    #[inline(always)]
    unsafe fn mul(self, o: L) -> L {
        L(self.0.wrapping_mul(o.0), self.1.wrapping_mul(o.1))
    }
    #[inline(always)]
    unsafe fn shr32(self) -> L {
        L(self.0 >> 32, self.1 >> 32)
    }
    #[inline(always)]
    unsafe fn ge(self, o: L) -> L {
        L(
            if self.0 >= o.0 { !0 } else { 0 },
            if self.1 >= o.1 { !0 } else { 0 },
        )
    }
    #[inline(always)]
    unsafe fn select(mask: L, t: L, f: L) -> L {
        L(
            (t.0 & mask.0) | (f.0 & !mask.0),
            (t.1 & mask.1) | (f.1 & !mask.1),
        )
    }
    #[inline(always)]
    unsafe fn lane0(self) -> u64 {
        self.0
    }
    #[inline(always)]
    unsafe fn lane1(self) -> u64 {
        self.1
    }
    /// Load two adjacent u64 (positions p[0], p[1]) into lanes 0,1.
    #[inline(always)]
    unsafe fn load(p: *const u64) -> L {
        L(*p, *p.add(1))
    }
    /// Store lanes 0,1 into two adjacent u64 (p[0], p[1]).
    #[inline(always)]
    unsafe fn store(p: *mut u64, v: L) {
        *p = v.0;
        *p.add(1) = v.1;
    }
    #[inline(always)]
    unsafe fn and(self, o: L) -> L {
        L(self.0 & o.0, self.1 & o.1)
    }
    #[inline(always)]
    unsafe fn unzip(x: L, y: L) -> (L, L) {
        (L(x.0, y.0), L(x.1, y.1))
    }
}

/// Lanewise reduce [0,4p) -> [0,2p).
#[inline(always)]
unsafe fn red2p_l(a: L, p2v: L) -> L {
    let t = a.sub(p2v);
    let m = a.ge(p2v);
    L::select(m, t, a)
}

/// Lanewise lazy Shoup `x * c mod p` left in [0,2p) (no final conditional sub).
/// Valid for any lane < 2^32 (Harvey's bound). `pv = splat(p)`.
#[inline(always)]
unsafe fn shoup_lazy_l(x: L, c: u32, cs: u32, pv: L) -> L {
    let cv = L::splat(c as u64);
    let csv = L::splat(cs as u64);
    let q = x.mul(csv).shr32();
    x.mul(cv).sub(q.mul(pv))
}

/// Lazy Shoup with the multiplier already in vector form (different constant per
/// lane). Result in [0,2p). `pv = splat(p)`.
#[inline(always)]
unsafe fn shoup_lazy_lv(x: L, cv: L, csv: L, pv: L) -> L {
    let q = x.mul(csv).shr32();
    x.mul(cv).sub(q.mul(pv))
}

/// Lanewise reduce [0,2p) -> [0,p).
#[inline(always)]
unsafe fn redp_l(a: L, pv: L) -> L {
    let t = a.sub(pv);
    let m = a.ge(pv);
    L::select(m, t, a)
}

/// Build the three radix-4 stage twiddle vectors (and their Shoup constants) for a
/// pair of adjacent butterflies with exponents `e0` and `e1`. Each returned `L`
/// holds the constant for lane 0 (e0) and lane 1 (e1).
#[inline(always)]
unsafe fn twiddles3_l(
    w: &[u32; N], ws: &[u32; N], e0: usize, e1: usize,
) -> (L, L, L, L, L, L) {
    let g = |t: &[u32; N], a: usize, b: usize| {
        L::new(*t.get_unchecked(a) as u64, *t.get_unchecked(b) as u64)
    };
    (
        g(w, e0, e1), g(ws, e0, e1),
        g(w, 2 * e0, 2 * e1), g(ws, 2 * e0, 2 * e1),
        g(w, 3 * e0, 3 * e1), g(ws, 3 * e0, 3 * e1),
    )
}

/// Vectorized inverse radix-4 DIT butterfly (two adjacent butterflies in the two
/// lanes), Harvey-lazy: inputs/outputs in [0,4p). Per-lane stage twiddles; the
/// inverse 4th-root `J` (`jcv`,`jcsv`) is the same for both lanes (splat). The
/// untwiddled input `a` is reduced; the twiddled inputs go through the lazy Shoup
/// which tolerates [0,4p). No trivial branch — the e=0 column carries the real
/// w^0=1 twiddle (its Shoup const reduces correctly), so both lanes use one path.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
unsafe fn r4_lazy_dit_l(
    a: L, b: L, c: L, d: L, pv: L, p2v: L, jcv: L, jcsv: L,
    t1c: L, t1s: L, t2c: L, t2s: L, t3c: L, t3s: L,
) -> (L, L, L, L) {
    let a = red2p_l(a, p2v); // [0,4p) -> [0,2p)
    let b = shoup_lazy_lv(b, t1c, t1s, pv);
    let c = shoup_lazy_lv(c, t2c, t2s, pv);
    let d = shoup_lazy_lv(d, t3c, t3s, pv);
    let s0 = red2p_l(a.add(c), p2v);
    let s1 = red2p_l(a.add(p2v).sub(c), p2v);
    let s2 = red2p_l(b.add(d), p2v);
    let s3 = b.add(p2v).sub(d); // [0,4p); feeds only the lazy Shoup
    let js3 = shoup_lazy_lv(s3, jcv, jcsv, pv);
    (s0.add(s2), s1.add(js3), s0.add(p2v).sub(s2), s1.add(p2v).sub(js3))
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

/// One radix-4 DIF butterfly on four lane-vectors (lazy: in [0,2p), out [0,2p)).
/// `triv` skips the (unit) stage twiddles. Both operands ride the two lanes.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
unsafe fn r4_lazy_l(
    a: L, b: L, c: L, d: L, pv: L, p2v: L, ic: u32, ics: u32, triv: bool,
    t1c: u32, t1s: u32, t2c: u32, t2s: u32, t3c: u32, t3s: u32,
) -> (L, L, L, L) {
    let s0 = red2p_l(a.add(c), p2v);
    let s2 = red2p_l(b.add(d), p2v);
    let s1 = red2p_l(a.add(p2v).sub(c), p2v);
    let s3 = b.add(p2v).sub(d); // in [0,4p); feeds only the lazy Shoup
    let is3 = shoup_lazy_l(s3, ic, ics, pv);
    let y0 = red2p_l(s0.add(s2), p2v);
    let y2 = s0.add(p2v).sub(s2);
    let y1 = s1.add(is3);
    let y3 = s1.add(p2v).sub(is3);
    if triv {
        (y0, red2p_l(y1, p2v), red2p_l(y2, p2v), red2p_l(y3, p2v))
    } else {
        (
            y0,
            shoup_lazy_l(y1, t1c, t1s, pv),
            shoup_lazy_l(y2, t2c, t2s, pv),
            shoup_lazy_l(y3, t3c, t3s, pv),
        )
    }
}

/// Forward radix-4 DIF of both operands in lockstep (lanes), through the middle
/// stages only (the psi pre-weight is folded into the first stage's load). The
/// last two stages are completed in the fused `boundary` pass.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(target_arch = "wasm32", target_feature(enable = "simd128"))]
unsafe fn dif4_2_simd(
    a: &[u32; N], b: &[u32; N], xab: &mut [L; N], psi: &[u32; N], psis: &[u32; N],
    w: &[u32; N], ws: &[u32; N], ic: u32, ics: u32, p: u64,
) {
    let pv = L::splat(p);
    let p2v = L::splat(p << 1);
    // First stage (half-block N/4) with the psi pre-weight folded into the load.
    let len0 = N / 4;
    let mut j = 0;
    while j < len0 {
        let e = j; // step = 1
        let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3(w, ws, e);
        let pw = |idx: usize| -> L {
            let v = L::new(*a.get_unchecked(idx) as u64, *b.get_unchecked(idx) as u64);
            shoup_lazy_l(v, *psi.get_unchecked(idx), *psis.get_unchecked(idx), pv)
        };
        let (y0, y1, y2, y3) = r4_lazy_l(
            pw(j), pw(j + len0), pw(j + 2 * len0), pw(j + 3 * len0), pv, p2v, ic, ics,
            e == 0, t1c, t1s, t2c, t2s, t3c, t3s,
        );
        *xab.get_unchecked_mut(j) = y0;
        *xab.get_unchecked_mut(j + len0) = y1;
        *xab.get_unchecked_mut(j + 2 * len0) = y2;
        *xab.get_unchecked_mut(j + 3 * len0) = y3;
        j += 1;
    }
    // Remaining strided stages (half-blocks 64, 16) on xab.
    let mut len = N / 16;
    while len >= 16 {
        let step = N / (4 * len);
        let mut i = 0;
        while i < N {
            let mut e = 0usize;
            let mut jj = 0;
            while jj < len {
                let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3(w, ws, e);
                let base = i + jj;
                let (y0, y1, y2, y3) = r4_lazy_l(
                    *xab.get_unchecked(base),
                    *xab.get_unchecked(base + len),
                    *xab.get_unchecked(base + 2 * len),
                    *xab.get_unchecked(base + 3 * len),
                    pv, p2v, ic, ics, e == 0, t1c, t1s, t2c, t2s, t3c, t3s,
                );
                *xab.get_unchecked_mut(base) = y0;
                *xab.get_unchecked_mut(base + len) = y1;
                *xab.get_unchecked_mut(base + 2 * len) = y2;
                *xab.get_unchecked_mut(base + 3 * len) = y3;
                e += step;
                jj += 1;
            }
            i += 4 * len;
        }
        len >>= 2;
    }
}

// ---- Contiguous 16-element tile sub-stages (used by the fused boundary pass) ----

#[inline(always)]
unsafe fn dif_l4_v(t: &mut [L; 16], w: &[u32; N], ws: &[u32; N], ic: u32, ics: u32, pv: L, p2v: L) {
    for g in 0..4 {
        let e = 64 * g;
        let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3(w, ws, e);
        let (y0, y1, y2, y3) = r4_lazy_l(
            *t.get_unchecked(g), *t.get_unchecked(g + 4), *t.get_unchecked(g + 8),
            *t.get_unchecked(g + 12), pv, p2v, ic, ics, e == 0, t1c, t1s, t2c, t2s, t3c, t3s,
        );
        *t.get_unchecked_mut(g) = y0;
        *t.get_unchecked_mut(g + 4) = y1;
        *t.get_unchecked_mut(g + 8) = y2;
        *t.get_unchecked_mut(g + 12) = y3;
    }
}

#[inline(always)]
unsafe fn dif_l1_v(t: &mut [L; 16], ic: u32, ics: u32, pv: L, p2v: L) {
    for h in 0..4 {
        let b4 = 4 * h;
        let (y0, y1, y2, y3) = r4_lazy_l(
            *t.get_unchecked(b4), *t.get_unchecked(b4 + 1), *t.get_unchecked(b4 + 2),
            *t.get_unchecked(b4 + 3), pv, p2v, ic, ics, true, 0, 0, 0, 0, 0, 0,
        );
        *t.get_unchecked_mut(b4) = y0;
        *t.get_unchecked_mut(b4 + 1) = y1;
        *t.get_unchecked_mut(b4 + 2) = y2;
        *t.get_unchecked_mut(b4 + 3) = y3;
    }
}

/// Scalar lazy Shoup (single value), used by the inverse DIT. Result in [0,2p).
#[inline(always)]
fn shoup_lazy(x: u64, c: u32, cs: u32, p: u64) -> u64 {
    let q = x.wrapping_mul(cs as u64) >> 32;
    x.wrapping_mul(c as u64).wrapping_sub(q.wrapping_mul(p))
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

/// Second inverse DIT sub-stage on a 16-element tile (stride 4). The four
/// butterflies g=0..3 sit at adjacent slots for every radix-4 leg (g, g+4, g+8,
/// g+12), so a pair (g, g+1) rides the two lanes with contiguous v128 loads; the
/// two lanes' twiddles (e=64g, 64(g+1)) are loaded as pairs.
#[inline(always)]
unsafe fn dit_l4_v(t: &mut [u64; 16], iw: &[u32; N], iws: &[u32; N], jcv: L, jcsv: L, pv: L, p2v: L) {
    let tp = t.as_mut_ptr();
    let mut g = 0;
    while g < 4 {
        let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3_l(iw, iws, 64 * g, 64 * (g + 1));
        let a = L::load(tp.add(g));
        let b = L::load(tp.add(g + 4));
        let c = L::load(tp.add(g + 8));
        let d = L::load(tp.add(g + 12));
        let (o0, o1, o2, o3) =
            r4_lazy_dit_l(a, b, c, d, pv, p2v, jcv, jcsv, t1c, t1s, t2c, t2s, t3c, t3s);
        L::store(tp.add(g), o0);
        L::store(tp.add(g + 4), o1);
        L::store(tp.add(g + 8), o2);
        L::store(tp.add(g + 12), o3);
        g += 2;
    }
}

/// Remaining inverse DIT stages (the first two are done by `boundary`): the middle
/// strided stages (half-blocks 16, 64) then the final stage (half-block 256) with the
/// psi^{-1}*N^{-1} post-weight folded into the output store. Values in [0,4p).
/// Vectorized: butterflies `j` and `j+1` ride the two lanes. Their per-slot memory
/// positions are adjacent (`x[base]`, `x[base+1]`), so each radix-4 input/output is a
/// contiguous v128 load/store; the two lanes' twiddles differ and are loaded as pairs.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(target_arch = "wasm32", target_feature(enable = "simd128"))]
unsafe fn dit4_rest(
    x: &mut [u64; N], iw: &[u32; N], iws: &[u32; N], jc: u32, jcs: u32,
    ipsi: &[u32; N], ipsis: &[u32; N], p: u64,
) {
    let pv = L::splat(p);
    let p2v = L::splat(p << 1);
    let jcv = L::splat(jc as u64);
    let jcsv = L::splat(jcs as u64);
    let xp = x.as_mut_ptr();
    let mut len = 16;
    while len < N / 4 {
        let step = N / (4 * len);
        let mut i = 0;
        while i < N {
            let mut j = 0;
            while j < len {
                let e0 = j * step;
                let e1 = e0 + step;
                let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3_l(iw, iws, e0, e1);
                let base = i + j;
                let a = L::load(xp.add(base));
                let b = L::load(xp.add(base + len));
                let c = L::load(xp.add(base + 2 * len));
                let d = L::load(xp.add(base + 3 * len));
                let (o0, o1, o2, o3) =
                    r4_lazy_dit_l(a, b, c, d, pv, p2v, jcv, jcsv, t1c, t1s, t2c, t2s, t3c, t3s);
                L::store(xp.add(base), o0);
                L::store(xp.add(base + len), o1);
                L::store(xp.add(base + 2 * len), o2);
                L::store(xp.add(base + 3 * len), o3);
                j += 2;
            }
            i += 4 * len;
        }
        len <<= 2;
    }
    // Final stage (half-block 256, step=1, e=j) with the psi^{-1}*N^{-1} post-weight
    // folded into the output store (no standalone post-weight pass).
    let mut j = 0usize;
    while j < N / 4 {
        let (t1c, t1s, t2c, t2s, t3c, t3s) = twiddles3_l(iw, iws, j, j + 1);
        let a = L::load(xp.add(j));
        let b = L::load(xp.add(j + N / 4));
        let c = L::load(xp.add(j + N / 2));
        let d = L::load(xp.add(j + 3 * N / 4));
        let (o0, o1, o2, o3) =
            r4_lazy_dit_l(a, b, c, d, pv, p2v, jcv, jcsv, t1c, t1s, t2c, t2s, t3c, t3s);
        // Post-weight (shoup with conditional subtract -> [0,p)) per lane position.
        let pw = |o: L, pos: usize| -> L {
            let ipv = L::new(*ipsi.get_unchecked(pos) as u64, *ipsi.get_unchecked(pos + 1) as u64);
            let ipsv = L::new(*ipsis.get_unchecked(pos) as u64, *ipsis.get_unchecked(pos + 1) as u64);
            redp_l(shoup_lazy_lv(o, ipv, ipsv, pv), pv)
        };
        L::store(xp.add(j), pw(o0, j));
        L::store(xp.add(j + N / 4), pw(o1, j + N / 4));
        L::store(xp.add(j + N / 2), pw(o2, j + N / 2));
        L::store(xp.add(j + 3 * N / 4), pw(o3, j + 3 * N / 4));
        j += 2;
    }
}

/// Fused transform boundary on contiguous 16-element blocks: `xab` holds both
/// operands (lanes) after the middle forward stages; per block this completes the
/// forward (last two DIF stages) for each, does the Montgomery pointwise product
/// (combining the two lanes), and starts the inverse (first two DIT stages) —
/// entirely in registers, so the full forward spectra never touch memory.
#[cfg_attr(target_arch = "wasm32", target_feature(enable = "simd128"))]
unsafe fn boundary_simd(xab: &[L; N], out: &mut [u64; N], t: &PrimeTables) {
    let p = t.p;
    let p2 = p << 1;
    let pv = L::splat(p);
    let p2v = L::splat(p2);
    let (icf, icfs) = (t.w[N / 4], t.ws[N / 4]); // forward 4th root I
    let (jc, jcs) = (t.iw[N / 4], t.iws[N / 4]); // inverse 4th root J
    let jcv = L::splat(jc as u64);
    let jcsv = L::splat(jcs as u64);
    let pinvv = L::splat(t.pinv as u64);
    let maskv = L::splat(0xffff_ffff);
    let mut i = 0;
    while i < N {
        let mut ta = [L::splat(0); 16];
        for k in 0..16 {
            *ta.get_unchecked_mut(k) = *xab.get_unchecked(i + k);
        }
        dif_l4_v(&mut ta, &t.w, &t.ws, icf, icfs, pv, p2v);
        dif_l1_v(&mut ta, icf, icfs, pv, p2v);
        // Pointwise: a*b per position. Deinterleave the packed (a,b) lanes of two
        // adjacent positions into an a-vector and a b-vector, then one vector
        // Montgomery product yields both products contiguously.
        let mut tc = [0u64; 16];
        let tcp = tc.as_mut_ptr();
        let mut k = 0;
        while k < 16 {
            let (av, bv) = L::unzip(*ta.get_unchecked(k), *ta.get_unchecked(k + 1));
            L::store(tcp.add(k), mont_mul_l(av, bv, pv, pinvv, maskv));
            k += 2;
        }
        dit_l1_in2p(&mut tc, jc, jcs, p, p2);
        dit_l4_v(&mut tc, &t.iw, &t.iws, jcv, jcsv, pv, p2v);
        for k in 0..16 {
            *out.get_unchecked_mut(i + k) = *tc.get_unchecked(k);
        }
        i += 16;
    }
}

fn convolve_mod(t: &PrimeTables, a: &[u32; N], b: &[u32; N]) -> [u64; N] {
    let mut x = [0u64; N];
    unsafe {
        fwd_boundary(t, a, b, &mut x); // forward (SIMD a/b) + pointwise + first inv stages
        dit4_rest(&mut x, &t.iw, &t.iws, t.iw[N / 4], t.iws[N / 4], &t.ipsi, &t.ipsis, t.p); // rest of inverse (SIMD)
    }
    x
}

/// Forward transform of both operands (SIMD lockstep) then the fused boundary pass.
#[cfg_attr(target_arch = "wasm32", target_feature(enable = "simd128"))]
unsafe fn fwd_boundary(t: &PrimeTables, a: &[u32; N], b: &[u32; N], out: &mut [u64; N]) {
    let mut xab = [L::splat(0); N];
    dif4_2_simd(a, b, &mut xab, &t.psi, &t.psis, &t.w, &t.ws, t.w[N / 4], t.ws[N / 4], t.p);
    boundary_simd(&xab, out, t);
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

/// Garner CRT reconstruction, vectorized two coefficients (j, j+1) at a time. The
/// mixing constants are identical for both lanes (splat); the per-prime residues are
/// adjacent in memory so each `r*[j..j+2]` is a contiguous v128 load. Every modular
/// multiply is a lane Shoup (reduced to [0,p) with a conditional subtract).
#[cfg_attr(target_arch = "wasm32", target_feature(enable = "simd128"))]
unsafe fn crt_combine(r0: &[u64; N], r1: &[u64; N], r2: &[u64; N], plan: &Plan, res: &mut [u32; N]) {
    let p1v = L::splat(P1);
    let p2v = L::splat(P2);
    let p2_3v = L::splat(3 * P2);
    let inv01_v = L::splat(plan.inv_p0_mod_p1 as u64);
    let inv01_s_v = L::splat(plan.inv_p0_mod_p1_s as u64);
    let p0m2_v = L::splat(plan.p0_mod_p2 as u64);
    let p0m2_s_v = L::splat(plan.p0_mod_p2_s as u64);
    let invm01_v = L::splat(plan.inv_m01_mod_p2 as u64);
    let invm01_s_v = L::splat(plan.inv_m01_mod_p2_s as u64);
    let p2_half_v = L::splat(plan.p2_half);
    let p0_lo_v = L::splat(plan.p0_lo as u64);
    let m01_lo_v = L::splat(plan.m01_lo as u64);
    let p_lo_v = L::splat(plan.p_lo as u64);
    let r0p = r0.as_ptr();
    let r1p = r1.as_ptr();
    let r2p = r2.as_ptr();
    let mut j = 0;
    while j < N {
        let v0 = L::load(r0p.add(j)); // < P0 < P1
        // v1 = (r1 - v0) * inv(P0) mod P1.
        let t1 = L::load(r1p.add(j)).add(p1v).sub(v0); // [0, 2*P1)
        let v1 = redp_l(shoup_lazy_lv(t1, inv01_v, inv01_s_v, p1v), p1v);
        // w = (v0 + P0*v1) mod P2 kept lazy in [0, 3*P2); v2 = (r2 - w) * inv(P0*P1).
        let term = redp_l(shoup_lazy_lv(v1, p0m2_v, p0m2_s_v, p2v), p2v);
        let w = v0.add(term); // < P0 + P2 < 3*P2
        let t2 = L::load(r2p.add(j)).add(p2_3v).sub(w); // (0, 4*P2)
        let v2 = redp_l(shoup_lazy_lv(t2, invm01_v, invm01_s_v, p2v), p2v);
        // u = v0 + P0*v1 + P0*P1*v2 ; low 32 bits = product mod 2^32, sign from v2.
        let lo = v0.add(p0_lo_v.mul(v1)).add(m01_lo_v.mul(v2));
        let m = v2.ge(p2_half_v);
        let out = L::select(m, lo.sub(p_lo_v), lo);
        *res.get_unchecked_mut(j) = out.lane0() as u32;
        *res.get_unchecked_mut(j + 1) = out.lane1() as u32;
        j += 2;
    }
}

/// Negacyclic polynomial multiplication: a(X) * b(X) mod (X^1024+1).
pub fn poly_mul(plan: &mut Plan, a: &[u32; 1024], b: &[u32; 1024]) -> [u32; 1024] {
    let r0 = convolve_mod(&plan.t[0], a, b);
    let r1 = convolve_mod(&plan.t[1], a, b);
    let r2 = convolve_mod(&plan.t[2], a, b);

    let mut res = [0u32; N];
    unsafe {
        crt_combine(&r0, &r1, &r2, plan, &mut res);
    }
    res
}
