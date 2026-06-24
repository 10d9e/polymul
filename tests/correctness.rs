//! Correctness gate. FROZEN — do not edit as part of autoresearch.
//!
//! These tests use synthetic inputs (NOT the scored fixture corpus) so candidates
//! cannot pass by overfitting. Any algorithm change must match the reference oracle.

use polymul::algorithm::{plan_new, poly_mul};
use polymul::harness::reference;

fn assert_eq_poly(got: &[u32; 1024], expect: &[u32; 1024]) {
    assert_eq!(got, expect, "polynomial mismatch");
}

fn mul(a: &[u32; 1024], b: &[u32; 1024]) -> [u32; 1024] {
    let mut plan = plan_new();
    poly_mul(&mut plan, a, b)
}

fn ref_mul(a: &[u32; 1024], b: &[u32; 1024]) -> [u32; 1024] {
    reference::poly_mul(a, b)
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn poly(&mut self) -> [u32; 1024] {
        let mut out = [0u32; 1024];
        for c in &mut out {
            *c = self.next() as u32;
        }
        out
    }
}

#[test]
fn zero_polynomial() {
    let zero = [0u32; 1024];
    let mut rng = Rng(0xDEAD_BEEF_CAFE_BABE);
    let a = rng.poly();
    assert_eq_poly(&mul(&a, &zero), &zero);
    assert_eq_poly(&mul(&zero, &a), &zero);
}

#[test]
fn identity_delta() {
    let mut delta = [0u32; 1024];
    delta[0] = 1;
    let mut rng = Rng(0x1234_5678_9ABC_DEF0);
    let a = rng.poly();
    assert_eq_poly(&mul(&a, &delta), &a);
    assert_eq_poly(&mul(&delta, &a), &a);
}

#[test]
fn sparse_single_coeff() {
    for idx in [0, 1, 511, 1023] {
        let mut a = [0u32; 1024];
        a[idx] = 42;
        let mut b = [0u32; 1024];
        b[idx] = 7;
        let got = mul(&a, &b);
        let expect = ref_mul(&a, &b);
        assert_eq_poly(&got, &expect);
    }
}

#[test]
fn boundary_coefficients() {
    let mut a = [0u32; 1024];
    let mut b = [0u32; 1024];
    a[0] = u32::MAX;
    b[0] = 1;
    a[512] = 1;
    b[512] = u32::MAX;
    assert_eq_poly(&mul(&a, &b), &ref_mul(&a, &b));
}

#[test]
fn commutativity() {
    let mut rng = Rng(0xFEED_FACE_00C0_FFEE);
    for _ in 0..8 {
        let a = rng.poly();
        let b = rng.poly();
        assert_eq_poly(&mul(&a, &b), &mul(&b, &a));
    }
}

#[test]
fn prng_vectors_distinct_from_corpus() {
    let mut rng = Rng(0xC0FFEE00_BAD_DECAF);
    for _ in 0..16 {
        let a = rng.poly();
        let b = rng.poly();
        assert_eq_poly(&mul(&a, &b), &ref_mul(&a, &b));
    }
}

#[test]
fn all_ones_times_two() {
    let a = [1u32; 1024];
    let b = [2u32; 1024];
    assert_eq_poly(&mul(&a, &b), &ref_mul(&a, &b));
}
