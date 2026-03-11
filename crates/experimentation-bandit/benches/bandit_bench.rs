//! Benchmarks for bandit algorithm hot paths.
//! Run: cargo bench --package experimentation-bandit

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use experimentation_bandit::linucb::LinUcbPolicy;
use experimentation_bandit::policy::Policy;
use experimentation_bandit::thompson::ThompsonSamplingPolicy;
use std::collections::HashMap;

fn bench_thompson_select_arm(c: &mut Criterion) {
    let arm_ids: Vec<String> = (0..10).map(|i| format!("arm_{i}")).collect();
    let mut policy = ThompsonSamplingPolicy::new("bench-exp".into(), arm_ids);

    // Warm up with some rewards so posteriors are non-trivial
    for i in 0..100 {
        let arm = format!("arm_{}", i % 10);
        let reward = if i % 3 == 0 { 1.0 } else { 0.0 };
        policy.update(&arm, reward, None);
    }

    c.bench_function("thompson_select_arm_10", |b| {
        b.iter(|| policy.select_arm(black_box(None)))
    });
}

fn bench_thompson_update_reward(c: &mut Criterion) {
    let arm_ids: Vec<String> = (0..10).map(|i| format!("arm_{i}")).collect();
    let mut policy = ThompsonSamplingPolicy::new("bench-exp".into(), arm_ids);

    c.bench_function("thompson_update_reward", |b| {
        b.iter(|| {
            policy.update(black_box("arm_3"), black_box(1.0), None);
        })
    });
}

fn bench_linucb_select_arm(c: &mut Criterion) {
    let arm_ids: Vec<String> = (0..10).map(|i| format!("arm_{i}")).collect();
    let feature_keys: Vec<String> = (0..8).map(|i| format!("feat_{i}")).collect();
    let mut policy =
        LinUcbPolicy::new("bench-exp".into(), arm_ids, feature_keys.clone(), 1.0, 0.1);

    // Warm up with some context-reward pairs
    for i in 0..100 {
        let arm = format!("arm_{}", i % 10);
        let ctx: HashMap<String, f64> = feature_keys
            .iter()
            .enumerate()
            .map(|(j, k)| (k.clone(), ((i + j) as f64) * 0.1))
            .collect();
        let reward = if i % 4 == 0 { 1.0 } else { 0.2 };
        policy.update(&arm, reward, Some(&ctx));
    }

    let context: HashMap<String, f64> = feature_keys
        .iter()
        .enumerate()
        .map(|(j, k)| (k.clone(), (j as f64) * 0.5))
        .collect();

    c.bench_function("linucb_select_arm_10_d8", |b| {
        b.iter(|| policy.select_arm(black_box(Some(&context))))
    });
}

fn bench_linucb_update(c: &mut Criterion) {
    let arm_ids: Vec<String> = (0..10).map(|i| format!("arm_{i}")).collect();
    let feature_keys: Vec<String> = (0..8).map(|i| format!("feat_{i}")).collect();
    let mut policy =
        LinUcbPolicy::new("bench-exp".into(), arm_ids, feature_keys.clone(), 1.0, 0.1);

    let context: HashMap<String, f64> = feature_keys
        .iter()
        .enumerate()
        .map(|(j, k)| (k.clone(), (j as f64) * 0.5))
        .collect();

    c.bench_function("linucb_update_d8", |b| {
        b.iter(|| {
            policy.update(
                black_box("arm_2"),
                black_box(0.8),
                black_box(Some(&context)),
            );
        })
    });
}

criterion_group!(
    benches,
    bench_thompson_select_arm,
    bench_thompson_update_reward,
    bench_linucb_select_arm,
    bench_linucb_update
);
criterion_main!(benches);
