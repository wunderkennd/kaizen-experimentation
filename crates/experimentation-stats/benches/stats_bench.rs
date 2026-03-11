//! Benchmarks for statistical methods.
//! Run: cargo bench --package experimentation-stats

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use experimentation_stats::cuped::cuped_adjust;
use experimentation_stats::sequential::{gst_boundaries, msprt_normal, SpendingFunction};
use experimentation_stats::srm::srm_check;
use experimentation_stats::ttest::welch_ttest;
use std::collections::HashMap;

fn bench_welch_ttest(c: &mut Criterion) {
    let control: Vec<f64> = (0..10_000).map(|i| (i as f64) * 0.1).collect();
    let treatment: Vec<f64> = (0..10_000).map(|i| (i as f64) * 0.1 + 0.5).collect();

    c.bench_function("welch_ttest_10k", |b| {
        b.iter(|| welch_ttest(black_box(&control), black_box(&treatment), 0.05))
    });
}

fn bench_srm_check(c: &mut Criterion) {
    let mut observed = HashMap::new();
    let mut expected = HashMap::new();
    for i in 0..5 {
        let key = format!("variant_{i}");
        observed.insert(key.clone(), 2000u64);
        expected.insert(key, 0.2f64);
    }

    c.bench_function("srm_check_10k", |b| {
        b.iter(|| srm_check(black_box(&observed), black_box(&expected), 0.01))
    });
}

fn bench_msprt_normal(c: &mut Criterion) {
    c.bench_function("msprt_normal", |b| {
        b.iter(|| {
            msprt_normal(
                black_box(2.1),
                black_box(5000.0),
                black_box(1.0),
                black_box(0.01),
                black_box(0.05),
            )
        })
    });
}

fn bench_gst_boundaries(c: &mut Criterion) {
    c.bench_function("gst_boundaries_5_obf", |b| {
        b.iter(|| {
            gst_boundaries(
                black_box(5),
                black_box(0.05),
                black_box(SpendingFunction::OBrienFleming),
            )
        })
    });
}

fn bench_cuped_adjustment(c: &mut Criterion) {
    let control_y: Vec<f64> = (0..10_000).map(|i| (i as f64) * 0.01 + 5.0).collect();
    let treatment_y: Vec<f64> = (0..10_000).map(|i| (i as f64) * 0.01 + 5.3).collect();
    let control_x: Vec<f64> = (0..10_000).map(|i| (i as f64) * 0.01 + 4.8).collect();
    let treatment_x: Vec<f64> = (0..10_000).map(|i| (i as f64) * 0.01 + 4.9).collect();

    c.bench_function("cuped_adjustment_10k", |b| {
        b.iter(|| {
            cuped_adjust(
                black_box(&control_y),
                black_box(&treatment_y),
                black_box(&control_x),
                black_box(&treatment_x),
                black_box(0.05),
            )
        })
    });
}

criterion_group!(
    benches,
    bench_welch_ttest,
    bench_srm_check,
    bench_msprt_normal,
    bench_gst_boundaries,
    bench_cuped_adjustment
);
criterion_main!(benches);
