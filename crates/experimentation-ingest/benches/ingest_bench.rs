//! Benchmarks for the M2 ingestion hot path: validation + dedup + serialization.
//!
//! Run: `cargo bench -p experimentation-ingest`
//! Or via just: `just bench-crate experimentation-ingest`
//!
//! SLA target: p99 < 10ms for the full ingest path at 100K events/sec.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use prost::Message;

use experimentation_ingest::dedup::{DedupConfig, DedupMetrics, EventDedup};
use experimentation_ingest::validation;
use experimentation_proto::common::{
    ExposureEvent, MetricEvent, PlaybackMetrics, QoEEvent, RewardEvent,
};

// ═══════════════════════════════════════════════════════════════════════════
//  Helpers — build valid events for benchmarking
// ═══════════════════════════════════════════════════════════════════════════

fn now_proto() -> Option<prost_types::Timestamp> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    Some(prost_types::Timestamp {
        seconds: now.as_secs() as i64,
        nanos: 0,
    })
}

fn valid_exposure() -> ExposureEvent {
    ExposureEvent {
        event_id: "evt-bench-001".into(),
        experiment_id: "exp-bench-001".into(),
        user_id: "user-bench-001".into(),
        variant_id: "control".into(),
        timestamp: now_proto(),
        platform: "ios".into(),
        session_id: "sess-bench-001".into(),
        assignment_probability: 0.5,
        interleaving_provenance: Default::default(),
        bandit_context_json: String::new(),
        lifecycle_segment: 3, // ESTABLISHED
    }
}

fn valid_exposure_with_provenance() -> ExposureEvent {
    let mut event = valid_exposure();
    for i in 0..10 {
        event
            .interleaving_provenance
            .insert(format!("item-{i}"), format!("algo-{}", i % 3));
    }
    event
}

fn valid_metric_event() -> MetricEvent {
    MetricEvent {
        event_id: "evt-bench-002".into(),
        user_id: "user-bench-001".into(),
        event_type: "play_start".into(),
        value: 42.5,
        content_id: "content-bench-001".into(),
        session_id: "sess-bench-001".into(),
        timestamp: now_proto(),
        properties: Default::default(),
    }
}

fn valid_reward_event() -> RewardEvent {
    RewardEvent {
        event_id: "evt-bench-003".into(),
        experiment_id: "exp-bench-001".into(),
        user_id: "user-bench-001".into(),
        arm_id: "arm-alpha".into(),
        reward: 0.85,
        timestamp: now_proto(),
        context_json: r#"{"feature_a": 1.5}"#.into(),
    }
}

fn valid_qoe_event() -> QoEEvent {
    QoEEvent {
        event_id: "evt-bench-004".into(),
        session_id: "sess-bench-001".into(),
        content_id: "content-bench-001".into(),
        user_id: "user-bench-001".into(),
        metrics: Some(PlaybackMetrics {
            time_to_first_frame_ms: 250,
            rebuffer_count: 1,
            rebuffer_ratio: 0.02,
            avg_bitrate_kbps: 5000,
            resolution_switches: 2,
            peak_resolution_height: 1080,
            startup_failure_rate: 0.0,
            playback_duration_ms: 60_000,
            ebvs_detected: false,
        }),
        cdn_provider: "akamai".into(),
        abr_algorithm: "buffer-based-v2".into(),
        encoding_profile: "h265-hdr10".into(),
        timestamp: now_proto(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Validation benchmarks
// ═══════════════════════════════════════════════════════════════════════════

fn bench_validate_exposure(c: &mut Criterion) {
    let event = valid_exposure();
    c.bench_function("validate_exposure", |b| {
        b.iter(|| validation::validate_exposure(black_box(&event)))
    });
}

fn bench_validate_exposure_with_provenance(c: &mut Criterion) {
    let event = valid_exposure_with_provenance();
    c.bench_function("validate_exposure_10_provenance", |b| {
        b.iter(|| validation::validate_exposure(black_box(&event)))
    });
}

fn bench_validate_metric_event(c: &mut Criterion) {
    let event = valid_metric_event();
    c.bench_function("validate_metric_event", |b| {
        b.iter(|| validation::validate_metric_event(black_box(&event)))
    });
}

fn bench_validate_reward_event(c: &mut Criterion) {
    let event = valid_reward_event();
    c.bench_function("validate_reward_event", |b| {
        b.iter(|| validation::validate_reward_event(black_box(&event)))
    });
}

fn bench_validate_qoe_event(c: &mut Criterion) {
    let event = valid_qoe_event();
    c.bench_function("validate_qoe_event", |b| {
        b.iter(|| validation::validate_qoe_event(black_box(&event)))
    });
}

fn bench_validate_playback_metrics(c: &mut Criterion) {
    let metrics = valid_qoe_event().metrics.unwrap();
    c.bench_function("validate_playback_metrics", |b| {
        b.iter(|| validation::validate_playback_metrics(black_box(&metrics)))
    });
}

// ═══════════════════════════════════════════════════════════════════════════
//  Bloom filter dedup benchmarks
// ═══════════════════════════════════════════════════════════════════════════

fn bench_dedup_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom_dedup_insert");

    for size in [1_000, 10_000, 100_000] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{size}_existing")),
            &size,
            |b, &size| {
                let mut dedup = EventDedup::new(size * 2, 0.001);
                // Pre-fill to target size
                for i in 0..size {
                    dedup.is_duplicate(&format!("prefill-{i}"));
                }
                let mut counter = 0u64;
                b.iter(|| {
                    counter += 1;
                    dedup.is_duplicate(black_box(&format!("new-{counter}")))
                });
            },
        );
    }
    group.finish();
}

fn bench_dedup_check_duplicate(c: &mut Criterion) {
    let mut dedup = EventDedup::new(100_000, 0.001);
    // Insert the event we'll check
    dedup.is_duplicate("known-duplicate");

    c.bench_function("bloom_dedup_check_duplicate", |b| {
        b.iter(|| dedup.is_duplicate(black_box("known-duplicate")))
    });
}

fn bench_dedup_check_novel(c: &mut Criterion) {
    let mut dedup = EventDedup::new(100_000, 0.001);
    // Pre-fill with other events
    for i in 0..10_000 {
        dedup.is_duplicate(&format!("other-{i}"));
    }

    let mut counter = 0u64;
    c.bench_function("bloom_dedup_check_novel", |b| {
        b.iter(|| {
            counter += 1;
            dedup.is_duplicate(black_box(&format!("novel-{counter}")))
        })
    });
}

fn bench_dedup_rotation(c: &mut Criterion) {
    c.bench_function("bloom_dedup_rotation", |b| {
        let config = DedupConfig {
            items_per_interval: 100_000,
            fp_rate: 0.001,
            rotation_interval_secs: 3600,
        };
        let mut dedup = EventDedup::with_config(config, DedupMetrics::noop());
        // Fill current filter
        for i in 0..1_000 {
            dedup.is_duplicate(&format!("fill-{i}"));
        }
        b.iter(|| dedup.rotate());
    });
}

fn bench_dedup_production_config(c: &mut Criterion) {
    // Benchmark with production-equivalent config: 100M/day, 0.1% FPR
    let config = DedupConfig::from_daily(100_000_000, 0.001);
    let mut dedup = EventDedup::with_config(config, DedupMetrics::noop());

    let mut counter = 0u64;
    c.bench_function("bloom_dedup_production_insert", |b| {
        b.iter(|| {
            counter += 1;
            dedup.is_duplicate(black_box(&format!("prod-{counter}")))
        })
    });
}

// ═══════════════════════════════════════════════════════════════════════════
//  Protobuf serialization benchmarks
// ═══════════════════════════════════════════════════════════════════════════

fn bench_encode_exposure(c: &mut Criterion) {
    let event = valid_exposure();
    c.bench_function("encode_exposure", |b| {
        b.iter(|| black_box(&event).encode_to_vec())
    });
}

fn bench_decode_exposure(c: &mut Criterion) {
    let bytes = valid_exposure().encode_to_vec();
    c.bench_function("decode_exposure", |b| {
        b.iter(|| ExposureEvent::decode(black_box(bytes.as_slice())).unwrap())
    });
}

fn bench_encode_metric_event(c: &mut Criterion) {
    let event = valid_metric_event();
    c.bench_function("encode_metric_event", |b| {
        b.iter(|| black_box(&event).encode_to_vec())
    });
}

fn bench_decode_metric_event(c: &mut Criterion) {
    let bytes = valid_metric_event().encode_to_vec();
    c.bench_function("decode_metric_event", |b| {
        b.iter(|| MetricEvent::decode(black_box(bytes.as_slice())).unwrap())
    });
}

fn bench_encode_qoe_event(c: &mut Criterion) {
    let event = valid_qoe_event();
    c.bench_function("encode_qoe_event", |b| {
        b.iter(|| black_box(&event).encode_to_vec())
    });
}

fn bench_decode_qoe_event(c: &mut Criterion) {
    let bytes = valid_qoe_event().encode_to_vec();
    c.bench_function("decode_qoe_event", |b| {
        b.iter(|| QoEEvent::decode(black_box(bytes.as_slice())).unwrap())
    });
}

fn bench_encode_reward_event(c: &mut Criterion) {
    let event = valid_reward_event();
    c.bench_function("encode_reward_event", |b| {
        b.iter(|| black_box(&event).encode_to_vec())
    });
}

// ═══════════════════════════════════════════════════════════════════════════
//  Full ingest path benchmark (validate + dedup + encode)
// ═══════════════════════════════════════════════════════════════════════════

fn bench_full_ingest_exposure(c: &mut Criterion) {
    let mut dedup = EventDedup::new(1_000_000, 0.001);
    let mut counter = 0u64;

    c.bench_function("full_ingest_exposure", |b| {
        b.iter(|| {
            counter += 1;
            let event = ExposureEvent {
                event_id: format!("evt-{counter}"),
                experiment_id: "exp-1".into(),
                user_id: format!("user-{}", counter % 10_000),
                variant_id: "control".into(),
                timestamp: now_proto(),
                assignment_probability: 0.5,
                ..Default::default()
            };
            // Validate
            let _ = validation::validate_exposure(black_box(&event));
            // Dedup
            let _ = dedup.is_duplicate(black_box(&event.event_id));
            // Serialize
            black_box(event.encode_to_vec())
        })
    });
}

fn bench_full_ingest_metric(c: &mut Criterion) {
    let mut dedup = EventDedup::new(1_000_000, 0.001);
    let mut counter = 0u64;

    c.bench_function("full_ingest_metric_event", |b| {
        b.iter(|| {
            counter += 1;
            let event = MetricEvent {
                event_id: format!("evt-{counter}"),
                user_id: format!("user-{}", counter % 10_000),
                event_type: "play_start".into(),
                value: 42.5,
                timestamp: now_proto(),
                ..Default::default()
            };
            let _ = validation::validate_metric_event(black_box(&event));
            let _ = dedup.is_duplicate(black_box(&event.event_id));
            black_box(event.encode_to_vec())
        })
    });
}

fn bench_full_ingest_qoe(c: &mut Criterion) {
    let mut dedup = EventDedup::new(1_000_000, 0.001);
    let mut counter = 0u64;

    c.bench_function("full_ingest_qoe_event", |b| {
        b.iter(|| {
            counter += 1;
            let event = QoEEvent {
                event_id: format!("evt-{counter}"),
                session_id: format!("sess-{counter}"),
                content_id: "content-1".into(),
                user_id: format!("user-{}", counter % 10_000),
                metrics: Some(PlaybackMetrics {
                    time_to_first_frame_ms: 250,
                    rebuffer_count: 1,
                    rebuffer_ratio: 0.02,
                    avg_bitrate_kbps: 5000,
                    resolution_switches: 2,
                    peak_resolution_height: 1080,
                    startup_failure_rate: 0.0,
                    playback_duration_ms: 60_000,
                    ebvs_detected: false,
                }),
                timestamp: now_proto(),
                ..Default::default()
            };
            let _ = validation::validate_qoe_event(black_box(&event));
            let _ = dedup.is_duplicate(black_box(&event.event_id));
            black_box(event.encode_to_vec())
        })
    });
}

// ═══════════════════════════════════════════════════════════════════════════
//  Throughput benchmark (batch of 1000 events)
// ═══════════════════════════════════════════════════════════════════════════

fn bench_throughput_1k_exposures(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput");
    group.throughput(Throughput::Elements(1000));
    group.bench_function("1k_exposures_validate_dedup_encode", |b| {
        let mut dedup = EventDedup::new(1_000_000, 0.001);
        let mut counter = 0u64;
        b.iter(|| {
            for _ in 0..1000 {
                counter += 1;
                let event = ExposureEvent {
                    event_id: format!("evt-{counter}"),
                    experiment_id: "exp-1".into(),
                    user_id: format!("user-{}", counter % 10_000),
                    variant_id: "control".into(),
                    timestamp: now_proto(),
                    ..Default::default()
                };
                let _ = validation::validate_exposure(&event);
                let _ = dedup.is_duplicate(&event.event_id);
                black_box(event.encode_to_vec());
            }
        });
    });
    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════

criterion_group!(
    validation_benches,
    bench_validate_exposure,
    bench_validate_exposure_with_provenance,
    bench_validate_metric_event,
    bench_validate_reward_event,
    bench_validate_qoe_event,
    bench_validate_playback_metrics,
);

criterion_group!(
    dedup_benches,
    bench_dedup_insert,
    bench_dedup_check_duplicate,
    bench_dedup_check_novel,
    bench_dedup_rotation,
    bench_dedup_production_config,
);

criterion_group!(
    serialization_benches,
    bench_encode_exposure,
    bench_decode_exposure,
    bench_encode_metric_event,
    bench_decode_metric_event,
    bench_encode_qoe_event,
    bench_decode_qoe_event,
    bench_encode_reward_event,
);

criterion_group!(
    ingest_path_benches,
    bench_full_ingest_exposure,
    bench_full_ingest_metric,
    bench_full_ingest_qoe,
    bench_throughput_1k_exposures,
);

criterion_main!(
    validation_benches,
    dedup_benches,
    serialization_benches,
    ingest_path_benches,
);
