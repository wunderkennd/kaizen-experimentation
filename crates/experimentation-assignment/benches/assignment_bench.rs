use std::collections::HashMap;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use experimentation_assignment::config::Config;
use experimentation_assignment::service::AssignmentServiceImpl;

const DEV_CONFIG: &str = include_str!("../../../dev/config.json");

fn bench_get_assignment(c: &mut Criterion) {
    let config = Config::from_json(DEV_CONFIG).unwrap();
    let svc = AssignmentServiceImpl::new(Arc::new(config));
    let no_attrs = HashMap::new();

    c.bench_function("get_assignment_single", |b| {
        b.iter(|| {
            svc.assign(black_box("exp_dev_001"), black_box("user_42"), black_box(&no_attrs))
                .unwrap()
        })
    });

    c.bench_function("get_assignment_1000_users", |b| {
        b.iter(|| {
            for i in 0..1000 {
                let user_id = format!("user_{i}");
                svc.assign(black_box("exp_dev_001"), black_box(&user_id), black_box(&no_attrs))
                    .unwrap();
            }
        })
    });
}

criterion_group!(benches, bench_get_assignment);
criterion_main!(benches);
