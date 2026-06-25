//! Wall-clock timing benchmark for `poly_mul`, complementing the deterministic
//! WORK metric (`scripts/measure-complexity.sh`). WORK is the scoring metric;
//! this is an informational cross-check of real runtime on the host CPU.
//!
//! Run locally with `cargo bench`; CI runs it via `.github/workflows/bench.yml`.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use polymul::harness::fixtures::{self, NUM_PAIRS};
use polymul::{plan_new, poly_mul};

fn poly_mul_bench(c: &mut Criterion) {
    // The plan is precomputed once (as in real use and in the WORK metric).
    let mut plan = plan_new();
    let pairs: Vec<fixtures::Pair> = (0..NUM_PAIRS).map(fixtures::pair).collect();

    // One representative multiply — the core operation latency.
    let p = &pairs[0];
    c.bench_function("poly_mul/single", |b| {
        b.iter(|| black_box(poly_mul(&mut plan, black_box(&p.a), black_box(&p.b))));
    });

    // The full fixture corpus (the same 32 pairs the WORK metric scores).
    let mut group = c.benchmark_group("poly_mul/corpus");
    group.throughput(Throughput::Elements(NUM_PAIRS as u64));
    group.bench_function("32_pairs", |b| {
        b.iter(|| {
            let mut acc = 0u32;
            for p in &pairs {
                let out = poly_mul(&mut plan, &p.a, &p.b);
                acc ^= fixtures::checksum(&out);
            }
            black_box(acc)
        });
    });
    group.finish();
}

criterion_group!(benches, poly_mul_bench);
criterion_main!(benches);
