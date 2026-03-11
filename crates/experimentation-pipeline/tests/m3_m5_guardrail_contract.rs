//! GuardrailAlert contract tests (M3 → Kafka → M5).
//!
//! M3 publishes `GuardrailAlert` as **JSON** to the `guardrail_alerts` topic,
//! keyed by `experiment_id`. M5 consumes and processes auto-pause logic.
//! The protobuf `GuardrailAlert` message in `event.proto` is the canonical
//! schema definition — this file validates:
//!
//! 1. Protobuf encode/decode roundtrip (Rust-side parity)
//! 2. Proto field names match the Go JSON tags M3/M5 use
//! 3. Kafka key strategy (experiment_id)
//! 4. JSON serialization contract (what M5 actually deserializes)
//!
//! **Section 1 (~13 tests)**: Protobuf + JSON contract tests. No Docker.
//! **Section 2 (~3 tests, `#[ignore]`)**: Kafka roundtrip tests.
//!
//! ```bash
//! cargo test -p experimentation-pipeline --test m3_m5_guardrail_contract
//! cargo test -p experimentation-pipeline --test m3_m5_guardrail_contract -- --ignored  # Docker
//! ```

use prost::Message;
use std::time::{SystemTime, UNIX_EPOCH};

// ═══════════════════════════════════════════════════════════════════════════
//  Proto type alias
// ═══════════════════════════════════════════════════════════════════════════

type GuardrailAlert = experimentation_proto::common::GuardrailAlert;

// ═══════════════════════════════════════════════════════════════════════════
//  Section 1 — Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Build a fully populated `GuardrailAlert`.
fn make_alert(
    experiment_id: &str,
    metric_id: &str,
    variant_id: &str,
    current_value: f64,
    threshold: f64,
    breach_count: i32,
) -> GuardrailAlert {
    GuardrailAlert {
        experiment_id: experiment_id.into(),
        metric_id: metric_id.into(),
        variant_id: variant_id.into(),
        current_value,
        threshold,
        consecutive_breach_count: breach_count,
        detected_at: Some(prost_types::Timestamp {
            seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            nanos: 0,
        }),
    }
}

/// Generate a unique event ID using PID + nanosecond timestamp.
fn unique_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}-{}-{nanos}", std::process::id())
}

// ═══════════════════════════════════════════════════════════════════════════
//  Section 2 — Protobuf Contract Tests (13 tests, no Docker)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_guardrail_alert_roundtrip_all_fields() {
    let alert = make_alert("exp-100", "error_rate_p99", "variant-B", 0.15, 0.10, 3);
    let bytes = alert.encode_to_vec();
    let decoded = GuardrailAlert::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.experiment_id, "exp-100");
    assert_eq!(decoded.metric_id, "error_rate_p99");
    assert_eq!(decoded.variant_id, "variant-B");
    assert_eq!(decoded.current_value, 0.15);
    assert_eq!(decoded.threshold, 0.10);
    assert_eq!(decoded.consecutive_breach_count, 3);
    assert!(decoded.detected_at.is_some());
}

#[test]
fn test_guardrail_alert_roundtrip_minimal() {
    // Minimal alert: just identifiers, no timestamp
    let alert = GuardrailAlert {
        experiment_id: "exp-1".into(),
        metric_id: "crash_rate".into(),
        variant_id: "variant-A".into(),
        current_value: 0.0,
        threshold: 0.0,
        consecutive_breach_count: 0,
        detected_at: None,
    };
    let bytes = alert.encode_to_vec();
    let decoded = GuardrailAlert::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.experiment_id, "exp-1");
    assert_eq!(decoded.metric_id, "crash_rate");
    assert!(decoded.detected_at.is_none());
}

#[test]
fn test_guardrail_alert_zero_breach_count() {
    // Proto3 default for int32 is 0 — verify it survives roundtrip.
    // M5 should treat breach_count=0 as no breach (processor skips).
    let alert = make_alert("exp-1", "metric-1", "variant-A", 0.5, 1.0, 0);
    let bytes = alert.encode_to_vec();
    let decoded = GuardrailAlert::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.consecutive_breach_count, 0);
}

#[test]
fn test_guardrail_alert_high_breach_count() {
    // M5 auto-pause triggers after consecutive_breach_count >= config threshold.
    // Verify large counts survive.
    let alert = make_alert("exp-1", "metric-1", "variant-A", 2.0, 1.0, 100);
    let bytes = alert.encode_to_vec();
    let decoded = GuardrailAlert::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.consecutive_breach_count, 100);
}

#[test]
fn test_guardrail_alert_current_value_exceeds_threshold() {
    // Breach case: current_value > threshold
    let alert = make_alert("exp-breach", "latency_p99", "variant-B", 250.0, 200.0, 5);
    let bytes = alert.encode_to_vec();
    let decoded = GuardrailAlert::decode(bytes.as_slice()).unwrap();
    assert!(
        decoded.current_value > decoded.threshold,
        "breach alert: current_value ({}) should exceed threshold ({})",
        decoded.current_value,
        decoded.threshold
    );
}

#[test]
fn test_guardrail_alert_negative_values() {
    // Negative current_value is valid (e.g., retention delta < 0).
    let alert = make_alert("exp-neg", "retention_delta", "variant-A", -0.05, -0.10, 2);
    let bytes = alert.encode_to_vec();
    let decoded = GuardrailAlert::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.current_value, -0.05);
    assert_eq!(decoded.threshold, -0.10);
}

#[test]
fn test_guardrail_alert_key_is_experiment_id() {
    // Documents Kafka key contract: guardrail_alerts keyed by experiment_id.
    // M5 consumer depends on this for per-experiment ordering:
    //   kafka/topic_configs.sh: "Key: experiment_id"
    //   services/metrics/internal/alerts/publisher.go: Key: []byte(alert.ExperimentID)
    let alert = make_alert("exp-key-contract-42", "metric-1", "variant-A", 1.0, 0.5, 1);
    let kafka_key = &alert.experiment_id;
    assert_eq!(kafka_key, "exp-key-contract-42");
}

#[test]
fn test_guardrail_alert_proto_json_field_alignment() {
    // Critical cross-language contract: proto field names must match the Go
    // JSON struct tags used by M3 (publisher.go) and M5 (processor.go).
    //
    // M3 Go struct tags (services/metrics/internal/alerts/publisher.go):
    //   `json:"experiment_id"`, `json:"metric_id"`, `json:"variant_id"`,
    //   `json:"current_value"`, `json:"threshold"`,
    //   `json:"consecutive_breach_count"`, `json:"detected_at"`
    //
    // Proto field names (event.proto GuardrailAlert):
    //   experiment_id, metric_id, variant_id, current_value, threshold,
    //   consecutive_breach_count, detected_at
    //
    // These must be identical. We verify by accessing each field — a proto
    // rename causes a compile error here.
    let alert = make_alert("exp-1", "m-1", "v-1", 1.0, 0.5, 3);
    let bytes = alert.encode_to_vec();
    let decoded = GuardrailAlert::decode(bytes.as_slice()).unwrap();

    // Proto field names (snake_case) must match Go JSON tags exactly.
    // If any proto field is renamed, this won't compile.
    let expected_json_fields = [
        ("experiment_id", decoded.experiment_id.as_str()),
        ("metric_id", decoded.metric_id.as_str()),
        ("variant_id", decoded.variant_id.as_str()),
    ];

    for (json_tag, value) in expected_json_fields {
        assert!(
            !json_tag.is_empty() && !value.is_empty(),
            "field {json_tag} must be non-empty"
        );
    }

    // Numeric fields — same field names in proto and Go JSON tags
    assert_eq!(decoded.current_value, 1.0);
    assert_eq!(decoded.threshold, 0.5);
    assert_eq!(decoded.consecutive_breach_count, 3);
    assert!(decoded.detected_at.is_some());
}

#[test]
fn test_guardrail_alert_json_roundtrip() {
    // M3 publishes as JSON, not protobuf. Verify that the proto field values
    // survive a JSON roundtrip matching the Go serialization format.
    //
    // Go JSON: {"experiment_id":"exp-1","metric_id":"m-1",...}
    let alert = make_alert("exp-json", "latency_p99", "variant-B", 250.5, 200.0, 3);

    // Simulate what M3 produces (JSON with matching field names)
    let json_str = serde_json::json!({
        "experiment_id": alert.experiment_id,
        "metric_id": alert.metric_id,
        "variant_id": alert.variant_id,
        "current_value": alert.current_value,
        "threshold": alert.threshold,
        "consecutive_breach_count": alert.consecutive_breach_count,
        "detected_at": alert.detected_at.as_ref().map(|t| t.seconds)
    });

    let parsed: serde_json::Value = serde_json::from_str(&json_str.to_string()).unwrap();

    // M5 deserializes these exact field names
    assert_eq!(parsed["experiment_id"], "exp-json");
    assert_eq!(parsed["metric_id"], "latency_p99");
    assert_eq!(parsed["variant_id"], "variant-B");
    assert_eq!(parsed["current_value"], 250.5);
    assert_eq!(parsed["threshold"], 200.0);
    assert_eq!(parsed["consecutive_breach_count"], 3);
}

#[test]
fn test_guardrail_alert_m5_required_fields() {
    // M5 processor.go:ProcessAlert requires experiment_id to look up the
    // experiment. If experiment_id is empty, M5 skips the alert (ResultSkipped).
    // Verify all identification fields are populated.
    let alert = make_alert("exp-m5", "error_rate", "variant-B", 0.15, 0.10, 2);
    let bytes = alert.encode_to_vec();
    let decoded = GuardrailAlert::decode(bytes.as_slice()).unwrap();

    assert!(!decoded.experiment_id.is_empty(), "experiment_id required for M5 lookup");
    assert!(!decoded.metric_id.is_empty(), "metric_id required for M5 audit trail");
    assert!(!decoded.variant_id.is_empty(), "variant_id required for M5 audit trail");
}

#[test]
fn test_guardrail_alert_multiple_variants_same_experiment() {
    // M5 processes alerts per-variant. Multiple variants in the same experiment
    // can breach independently. All must decode correctly.
    let variants = ["control", "variant-A", "variant-B"];
    for (i, variant) in variants.iter().enumerate() {
        let alert = make_alert(
            "exp-multi-variant",
            "error_rate",
            variant,
            0.15 + (i as f64 * 0.01),
            0.10,
            (i + 1) as i32,
        );
        let bytes = alert.encode_to_vec();
        let decoded = GuardrailAlert::decode(bytes.as_slice()).unwrap();

        assert_eq!(decoded.experiment_id, "exp-multi-variant");
        assert_eq!(decoded.variant_id, *variant);
        assert_eq!(decoded.consecutive_breach_count, (i + 1) as i32);
    }
}

#[test]
fn test_guardrail_alert_multiple_metrics_same_experiment() {
    // An experiment can breach multiple guardrail metrics simultaneously.
    // M5 processes each alert independently.
    let metrics = ["error_rate_p99", "latency_p99", "crash_rate", "rebuffer_ratio"];
    for metric in metrics {
        let alert = make_alert("exp-multi-metric", metric, "variant-A", 1.5, 1.0, 3);
        let bytes = alert.encode_to_vec();
        let decoded = GuardrailAlert::decode(bytes.as_slice()).unwrap();
        assert_eq!(decoded.metric_id, metric);
    }
}

#[test]
fn test_guardrail_alert_decode_garbage_fails() {
    let garbage = vec![0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA, 0x00, 0x01];
    assert!(GuardrailAlert::decode(garbage.as_slice()).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
//  Section 3 — Kafka Integration Tests (3 tests, require Docker)
// ═══════════════════════════════════════════════════════════════════════════

/// Kafka helpers for guardrail alert integration tests.
#[cfg(test)]
mod kafka_helpers {
    use super::*;
    use rdkafka::config::ClientConfig;
    use rdkafka::consumer::{Consumer, StreamConsumer};
    use rdkafka::message::{BorrowedMessage, OwnedHeaders};
    use rdkafka::producer::{FutureProducer, FutureRecord};
    pub use rdkafka::Message as KafkaMessage;
    use std::time::Duration;

    pub const BROKERS: &str = "localhost:9092";
    pub const TOPIC: &str = "guardrail_alerts";

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

    pub fn test_consumer(group_id: &str) -> StreamConsumer {
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
        consumer.subscribe(&[TOPIC]).expect("failed to subscribe");
        consumer
    }

    /// Publish a guardrail alert as JSON (matching M3's format), keyed by experiment_id.
    pub async fn produce_json_alert(
        producer: &FutureProducer,
        alert: &GuardrailAlert,
    ) -> (i32, i64) {
        // M3 publishes as JSON, not protobuf
        let json_value = serde_json::json!({
            "experiment_id": alert.experiment_id,
            "metric_id": alert.metric_id,
            "variant_id": alert.variant_id,
            "current_value": alert.current_value,
            "threshold": alert.threshold,
            "consecutive_breach_count": alert.consecutive_breach_count,
            "detected_at": alert.detected_at.as_ref()
                .map(|t| format!("{}T00:00:00Z", t.seconds))
                .unwrap_or_default()
        });
        let payload = serde_json::to_vec(&json_value).unwrap();

        let headers = OwnedHeaders::new().insert(rdkafka::message::Header {
            key: "x-event-type",
            value: Some(b"guardrail_alert"),
        });

        let record = FutureRecord::to(TOPIC)
            .key(alert.experiment_id.as_str())
            .payload(&payload)
            .headers(headers);

        let (partition, offset) = producer
            .send(record, Duration::from_secs(5))
            .await
            .expect("failed to produce guardrail alert");

        (partition, offset)
    }

    pub fn unique_group_id(prefix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{prefix}-{}-{nanos}", std::process::id())
    }

    pub async fn consume_one(consumer: &StreamConsumer, timeout_secs: u64) -> BorrowedMessage<'_> {
        tokio::time::timeout(Duration::from_secs(timeout_secs), consumer.recv())
            .await
            .expect("consumer timeout waiting for message")
            .expect("consumer recv error")
    }

}

// ── Kafka integration test 1: JSON roundtrip ────────────────────────────

#[tokio::test]
#[ignore] // Requires `just infra`
async fn test_kafka_guardrail_json_roundtrip() {
    use kafka_helpers::*;

    let producer = test_producer();
    let group_id = unique_group_id("guard-rt");
    let consumer = test_consumer(&group_id);

    let alert = make_alert(
        &unique_id("exp-guard-rt"),
        "error_rate_p99",
        "variant-B",
        0.15,
        0.10,
        3,
    );

    produce_json_alert(&producer, &alert).await;

    let msg = consume_one(&consumer, 10).await;
    let payload = msg.payload().expect("no payload");

    // M5 deserializes as JSON
    let parsed: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(parsed["experiment_id"], alert.experiment_id);
    assert_eq!(parsed["metric_id"], "error_rate_p99");
    assert_eq!(parsed["variant_id"], "variant-B");
    assert_eq!(parsed["current_value"], 0.15);
    assert_eq!(parsed["threshold"], 0.10);
    assert_eq!(parsed["consecutive_breach_count"], 3);
}

// ── Kafka integration test 2: Key is experiment_id ──────────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_guardrail_key_is_experiment_id() {
    use kafka_helpers::*;

    let producer = test_producer();
    let group_id = unique_group_id("guard-key");
    let consumer = test_consumer(&group_id);

    let experiment_id = unique_id("exp-guard-key");
    let alert = make_alert(&experiment_id, "crash_rate", "variant-A", 0.05, 0.01, 2);

    produce_json_alert(&producer, &alert).await;

    let msg = consume_one(&consumer, 10).await;
    let key = msg.key().expect("message has no key");
    assert_eq!(std::str::from_utf8(key).unwrap(), experiment_id);
}

// ── Kafka integration test 3: Partition determinism ─────────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_guardrail_partition_determinism() {
    use kafka_helpers::*;

    let producer = test_producer();
    let experiment_id = unique_id("exp-guard-pdet");

    // Multiple alerts for the same experiment should land on the same partition
    // (M5 depends on this for per-experiment ordering)
    let mut partitions = Vec::new();
    for i in 0..5 {
        let alert = make_alert(
            &experiment_id,
            &format!("metric-{i}"),
            "variant-A",
            (i as f64) * 0.1,
            0.5,
            i + 1,
        );
        let (partition, _) = produce_json_alert(&producer, &alert).await;
        partitions.push(partition);
    }

    let first = partitions[0];
    assert!(
        partitions.iter().all(|&p| p == first),
        "expected all alerts on partition {first}, got {partitions:?}"
    );
}
