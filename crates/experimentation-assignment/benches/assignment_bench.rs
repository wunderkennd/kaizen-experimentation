use std::collections::HashMap;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use experimentation_assignment::config::Config;
use experimentation_assignment::service::AssignmentServiceImpl;
use experimentation_proto::experimentation::assignment::v1::RankedList;

const DEV_CONFIG: &str = include_str!("../../../dev/config.json");

fn bench_get_assignment(c: &mut Criterion) {
    let config = Config::from_json(DEV_CONFIG).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));
    let no_attrs = HashMap::new();

    c.bench_function("get_assignment_single", |b| {
        b.iter(|| {
            svc.assign(black_box("exp_dev_001"), black_box("user_42"), black_box(""), black_box(&no_attrs))
                .unwrap()
        })
    });

    c.bench_function("get_assignment_1000_users", |b| {
        b.iter(|| {
            for i in 0..1000 {
                let user_id = format!("user_{i}");
                svc.assign(black_box("exp_dev_001"), black_box(&user_id), black_box(""), black_box(&no_attrs))
                    .unwrap();
            }
        })
    });

    c.bench_function("get_assignment_session_level", |b| {
        b.iter(|| {
            svc.assign(black_box("exp_dev_003"), black_box("user_42"), black_box("session_42"), black_box(&no_attrs))
                .unwrap()
        })
    });

    c.bench_function("get_assignment_session_1000", |b| {
        b.iter(|| {
            for i in 0..1000 {
                let session_id = format!("session_{i}");
                svc.assign(black_box("exp_dev_003"), black_box("user_42"), black_box(&session_id), black_box(&no_attrs))
                    .unwrap();
            }
        })
    });
}

fn make_interleave_lists(n: usize) -> HashMap<String, RankedList> {
    let mut m = HashMap::new();
    m.insert(
        "algo_a".to_string(),
        RankedList {
            item_ids: (0..n).map(|i| format!("a_item_{i}")).collect(),
        },
    );
    m.insert(
        "algo_b".to_string(),
        RankedList {
            item_ids: (0..n).map(|i| format!("b_item_{i}")).collect(),
        },
    );
    m
}

fn bench_get_interleaved_list(c: &mut Criterion) {
    let config = Config::from_json(DEV_CONFIG).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let lists_10 = make_interleave_lists(10);
    c.bench_function("get_interleaved_list_10_items", |b| {
        b.iter(|| {
            svc.interleave(
                black_box("exp_dev_004"),
                black_box("user_42"),
                black_box(&lists_10),
            )
            .unwrap()
        })
    });

    let lists_100 = make_interleave_lists(100);
    c.bench_function("get_interleaved_list_100_items", |b| {
        b.iter(|| {
            svc.interleave(
                black_box("exp_dev_004"),
                black_box("user_42"),
                black_box(&lists_100),
            )
            .unwrap()
        })
    });
}

fn bench_optimized_interleave(c: &mut Criterion) {
    let config = Config::from_json(DEV_CONFIG).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let lists_10 = make_interleave_lists(10);
    c.bench_function("get_optimized_interleave_10_items", |b| {
        b.iter(|| {
            svc.interleave(
                black_box("exp_dev_006"),
                black_box("user_42"),
                black_box(&lists_10),
            )
            .unwrap()
        })
    });

    let lists_50 = make_interleave_lists(50);
    c.bench_function("get_optimized_interleave_50_items", |b| {
        b.iter(|| {
            svc.interleave(
                black_box("exp_dev_006"),
                black_box("user_42"),
                black_box(&lists_50),
            )
            .unwrap()
        })
    });
}

fn make_multileave_lists(n: usize) -> HashMap<String, RankedList> {
    let mut m = HashMap::new();
    for (i, algo) in ["algo_x", "algo_y", "algo_z"].iter().enumerate() {
        m.insert(
            algo.to_string(),
            RankedList {
                item_ids: (0..n).map(|j| format!("{algo}_item_{}_{j}", i)).collect(),
            },
        );
    }
    m
}

fn bench_multileave(c: &mut Criterion) {
    let config = Config::from_json(DEV_CONFIG).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let lists_10 = make_multileave_lists(10);
    c.bench_function("get_multileave_3_algos_10_items", |b| {
        b.iter(|| {
            svc.interleave(
                black_box("exp_dev_007"),
                black_box("user_42"),
                black_box(&lists_10),
            )
            .unwrap()
        })
    });

    let lists_50 = make_multileave_lists(50);
    c.bench_function("get_multileave_3_algos_50_items", |b| {
        b.iter(|| {
            svc.interleave(
                black_box("exp_dev_007"),
                black_box("user_42"),
                black_box(&lists_50),
            )
            .unwrap()
        })
    });
}

fn bench_bandit_assignment(c: &mut Criterion) {
    let config = Config::from_json(DEV_CONFIG).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));
    let no_attrs = HashMap::new();

    c.bench_function("get_assignment_mab_single", |b| {
        b.iter(|| {
            svc.assign(
                black_box("exp_dev_005"),
                black_box("user_42"),
                black_box(""),
                black_box(&no_attrs),
            )
            .unwrap()
        })
    });

    c.bench_function("get_assignment_mab_1000_users", |b| {
        b.iter(|| {
            for i in 0..1000 {
                let user_id = format!("user_{i}");
                svc.assign(
                    black_box("exp_dev_005"),
                    black_box(&user_id),
                    black_box(""),
                    black_box(&no_attrs),
                )
                .unwrap();
            }
        })
    });
}

criterion_group!(
    benches,
    bench_get_assignment,
    bench_get_interleaved_list,
    bench_optimized_interleave,
    bench_multileave,
    bench_bandit_assignment,
);
criterion_main!(benches);
