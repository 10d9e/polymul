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
// psi pre/post weights, and the Garner mixing constants) uses Plantard's method —
// one signed high-half multiply for the quotient estimate and one multiply by p,
// no hardware `%`. Plantard needs only TWO multiplies and ONE precomputed constant
// per multiplier (vs Shoup's three multiplies and two constants), so under a cost
// model where division is ~25x an add it is far cheaper than `%`, and cheaper than
// Shoup too. The variable-by-variable pointwise product stays Montgomery.
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

// Three primes p with 2048 | (p-1) and primitive root 3; each p ~ 2^27 so that
// products of residues fit in u64 and lazy values keep ~32x headroom under 2^32.
// Their product is ~2^81 > 2^75, enough to recover the exact signed product mod 2^32.
const P0: u64 = 134250497;
const P1: u64 = 134275073;
const P2: u64 = 134330369;
const GEN: u64 = 3; // primitive root for all three primes

/// Per-prime NTT tables. Each multiplier `c` is stored as a single Plantard constant
/// `bprime` so that `c * x mod p` needs no division (see `plantard_const`).
struct PrimeTables {
    p: u64,
    // Each constant modular multiply uses Plantard's method: one precomputed u64
    // `bprime = (c * (-2^64 mod p) mod p) * p^{-1} mod 2^64` per multiplier `c`, so a
    // butterfly twiddle is a SINGLE u64 (no separate Shoup constant). The forward
    // broadcasts it (one `v128.load64_splat`); the inverse loads adjacent pairs.
    psip: [u64; N],  // Plantard const for psi^j * R   (negacyclic pre-weight, Montgomery R)
    ipsip: [u64; N], // Plantard const for psi^{-j} * N^{-1} * R^{-1} (post-weight)
    wp: [u64; N],    // Plantard const for w^e   (forward twiddle, w = psi^2)
    iwp: [u64; N],   // Plantard const for w^{-e} (inverse twiddle)
    // iwp at strides 2 and 3, so the inverse final stage (step=1) loads the t2/t3
    // twiddles of an adjacent butterfly pair (j, j+1) with a single contiguous v128
    // load instead of two scattered scalar loads.
    iwp2: [u64; N / 2], // iwp2[j] = iwp[2j]
    iwp3: [u64; N / 2], // iwp3[j] = iwp[3j]  (note 3j < N for j < N/3; only j<N/4 used)
    pinv: u32,       // -p^{-1} mod 2^32, for the Montgomery pointwise product
}

/// Opaque plan holding precomputed tables (built once in `plan_new`, free).
pub struct Plan {
    t: [PrimeTables; 3],
    // Garner CRT mixing constants as Plantard constants (division-free).
    inv_p0_mod_p1_p: u64,
    p0_mod_p2_p: u64,
    inv_m01_mod_p2_p: u64,
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
    // m = (t mod R) * (-p^{-1}) mod R. Masking t to R first is unnecessary: the low 32
    // bits of t*pinv already equal ((t mod R)*pinv) mod R, so one `and` is enough.
    let m = t.mul(pinvv).and(maskv);
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
    /// Arithmetic (sign-extending) shift right by 32, for Plantard's signed high half.
    #[inline]
    #[target_feature(enable = "simd128")]
    unsafe fn ashr32(self) -> L {
        L(i64x2_shr(self.0, 32))
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
    unsafe fn ashr32(self) -> L {
        L(((self.0 as i64) >> 32) as u64, ((self.1 as i64) >> 32) as u64)
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

/// Plantard modular multiply by a precomputed constant: `bpv` lanes hold
/// `bprime = (c * (-2^64 mod q) mod q) * q^{-1} mod 2^64`. For any non-negative input
/// `x < 8q` this returns `x*c mod q` represented in [0,2q) — a drop-in replacement for
/// `shoup_lazy_lv` using only TWO multiplies (vs Shoup's three) and ONE constant load
/// (vs two). `caddv = splat(2^32 + 1)` folds in both Plantard's rounding `+1` and the
/// `+q` that shifts the centred result into [0,2q); `qv = splat(q)`.
#[inline(always)]
unsafe fn plantard_lv(x: L, bpv: L, qv: L, caddv: L) -> L {
    let h = x.mul(bpv).ashr32(); // high 32 of x*bprime (signed)
    h.add(caddv).mul(qv).shr32() // ((h + 2^32 + 1) * q) >> 32  in [0,2q)
}

/// Lanewise reduce [0,2p) -> [0,p).
#[inline(always)]
unsafe fn redp_l(a: L, pv: L) -> L {
    let t = a.sub(pv);
    let m = a.ge(pv);
    L::select(m, t, a)
}

/// Build the three radix-4 stage Plantard twiddle constants (w^e, w^{2e}, w^{3e}) for
/// a pair of adjacent butterflies with exponents `e0` and `e1`. Each returned `L` holds
/// the constant for lane 0 (e0) and lane 1 (e1).
#[inline(always)]
unsafe fn twiddles3_l(iwp: &[u64; N], e0: usize, e1: usize) -> (L, L, L) {
    let g = |a: usize, b: usize| L::new(*iwp.get_unchecked(a), *iwp.get_unchecked(b));
    (g(e0, e1), g(2 * e0, 2 * e1), g(3 * e0, 3 * e1))
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
    a: L, b: L, c: L, d: L, pv: L, p2v: L, cav: L, jcp: L,
    t1p: L, t2p: L, t3p: L,
) -> (L, L, L, L) {
    let a = red2p_l(a, p2v); // [0,4p) -> [0,2p)
    let b = plantard_lv(b, t1p, pv, cav);
    let c = plantard_lv(c, t2p, pv, cav);
    let d = plantard_lv(d, t3p, pv, cav);
    let s0 = red2p_l(a.add(c), p2v);
    let s1 = red2p_l(a.add(p2v).sub(c), p2v);
    let s2 = red2p_l(b.add(d), p2v);
    let s3 = b.add(p2v).sub(d); // [0,4p); feeds only the lazy Plantard
    let js3 = plantard_lv(s3, jcp, pv, cav);
    (s0.add(s2), s1.add(js3), s0.add(p2v).sub(s2), s1.add(p2v).sub(js3))
}

/// Final inverse DIT butterfly: identical to `r4_lazy_dit_l` but the s0/s1/s2
/// reductions are dropped. The outputs feed only the psi^{-1}*N^{-1} post-weight
/// Shoup, which tolerates any value < 2^32, so they may grow to [0,8p) (well under
/// 2^32 at the 27-bit primes). The untwiddled input `a` is still reduced so the
/// sums stay bounded.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
unsafe fn r4_lazy_dit_l_final(
    a: L, b: L, c: L, d: L, pv: L, p2v: L, p4v: L, cav: L, jcp: L,
    t1p: L, t2p: L, t3p: L,
) -> (L, L, L, L) {
    let a = red2p_l(a, p2v); // [0,4p) -> [0,2p)
    let b = plantard_lv(b, t1p, pv, cav);
    let c = plantard_lv(c, t2p, pv, cav);
    let d = plantard_lv(d, t3p, pv, cav);
    let s0 = a.add(c); // [0,4p)
    let s1 = a.add(p2v).sub(c); // [0,4p)
    let s2 = b.add(d); // [0,4p)
    let s3 = b.add(p2v).sub(d); // [0,4p)
    let js3 = plantard_lv(s3, jcp, pv, cav);
    // out2 subtracts s2 (< 4p) so it needs a 4p bias; out3 subtracts js3 (< 2p).
    (s0.add(s2), s1.add(js3), s0.add(p4v).sub(s2), s1.add(p2v).sub(js3))
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
        psip: [0; N],
        ipsip: [0; N],
        wp: [0; N],
        iwp: [0; N],
        iwp2: [0; N / 2],
        iwp3: [0; N / 2],
        pinv: inv.wrapping_neg(),
    };

    let mut acc = 1u64; // psi^j
    let mut iacc = 1u64; // psi^{-j}
    for j in 0..N {
        let pm = (acc as u128 * r_mod as u128 % p as u128) as u64; // psi^j * R  (Montgomery)
        pt.psip[j] = plantard_const(pm, p);
        let ip = (iacc as u128 * ninv as u128 % p as u128) as u64; // psi^{-j} * N^{-1}
        let ipm = (ip as u128 * rinv as u128 % p as u128) as u64; //   * R^{-1} (de-Montgomery)
        pt.ipsip[j] = plantard_const(ipm, p);
        acc = (acc as u128 * psi_root as u128 % p as u128) as u64;
        iacc = (iacc as u128 * psi_inv as u128 % p as u128) as u64;
    }

    let mut wacc = 1u64; // w^e
    let mut iwacc = 1u64; // w^{-e}
    for e in 0..N {
        pt.wp[e] = plantard_const(wacc, p);
        pt.iwp[e] = plantard_const(iwacc, p);
        wacc = (wacc as u128 * w_root as u128 % p as u128) as u64;
        iwacc = (iwacc as u128 * w_inv as u128 % p as u128) as u64;
    }
    for j in 0..N / 2 {
        pt.iwp2[j] = pt.iwp[(2 * j) % N];
        pt.iwp3[j] = pt.iwp[(3 * j) % N];
    }
    pt
}

/// q^{-1} mod 2^64 via Newton's iteration (q odd). 1 -> 2 -> ... -> 64 correct bits.
#[inline(always)]
fn inv_2_64(q: u64) -> u64 {
    let mut x = 1u64;
    for _ in 0..6 {
        x = x.wrapping_mul(2u64.wrapping_sub(q.wrapping_mul(x)));
    }
    x
}

/// Plantard constant for multiplying by `c` modulo `p`: `(c * (-2^64 mod p) mod p) *
/// p^{-1} mod 2^64`. With it, `plantard_lv(x, .)` returns `x*c mod p` in [0,2p) for any
/// non-negative `x < 8p`, in two multiplies (see `plantard_lv`).
#[inline(always)]
fn plantard_const(c: u64, p: u64) -> u64 {
    let r2 = ((1u128 << 64) % p as u128) as u64; // 2^64 mod p
    let neg_r2 = (p - r2) % p; // -2^64 mod p
    let beff = (c as u128 * neg_r2 as u128 % p as u128) as u64;
    (beff as u128 * inv_2_64(p) as u128 & ((1u128 << 64) - 1)) as u64
}

/// Forward stage Plantard twiddle constants (w^e, w^{2e}, w^{3e}), each broadcast to
/// both lanes. The forward pairs operands a/b, which share the twiddle, so from a u64
/// table `L::splat(*ptr)` lowers to a single `v128.load64_splat` — the load and
/// broadcast fuse into one instruction, with no separate splat op.
#[inline(always)]
unsafe fn twiddles3_splat(wp: &[u64; N], e: usize) -> (L, L, L) {
    (
        L::splat(*wp.get_unchecked(e)),
        L::splat(*wp.get_unchecked(2 * e)),
        L::splat(*wp.get_unchecked(3 * e)),
    )
}

/// One radix-4 DIF butterfly on four lane-vectors (lazy: in [0,2p), out [0,2p)).
/// `triv` skips the (unit) stage twiddles. Both operands ride the two lanes; all
/// twiddles arrive already broadcast (the 4th-root `icv`/`icsv` is hoisted by the
/// caller, the stage twiddles come from `twiddles3_splat`).
#[allow(clippy::too_many_arguments)]
#[inline(always)]
unsafe fn r4_lazy_l(
    a: L, b: L, c: L, d: L, pv: L, p2v: L, cav: L, icp: L, triv: bool,
    t1p: L, t2p: L, t3p: L,
) -> (L, L, L, L) {
    let s0 = red2p_l(a.add(c), p2v);
    let s2 = red2p_l(b.add(d), p2v);
    // s1 feeds only Plantard multiplies (non-triv) which tolerate any non-negative
    // value < 8p, so it stays lazy in [0,4p). The triv path red2p's the outputs, so it
    // needs the reduced [0,2p) form.
    let s1 = if triv { red2p_l(a.add(p2v).sub(c), p2v) } else { a.add(p2v).sub(c) };
    let s3 = b.add(p2v).sub(d); // in [0,4p); feeds only the lazy Plantard
    let is3 = plantard_lv(s3, icp, pv, cav);
    let y0 = red2p_l(s0.add(s2), p2v);
    let y2 = s0.add(p2v).sub(s2);
    let y1 = s1.add(is3);
    let y3 = s1.add(p2v).sub(is3);
    if triv {
        (y0, red2p_l(y1, p2v), red2p_l(y2, p2v), red2p_l(y3, p2v))
    } else {
        (
            y0,
            plantard_lv(y1, t1p, pv, cav),
            plantard_lv(y2, t2p, pv, cav),
            plantard_lv(y3, t3p, pv, cav),
        )
    }
}

/// Forward radix-4 DIF of both operands in lockstep (lanes), through the middle
/// stages only (the psi pre-weight is folded into the first stage's load). The
/// last two stages are completed in the fused `boundary` pass.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(target_arch = "wasm32", target_feature(enable = "simd128"))]
unsafe fn dif4_2_simd(
    a: &[u32; N], b: &[u32; N], xab: &mut [L; N], psip: &[u64; N],
    wp: &[u64; N], icp: u64, p: u64,
) {
    let pv = L::splat(p);
    let p2v = L::splat(p << 1);
    let cav = L::splat((1u64 << 32) + 1);
    let icpv = L::splat(icp);
    // First stage (half-block N/4) with the psi pre-weight folded into the load.
    let len0 = N / 4;
    let mut j = 0;
    while j < len0 {
        let e = j; // step = 1
        let (t1p, t2p, t3p) = twiddles3_splat(wp, e);
        let pw = |idx: usize| -> L {
            let v = L::new(*a.get_unchecked(idx) as u64, *b.get_unchecked(idx) as u64);
            plantard_lv(v, L::splat(*psip.get_unchecked(idx)), pv, cav)
        };
        let (y0, y1, y2, y3) = r4_lazy_l(
            pw(j), pw(j + len0), pw(j + 2 * len0), pw(j + 3 * len0), pv, p2v, cav, icpv,
            e == 0, t1p, t2p, t3p,
        );
        *xab.get_unchecked_mut(j) = y0;
        *xab.get_unchecked_mut(j + len0) = y1;
        *xab.get_unchecked_mut(j + 2 * len0) = y2;
        *xab.get_unchecked_mut(j + 3 * len0) = y3;
        j += 1;
    }
    // Remaining strided stages (half-blocks 64, 16) on xab. The stage twiddle
    // depends only on `e` (the butterfly column), not on the block `i`, so the
    // butterfly loop is the OUTER loop: each twiddle vector is loaded once and the
    // inner block loop reuses it from registers (instead of reloading per block).
    let mut len = N / 16;
    while len >= 16 {
        let step = N / (4 * len);
        let mut e = 0usize;
        let mut jj = 0;
        while jj < len {
            let (t1p, t2p, t3p) = twiddles3_splat(wp, e);
            let triv = e == 0;
            let mut i = 0;
            while i < N {
                let base = i + jj;
                let (y0, y1, y2, y3) = r4_lazy_l(
                    *xab.get_unchecked(base),
                    *xab.get_unchecked(base + len),
                    *xab.get_unchecked(base + 2 * len),
                    *xab.get_unchecked(base + 3 * len),
                    pv, p2v, cav, icpv, triv, t1p, t2p, t3p,
                );
                *xab.get_unchecked_mut(base) = y0;
                *xab.get_unchecked_mut(base + len) = y1;
                *xab.get_unchecked_mut(base + 2 * len) = y2;
                *xab.get_unchecked_mut(base + 3 * len) = y3;
                i += 4 * len;
            }
            e += step;
            jj += 1;
        }
        len >>= 2;
    }
}

// ---- Contiguous 16-element tile sub-stages (used by the fused boundary pass) ----

#[inline(always)]
unsafe fn dif_l4_v(t: &mut [L; 16], wp: &[u64; N], cav: L, icpv: L, pv: L, p2v: L) {
    for g in 0..4 {
        let e = 64 * g;
        let (t1p, t2p, t3p) = twiddles3_splat(wp, e);
        let (y0, y1, y2, y3) = r4_lazy_l(
            *t.get_unchecked(g), *t.get_unchecked(g + 4), *t.get_unchecked(g + 8),
            *t.get_unchecked(g + 12), pv, p2v, cav, icpv, e == 0, t1p, t2p, t3p,
        );
        *t.get_unchecked_mut(g) = y0;
        *t.get_unchecked_mut(g + 4) = y1;
        *t.get_unchecked_mut(g + 8) = y2;
        *t.get_unchecked_mut(g + 12) = y3;
    }
}

#[inline(always)]
unsafe fn dif_l1_v(t: &mut [L; 16], cav: L, icpv: L, pv: L, p2v: L) {
    // Last forward stage (unit stage twiddles). Its outputs feed only the pointwise
    // Montgomery product, which tolerates inputs up to ~5.6p (K^2 p < 2^32 at the
    // 27-bit primes), so the four output reductions are skipped: outputs stay [0,4p).
    for h in 0..4 {
        let b4 = 4 * h;
        let a = *t.get_unchecked(b4);
        let b = *t.get_unchecked(b4 + 1);
        let c = *t.get_unchecked(b4 + 2);
        let d = *t.get_unchecked(b4 + 3);
        let s0 = red2p_l(a.add(c), p2v);
        let s2 = red2p_l(b.add(d), p2v);
        let s1 = red2p_l(a.add(p2v).sub(c), p2v);
        let s3 = b.add(p2v).sub(d); // [0,4p); feeds the 4th-root Plantard
        let is3 = plantard_lv(s3, icpv, pv, cav);
        *t.get_unchecked_mut(b4) = s0.add(s2); // [0,4p)
        *t.get_unchecked_mut(b4 + 1) = s1.add(is3); // [0,4p)
        *t.get_unchecked_mut(b4 + 2) = s0.add(p2v).sub(s2); // [0,4p)
        *t.get_unchecked_mut(b4 + 3) = s1.add(p2v).sub(is3); // [0,4p)
    }
}

/// Scalar Plantard multiply by a precomputed constant (single value), used by the
/// scalar inverse first DIT sub-stage. Result in [0,2p) for any non-negative x < 8p.
#[inline(always)]
fn plantard_s(x: u64, bp: u64, p: u64) -> u64 {
    let h = ((x.wrapping_mul(bp) as i64) >> 32) as u64; // signed high 32
    (h.wrapping_add((1u64 << 32) + 1).wrapping_mul(p)) >> 32
}

/// First inverse sub-stage (trivial twiddles) specialized for inputs already in
/// [0,2p) (the Montgomery pointwise output), so the per-input reductions are skipped.
#[inline(always)]
unsafe fn dit_l1_in2p(t: &mut [u64; 16], jcp: u64, p: u64, p2: u64) {
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
        let js3 = plantard_s(s3, jcp, p);
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
unsafe fn dit_l4_v(t: &mut [u64; 16], iwp: &[u64; N], cav: L, jcpv: L, pv: L, p2v: L) {
    let tp = t.as_mut_ptr();
    let mut g = 0;
    while g < 4 {
        let (t1p, t2p, t3p) = twiddles3_l(iwp, 64 * g, 64 * (g + 1));
        let a = L::load(tp.add(g));
        let b = L::load(tp.add(g + 4));
        let c = L::load(tp.add(g + 8));
        let d = L::load(tp.add(g + 12));
        let (o0, o1, o2, o3) =
            r4_lazy_dit_l(a, b, c, d, pv, p2v, cav, jcpv, t1p, t2p, t3p);
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
    x: &mut [u64; N], iwp: &[u64; N], iwp2: &[u64; N / 2], iwp3: &[u64; N / 2], jcp: u64,
    ipsip: &[u64; N], p: u64,
) {
    let pv = L::splat(p);
    let p2v = L::splat(p << 1);
    let p4v = L::splat(p << 2);
    let cav = L::splat((1u64 << 32) + 1);
    let jcpv = L::splat(jcp);
    let xp = x.as_mut_ptr();
    // Butterfly-outer loop: the per-lane twiddles depend only on the column `j`, not
    // the block `i`, so they are loaded once and reused across the inner block loop
    // from registers (instead of reloading per block).
    let mut len = 16;
    while len < N / 4 {
        let step = N / (4 * len);
        let mut j = 0;
        while j < len {
            let e0 = j * step;
            let e1 = e0 + step;
            let (t1p, t2p, t3p) = twiddles3_l(iwp, e0, e1);
            let mut i = 0;
            while i < N {
                let base = i + j;
                let a = L::load(xp.add(base));
                let b = L::load(xp.add(base + len));
                let c = L::load(xp.add(base + 2 * len));
                let d = L::load(xp.add(base + 3 * len));
                let (o0, o1, o2, o3) =
                    r4_lazy_dit_l(a, b, c, d, pv, p2v, cav, jcpv, t1p, t2p, t3p);
                L::store(xp.add(base), o0);
                L::store(xp.add(base + len), o1);
                L::store(xp.add(base + 2 * len), o2);
                L::store(xp.add(base + 3 * len), o3);
                i += 4 * len;
            }
            j += 2;
        }
        len <<= 2;
    }
    // Final stage (half-block 256, step=1, e=j) with the psi^{-1}*N^{-1} post-weight
    // folded into the output store (no standalone post-weight pass).
    let mut j = 0usize;
    let iwp2p = iwp2.as_ptr();
    let iwp3p = iwp3.as_ptr();
    let ipsipp = ipsip.as_ptr();
    while j < N / 4 {
        // step=1, so the pair (j, j+1) has adjacent twiddle exponents: t1 is iwp[j..],
        // t2 is iwp2[j..] = iwp[2j],iwp[2j+2], t3 is iwp3[j..] = iwp[3j],iwp[3j+3] — each
        // a single contiguous v128 load instead of two scattered scalar loads.
        let (t1p, t2p, t3p) = (L::load(iwp.as_ptr().add(j)), L::load(iwp2p.add(j)), L::load(iwp3p.add(j)));
        let a = L::load(xp.add(j));
        let b = L::load(xp.add(j + N / 4));
        let c = L::load(xp.add(j + N / 2));
        let d = L::load(xp.add(j + 3 * N / 4));
        let (o0, o1, o2, o3) =
            r4_lazy_dit_l_final(a, b, c, d, pv, p2v, p4v, cav, jcpv, t1p, t2p, t3p);
        // Post-weight (Plantard with conditional subtract -> [0,p)) per lane position.
        // The pair (pos, pos+1) is adjacent in ipsip, so a single contiguous load.
        let pw = |o: L, pos: usize| -> L {
            redp_l(plantard_lv(o, L::load(ipsipp.add(pos)), pv, cav), pv)
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
    let cav = L::splat((1u64 << 32) + 1);
    let icfpv = L::splat(t.wp[N / 4]); // forward 4th root I, Plantard const (broadcast)
    let jcp = t.iwp[N / 4]; // inverse 4th root J, Plantard const
    let jcpv = L::splat(jcp);
    let pinvv = L::splat(t.pinv as u64);
    let maskv = L::splat(0xffff_ffff);
    let mut i = 0;
    while i < N {
        let mut ta = [L::splat(0); 16];
        for k in 0..16 {
            *ta.get_unchecked_mut(k) = *xab.get_unchecked(i + k);
        }
        dif_l4_v(&mut ta, &t.wp, cav, icfpv, pv, p2v);
        dif_l1_v(&mut ta, cav, icfpv, pv, p2v);
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
        dit_l1_in2p(&mut tc, jcp, p, p2);
        dit_l4_v(&mut tc, &t.iwp, cav, jcpv, pv, p2v);
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
        dit4_rest(&mut x, &t.iwp, &t.iwp2, &t.iwp3, t.iwp[N / 4], &t.ipsip, t.p); // rest of inverse (SIMD)
    }
    x
}

/// Forward transform of both operands (SIMD lockstep) then the fused boundary pass.
#[cfg_attr(target_arch = "wasm32", target_feature(enable = "simd128"))]
unsafe fn fwd_boundary(t: &PrimeTables, a: &[u32; N], b: &[u32; N], out: &mut [u64; N]) {
    let mut xab = [L::splat(0); N];
    dif4_2_simd(a, b, &mut xab, &t.psip, &t.wp, t.wp[N / 4], t.p);
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
        inv_p0_mod_p1_p: plantard_const(inv01, P1),
        p0_mod_p2_p: plantard_const(p0m2, P2),
        inv_m01_mod_p2_p: plantard_const(invm01, P2),
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
    let cav = L::splat((1u64 << 32) + 1);
    let inv01_v = L::splat(plan.inv_p0_mod_p1_p);
    let p0m2_v = L::splat(plan.p0_mod_p2_p);
    let invm01_v = L::splat(plan.inv_m01_mod_p2_p);
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
        let v1 = redp_l(plantard_lv(t1, inv01_v, p1v, cav), p1v);
        // w = (v0 + P0*v1) mod P2 kept lazy; term stays in [0,2*P2) (it only feeds w,
        // which the 3*P2 bias on t2 absorbs), so its reduction is dropped.
        let term = plantard_lv(v1, p0m2_v, p2v, cav); // [0, 2*P2)
        let w = v0.add(term); // < P0 + 2*P2 < 3*P2
        let t2 = L::load(r2p.add(j)).add(p2_3v).sub(w); // (0, 4*P2)
        let v2 = redp_l(plantard_lv(t2, invm01_v, p2v, cav), p2v);
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
