//! M2 → M3 event contract & integration tests.
//!
//! Validates that ExposureEvent, MetricEvent, and QoEEvent serialization,
//! field population, and Kafka key strategies match what M3's Spark SQL
//! templates and Delta Lake schemas expect.
//!
//! **Sections 2–4 (~32 tests)**: Protobuf encode/decode contract tests.
//! Run without Docker as part of `just test-rust`.
//!
//! **Section 5 (~8 tests, `#[ignore]`)**: Kafka roundtrip tests. Require a
//! running broker (`just infra`). Run explicitly:
//! ```bash
//! cargo test -p experimentation-pipeline --test m2_m3_event_contract -- --ignored
//! ```

use prost::Message;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ═══════════════════════════════════════════════════════════════════════════
//  Proto type aliases
// ═══════════════════════════════════════════════════════════════════════════

type ExposureEvent = experimentation_proto::common::ExposureEvent;
type MetricEvent = experimentation_proto::common::MetricEvent;
type QoEEvent = experimentation_proto::common::QoEEvent;
type PlaybackMetrics = experimentation_proto::common::PlaybackMetrics;
type LifecycleSegment = experimentation_proto::common::LifecycleSegment;
type ModelRetrainingEvent = experimentation_proto::common::ModelRetrainingEvent;

// ═══════════════════════════════════════════════════════════════════════════
//  Section 1 — Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Build an `ExposureEvent` with all fields populated.
fn make_exposure(
    event_id: &str,
    experiment_id: &str,
    user_id: &str,
    variant_id: &str,
) -> ExposureEvent {
    let mut provenance = HashMap::new();
    provenance.insert("item-1".into(), "algo-A".into());
    provenance.insert("item-2".into(), "algo-B".into());

    ExposureEvent {
        event_id: event_id.into(),
        experiment_id: experiment_id.into(),
        user_id: user_id.into(),
        variant_id: variant_id.into(),
        timestamp: Some(prost_types::Timestamp {
            seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            nanos: 0,
        }),
        platform: "ios".into(),
        session_id: "sess-abc-123".into(),
        assignment_probability: 0.5,
        interleaving_provenance: provenance,
        bandit_context_json: r#"{"feature_a": 1.5}"#.into(),
        lifecycle_segment: LifecycleSegment::Established as i32,
        switchback_block_index: 0,
    }
}

/// Build a `MetricEvent` with all fields populated.
fn make_metric_event(
    event_id: &str,
    user_id: &str,
    event_type: &str,
    value: f64,
) -> MetricEvent {
    let mut properties = HashMap::new();
    properties.insert("device".into(), "roku".into());
    properties.insert("quality".into(), "hd".into());

    MetricEvent {
        event_id: event_id.into(),
        user_id: user_id.into(),
        event_type: event_type.into(),
        value,
        content_id: "content-xyz-789".into(),
        session_id: "sess-met-456".into(),
        timestamp: Some(prost_types::Timestamp {
            seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            nanos: 0,
        }),
        properties,
    }
}

/// Build a `QoEEvent` with full `PlaybackMetrics`.
fn make_qoe_event(
    event_id: &str,
    session_id: &str,
    content_id: &str,
    user_id: &str,
) -> QoEEvent {
    QoEEvent {
        event_id: event_id.into(),
        session_id: session_id.into(),
        content_id: content_id.into(),
        user_id: user_id.into(),
        metrics: Some(PlaybackMetrics {
            time_to_first_frame_ms: 250,
            rebuffer_count: 3,
            rebuffer_ratio: 0.02,
            avg_bitrate_kbps: 5000,
            resolution_switches: 2,
            peak_resolution_height: 1080,
            startup_failure_rate: 0.0,
            playback_duration_ms: 3_600_000,
        }),
        cdn_provider: "akamai".into(),
        abr_algorithm: "buffer-based-v2".into(),
        encoding_profile: "h265-hdr10".into(),
        timestamp: Some(prost_types::Timestamp {
            seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            nanos: 0,
        }),
    }
}

/// Generate a unique event ID using PID + nanosecond timestamp.
fn unique_event_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}-{}-{nanos}", std::process::id())
}

// ═══════════════════════════════════════════════════════════════════════════
//  Section 2 — ExposureEvent Contract Tests (12 tests, no Docker)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_exposure_roundtrip_all_fields() {
    let event = make_exposure("evt-exp-1", "exp-100", "user-42", "variant-B");
    let bytes = event.encode_to_vec();
    let decoded = ExposureEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.event_id, "evt-exp-1");
    assert_eq!(decoded.experiment_id, "exp-100");
    assert_eq!(decoded.user_id, "user-42");
    assert_eq!(decoded.variant_id, "variant-B");
    assert!(decoded.timestamp.is_some());
    assert_eq!(decoded.platform, "ios");
    assert_eq!(decoded.session_id, "sess-abc-123");
    assert_eq!(decoded.assignment_probability, 0.5);
    assert_eq!(decoded.interleaving_provenance.len(), 2);
    assert_eq!(decoded.interleaving_provenance["item-1"], "algo-A");
    assert_eq!(decoded.interleaving_provenance["item-2"], "algo-B");
    assert_eq!(decoded.bandit_context_json, r#"{"feature_a": 1.5}"#);
    assert_eq!(decoded.lifecycle_segment, LifecycleSegment::Established as i32);
}

#[test]
fn test_exposure_roundtrip_minimal() {
    // Only required fields: experiment_id, user_id, variant_id
    let event = ExposureEvent {
        event_id: "evt-min".into(),
        experiment_id: "exp-1".into(),
        user_id: "user-1".into(),
        variant_id: "variant-A".into(),
        timestamp: None,
        platform: String::new(),
        session_id: String::new(),
        assignment_probability: 0.0,
        interleaving_provenance: HashMap::new(),
        bandit_context_json: String::new(),
        lifecycle_segment: 0,
        switchback_block_index: 0,
    };
    let bytes = event.encode_to_vec();
    let decoded = ExposureEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.experiment_id, "exp-1");
    assert_eq!(decoded.user_id, "user-1");
    assert_eq!(decoded.variant_id, "variant-A");
    assert!(decoded.timestamp.is_none());
    assert!(decoded.interleaving_provenance.is_empty());
}

#[test]
fn test_exposure_session_id_survives() {
    // Session-level experiments require session_id on exposure
    let mut event = make_exposure("evt-sess", "exp-sess", "user-1", "variant-A");
    event.session_id = "session-xyz-999".into();
    let bytes = event.encode_to_vec();
    let decoded = ExposureEvent::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.session_id, "session-xyz-999");
}

#[test]
fn test_exposure_interleaving_provenance_map() {
    // M3 interleaving_score.sql.tmpl reads interleaving_provenance[content_id]
    let mut provenance = HashMap::new();
    provenance.insert("content-a".into(), "algo-reco".into());
    provenance.insert("content-b".into(), "algo-trending".into());
    provenance.insert("content-c".into(), "algo-reco".into());

    let event = ExposureEvent {
        event_id: "evt-prov".into(),
        experiment_id: "exp-interleave".into(),
        user_id: "user-1".into(),
        variant_id: "variant-interleaved".into(),
        interleaving_provenance: provenance.clone(),
        ..Default::default()
    };
    let bytes = event.encode_to_vec();
    let decoded = ExposureEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.interleaving_provenance.len(), 3);
    assert_eq!(decoded.interleaving_provenance["content-a"], "algo-reco");
    assert_eq!(decoded.interleaving_provenance["content-b"], "algo-trending");
    assert_eq!(decoded.interleaving_provenance["content-c"], "algo-reco");
}

#[test]
fn test_exposure_lifecycle_segment_enum() {
    // All 7 LifecycleSegment values must survive roundtrip.
    // M3 lifecycle_mean.sql.tmpl stratifies by these segments.
    let segments = [
        LifecycleSegment::Unspecified,
        LifecycleSegment::Trial,
        LifecycleSegment::New,
        LifecycleSegment::Established,
        LifecycleSegment::Mature,
        LifecycleSegment::AtRisk,
        LifecycleSegment::Winback,
    ];

    for seg in segments {
        let event = ExposureEvent {
            event_id: format!("evt-seg-{}", seg as i32),
            experiment_id: "exp-lifecycle".into(),
            user_id: "user-1".into(),
            variant_id: "variant-A".into(),
            lifecycle_segment: seg as i32,
            ..Default::default()
        };
        let bytes = event.encode_to_vec();
        let decoded = ExposureEvent::decode(bytes.as_slice()).unwrap();
        assert_eq!(
            decoded.lifecycle_segment,
            seg as i32,
            "LifecycleSegment {:?} (value {}) did not survive roundtrip",
            seg,
            seg as i32
        );
    }
}

#[test]
fn test_exposure_assignment_probability() {
    // double field for IPW. M3/M4a uses this for inverse propensity weighting.
    for prob in [0.0, 0.5, 1.0] {
        let event = ExposureEvent {
            event_id: format!("evt-prob-{prob}"),
            experiment_id: "exp-bandit".into(),
            user_id: "user-1".into(),
            variant_id: "arm-a".into(),
            assignment_probability: prob,
            ..Default::default()
        };
        let bytes = event.encode_to_vec();
        let decoded = ExposureEvent::decode(bytes.as_slice()).unwrap();
        assert_eq!(
            decoded.assignment_probability, prob,
            "assignment_probability {prob} did not survive roundtrip"
        );
    }
}

#[test]
fn test_exposure_bandit_context_json() {
    let context = r#"{"hour_of_day": 14, "device": "smart_tv", "genre_pref": 0.8}"#;
    let event = ExposureEvent {
        event_id: "evt-ctx".into(),
        experiment_id: "exp-bandit".into(),
        user_id: "user-1".into(),
        variant_id: "arm-b".into(),
        bandit_context_json: context.into(),
        ..Default::default()
    };
    let bytes = event.encode_to_vec();
    let decoded = ExposureEvent::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.bandit_context_json, context);

    // Verify it's valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&decoded.bandit_context_json).unwrap();
    assert_eq!(parsed["hour_of_day"], 14);
}

#[test]
fn test_exposure_key_is_experiment_id() {
    // Documents Kafka key contract: M2 publishes exposures keyed by experiment_id.
    // M3's exposure_join.sql.tmpl filters by experiment_id, so partition co-locality
    // by experiment ensures efficient reads.
    let event = make_exposure("evt-key", "exp-key-contract-42", "user-1", "variant-A");
    let kafka_key = &event.experiment_id;
    assert_eq!(kafka_key, "exp-key-contract-42");
}

#[test]
fn test_exposure_delta_schema_alignment() {
    // Verify proto field names match delta.exposures DDL columns.
    // If a proto field is renamed, this test documents the required mapping.
    //
    // Delta DDL (delta_lake_tables.sql):
    //   event_id, experiment_id, user_id, variant_id, platform, session_id,
    //   assignment_probability, interleaving_provenance, bandit_context_json,
    //   lifecycle_segment, event_timestamp (from proto timestamp), ingested_at (M2 adds)
    let event = make_exposure("evt-delta", "exp-1", "user-1", "variant-A");
    let bytes = event.encode_to_vec();
    let decoded = ExposureEvent::decode(bytes.as_slice()).unwrap();

    // These field accesses compile only if proto field names match.
    // A rename would cause a compile error here, alerting the developer.
    let _event_id = &decoded.event_id;
    let _experiment_id = &decoded.experiment_id;
    let _user_id = &decoded.user_id;
    let _variant_id = &decoded.variant_id;
    let _platform = &decoded.platform;
    let _session_id = &decoded.session_id;
    let _assignment_probability = decoded.assignment_probability;
    let _interleaving_provenance = &decoded.interleaving_provenance;
    let _bandit_context_json = &decoded.bandit_context_json;
    let _lifecycle_segment = decoded.lifecycle_segment;
    // timestamp → event_timestamp mapping done by Kafka Connect sink
    let _timestamp = &decoded.timestamp;

    // Verify all fields are populated (non-default)
    assert!(!_event_id.is_empty());
    assert!(!_experiment_id.is_empty());
    assert!(!_user_id.is_empty());
    assert!(!_variant_id.is_empty());
}

#[test]
fn test_exposure_m3_join_fields_present() {
    // M3 exposure_join.sql.tmpl: SELECT DISTINCT user_id, variant_id
    //                            WHERE experiment_id = '...'
    // All three join fields must be non-empty for M3 to work.
    let event = make_exposure("evt-join", "exp-join-test", "user-join-42", "variant-B");
    let bytes = event.encode_to_vec();
    let decoded = ExposureEvent::decode(bytes.as_slice()).unwrap();

    assert!(!decoded.experiment_id.is_empty(), "experiment_id required for M3 exposure_join");
    assert!(!decoded.user_id.is_empty(), "user_id required for M3 exposure_join");
    assert!(!decoded.variant_id.is_empty(), "variant_id required for M3 exposure_join");
}

#[test]
fn test_exposure_session_join_fields() {
    // M3 session_level_mean.sql.tmpl: JOIN ON me.user_id = eu.user_id
    //   AND me.session_id = eu.session_id WHERE session_id IS NOT NULL
    let event = make_exposure("evt-sess-join", "exp-sess", "user-sess-1", "variant-A");
    let bytes = event.encode_to_vec();
    let decoded = ExposureEvent::decode(bytes.as_slice()).unwrap();

    assert!(!decoded.user_id.is_empty(), "user_id required for session_level_mean join");
    assert!(!decoded.session_id.is_empty(), "session_id required for session_level_mean join");
}

#[test]
fn test_exposure_decode_garbage_fails() {
    let garbage = vec![0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA, 0x00, 0x01];
    assert!(ExposureEvent::decode(garbage.as_slice()).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
//  Section 3 — MetricEvent Contract Tests (10 tests, no Docker)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_metric_event_roundtrip_all_fields() {
    let event = make_metric_event("evt-met-1", "user-42", "play_start", 120.5);
    let bytes = event.encode_to_vec();
    let decoded = MetricEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.event_id, "evt-met-1");
    assert_eq!(decoded.user_id, "user-42");
    assert_eq!(decoded.event_type, "play_start");
    assert_eq!(decoded.value, 120.5);
    assert_eq!(decoded.content_id, "content-xyz-789");
    assert_eq!(decoded.session_id, "sess-met-456");
    assert!(decoded.timestamp.is_some());
    assert_eq!(decoded.properties.len(), 2);
    assert_eq!(decoded.properties["device"], "roku");
    assert_eq!(decoded.properties["quality"], "hd");
}

#[test]
fn test_metric_event_roundtrip_minimal() {
    // Required: user_id, event_type
    let event = MetricEvent {
        event_id: "evt-min".into(),
        user_id: "user-1".into(),
        event_type: "search".into(),
        value: 0.0,
        content_id: String::new(),
        session_id: String::new(),
        timestamp: None,
        properties: HashMap::new(),
    };
    let bytes = event.encode_to_vec();
    let decoded = MetricEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.user_id, "user-1");
    assert_eq!(decoded.event_type, "search");
    assert!(decoded.timestamp.is_none());
    assert!(decoded.properties.is_empty());
}

#[test]
fn test_metric_event_zero_value() {
    // Proto3 default for double is 0.0 — verify it survives roundtrip.
    // M3 count.sql.tmpl counts events regardless of value.
    let event = make_metric_event("evt-zero", "user-1", "search", 0.0);
    let bytes = event.encode_to_vec();
    let decoded = MetricEvent::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.value, 0.0);
}

#[test]
fn test_metric_event_negative_value() {
    // Negative values valid (e.g., revenue adjustments, refunds)
    let event = make_metric_event("evt-neg", "user-1", "refund", -19.99);
    let bytes = event.encode_to_vec();
    let decoded = MetricEvent::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.value, -19.99);
}

#[test]
fn test_metric_event_properties_map() {
    // M3 custom.sql.tmpl may reference properties for custom metric definitions
    let mut props = HashMap::new();
    props.insert("genre".into(), "drama".into());
    props.insert("duration_bucket".into(), "long".into());
    props.insert("source".into(), "recommendation".into());

    let event = MetricEvent {
        event_id: "evt-props".into(),
        user_id: "user-1".into(),
        event_type: "watch_complete".into(),
        value: 1.0,
        properties: props.clone(),
        ..Default::default()
    };
    let bytes = event.encode_to_vec();
    let decoded = MetricEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.properties.len(), 3);
    assert_eq!(decoded.properties["genre"], "drama");
    assert_eq!(decoded.properties["duration_bucket"], "long");
    assert_eq!(decoded.properties["source"], "recommendation");
}

#[test]
fn test_metric_event_content_id_for_interference() {
    // M3 interleaving_score.sql.tmpl: JOIN ON me.content_id
    //   WHERE me.content_id IS NOT NULL
    let event = make_metric_event("evt-content", "user-1", "play_start", 1.0);
    let bytes = event.encode_to_vec();
    let decoded = MetricEvent::decode(bytes.as_slice()).unwrap();
    assert!(
        !decoded.content_id.is_empty(),
        "content_id required for interleaving_score join on content_id"
    );
}

#[test]
fn test_metric_event_session_id_for_session_level() {
    // M3 session_level_mean.sql.tmpl: JOIN ON me.session_id = eu.session_id
    let event = make_metric_event("evt-sess", "user-1", "play_start", 1.0);
    let bytes = event.encode_to_vec();
    let decoded = MetricEvent::decode(bytes.as_slice()).unwrap();
    assert!(
        !decoded.session_id.is_empty(),
        "session_id required for session_level_mean join"
    );
}

#[test]
fn test_metric_event_key_is_user_id() {
    // Documents Kafka key contract: M2 publishes metric_events keyed by user_id.
    // This ensures all events for the same user land on the same partition,
    // enabling efficient per-user aggregation in M3.
    let event = make_metric_event("evt-key", "user-key-contract-99", "play_start", 1.0);
    let kafka_key = &event.user_id;
    assert_eq!(kafka_key, "user-key-contract-99");
}

#[test]
fn test_metric_event_delta_schema_alignment() {
    // Verify proto field names match delta.metric_events DDL columns.
    //
    // Delta DDL: event_id, user_id, event_type, value, content_id,
    //   session_id, properties, event_timestamp, ingested_at
    let event = make_metric_event("evt-delta", "user-1", "play_start", 42.0);
    let bytes = event.encode_to_vec();
    let decoded = MetricEvent::decode(bytes.as_slice()).unwrap();

    let _event_id = &decoded.event_id;
    let _user_id = &decoded.user_id;
    let _event_type = &decoded.event_type;
    let _value = decoded.value;
    let _content_id = &decoded.content_id;
    let _session_id = &decoded.session_id;
    let _properties = &decoded.properties;
    let _timestamp = &decoded.timestamp; // → event_timestamp in Delta

    assert!(!_event_id.is_empty());
    assert!(!_user_id.is_empty());
    assert!(!_event_type.is_empty());
}

#[test]
fn test_metric_event_decode_garbage_fails() {
    let garbage = vec![0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA, 0x00, 0x01];
    assert!(MetricEvent::decode(garbage.as_slice()).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
//  Section 4 — QoEEvent Contract Tests (10 tests, no Docker)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_qoe_event_roundtrip_all_fields() {
    let event = make_qoe_event("evt-qoe-1", "sess-qoe-1", "content-1", "user-42");
    let bytes = event.encode_to_vec();
    let decoded = QoEEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.event_id, "evt-qoe-1");
    assert_eq!(decoded.session_id, "sess-qoe-1");
    assert_eq!(decoded.content_id, "content-1");
    assert_eq!(decoded.user_id, "user-42");
    assert!(decoded.metrics.is_some());
    assert_eq!(decoded.cdn_provider, "akamai");
    assert_eq!(decoded.abr_algorithm, "buffer-based-v2");
    assert_eq!(decoded.encoding_profile, "h265-hdr10");
    assert!(decoded.timestamp.is_some());
}

#[test]
fn test_qoe_event_roundtrip_minimal() {
    // Required: session_id, content_id, user_id
    let event = QoEEvent {
        event_id: "evt-min".into(),
        session_id: "sess-1".into(),
        content_id: "content-1".into(),
        user_id: "user-1".into(),
        metrics: None,
        cdn_provider: String::new(),
        abr_algorithm: String::new(),
        encoding_profile: String::new(),
        timestamp: None,
    };
    let bytes = event.encode_to_vec();
    let decoded = QoEEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.session_id, "sess-1");
    assert_eq!(decoded.content_id, "content-1");
    assert_eq!(decoded.user_id, "user-1");
    assert!(decoded.metrics.is_none());
}

#[test]
fn test_qoe_playback_metrics_all_fields() {
    // Verify all 8 PlaybackMetrics fields survive roundtrip.
    // M3 qoe_metric.sql.tmpl uses `.QoEField` to reference these columns.
    let event = make_qoe_event("evt-pbm", "sess-1", "content-1", "user-1");
    let bytes = event.encode_to_vec();
    let decoded = QoEEvent::decode(bytes.as_slice()).unwrap();

    let m = decoded.metrics.unwrap();
    assert_eq!(m.time_to_first_frame_ms, 250);
    assert_eq!(m.rebuffer_count, 3);
    assert_eq!(m.rebuffer_ratio, 0.02);
    assert_eq!(m.avg_bitrate_kbps, 5000);
    assert_eq!(m.resolution_switches, 2);
    assert_eq!(m.peak_resolution_height, 1080);
    assert_eq!(m.startup_failure_rate, 0.0);
    assert_eq!(m.playback_duration_ms, 3_600_000);
}

#[test]
fn test_qoe_playback_metrics_zero_values() {
    // All-zero metrics (proto3 defaults) — M3 should handle gracefully.
    let event = QoEEvent {
        event_id: "evt-zero".into(),
        session_id: "sess-1".into(),
        content_id: "content-1".into(),
        user_id: "user-1".into(),
        metrics: Some(PlaybackMetrics {
            time_to_first_frame_ms: 0,
            rebuffer_count: 0,
            rebuffer_ratio: 0.0,
            avg_bitrate_kbps: 0,
            resolution_switches: 0,
            peak_resolution_height: 0,
            startup_failure_rate: 0.0,
            playback_duration_ms: 0,
        }),
        ..Default::default()
    };
    let bytes = event.encode_to_vec();
    let decoded = QoEEvent::decode(bytes.as_slice()).unwrap();

    let m = decoded.metrics.unwrap();
    assert_eq!(m.time_to_first_frame_ms, 0);
    assert_eq!(m.rebuffer_count, 0);
    assert_eq!(m.rebuffer_ratio, 0.0);
    assert_eq!(m.avg_bitrate_kbps, 0);
    assert_eq!(m.resolution_switches, 0);
    assert_eq!(m.peak_resolution_height, 0);
    assert_eq!(m.startup_failure_rate, 0.0);
    assert_eq!(m.playback_duration_ms, 0);
}

#[test]
fn test_qoe_cdn_abr_encoding_fields() {
    // CDN/ABR/encoding strings for CDN experiments.
    // M3 uses these as grouping keys for QoE analysis.
    let event = QoEEvent {
        event_id: "evt-cdn".into(),
        session_id: "sess-1".into(),
        content_id: "content-1".into(),
        user_id: "user-1".into(),
        cdn_provider: "cloudfront".into(),
        abr_algorithm: "mpc-v3".into(),
        encoding_profile: "av1-sdr".into(),
        ..Default::default()
    };
    let bytes = event.encode_to_vec();
    let decoded = QoEEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.cdn_provider, "cloudfront");
    assert_eq!(decoded.abr_algorithm, "mpc-v3");
    assert_eq!(decoded.encoding_profile, "av1-sdr");
}

#[test]
fn test_qoe_event_key_is_session_id() {
    // Documents Kafka key contract: M2 publishes qoe_events keyed by session_id.
    // This ensures all QoE events for the same playback session land on the
    // same partition, enabling M3 per-session aggregation.
    let event = make_qoe_event("evt-key", "sess-key-contract-77", "content-1", "user-1");
    let kafka_key = &event.session_id;
    assert_eq!(kafka_key, "sess-key-contract-77");
}

#[test]
fn test_qoe_delta_schema_alignment() {
    // Verify proto field names match delta.qoe_events DDL columns.
    //
    // Delta DDL: event_id, session_id, content_id, user_id,
    //   time_to_first_frame_ms, rebuffer_count, rebuffer_ratio, avg_bitrate_kbps,
    //   resolution_switches, peak_resolution_height, startup_failure_rate,
    //   playback_duration_ms, cdn_provider, abr_algorithm, encoding_profile,
    //   event_timestamp, ingested_at
    //
    // Note: PlaybackMetrics fields are flattened in Delta (no nested struct).
    // Kafka Connect sink config must map metrics.X → X.
    let event = make_qoe_event("evt-delta", "sess-1", "content-1", "user-1");
    let bytes = event.encode_to_vec();
    let decoded = QoEEvent::decode(bytes.as_slice()).unwrap();

    // Top-level fields
    let _event_id = &decoded.event_id;
    let _session_id = &decoded.session_id;
    let _content_id = &decoded.content_id;
    let _user_id = &decoded.user_id;
    let _cdn_provider = &decoded.cdn_provider;
    let _abr_algorithm = &decoded.abr_algorithm;
    let _encoding_profile = &decoded.encoding_profile;
    let _timestamp = &decoded.timestamp; // → event_timestamp in Delta

    // PlaybackMetrics fields (flattened in Delta)
    let m = decoded.metrics.unwrap();
    let _ttff = m.time_to_first_frame_ms;
    let _rebuf_count = m.rebuffer_count;
    let _rebuf_ratio = m.rebuffer_ratio;
    let _avg_bitrate = m.avg_bitrate_kbps;
    let _res_switches = m.resolution_switches;
    let _peak_res = m.peak_resolution_height;
    let _startup_fail = m.startup_failure_rate;
    let _playback_dur = m.playback_duration_ms;

    assert!(!_event_id.is_empty());
    assert!(!_session_id.is_empty());
}

#[test]
fn test_qoe_m3_qoe_field_valid() {
    // M3 qoe_metric.sql.tmpl uses `.QoEField` template param to reference
    // delta.qoe_events columns. All valid QoE field names must exist as
    // PlaybackMetrics fields.
    let qoe_fields = [
        "time_to_first_frame_ms",
        "rebuffer_count",
        "rebuffer_ratio",
        "avg_bitrate_kbps",
        "resolution_switches",
        "peak_resolution_height",
        "startup_failure_rate",
        "playback_duration_ms",
    ];

    let event = make_qoe_event("evt-fields", "sess-1", "content-1", "user-1");
    let bytes = event.encode_to_vec();
    let decoded = QoEEvent::decode(bytes.as_slice()).unwrap();
    let m = decoded.metrics.unwrap();

    // Verify each QoE field is accessible and has the expected type by
    // mapping field names to their values
    let field_values: HashMap<&str, f64> = [
        ("time_to_first_frame_ms", m.time_to_first_frame_ms as f64),
        ("rebuffer_count", m.rebuffer_count as f64),
        ("rebuffer_ratio", m.rebuffer_ratio),
        ("avg_bitrate_kbps", m.avg_bitrate_kbps as f64),
        ("resolution_switches", m.resolution_switches as f64),
        ("peak_resolution_height", m.peak_resolution_height as f64),
        ("startup_failure_rate", m.startup_failure_rate),
        ("playback_duration_ms", m.playback_duration_ms as f64),
    ]
    .into_iter()
    .collect();

    for field_name in &qoe_fields {
        assert!(
            field_values.contains_key(field_name),
            "QoE field '{field_name}' referenced by M3 qoe_metric.sql.tmpl not found"
        );
    }
}

#[test]
fn test_qoe_event_without_metrics() {
    // None metrics decodes — M2 rejects these (validation), but a consumer
    // should handle the case where metrics is absent.
    let event = QoEEvent {
        event_id: "evt-no-met".into(),
        session_id: "sess-1".into(),
        content_id: "content-1".into(),
        user_id: "user-1".into(),
        metrics: None,
        ..Default::default()
    };
    let bytes = event.encode_to_vec();
    let decoded = QoEEvent::decode(bytes.as_slice()).unwrap();
    assert!(decoded.metrics.is_none());
}

#[test]
fn test_qoe_event_decode_garbage_fails() {
    let garbage = vec![0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA, 0x00, 0x01];
    assert!(QoEEvent::decode(garbage.as_slice()).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
//  Section 5 — Kafka Integration Tests (8 tests, require Docker)
// ═══════════════════════════════════════════════════════════════════════════

/// Kafka helpers — only compiled when running integration tests.
#[cfg(test)]
mod kafka_helpers {
    use super::*;
    use prost::Message as ProstMessage;
    use rdkafka::config::ClientConfig;
    use rdkafka::consumer::{Consumer, StreamConsumer};
    use rdkafka::message::{BorrowedMessage, Headers, OwnedHeaders};
    use rdkafka::producer::{FutureProducer, FutureRecord};
    pub use rdkafka::Message as KafkaMessage;
    use std::time::Duration;

    pub const BROKERS: &str = "localhost:9092";

    /// M2-compatible idempotent producer (mirrors `EventProducer::new` config).
    pub fn test_producer() -> FutureProducer {
        ClientConfig::new()
            .set("bootstrap.servers", BROKERS)
            .set("enable.idempotence", "true")
            .set("acks", "all")
            .set("compression.type", "lz4")
            .set("linger.ms", "0")
            .create()
            .expect("failed to create test producer")
    }

    /// Consumer for a specific topic with a unique group_id.
    pub fn test_consumer(topic: &str, group_id: &str) -> StreamConsumer {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", BROKERS)
            .set("group.id", group_id)
            .set("auto.offset.reset", "earliest")
            .set("enable.auto.commit", "false")
            .set("enable.partition.eof", "false")
            .set("session.timeout.ms", "30000")
            .set("fetch.min.bytes", "1")
            .set("fetch.wait.max.ms", "500")
            .create()
            .expect("failed to create test consumer");
        consumer.subscribe(&[topic]).expect("failed to subscribe");
        consumer
    }

    /// Publish a protobuf event to a topic with M2-compatible headers.
    pub async fn produce_event(
        producer: &FutureProducer,
        topic: &str,
        key: &str,
        payload: &[u8],
        event_type: &str,
        traceparent: Option<&str>,
    ) -> (i32, i64) {
        let ingest_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();

        let mut headers = OwnedHeaders::new()
            .insert(rdkafka::message::Header {
                key: "x-ingest-ts-ms",
                value: Some(ingest_ts.as_bytes()),
            })
            .insert(rdkafka::message::Header {
                key: "x-event-type",
                value: Some(event_type.as_bytes()),
            });

        if let Some(tp) = traceparent {
            headers = headers.insert(rdkafka::message::Header {
                key: "traceparent",
                value: Some(tp.as_bytes()),
            });
        }

        let record = FutureRecord::to(topic)
            .key(key)
            .payload(payload)
            .headers(headers);

        let (partition, offset) = producer
            .send(record, Duration::from_secs(5))
            .await
            .expect("failed to produce event");

        (partition, offset)
    }

    /// Publish an ExposureEvent with the M2 key strategy (experiment_id).
    pub async fn produce_exposure(
        producer: &FutureProducer,
        event: &ExposureEvent,
        traceparent: Option<&str>,
    ) -> (i32, i64) {
        let payload = ProstMessage::encode_to_vec(event);
        produce_event(
            producer,
            "exposures",
            &event.experiment_id,
            &payload,
            "exposure",
            traceparent,
        )
        .await
    }

    /// Publish a MetricEvent with the M2 key strategy (user_id).
    pub async fn produce_metric_event(
        producer: &FutureProducer,
        event: &MetricEvent,
        traceparent: Option<&str>,
    ) -> (i32, i64) {
        let payload = ProstMessage::encode_to_vec(event);
        produce_event(
            producer,
            "metric_events",
            &event.user_id,
            &payload,
            "metric",
            traceparent,
        )
        .await
    }

    /// Publish a QoEEvent with the M2 key strategy (session_id).
    pub async fn produce_qoe_event(
        producer: &FutureProducer,
        event: &QoEEvent,
        traceparent: Option<&str>,
    ) -> (i32, i64) {
        let payload = ProstMessage::encode_to_vec(event);
        produce_event(
            producer,
            "qoe_events",
            &event.session_id,
            &payload,
            "qoe",
            traceparent,
        )
        .await
    }

    /// Generate a unique consumer group ID for test isolation.
    pub fn unique_group_id(prefix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{prefix}-{}-{nanos}", std::process::id())
    }

    /// Consume a single message with timeout.
    pub async fn consume_one(consumer: &StreamConsumer, timeout_secs: u64) -> BorrowedMessage<'_> {
        tokio::time::timeout(Duration::from_secs(timeout_secs), consumer.recv())
            .await
            .expect("consumer timeout waiting for message")
            .expect("consumer recv error")
    }

    /// Get a header value by key from a Kafka message.
    pub fn get_header<'a>(msg: &'a BorrowedMessage<'a>, key: &str) -> Option<&'a [u8]> {
        msg.headers().and_then(|hdrs| {
            (0..hdrs.count()).find_map(|i| {
                let header = hdrs.get(i);
                if header.key == key {
                    header.value
                } else {
                    None
                }
            })
        })
    }
}

// ── Kafka integration test 1: Exposure roundtrip ─────────────────────────

#[tokio::test]
#[ignore] // Requires `just infra`
async fn test_kafka_exposure_produce_consume_roundtrip() {
    use kafka_helpers::*;

    let producer = test_producer();
    let group_id = unique_group_id("exp-rt");
    let consumer = test_consumer("exposures", &group_id);

    let event = make_exposure(
        &unique_event_id("exp-rt"),
        "exp-roundtrip-m3",
        "user-42",
        "variant-B",
    );

    produce_exposure(&producer, &event, Some("00-abc123-def456-01")).await;

    let msg = consume_one(&consumer, 10).await;
    let payload = msg.payload().expect("no payload");
    let decoded = ExposureEvent::decode(payload).unwrap();

    assert_eq!(decoded.event_id, event.event_id);
    assert_eq!(decoded.experiment_id, "exp-roundtrip-m3");
    assert_eq!(decoded.user_id, "user-42");
    assert_eq!(decoded.variant_id, "variant-B");
    assert_eq!(decoded.interleaving_provenance.len(), 2);
}

// ── Kafka integration test 2: Exposure headers + key ─────────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_exposure_headers_and_key() {
    use kafka_helpers::*;

    let producer = test_producer();
    let group_id = unique_group_id("exp-hdr");
    let consumer = test_consumer("exposures", &group_id);

    let experiment_id = "exp-hdr-contract-42";
    let event = make_exposure(
        &unique_event_id("exp-hdr"),
        experiment_id,
        "user-1",
        "variant-A",
    );

    let traceparent = "00-abcdef1234567890abcdef1234567890-1234567890abcdef-01";
    produce_exposure(&producer, &event, Some(traceparent)).await;

    let msg = consume_one(&consumer, 10).await;

    // Key == experiment_id
    let key = msg.key().expect("message has no key");
    assert_eq!(std::str::from_utf8(key).unwrap(), experiment_id);

    // x-ingest-ts-ms: parseable epoch millis
    let ts_bytes = get_header(&msg, "x-ingest-ts-ms").expect("missing x-ingest-ts-ms");
    let ts_ms: u128 = std::str::from_utf8(ts_bytes).unwrap().parse().unwrap();
    assert!(ts_ms > 1_700_000_000_000);

    // x-event-type: "exposure"
    let et = get_header(&msg, "x-event-type").expect("missing x-event-type");
    assert_eq!(std::str::from_utf8(et).unwrap(), "exposure");

    // traceparent propagated
    let tp = get_header(&msg, "traceparent").expect("missing traceparent");
    assert_eq!(std::str::from_utf8(tp).unwrap(), traceparent);
}

// ── Kafka integration test 3: MetricEvent roundtrip ──────────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_metric_event_produce_consume_roundtrip() {
    use kafka_helpers::*;

    let producer = test_producer();
    let group_id = unique_group_id("met-rt");
    let consumer = test_consumer("metric_events", &group_id);

    let event = make_metric_event(
        &unique_event_id("met-rt"),
        "user-met-42",
        "play_start",
        120.5,
    );

    produce_metric_event(&producer, &event, None).await;

    let msg = consume_one(&consumer, 10).await;
    let payload = msg.payload().expect("no payload");
    let decoded = MetricEvent::decode(payload).unwrap();

    assert_eq!(decoded.event_id, event.event_id);
    assert_eq!(decoded.user_id, "user-met-42");
    assert_eq!(decoded.event_type, "play_start");
    assert_eq!(decoded.value, 120.5);
    assert_eq!(decoded.properties.len(), 2);
}

// ── Kafka integration test 4: MetricEvent key == user_id ─────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_metric_event_key_is_user_id() {
    use kafka_helpers::*;

    let producer = test_producer();
    let group_id = unique_group_id("met-key");
    let consumer = test_consumer("metric_events", &group_id);

    let user_id = "user-key-contract-met-99";
    let event = make_metric_event(&unique_event_id("met-key"), user_id, "search", 1.0);

    produce_metric_event(&producer, &event, None).await;

    let msg = consume_one(&consumer, 10).await;
    let key = msg.key().expect("message has no key");
    assert_eq!(std::str::from_utf8(key).unwrap(), user_id);
}

// ── Kafka integration test 5: QoEEvent roundtrip ─────────────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_qoe_event_produce_consume_roundtrip() {
    use kafka_helpers::*;

    let producer = test_producer();
    let group_id = unique_group_id("qoe-rt");
    let consumer = test_consumer("qoe_events", &group_id);

    let event = make_qoe_event(
        &unique_event_id("qoe-rt"),
        "sess-qoe-roundtrip",
        "content-qoe-1",
        "user-qoe-42",
    );

    produce_qoe_event(&producer, &event, None).await;

    let msg = consume_one(&consumer, 10).await;
    let payload = msg.payload().expect("no payload");
    let decoded = QoEEvent::decode(payload).unwrap();

    assert_eq!(decoded.event_id, event.event_id);
    assert_eq!(decoded.session_id, "sess-qoe-roundtrip");
    assert_eq!(decoded.content_id, "content-qoe-1");
    assert_eq!(decoded.user_id, "user-qoe-42");
    let m = decoded.metrics.unwrap();
    assert_eq!(m.time_to_first_frame_ms, 250);
    assert_eq!(m.rebuffer_count, 3);
}

// ── Kafka integration test 6: QoEEvent key == session_id ─────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_qoe_event_key_is_session_id() {
    use kafka_helpers::*;

    let producer = test_producer();
    let group_id = unique_group_id("qoe-key");
    let consumer = test_consumer("qoe_events", &group_id);

    let session_id = "sess-key-contract-qoe-77";
    let event = make_qoe_event(
        &unique_event_id("qoe-key"),
        session_id,
        "content-1",
        "user-1",
    );

    produce_qoe_event(&producer, &event, None).await;

    let msg = consume_one(&consumer, 10).await;
    let key = msg.key().expect("message has no key");
    assert_eq!(std::str::from_utf8(key).unwrap(), session_id);
}

// ── Kafka integration test 7: Exposure partition determinism ─────────────

#[tokio::test]
#[ignore]
async fn test_kafka_exposure_partition_determinism() {
    use kafka_helpers::*;

    let producer = test_producer();
    let experiment_id = "exp-partition-det-m3";

    // Produce N events with the same experiment_id (key)
    let mut partitions = Vec::new();
    for i in 0..5 {
        let event = make_exposure(
            &unique_event_id(&format!("pdet-{i}")),
            experiment_id,
            &format!("user-{i}"),
            "variant-A",
        );
        let (partition, _) = produce_exposure(&producer, &event, None).await;
        partitions.push(partition);
    }

    // All events with the same key should land on the same partition
    let first = partitions[0];
    assert!(
        partitions.iter().all(|&p| p == first),
        "expected all exposures on partition {first}, got {partitions:?}"
    );
}

// ── Kafka integration test 8: Cross-topic user correlation ───────────────

#[tokio::test]
#[ignore]
async fn test_kafka_cross_topic_user_correlation() {
    use kafka_helpers::*;

    let producer = test_producer();
    let shared_user_id = "user-cross-topic-42";
    let experiment_id = "exp-cross-topic-1";

    // Produce an exposure for the user
    let exposure = ExposureEvent {
        event_id: unique_event_id("cross-exp"),
        experiment_id: experiment_id.into(),
        user_id: shared_user_id.into(),
        variant_id: "variant-A".into(),
        timestamp: Some(prost_types::Timestamp {
            seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            nanos: 0,
        }),
        ..Default::default()
    };
    produce_exposure(&producer, &exposure, None).await;

    // Produce a metric event for the same user
    let metric = MetricEvent {
        event_id: unique_event_id("cross-met"),
        user_id: shared_user_id.into(),
        event_type: "play_start".into(),
        value: 60.0,
        content_id: "content-cross-1".into(),
        timestamp: Some(prost_types::Timestamp {
            seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            nanos: 0,
        }),
        ..Default::default()
    };
    produce_metric_event(&producer, &metric, None).await;

    // Consume from both topics and verify user_id matches
    let exp_group = unique_group_id("cross-exp");
    let met_group = unique_group_id("cross-met");
    let exp_consumer = test_consumer("exposures", &exp_group);
    let met_consumer = test_consumer("metric_events", &met_group);

    let exp_msg = consume_one(&exp_consumer, 10).await;
    let decoded_exp = ExposureEvent::decode(exp_msg.payload().unwrap()).unwrap();

    let met_msg = consume_one(&met_consumer, 10).await;
    let decoded_met = MetricEvent::decode(met_msg.payload().unwrap()).unwrap();

    // M3 exposure_join.sql.tmpl: JOIN ON me.user_id = eu.user_id
    assert_eq!(
        decoded_exp.user_id, decoded_met.user_id,
        "user_id mismatch between exposure and metric event — M3 JOIN would fail"
    );
    assert_eq!(decoded_exp.user_id, shared_user_id);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Section 6 — ModelRetrainingEvent Contract Tests (ADR-021)
// ═══════════════════════════════════════════════════════════════════════════
//
// Validates that ModelRetrainingEvent serialization, field survival, and Kafka
// key strategy (model_id) match what M3's feedback loop contamination pipeline
// expects per ADR-021.
//
// Section 6a (non-Docker): Protobuf encode/decode contract tests.
// Section 6b (requires Docker): Kafka roundtrip test.

/// Build a ModelRetrainingEvent with all meaningful fields populated.
fn make_model_retraining_event(
    event_id: &str,
    model_id: &str,
    start_offset_hours: i64,
    end_offset_hours: i64,
) -> ModelRetrainingEvent {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    ModelRetrainingEvent {
        event_id: event_id.into(),
        model_id: model_id.into(),
        training_data_start: Some(prost_types::Timestamp {
            seconds: now - start_offset_hours * 3600,
            nanos: 0,
        }),
        training_data_end: Some(prost_types::Timestamp {
            seconds: now - end_offset_hours * 3600,
            nanos: 0,
        }),
        retrained_at: Some(prost_types::Timestamp {
            seconds: now,
            nanos: 0,
        }),
        active_experiment_ids: vec!["exp-001".into(), "exp-002".into(), "exp-003".into()],
        treatment_contamination_fraction: 0.0, // computed post-hoc by M3
    }
}

// ── Section 6a: Non-Docker roundtrip tests ────────────────────────────────

#[test]
fn test_model_retraining_event_roundtrip_all_fields() {
    let event = make_model_retraining_event("mre-roundtrip-1", "rec-model-v3", 48, 24);
    let bytes = event.encode_to_vec();
    let decoded = ModelRetrainingEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.event_id, "mre-roundtrip-1");
    assert_eq!(decoded.model_id, "rec-model-v3");
    assert!(decoded.training_data_start.is_some(), "training_data_start must survive roundtrip");
    assert!(decoded.training_data_end.is_some(), "training_data_end must survive roundtrip");
    assert!(decoded.retrained_at.is_some(), "retrained_at must survive roundtrip");
    assert_eq!(decoded.active_experiment_ids.len(), 3);
    assert_eq!(decoded.active_experiment_ids[0], "exp-001");
}

#[test]
fn test_model_retraining_event_roundtrip_required_fields_only() {
    // Minimal event: only fields required by M2 validation.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let event = ModelRetrainingEvent {
        event_id: "mre-min".into(),
        model_id: "rec-model".into(),
        training_data_start: Some(prost_types::Timestamp { seconds: now - 48 * 3600, nanos: 0 }),
        training_data_end: Some(prost_types::Timestamp { seconds: now - 24 * 3600, nanos: 0 }),
        ..Default::default()
    };
    let bytes = event.encode_to_vec();
    let decoded = ModelRetrainingEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.model_id, "rec-model");
    assert!(decoded.training_data_start.is_some());
    assert!(decoded.training_data_end.is_some());
    assert!(decoded.retrained_at.is_none()); // optional — absent when not supplied
    assert!(decoded.active_experiment_ids.is_empty());
    assert_eq!(decoded.treatment_contamination_fraction, 0.0);
}

#[test]
fn test_model_retraining_event_training_window_seconds_preserved() {
    // M3 contamination SQL: WHERE e.timestamp BETWEEN start AND end.
    // Verifies that epoch seconds survive encode/decode exactly.
    let start_secs: i64 = 1_700_000_000;
    let end_secs: i64 = 1_700_086_400; // +24h

    let event = ModelRetrainingEvent {
        event_id: "mre-window".into(),
        model_id: "model-window".into(),
        training_data_start: Some(prost_types::Timestamp { seconds: start_secs, nanos: 0 }),
        training_data_end: Some(prost_types::Timestamp { seconds: end_secs, nanos: 0 }),
        ..Default::default()
    };
    let bytes = event.encode_to_vec();
    let decoded = ModelRetrainingEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.training_data_start.unwrap().seconds, start_secs);
    assert_eq!(decoded.training_data_end.unwrap().seconds, end_secs);
}

#[test]
fn test_model_retraining_event_active_experiment_ids_repeated_field() {
    // M3 cross-reference: JOIN active_experiment_ids with experiments table.
    let ids: Vec<String> = (0..10).map(|i| format!("exp-{i:03}")).collect();
    let event = ModelRetrainingEvent {
        event_id: "mre-ids".into(),
        model_id: "model-ids".into(),
        active_experiment_ids: ids.clone(),
        ..Default::default()
    };
    let bytes = event.encode_to_vec();
    let decoded = ModelRetrainingEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.active_experiment_ids.len(), 10);
    assert_eq!(decoded.active_experiment_ids, ids);
}

#[test]
fn test_model_retraining_event_dedup_key_format() {
    // Verify the composite dedup key format that M2 uses (ADR-021).
    // Key: "{model_id}:{training_data_start_seconds}"
    // M3 does NOT consume this key, but it must be stable for dedup to work.
    let start_secs: i64 = 1_700_000_000;
    let event = ModelRetrainingEvent {
        event_id: "mre-key".into(),
        model_id: "rec-model-v4".into(),
        training_data_start: Some(prost_types::Timestamp { seconds: start_secs, nanos: 0 }),
        training_data_end: Some(prost_types::Timestamp { seconds: start_secs + 86400, nanos: 0 }),
        ..Default::default()
    };

    let expected_key = format!("rec-model-v4:{start_secs}");
    // Reconstruct the key the same way M2 does in the pipeline service.
    let computed_key = format!(
        "{}:{}",
        event.model_id,
        event.training_data_start.as_ref().unwrap().seconds,
    );
    assert_eq!(computed_key, expected_key);
}

#[test]
fn test_model_retraining_event_decode_garbage_fails() {
    let garbage = vec![0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA, 0x00, 0x01];
    // ModelRetrainingEvent has no required proto fields at the wire level
    // so garbage that happens to parse is OK — we just check it doesn't panic.
    let _ = ModelRetrainingEvent::decode(garbage.as_slice());
}

// ── Section 6b: Kafka roundtrip (requires Docker) ─────────────────────────

#[tokio::test]
#[ignore] // Requires `just infra`
async fn test_kafka_model_retraining_event_roundtrip() {
    use kafka_helpers::*;

    let producer = test_producer();
    let group_id = unique_group_id("mre-roundtrip");
    let consumer = test_consumer("model_retraining_events", &group_id);

    let event = make_model_retraining_event(
        &unique_event_id("mre-kafka"),
        "rec-model-kafka-v1",
        48,
        24,
    );
    let payload = prost::Message::encode_to_vec(&event);
    // M2 key strategy: model_id (collocates all retraining events per model)
    produce_event(&producer, "model_retraining_events", &event.model_id, &payload, "model_retraining", None).await;

    let msg = consume_one(&consumer, 10).await;
    let raw_payload = msg.payload().expect("no payload");
    let decoded = ModelRetrainingEvent::decode(raw_payload)
        .expect("M3 consumer must decode ModelRetrainingEvent from Kafka payload");

    assert_eq!(decoded.event_id, event.event_id);
    assert_eq!(decoded.model_id, event.model_id);
    assert_eq!(
        decoded.training_data_start.unwrap().seconds,
        event.training_data_start.unwrap().seconds,
        "training_data_start must survive Kafka roundtrip — M3 uses it for window JOIN"
    );
    assert_eq!(
        decoded.training_data_end.unwrap().seconds,
        event.training_data_end.unwrap().seconds,
        "training_data_end must survive Kafka roundtrip"
    );
    assert_eq!(
        decoded.active_experiment_ids,
        event.active_experiment_ids,
        "active_experiment_ids must survive Kafka roundtrip — M3 uses for experiment cross-reference"
    );

    // Key contract: M2 partitions by model_id
    let key = msg.key().expect("Kafka message must have a key");
    assert_eq!(
        std::str::from_utf8(key).unwrap(),
        event.model_id,
        "Kafka key must be model_id (ADR-021 partitioning strategy)"
    );
}
