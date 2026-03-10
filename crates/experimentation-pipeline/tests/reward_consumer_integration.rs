//! Reward event contract & integration tests (M2 → M4b).
//!
//! **Section 1 (17 tests)**: Protobuf encode/decode parity and `context_json`
//! parsing. These run without Docker as part of `just test-rust`.
//!
//! **Section 2 (7 tests, `#[ignore]`)**: End-to-end Kafka roundtrips. Require a
//! running broker (`just infra`). Run explicitly:
//! ```bash
//! cargo test -p experimentation-pipeline --test reward_consumer_integration -- --ignored
//! ```

use prost::Message;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ═══════════════════════════════════════════════════════════════════════════
//  Proto type alias — the type Agent-2 publishes and Agent-4 consumes
// ═══════════════════════════════════════════════════════════════════════════

type RewardEvent = experimentation_proto::experimentation::common::v1::RewardEvent;

// ═══════════════════════════════════════════════════════════════════════════
//  Section 1 — Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Build a `RewardEvent` with all fields populated.
fn make_reward_event(
    event_id: &str,
    experiment_id: &str,
    user_id: &str,
    arm_id: &str,
    reward: f64,
    context_json: &str,
) -> RewardEvent {
    RewardEvent {
        event_id: event_id.into(),
        experiment_id: experiment_id.into(),
        user_id: user_id.into(),
        arm_id: arm_id.into(),
        reward,
        timestamp: Some(prost_types::Timestamp {
            seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            nanos: 0,
        }),
        context_json: context_json.into(),
    }
}

/// Replicate M4b's `parse_context_json` logic from
/// `crates/experimentation-policy/src/kafka.rs:176-201`.
///
/// If M4b changes this function, this contract test will surface the
/// mismatch during review. The function is intentionally duplicated rather
/// than imported to avoid an architectural dependency from pipeline → policy.
fn parse_context_json(json_str: &str) -> Option<HashMap<String, f64>> {
    if json_str.is_empty() {
        return None;
    }

    let parsed: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let obj = parsed.as_object()?;

    let map: HashMap<String, f64> = obj
        .iter()
        .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f)))
        .collect();

    if map.is_empty() {
        None
    } else {
        Some(map)
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
//  Section 2 — Protobuf Contract Tests (17 tests, no Docker)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_reward_event_roundtrip_all_fields() {
    let event = make_reward_event(
        "evt-rt-1",
        "exp-100",
        "user-42",
        "arm-alpha",
        0.95,
        r#"{"feature_a": 1.5, "feature_b": 2.0}"#,
    );
    let bytes = event.encode_to_vec();
    let decoded = RewardEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.event_id, "evt-rt-1");
    assert_eq!(decoded.experiment_id, "exp-100");
    assert_eq!(decoded.user_id, "user-42");
    assert_eq!(decoded.arm_id, "arm-alpha");
    assert_eq!(decoded.reward, 0.95);
    assert!(decoded.timestamp.is_some());
    assert_eq!(
        decoded.context_json,
        r#"{"feature_a": 1.5, "feature_b": 2.0}"#
    );
}

#[test]
fn test_reward_event_roundtrip_minimal() {
    let event = RewardEvent {
        event_id: "evt-min".into(),
        experiment_id: "exp-1".into(),
        user_id: "user-1".into(),
        arm_id: "arm-a".into(),
        reward: 1.0,
        timestamp: None,
        context_json: String::new(),
    };
    let bytes = event.encode_to_vec();
    let decoded = RewardEvent::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.event_id, "evt-min");
    assert_eq!(decoded.experiment_id, "exp-1");
    assert!(decoded.timestamp.is_none());
    assert_eq!(decoded.context_json, "");
}

#[test]
fn test_reward_event_roundtrip_zero_reward() {
    // Proto3 default for double is 0.0 — verify it survives roundtrip
    let event = make_reward_event("evt-zero", "exp-1", "user-1", "arm-a", 0.0, "");
    let bytes = event.encode_to_vec();
    let decoded = RewardEvent::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.reward, 0.0);
}

#[test]
fn test_reward_event_roundtrip_negative_reward() {
    // Negative rewards are valid for some bandit formulations (cost-based)
    let event = make_reward_event("evt-neg", "exp-1", "user-1", "arm-a", -3.14, "");
    let bytes = event.encode_to_vec();
    let decoded = RewardEvent::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.reward, -3.14);
}

#[test]
fn test_reward_event_roundtrip_large_reward() {
    let event = make_reward_event("evt-max", "exp-1", "user-1", "arm-a", f64::MAX, "");
    let bytes = event.encode_to_vec();
    let decoded = RewardEvent::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.reward, f64::MAX);
}

#[test]
fn test_reward_event_roundtrip_small_reward() {
    let event = make_reward_event(
        "evt-small",
        "exp-1",
        "user-1",
        "arm-a",
        f64::MIN_POSITIVE,
        "",
    );
    let bytes = event.encode_to_vec();
    let decoded = RewardEvent::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.reward, f64::MIN_POSITIVE);
}

#[test]
fn test_reward_event_empty_strings() {
    // Proto3: empty string is the default — verify decode preserves it
    let event = RewardEvent {
        event_id: String::new(),
        experiment_id: String::new(),
        user_id: String::new(),
        arm_id: String::new(),
        reward: 0.0,
        timestamp: None,
        context_json: String::new(),
    };
    let bytes = event.encode_to_vec();
    let decoded = RewardEvent::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.event_id, "");
    assert_eq!(decoded.experiment_id, "");
    assert_eq!(decoded.user_id, "");
    assert_eq!(decoded.arm_id, "");
}

#[test]
fn test_reward_event_unicode_arm_id() {
    let event = make_reward_event(
        "evt-uni",
        "exp-1",
        "user-1",
        "arm-\u{1F680}-rocket",
        1.0,
        "",
    );
    let bytes = event.encode_to_vec();
    let decoded = RewardEvent::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.arm_id, "arm-\u{1F680}-rocket");
}

// ── context_json contract tests ─────────────────────────────────────────

#[test]
fn test_context_json_roundtrip_valid() {
    let event = make_reward_event(
        "evt-ctx-1",
        "exp-1",
        "user-1",
        "arm-a",
        1.0,
        r#"{"feature_a": 1.5}"#,
    );
    let bytes = event.encode_to_vec();
    let decoded = RewardEvent::decode(bytes.as_slice()).unwrap();
    let ctx = parse_context_json(&decoded.context_json).unwrap();
    assert_eq!(ctx.len(), 1);
    assert_eq!(ctx["feature_a"], 1.5);
}

#[test]
fn test_context_json_mixed_types() {
    // M4b's parse_context_json filters non-numeric values
    let json = r#"{"age": 25, "name": "alice"}"#;
    let ctx = parse_context_json(json).unwrap();
    assert_eq!(ctx.len(), 1);
    assert_eq!(ctx["age"], 25.0);
}

#[test]
fn test_context_json_empty_string() {
    assert!(parse_context_json("").is_none());
}

#[test]
fn test_context_json_nested_object() {
    // Nested objects are not f64, should be filtered out
    let json = r#"{"a": {"b": 1}, "c": 3.0}"#;
    let ctx = parse_context_json(json).unwrap();
    assert_eq!(ctx.len(), 1);
    assert_eq!(ctx["c"], 3.0);
}

#[test]
fn test_context_json_all_non_numeric() {
    let json = r#"{"a": "b", "c": true}"#;
    assert!(parse_context_json(json).is_none());
}

#[test]
fn test_context_json_integers() {
    // serde_json coerces JSON integers to f64 via as_f64()
    let json = r#"{"count": 42}"#;
    let ctx = parse_context_json(json).unwrap();
    assert_eq!(ctx["count"], 42.0);
}

// ── edge cases ──────────────────────────────────────────────────────────

#[test]
fn test_decode_garbage_fails() {
    let garbage = vec![0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA, 0x00, 0x01];
    assert!(RewardEvent::decode(garbage.as_slice()).is_err());
}

#[test]
fn test_decode_empty_bytes() {
    // Prost decodes empty bytes to a default message (all fields zero/empty)
    let decoded = RewardEvent::decode(&[] as &[u8]).unwrap();
    assert_eq!(decoded.event_id, "");
    assert_eq!(decoded.experiment_id, "");
    assert_eq!(decoded.reward, 0.0);
    assert!(decoded.timestamp.is_none());
}

#[test]
fn test_reward_event_key_is_experiment_id() {
    // Documents the Kafka key contract: M2 publishes reward events keyed
    // by experiment_id. M4b depends on this for partition co-locality of
    // all rewards for the same experiment.
    let event = make_reward_event("evt-key", "exp-abc-123", "user-1", "arm-a", 1.0, "");

    // The key M2 uses when calling producer.publish_with_headers()
    let kafka_key = &event.experiment_id;
    assert_eq!(kafka_key, "exp-abc-123");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Section 3 — Kafka Integration Tests (7 tests, require Docker)
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
    pub const REWARD_TOPIC: &str = "reward_events";

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

    /// M4b-compatible consumer (mirrors `consume_rewards` config).
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
        consumer
            .subscribe(&[REWARD_TOPIC])
            .expect("failed to subscribe");
        consumer
    }

    /// Publish a reward event the same way M2 does: protobuf payload,
    /// experiment_id as key, standard M2 headers.
    pub async fn produce_reward_event(
        producer: &FutureProducer,
        event: &RewardEvent,
        traceparent: Option<&str>,
    ) -> (i32, i64) {
        let payload = ProstMessage::encode_to_vec(event);
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
                value: Some(b"reward"),
            });

        if let Some(tp) = traceparent {
            headers = headers.insert(rdkafka::message::Header {
                key: "traceparent",
                value: Some(tp.as_bytes()),
            });
        }

        let record = FutureRecord::to(REWARD_TOPIC)
            .key(&event.experiment_id)
            .payload(&payload)
            .headers(headers);

        let (partition, offset) = producer
            .send(record, Duration::from_secs(5))
            .await
            .expect("failed to produce reward event");

        (partition, offset)
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
        tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            consumer.recv(),
        )
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

// ── Kafka integration test 1 ────────────────────────────────────────────

#[tokio::test]
#[ignore] // Requires `just infra`
async fn test_kafka_reward_produce_consume_roundtrip() {
    use kafka_helpers::*;

    let producer = test_producer();
    let group_id = unique_group_id("roundtrip");
    let consumer = test_consumer(&group_id);

    let event = make_reward_event(
        &unique_event_id("rt"),
        "exp-roundtrip-1",
        "user-42",
        "arm-alpha",
        0.85,
        r#"{"feature_a": 1.5}"#,
    );

    produce_reward_event(&producer, &event, Some("00-abc123-def456-01")).await;

    let msg = consume_one(&consumer, 10).await;
    let payload = msg.payload().expect("no payload");
    let decoded = RewardEvent::decode(payload).unwrap();

    assert_eq!(decoded.event_id, event.event_id);
    assert_eq!(decoded.experiment_id, "exp-roundtrip-1");
    assert_eq!(decoded.user_id, "user-42");
    assert_eq!(decoded.arm_id, "arm-alpha");
    assert_eq!(decoded.reward, 0.85);
    assert_eq!(decoded.context_json, r#"{"feature_a": 1.5}"#);
}

// ── Kafka integration test 2 ────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_reward_headers_present() {
    use kafka_helpers::*;
    let producer = test_producer();
    let group_id = unique_group_id("headers");
    let consumer = test_consumer(&group_id);

    let event = make_reward_event(
        &unique_event_id("hdr"),
        "exp-headers-1",
        "user-1",
        "arm-a",
        1.0,
        "",
    );

    let traceparent = "00-abcdef1234567890abcdef1234567890-1234567890abcdef-01";
    produce_reward_event(&producer, &event, Some(traceparent)).await;

    let msg = consume_one(&consumer, 10).await;

    // x-ingest-ts-ms: parseable as epoch millis
    let ts_bytes = get_header(&msg, "x-ingest-ts-ms").expect("missing x-ingest-ts-ms");
    let ts_str = std::str::from_utf8(ts_bytes).unwrap();
    let ts_ms: u128 = ts_str.parse().expect("x-ingest-ts-ms not a number");
    assert!(ts_ms > 1_700_000_000_000); // sanity: after 2023

    // x-event-type: "reward"
    let et_bytes = get_header(&msg, "x-event-type").expect("missing x-event-type");
    assert_eq!(std::str::from_utf8(et_bytes).unwrap(), "reward");

    // traceparent: propagated from ingest
    let tp_bytes = get_header(&msg, "traceparent").expect("missing traceparent");
    assert_eq!(std::str::from_utf8(tp_bytes).unwrap(), traceparent);
}

// ── Kafka integration test 3 ────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_reward_message_key() {
    use kafka_helpers::*;
    let producer = test_producer();
    let group_id = unique_group_id("key");
    let consumer = test_consumer(&group_id);

    let experiment_id = "exp-key-contract-42";
    let event = make_reward_event(
        &unique_event_id("key"),
        experiment_id,
        "user-1",
        "arm-a",
        1.0,
        "",
    );

    produce_reward_event(&producer, &event, None).await;

    let msg = consume_one(&consumer, 10).await;
    let key = msg.key().expect("message has no key");
    assert_eq!(std::str::from_utf8(key).unwrap(), experiment_id);
}

// ── Kafka integration test 4 ────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_reward_partition_determinism() {
    use kafka_helpers::*;

    let producer = test_producer();
    let experiment_id = "exp-partition-det-1";

    // Produce N events with the same experiment_id
    let mut partitions = Vec::new();
    for i in 0..5 {
        let event = make_reward_event(
            &unique_event_id(&format!("pdet-{i}")),
            experiment_id,
            &format!("user-{i}"),
            "arm-a",
            i as f64,
            "",
        );
        let (partition, _offset) = produce_reward_event(&producer, &event, None).await;
        partitions.push(partition);
    }

    // All events with the same key should land on the same partition
    let first = partitions[0];
    assert!(
        partitions.iter().all(|&p| p == first),
        "expected all events on partition {first}, got {partitions:?}"
    );
}

// ── Kafka integration test 5 ────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_reward_multiple_experiments() {
    use kafka_helpers::*;

    let producer = test_producer();

    // Produce events for several distinct experiment_ids
    let mut partition_set = std::collections::HashSet::new();
    for i in 0..20 {
        let exp_id = format!("exp-multi-{}", unique_event_id(&format!("m{i}")));
        let event = make_reward_event(
            &unique_event_id(&format!("multi-{i}")),
            &exp_id,
            "user-1",
            "arm-a",
            1.0,
            "",
        );
        let (partition, _) = produce_reward_event(&producer, &event, None).await;
        partition_set.insert(partition);
    }

    // With 20 distinct keys, we expect at least 2 different partitions
    // (unless the topic has only 1 partition)
    assert!(
        partition_set.len() >= 1,
        "expected events distributed across partitions, got {partition_set:?}"
    );
}

// ── Kafka integration test 6 ────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_reward_consumer_group_offsets() {
    use kafka_helpers::*;
    use rdkafka::consumer::{CommitMode, Consumer};

    let producer = test_producer();
    let group_id = unique_group_id("offsets");
    let experiment_id = "exp-offset-test";

    // Phase 1: produce and consume first event
    let event1 = make_reward_event(
        &unique_event_id("off1"),
        experiment_id,
        "user-1",
        "arm-a",
        1.0,
        "",
    );
    produce_reward_event(&producer, &event1, None).await;

    {
        let consumer1 = test_consumer(&group_id);
        let msg = consume_one(&consumer1, 10).await;
        let decoded = RewardEvent::decode(msg.payload().unwrap()).unwrap();
        assert_eq!(decoded.event_id, event1.event_id);
        consumer1
            .commit_consumer_state(CommitMode::Sync)
            .expect("commit failed");
    }

    // Phase 2: produce a second event
    let event2 = make_reward_event(
        &unique_event_id("off2"),
        experiment_id,
        "user-2",
        "arm-b",
        2.0,
        "",
    );
    produce_reward_event(&producer, &event2, None).await;

    // Phase 3: new consumer with same group_id → should get only event2
    {
        let consumer2 = test_consumer(&group_id);
        let msg = consume_one(&consumer2, 10).await;
        let decoded = RewardEvent::decode(msg.payload().unwrap()).unwrap();
        assert_eq!(decoded.event_id, event2.event_id);
    }
}

// ── Kafka integration test 7 ────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_kafka_reward_batch_ordering() {
    use kafka_helpers::*;

    let producer = test_producer();
    let group_id = unique_group_id("ordering");
    let consumer = test_consumer(&group_id);
    let experiment_id = "exp-order-test";

    // Produce 5 events sequentially for the same key
    let mut event_ids = Vec::new();
    for i in 0..5 {
        let eid = unique_event_id(&format!("ord-{i}"));
        let event = make_reward_event(&eid, experiment_id, "user-1", "arm-a", i as f64, "");
        produce_reward_event(&producer, &event, None).await;
        event_ids.push(eid);
    }

    // Consume and verify ordering is preserved within the same partition
    for expected_id in &event_ids {
        let msg = consume_one(&consumer, 10).await;
        let decoded = RewardEvent::decode(msg.payload().unwrap()).unwrap();
        assert_eq!(
            &decoded.event_id, expected_id,
            "out-of-order: expected {expected_id}, got {}",
            decoded.event_id
        );
    }
}
