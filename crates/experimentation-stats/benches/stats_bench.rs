//! Benchmarks for statistical methods.
//! Run: cargo bench --package experimentation-stats

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use experimentation_stats::ttest::welch_ttest;

fn bench_welch_ttest(c: &mut Criterion) {
    let control: Vec<f64> = (0..10_000).map(|i| (i as f64) * 0.1).collect();
    let treatment: Vec<f64> = (0..10_000).map(|i| (i as f64) * 0.1 + 0.5).collect();

    c.bench_function("welch_ttest_10k", |b| {
        b.iter(|| welch_ttest(black_box(&control), black_box(&treatment), 0.05))
    });
}

criterion_group!(benches, bench_welch_ttest);
criterion_main!(benches);
