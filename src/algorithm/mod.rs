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
// Radix-8 twiddle tables are only indexed at [q8 + j] for q8 in {16, 128} and
// j < q8, i.e. indices below 256, so they are sized accordingly to keep `Plan`
// small enough for the wasm meter's linear memory.
const R8N: usize = 256;

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
    // Forward radix-8 (three fused DIF stages) twiddles, indexed [q8 + j] where q8
    // is the eighth-block size. ra{0..3} = w_{8q}^{j + k*q}, rb0 = w_{4q}^j,
    // rb1 = w_{4q}^{j+q}, rc0 = w_{2q}^j.
    ra0: [u64; R8N],
    ra1: [u64; R8N],
    ra2: [u64; R8N],
    ra3: [u64; R8N],
    rb0: [u64; R8N],
    rb1: [u64; R8N],
    rc0: [u64; R8N],
    // Inverse radix-8 (three fused DIT stages) twiddles, indexed [q8 + j].
    ica0: [u64; R8N],
    ica1: [u64; R8N],
    ica2: [u64; R8N],
    ica3: [u64; R8N],
    icb0: [u64; R8N],
    icb1: [u64; R8N],
    icc0: [u64; R8N],
    // Powers w16^0..w16^7 of the 16th root of unity (forward), used by the fused
    // radix-16 last pass (constant twiddles for stages L = 8,4,2,1).
    w16: [u64; 8],
    // Powers w16^{-0}..w16^{-7} (inverse), for the fused radix-16 inverse first pass.
    iw16: [u64; 8],
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

    // Forward radix-8 twiddles for the first two passes (q8 = N/8, N/64).
    let mut ra0 = [0u64; R8N];
    let mut ra1 = [0u64; R8N];
    let mut ra2 = [0u64; R8N];
    let mut ra3 = [0u64; R8N];
    let mut rb0 = [0u64; R8N];
    let mut rb1 = [0u64; R8N];
    let mut rc0 = [0u64; R8N];
    let mut q8 = N / 8;
    loop {
        let w8 = modpow(w_root, (N / (8 * q8)) as u64, p);
        let w4 = modpow(w_root, (N / (4 * q8)) as u64, p);
        let w2 = modpow(w_root, (N / (2 * q8)) as u64, p);
        let w8q1 = modpow(w8, q8 as u64, p); // w8^{q8}
        let w8q2 = modpow(w8, (2 * q8) as u64, p);
        let w8q3 = modpow(w8, (3 * q8) as u64, p);
        let w4q1 = modpow(w4, q8 as u64, p); // w4^{q8}
        let mut a = 1u64; // w8^j
        let mut b = 1u64; // w4^j
        let mut c = 1u64; // w2^j
        for j in 0..q8 {
            ra0[q8 + j] = a;
            ra1[q8 + j] = (a as u128 * w8q1 as u128 % p as u128) as u64;
            ra2[q8 + j] = (a as u128 * w8q2 as u128 % p as u128) as u64;
            ra3[q8 + j] = (a as u128 * w8q3 as u128 % p as u128) as u64;
            rb0[q8 + j] = b;
            rb1[q8 + j] = (b as u128 * w4q1 as u128 % p as u128) as u64;
            rc0[q8 + j] = c;
            a = (a as u128 * w8 as u128 % p as u128) as u64;
            b = (b as u128 * w4 as u128 % p as u128) as u64;
            c = (c as u128 * w2 as u128 % p as u128) as u64;
        }
        if q8 == N / 64 {
            break;
        }
        q8 /= 8;
    }

    // Inverse radix-8 twiddles for the last two inverse passes (q8 = N/64, N/8).
    let mut ica0 = [0u64; R8N];
    let mut ica1 = [0u64; R8N];
    let mut ica2 = [0u64; R8N];
    let mut ica3 = [0u64; R8N];
    let mut icb0 = [0u64; R8N];
    let mut icb1 = [0u64; R8N];
    let mut icc0 = [0u64; R8N];
    let mut q8 = N / 64;
    loop {
        let w8 = modpow(wi_root, (N / (8 * q8)) as u64, p);
        let w4 = modpow(wi_root, (N / (4 * q8)) as u64, p);
        let w2 = modpow(wi_root, (N / (2 * q8)) as u64, p);
        let w8q1 = modpow(w8, q8 as u64, p);
        let w8q2 = modpow(w8, (2 * q8) as u64, p);
        let w8q3 = modpow(w8, (3 * q8) as u64, p);
        let w4q1 = modpow(w4, q8 as u64, p);
        let mut a = 1u64;
        let mut b = 1u64;
        let mut c = 1u64;
        for j in 0..q8 {
            ica0[q8 + j] = a;
            ica1[q8 + j] = (a as u128 * w8q1 as u128 % p as u128) as u64;
            ica2[q8 + j] = (a as u128 * w8q2 as u128 % p as u128) as u64;
            ica3[q8 + j] = (a as u128 * w8q3 as u128 % p as u128) as u64;
            icb0[q8 + j] = b;
            icb1[q8 + j] = (b as u128 * w4q1 as u128 % p as u128) as u64;
            icc0[q8 + j] = c;
            a = (a as u128 * w8 as u128 % p as u128) as u64;
            b = (b as u128 * w4 as u128 % p as u128) as u64;
            c = (c as u128 * w2 as u128 % p as u128) as u64;
        }
        if q8 == N / 8 {
            break;
        }
        q8 *= 8;
    }

    // 16th-root powers for the fused radix-16 passes (forward and inverse).
    let w16r = modpow(w_root, (N / 16) as u64, p);
    let iw16r = modpow(wi_root, (N / 16) as u64, p);
    let mut w16 = [0u64; 8];
    let mut iw16 = [0u64; 8];
    let mut wacc = 1u64;
    let mut iwacc = 1u64;
    for k in 0..8 {
        w16[k] = wacc;
        iw16[k] = iwacc;
        wacc = (wacc as u128 * w16r as u128 % p as u128) as u64;
        iwacc = (iwacc as u128 * iw16r as u128 % p as u128) as u64;
    }

    PrimeTables {
        p, psi, ipsi,
        ra0, ra1, ra2, ra3, rb0, rb1, rc0,
        ica0, ica1, ica2, ica3, icb0, icb1, icc0,
        w16, iw16,
    }
}

/// Reduce the eight radix-8 DIF outputs (three fused radix-2 DIF stages) of the
/// values x0..x7 into the eight destination slots. `wr` reads x via the supplied
/// closure-free expressions; the lazy bounds match the comments.
macro_rules! r8_body {
    ($x0:expr,$x1:expr,$x2:expr,$x3:expr,$x4:expr,$x5:expr,$x6:expr,$x7:expr,
     $dst:expr,$i0:expr,$i1:expr,$i2:expr,$i3:expr,$i4:expr,$i5:expr,$i6:expr,$i7:expr,
     $ra0:expr,$ra1:expr,$ra2:expr,$ra3:expr,$rb0:expr,$rb1:expr,$rc0:expr,$p:expr) => {{
        let p = $p;
        // stage 1 (L = 4q): sums lazy in [0,2p), diffs reduced.
        let y0 = $x0 + $x4;
        let y1 = $x1 + $x5;
        let y2 = $x2 + $x6;
        let y3 = $x3 + $x7;
        let y4 = (($x0 + p - $x4) * $ra0) % p;
        let y5 = (($x1 + p - $x5) * $ra1) % p;
        let y6 = (($x2 + p - $x6) * $ra2) % p;
        let y7 = (($x3 + p - $x7) * $ra3) % p;
        // stage 2 (L = 2q).
        let z0 = y0 + y2;           // [0,4p)
        let z1 = y1 + y3;           // [0,4p)
        let z2 = ((y0 + 2 * p - y2) * $rb0) % p;
        let z3 = ((y1 + 2 * p - y3) * $rb1) % p;
        let z4 = y4 + y6;           // [0,2p)
        let z5 = y5 + y7;           // [0,2p)
        let z6 = ((y4 + p - y6) * $rb0) % p;
        let z7 = ((y5 + p - y7) * $rb1) % p;
        // stage 3 (L = q): write natural-positioned outputs.
        *$dst.get_unchecked_mut($i0) = (z0 + z1) % p;
        *$dst.get_unchecked_mut($i1) = ((z0 + 4 * p - z1) * $rc0) % p;
        *$dst.get_unchecked_mut($i2) = (z2 + z3) % p;
        *$dst.get_unchecked_mut($i3) = ((z2 + p - z3) * $rc0) % p;
        *$dst.get_unchecked_mut($i4) = (z4 + z5) % p;
        *$dst.get_unchecked_mut($i5) = ((z4 + 2 * p - z5) * $rc0) % p;
        *$dst.get_unchecked_mut($i6) = (z6 + z7) % p;
        *$dst.get_unchecked_mut($i7) = ((z6 + p - z7) * $rc0) % p;
    }};
}

/// Fused radix-8 DIF butterfly (three combined radix-2 DIF stages) in place.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn r8_dif(x: &mut [u64; N], i0: usize, i1: usize, i2: usize, i3: usize,
                 i4: usize, i5: usize, i6: usize, i7: usize,
                 ra0: u64, ra1: u64, ra2: u64, ra3: u64, rb0: u64, rb1: u64, rc0: u64, p: u64) {
    let x0 = *x.get_unchecked(i0);
    let x1 = *x.get_unchecked(i1);
    let x2 = *x.get_unchecked(i2);
    let x3 = *x.get_unchecked(i3);
    let x4 = *x.get_unchecked(i4);
    let x5 = *x.get_unchecked(i5);
    let x6 = *x.get_unchecked(i6);
    let x7 = *x.get_unchecked(i7);
    r8_body!(x0, x1, x2, x3, x4, x5, x6, x7, x, i0, i1, i2, i3, i4, i5, i6, i7,
             ra0, ra1, ra2, ra3, rb0, rb1, rc0, p);
}

/// Fused radix-8 DIF butterfly that folds in the psi pre-weight, reading raw u32
/// input and writing the u64 buffer.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn r8_dif_pre(src: &[u32; N], psi: &[u64; N], dst: &mut [u64; N],
                     i0: usize, i1: usize, i2: usize, i3: usize,
                     i4: usize, i5: usize, i6: usize, i7: usize,
                     ra0: u64, ra1: u64, ra2: u64, ra3: u64, rb0: u64, rb1: u64, rc0: u64, p: u64) {
    let x0 = (*src.get_unchecked(i0) as u64 * *psi.get_unchecked(i0)) % p;
    let x1 = (*src.get_unchecked(i1) as u64 * *psi.get_unchecked(i1)) % p;
    let x2 = (*src.get_unchecked(i2) as u64 * *psi.get_unchecked(i2)) % p;
    let x3 = (*src.get_unchecked(i3) as u64 * *psi.get_unchecked(i3)) % p;
    let x4 = (*src.get_unchecked(i4) as u64 * *psi.get_unchecked(i4)) % p;
    let x5 = (*src.get_unchecked(i5) as u64 * *psi.get_unchecked(i5)) % p;
    let x6 = (*src.get_unchecked(i6) as u64 * *psi.get_unchecked(i6)) % p;
    let x7 = (*src.get_unchecked(i7) as u64 * *psi.get_unchecked(i7)) % p;
    r8_body!(x0, x1, x2, x3, x4, x5, x6, x7, dst, i0, i1, i2, i3, i4, i5, i6, i7,
             ra0, ra1, ra2, ra3, rb0, rb1, rc0, p);
}

/// Fused radix-16 DIF butterfly (four combined radix-2 DIF stages, L = 8,4,2,1)
/// on the 16-block at `base`. Twiddles are the constant 16th-root powers (the last
/// stages do not depend on block position). Sums stay lazy across stages; the only
/// multiplications are on differences bounded by < 8p, so products stay < 8p*p
/// < 2^63 (the 16p values appear only in the final, multiply-free stage).
#[inline(always)]
unsafe fn r16_dif(x: &mut [u64; N], base: usize, w16: &[u64; 8], p: u64) {
    let mut t = [0u64; 16];
    let mut k = 0;
    while k < 16 {
        *t.get_unchecked_mut(k) = *x.get_unchecked(base + k);
        k += 1;
    }
    // Stages with half-size h = 8, 4, 2, 1. Twiddle for position k is w16^{(8/h)*k};
    // offset c = current value bound (a multiple of p) keeps the lazy diff positive.
    let mut h = 8usize;
    let mut c = p;
    loop {
        let tstep = 8 / h;
        let mut start = 0usize;
        while start < 16 {
            // kk = 0: twiddle w16^0 = 1, so the difference stays lazy (no multiply,
            // no % p); the final reduction handles it.
            let u0 = *t.get_unchecked(start);
            let v0 = *t.get_unchecked(start + h);
            *t.get_unchecked_mut(start) = u0 + v0;
            *t.get_unchecked_mut(start + h) = u0 + c - v0;
            let mut kk = 1usize;
            while kk < h {
                let u = *t.get_unchecked(start + kk);
                let v = *t.get_unchecked(start + kk + h);
                let tw = *w16.get_unchecked(tstep * kk);
                *t.get_unchecked_mut(start + kk) = u + v;
                *t.get_unchecked_mut(start + kk + h) = ((u + c - v) * tw) % p;
                kk += 1;
            }
            start += 2 * h;
        }
        c <<= 1;
        if h == 1 {
            break;
        }
        h >>= 1;
    }
    let mut k = 0;
    while k < 16 {
        *x.get_unchecked_mut(base + k) = *t.get_unchecked(k) % p;
        k += 1;
    }
}

/// Forward NTT of both multiply operands in lockstep. Two fused radix-8 passes
/// (the first folding in the psi pre-weight) then one fused radix-16 pass: 3 memory
/// passes covering all 10 radix-2 stages. The butterfly network is unchanged, so the
/// output is in base-2 bit-reversed order.
/// Natural-order input -> bit-reversed-order output.
#[inline(always)]
fn ntt_dif2(a: &[u32; N], b: &[u32; N], fa: &mut [u64; N], fb: &mut [u64; N], t: &PrimeTables) {
    let p = t.p;
    // Pass 1: radix-8 with psi pre-weight (q8 = N/8, single block).
    let q8 = N / 8;
    for j in 0..q8 {
        unsafe {
            let ra0 = *t.ra0.get_unchecked(q8 + j);
            let ra1 = *t.ra1.get_unchecked(q8 + j);
            let ra2 = *t.ra2.get_unchecked(q8 + j);
            let ra3 = *t.ra3.get_unchecked(q8 + j);
            let rb0 = *t.rb0.get_unchecked(q8 + j);
            let rb1 = *t.rb1.get_unchecked(q8 + j);
            let rc0 = *t.rc0.get_unchecked(q8 + j);
            let i1 = j + q8;
            let i2 = i1 + q8;
            let i3 = i2 + q8;
            let i4 = i3 + q8;
            let i5 = i4 + q8;
            let i6 = i5 + q8;
            let i7 = i6 + q8;
            r8_dif_pre(a, &t.psi, fa, j, i1, i2, i3, i4, i5, i6, i7,
                       ra0, ra1, ra2, ra3, rb0, rb1, rc0, p);
            r8_dif_pre(b, &t.psi, fb, j, i1, i2, i3, i4, i5, i6, i7,
                       ra0, ra1, ra2, ra3, rb0, rb1, rc0, p);
        }
    }
    // Pass 2: radix-8 (q8 = N/64).
    let q8 = N / 64;
    let mut start = 0usize;
    while start < N {
        for j in 0..q8 {
            unsafe {
                let ra0 = *t.ra0.get_unchecked(q8 + j);
                let ra1 = *t.ra1.get_unchecked(q8 + j);
                let ra2 = *t.ra2.get_unchecked(q8 + j);
                let ra3 = *t.ra3.get_unchecked(q8 + j);
                let rb0 = *t.rb0.get_unchecked(q8 + j);
                let rb1 = *t.rb1.get_unchecked(q8 + j);
                let rc0 = *t.rc0.get_unchecked(q8 + j);
                let i0 = start + j;
                let i1 = i0 + q8;
                let i2 = i1 + q8;
                let i3 = i2 + q8;
                let i4 = i3 + q8;
                let i5 = i4 + q8;
                let i6 = i5 + q8;
                let i7 = i6 + q8;
                r8_dif(fa, i0, i1, i2, i3, i4, i5, i6, i7,
                       ra0, ra1, ra2, ra3, rb0, rb1, rc0, p);
                r8_dif(fb, i0, i1, i2, i3, i4, i5, i6, i7,
                       ra0, ra1, ra2, ra3, rb0, rb1, rc0, p);
            }
        }
        start += 8 * q8;
    }
    // Final pass: one fused radix-16 DIF pass (stages L = 8,4,2,1) per 16-block.
    let mut start = 0usize;
    while start < N {
        unsafe {
            r16_dif(fa, start, &t.w16, p);
            r16_dif(fb, start, &t.w16, p);
        }
        start += 16;
    }
}

/// Fused radix-8 DIT butterfly (three combined radix-2 DIT stages) in place.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn r8_dit(x: &mut [u64; N], i0: usize, i1: usize, i2: usize, i3: usize,
                 i4: usize, i5: usize, i6: usize, i7: usize,
                 a0: u64, a1: u64, a2: u64, a3: u64, b0: u64, b1: u64, c0: u64, p: u64) {
    let x0 = *x.get_unchecked(i0);
    let x1 = *x.get_unchecked(i1);
    let x2 = *x.get_unchecked(i2);
    let x3 = *x.get_unchecked(i3);
    let x4 = *x.get_unchecked(i4);
    let x5 = *x.get_unchecked(i5);
    let x6 = *x.get_unchecked(i6);
    let x7 = *x.get_unchecked(i7);

    // stage 1 (L = q8): p* lazy in [0, 2p).
    let v1 = (x1 * c0) % p;
    let v3 = (x3 * c0) % p;
    let v5 = (x5 * c0) % p;
    let v7 = (x7 * c0) % p;
    let p0 = x0 + v1;
    let p1 = x0 + p - v1;
    let p2 = x2 + v3;
    let p3 = x2 + p - v3;
    let p4 = x4 + v5;
    let p5 = x4 + p - v5;
    let p6 = x6 + v7;
    let p7 = x6 + p - v7;
    // stage 2 (L = 2q8): q* lazy in [0, 3p).
    let w2 = (p2 * b0) % p;
    let w3 = (p3 * b1) % p;
    let w6 = (p6 * b0) % p;
    let w7 = (p7 * b1) % p;
    let q0 = p0 + w2;
    let q2 = p0 + p - w2;
    let q1 = p1 + w3;
    let q3 = p1 + p - w3;
    let q4 = p4 + w6;
    let q6 = p4 + p - w6;
    let q5 = p5 + w7;
    let q7 = p5 + p - w7;
    // stage 3 (L = 4q8).
    let u4 = (q4 * a0) % p;
    let u5 = (q5 * a1) % p;
    let u6 = (q6 * a2) % p;
    let u7 = (q7 * a3) % p;
    *x.get_unchecked_mut(i0) = (q0 + u4) % p;
    *x.get_unchecked_mut(i4) = (q0 + p - u4) % p;
    *x.get_unchecked_mut(i1) = (q1 + u5) % p;
    *x.get_unchecked_mut(i5) = (q1 + p - u5) % p;
    *x.get_unchecked_mut(i2) = (q2 + u6) % p;
    *x.get_unchecked_mut(i6) = (q2 + p - u6) % p;
    *x.get_unchecked_mut(i3) = (q3 + u7) % p;
    *x.get_unchecked_mut(i7) = (q3 + p - u7) % p;
}

/// Fused radix-8 DIT butterfly that folds the psi^{-j} * N^{-1} post-weight into
/// the eight output stores (each output then lands in natural order, reduced).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn r8_dit_post(x: &mut [u64; N], ipsi: &[u64; N],
                      i0: usize, i1: usize, i2: usize, i3: usize,
                      i4: usize, i5: usize, i6: usize, i7: usize,
                      a0: u64, a1: u64, a2: u64, a3: u64, b0: u64, b1: u64, c0: u64, p: u64) {
    let x0 = *x.get_unchecked(i0);
    let x1 = *x.get_unchecked(i1);
    let x2 = *x.get_unchecked(i2);
    let x3 = *x.get_unchecked(i3);
    let x4 = *x.get_unchecked(i4);
    let x5 = *x.get_unchecked(i5);
    let x6 = *x.get_unchecked(i6);
    let x7 = *x.get_unchecked(i7);

    let v1 = (x1 * c0) % p;
    let v3 = (x3 * c0) % p;
    let v5 = (x5 * c0) % p;
    let v7 = (x7 * c0) % p;
    let p0 = x0 + v1;
    let p1 = x0 + p - v1;
    let p2 = x2 + v3;
    let p3 = x2 + p - v3;
    let p4 = x4 + v5;
    let p5 = x4 + p - v5;
    let p6 = x6 + v7;
    let p7 = x6 + p - v7;
    let w2 = (p2 * b0) % p;
    let w3 = (p3 * b1) % p;
    let w6 = (p6 * b0) % p;
    let w7 = (p7 * b1) % p;
    let q0 = p0 + w2;
    let q2 = p0 + p - w2;
    let q1 = p1 + w3;
    let q3 = p1 + p - w3;
    let q4 = p4 + w6;
    let q6 = p4 + p - w6;
    let q5 = p5 + w7;
    let q7 = p5 + p - w7;
    let u4 = (q4 * a0) % p;
    let u5 = (q5 * a1) % p;
    let u6 = (q6 * a2) % p;
    let u7 = (q7 * a3) % p;
    *x.get_unchecked_mut(i0) = ((q0 + u4) * *ipsi.get_unchecked(i0)) % p;
    *x.get_unchecked_mut(i4) = ((q0 + p - u4) * *ipsi.get_unchecked(i4)) % p;
    *x.get_unchecked_mut(i1) = ((q1 + u5) * *ipsi.get_unchecked(i1)) % p;
    *x.get_unchecked_mut(i5) = ((q1 + p - u5) * *ipsi.get_unchecked(i5)) % p;
    *x.get_unchecked_mut(i2) = ((q2 + u6) * *ipsi.get_unchecked(i2)) % p;
    *x.get_unchecked_mut(i6) = ((q2 + p - u6) * *ipsi.get_unchecked(i6)) % p;
    *x.get_unchecked_mut(i3) = ((q3 + u7) * *ipsi.get_unchecked(i3)) % p;
    *x.get_unchecked_mut(i7) = ((q3 + p - u7) * *ipsi.get_unchecked(i7)) % p;
}

/// Fused radix-16 DIT first inverse pass (stages L = 1,2,4,8) over a 16-block,
/// folding in the pointwise product a[i] *= b[i]. Constant inverse-16th-root
/// twiddles. In DIT only the (reduced) upper operand is multiplied, so values grow
/// only linearly (< 5p) and every product stays < 4p*p < 2^62.
#[inline(always)]
unsafe fn r16_dit_pw(a: &mut [u64; N], b: &[u64; N], base: usize, iw16: &[u64; 8], p: u64) {
    let mut t = [0u64; 16];
    let mut k = 0;
    while k < 16 {
        *t.get_unchecked_mut(k) = (*a.get_unchecked(base + k) * *b.get_unchecked(base + k)) % p;
        k += 1;
    }
    // c = current value bound (doubles per stage). For kk = 0 the twiddle is 1, so
    // the upper operand stays lazy and needs no reduction.
    let mut h = 1usize;
    let mut c = p;
    loop {
        let tstep = 8 / h;
        let mut start = 0usize;
        while start < 16 {
            let u0 = *t.get_unchecked(start);
            let v0 = *t.get_unchecked(start + h);
            *t.get_unchecked_mut(start) = u0 + v0;
            *t.get_unchecked_mut(start + h) = u0 + c - v0;
            let mut kk = 1usize;
            while kk < h {
                let tw = *iw16.get_unchecked(tstep * kk);
                let u = *t.get_unchecked(start + kk);
                let v = (*t.get_unchecked(start + kk + h) * tw) % p;
                *t.get_unchecked_mut(start + kk) = u + v;
                *t.get_unchecked_mut(start + kk + h) = u + c - v;
                kk += 1;
            }
            start += 2 * h;
        }
        c <<= 1;
        if h == 8 {
            break;
        }
        h <<= 1;
    }
    let mut k = 0;
    while k < 16 {
        *a.get_unchecked_mut(base + k) = *t.get_unchecked(k) % p;
        k += 1;
    }
}

/// Inverse NTT: one fused radix-16 DIT pass (folding in the pointwise product),
/// then two fused radix-8 DIT passes (the last folding in the psi^{-j}*N^{-1}
/// post-weight) — 3 memory passes covering all 10 radix-2 stages.
/// Bit-reversed-order input -> natural-order output. Inverse of `ntt_dif2`'s
/// per-array transform.
#[inline(always)]
fn intt_dit(a: &mut [u64; N], b: &[u64; N], t: &PrimeTables) {
    let p = t.p;
    // First pass: one fused radix-16 DIT pass (stages L = 1,2,4,8), folding in the
    // pointwise product a[i] *= b[i].
    let mut start = 0usize;
    while start < N {
        unsafe {
            r16_dit_pw(a, b, start, &t.iw16, p);
        }
        start += 16;
    }

    // Pass 3 (q8 = N/64): radix-8 DIT (stages L = 16, 32, 64).
    {
        let q8 = N / 64;
        let mut start = 0usize;
        while start < N {
            for j in 0..q8 {
                unsafe {
                    let a0 = *t.ica0.get_unchecked(q8 + j);
                    let a1 = *t.ica1.get_unchecked(q8 + j);
                    let a2 = *t.ica2.get_unchecked(q8 + j);
                    let a3 = *t.ica3.get_unchecked(q8 + j);
                    let b0 = *t.icb0.get_unchecked(q8 + j);
                    let b1 = *t.icb1.get_unchecked(q8 + j);
                    let c0 = *t.icc0.get_unchecked(q8 + j);
                    let i0 = start + j;
                    let i1 = i0 + q8;
                    let i2 = i1 + q8;
                    let i3 = i2 + q8;
                    let i4 = i3 + q8;
                    let i5 = i4 + q8;
                    let i6 = i5 + q8;
                    let i7 = i6 + q8;
                    r8_dit(a, i0, i1, i2, i3, i4, i5, i6, i7, a0, a1, a2, a3, b0, b1, c0, p);
                }
            }
            start += 8 * q8;
        }
    }

    // Pass 4 (q8 = N/8, single block): radix-8 DIT folding in the post-weight.
    {
        let q8 = N / 8;
        for j in 0..q8 {
            unsafe {
                let a0 = *t.ica0.get_unchecked(q8 + j);
                let a1 = *t.ica1.get_unchecked(q8 + j);
                let a2 = *t.ica2.get_unchecked(q8 + j);
                let a3 = *t.ica3.get_unchecked(q8 + j);
                let b0 = *t.icb0.get_unchecked(q8 + j);
                let b1 = *t.icb1.get_unchecked(q8 + j);
                let c0 = *t.icc0.get_unchecked(q8 + j);
                let i1 = j + q8;
                let i2 = i1 + q8;
                let i3 = i2 + q8;
                let i4 = i3 + q8;
                let i5 = i4 + q8;
                let i6 = i5 + q8;
                let i7 = i6 + q8;
                r8_dit_post(a, &t.ipsi, j, i1, i2, i3, i4, i5, i6, i7,
                            a0, a1, a2, a3, b0, b1, c0, p);
            }
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
            // v1 = (r1 - v0) * inv(P0) mod P1.  v0 = r0[j] < P0 < P1 (so v0 % P1 == v0),
            // and t1 = r1 + p1 - v0 < 2*P1 feeds only `* inv01 % p1`, so it stays lazy.
            let t1 = *r1.get_unchecked(j) + p1 - v0;
            let v1 = (t1 * inv01) % p1;
            // w = v0 + (p0_mod_p2*v1 % p2) stays lazy in [0, P0+P2) < 3*P2; t2 =
            // r2 + 3*P2 - w stays lazy in (0, 4*P2) and feeds only `* inv_m01 % p2`.
            let w = v0 + p0_mod_p2 * v1 % p2;
            let t2 = *r2.get_unchecked(j) + 3 * p2 - w;
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
