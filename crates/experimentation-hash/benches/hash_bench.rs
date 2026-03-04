use criterion::{criterion_group, criterion_main, Criterion};
use experimentation_hash::bucket;

fn bench_bucket(c: &mut Criterion) {
    c.bench_function("bucket_10k", |b| {
        b.iter(|| bucket("user_12345", "experiment_salt_abc", 10000))
    });
}

fn bench_batch_bucket(c: &mut Criterion) {
    c.bench_function("bucket_batch_1000", |b| {
        b.iter(|| {
            for i in 0..1000 {
                let _ = bucket(&format!("user_{i}"), "salt", 10000);
            }
        })
    });
}

criterion_group!(benches, bench_bucket, bench_batch_bucket);
criterion_main!(benches);
