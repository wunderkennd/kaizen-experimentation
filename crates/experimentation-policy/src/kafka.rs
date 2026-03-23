//! Kafka consumer for reward events.
//!
//! Subscribes to the `reward_events` topic and forwards decoded `RewardUpdate`
//! messages to the PolicyCore via a bounded mpsc channel. Uses manual offset
//! commits for at-least-once delivery semantics.

use crate::config::PolicyConfig;
use crate::types::RewardUpdate;
use experimentation_core::error::assert_finite;
use prost::Message;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::Message as KafkaMessage;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Start the Kafka reward consumer.
///
/// Connects to the broker, subscribes to the reward topic, and enters the
/// consume loop. Returns `Err` only if initial setup fails (broker
/// unreachable, subscription error). Runtime errors inside the loop are
/// handled per the error matrix (skip, panic, or retry).
pub async fn consume_rewards(
    reward_tx: mpsc::Sender<RewardUpdate>,
    config: &PolicyConfig,
) -> Result<(), String> {
    info!(
        brokers = %config.kafka_brokers,
        group_id = %config.kafka_group_id,
        topic = %config.kafka_reward_topic,
        "Starting Kafka reward consumer"
    );

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &config.kafka_brokers)
        .set("group.id", &config.kafka_group_id)
        .set("auto.offset.reset", &config.kafka_auto_offset_reset)
        .set("enable.auto.commit", "false")
        .set("enable.partition.eof", "false")
        .set("session.timeout.ms", "30000")
        .set("fetch.min.bytes", "1")
        .set("fetch.wait.max.ms", "500")
        .create()
        .map_err(|e| format!("failed to create Kafka consumer: {e}"))?;

    consumer
        .subscribe(&[&config.kafka_reward_topic])
        .map_err(|e| format!("failed to subscribe to {}: {e}", config.kafka_reward_topic))?;

    info!("Kafka reward consumer connected, entering consume loop");

    consume_loop(
        &consumer,
        &reward_tx,
        config.kafka_commit_batch_size,
        Duration::from_secs(config.kafka_commit_interval_secs),
    )
    .await;

    Ok(())
}

/// Main consume loop. Runs until the reward channel is closed (shutdown).
async fn consume_loop(
    consumer: &StreamConsumer,
    reward_tx: &mpsc::Sender<RewardUpdate>,
    commit_batch_size: usize,
    commit_interval: Duration,
) {
    let mut messages_since_commit: usize = 0;
    let mut last_commit_time = Instant::now();

    loop {
        // Check for periodic commit before blocking on recv
        if should_commit(messages_since_commit, commit_batch_size, last_commit_time, commit_interval)
        {
            commit_offsets(consumer, &mut messages_since_commit, &mut last_commit_time);
        }

        let message = match tokio::time::timeout(Duration::from_secs(1), consumer.recv()).await {
            Ok(Ok(msg)) => msg,
            Ok(Err(e)) => {
                error!(error = %e, "Kafka consumer error");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            Err(_) => {
                // Timeout — allows periodic commit check
                continue;
            }
        };

        let payload = match message.payload() {
            Some(bytes) if !bytes.is_empty() => bytes,
            Some(_) => {
                warn!(
                    partition = message.partition(),
                    offset = message.offset(),
                    "Empty payload in reward event, skipping"
                );
                messages_since_commit += 1;
                continue;
            }
            None => {
                warn!(
                    partition = message.partition(),
                    offset = message.offset(),
                    "Null payload in reward event, skipping"
                );
                messages_since_commit += 1;
                continue;
            }
        };

        let reward_event = match decode_reward_event(payload) {
            Ok(event) => event,
            Err(e) => {
                error!(
                    error = %e,
                    partition = message.partition(),
                    offset = message.offset(),
                    "Failed to decode RewardEvent, skipping"
                );
                messages_since_commit += 1;
                continue;
            }
        };

        // Fail-fast: NaN/Infinity reward is a data integrity violation
        assert_finite(
            reward_event.reward,
            &format!(
                "RewardEvent reward for experiment={} arm={}",
                reward_event.experiment_id, reward_event.arm_id
            ),
        );

        let context = parse_context_json(&reward_event.context_json);

        let update = RewardUpdate {
            experiment_id: reward_event.experiment_id,
            arm_id: reward_event.arm_id,
            reward: reward_event.reward,
            context,
            kafka_offset: message.offset(),
               metric_values: None,
        };

        // Send to PolicyCore; blocks if channel is full (backpressure)
        if reward_tx.send(update).await.is_err() {
            info!("Reward channel closed, shutting down Kafka consumer");
            break;
        }

        messages_since_commit += 1;
    }

    // Final commit before exit
    if messages_since_commit > 0 {
        commit_offsets(consumer, &mut messages_since_commit, &mut last_commit_time);
    }
}

/// Decode a protobuf `RewardEvent` from raw bytes.
fn decode_reward_event(
    payload: &[u8],
) -> Result<experimentation_proto::experimentation::common::v1::RewardEvent, prost::DecodeError> {
    experimentation_proto::experimentation::common::v1::RewardEvent::decode(payload)
}

/// Parse a JSON string into context features for contextual bandits.
///
/// Returns `None` if the string is empty, invalid JSON, or contains no
/// numeric values. Non-f64 values are silently filtered.
fn parse_context_json(json_str: &str) -> Option<HashMap<String, f64>> {
    if json_str.is_empty() {
        return None;
    }

    let parsed: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, json = %json_str, "Failed to parse context_json");
            return None;
        }
    };

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

/// Check whether we should commit offsets based on batch size or time.
fn should_commit(
    messages_since_commit: usize,
    commit_batch_size: usize,
    last_commit_time: Instant,
    commit_interval: Duration,
) -> bool {
    if messages_since_commit == 0 {
        return false;
    }
    messages_since_commit >= commit_batch_size || last_commit_time.elapsed() >= commit_interval
}

/// Commit consumer offsets asynchronously and reset counters.
fn commit_offsets(
    consumer: &StreamConsumer,
    messages_since_commit: &mut usize,
    last_commit_time: &mut Instant,
) {
    if let Err(e) = consumer.commit_consumer_state(CommitMode::Async) {
        error!(error = %e, "Failed to commit Kafka offsets");
    }
    *messages_since_commit = 0;
    *last_commit_time = Instant::now();
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    // ── context JSON parsing ──────────────────────────────────────────

    #[test]
    fn test_parse_context_json_empty() {
        assert!(parse_context_json("").is_none());
    }

    #[test]
    fn test_parse_context_json_valid() {
        let json = r#"{"age": 25.0, "score": 0.95}"#;
        let ctx = parse_context_json(json).unwrap();
        assert_eq!(ctx.len(), 2);
        assert_eq!(ctx["age"], 25.0);
        assert_eq!(ctx["score"], 0.95);
    }

    #[test]
    fn test_parse_context_json_mixed_types() {
        let json = r#"{"age": 25.0, "name": "alice", "active": true}"#;
        let ctx = parse_context_json(json).unwrap();
        assert_eq!(ctx.len(), 1);
        assert_eq!(ctx["age"], 25.0);
    }

    #[test]
    fn test_parse_context_json_all_non_f64() {
        let json = r#"{"name": "alice", "active": true}"#;
        assert!(parse_context_json(json).is_none());
    }

    #[test]
    fn test_parse_context_json_invalid_json() {
        assert!(parse_context_json("{not valid json}").is_none());
    }

    #[test]
    fn test_parse_context_json_integers() {
        let json = r#"{"count": 42, "ratio": 3.14}"#;
        let ctx = parse_context_json(json).unwrap();
        assert_eq!(ctx.len(), 2);
        assert_eq!(ctx["count"], 42.0);
        assert_eq!(ctx["ratio"], 3.14);
    }

    // ── protobuf roundtrip ────────────────────────────────────────────

    #[test]
    fn test_reward_event_roundtrip() {
        let event = experimentation_proto::experimentation::common::v1::RewardEvent {
            event_id: "evt-1".into(),
            experiment_id: "exp-1".into(),
            user_id: "user-1".into(),
            arm_id: "arm-a".into(),
            reward: 1.0,
            timestamp: None,
            context_json: r#"{"x": 0.5}"#.into(),
        };
        let bytes = event.encode_to_vec();
        let decoded = decode_reward_event(&bytes).unwrap();
        assert_eq!(decoded.experiment_id, "exp-1");
        assert_eq!(decoded.arm_id, "arm-a");
        assert_eq!(decoded.reward, 1.0);
        assert_eq!(decoded.context_json, r#"{"x": 0.5}"#);
    }

    #[test]
    fn test_reward_event_to_update_with_context() {
        let event = experimentation_proto::experimentation::common::v1::RewardEvent {
            event_id: "evt-2".into(),
            experiment_id: "exp-2".into(),
            user_id: "user-2".into(),
            arm_id: "arm-b".into(),
            reward: 0.75,
            timestamp: None,
            context_json: r#"{"feature_a": 1.5, "feature_b": 2.0}"#.into(),
        };
        let context = parse_context_json(&event.context_json);
        let update = RewardUpdate {
            experiment_id: event.experiment_id.clone(),
            arm_id: event.arm_id.clone(),
            reward: event.reward,
            context,
            kafka_offset: 42,
            metric_values: None,
        };
        assert_eq!(update.experiment_id, "exp-2");
        assert_eq!(update.arm_id, "arm-b");
        assert_eq!(update.reward, 0.75);
        assert!(update.context.is_some());
        let ctx = update.context.unwrap();
        assert_eq!(ctx["feature_a"], 1.5);
        assert_eq!(ctx["feature_b"], 2.0);
        assert_eq!(update.kafka_offset, 42);
    }

    #[test]
    fn test_reward_event_to_update_no_context() {
        let event = experimentation_proto::experimentation::common::v1::RewardEvent {
            event_id: "evt-3".into(),
            experiment_id: "exp-3".into(),
            user_id: "user-3".into(),
            arm_id: "arm-c".into(),
            reward: 0.0,
            timestamp: None,
            context_json: String::new(),
        };
        let context = parse_context_json(&event.context_json);
        let update = RewardUpdate {
            experiment_id: event.experiment_id.clone(),
            arm_id: event.arm_id.clone(),
            reward: event.reward,
            context,
            kafka_offset: 0,
            metric_values: None,
        };
        assert!(update.context.is_none());
        assert_eq!(update.reward, 0.0);
    }

    // ── fail-fast on non-finite rewards ───────────────────────────────

    #[test]
    #[should_panic(expected = "FAIL-FAST: non-finite value")]
    fn test_nan_reward_panics() {
        assert_finite(f64::NAN, "test NaN reward");
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST: non-finite value")]
    fn test_infinity_reward_panics() {
        assert_finite(f64::INFINITY, "test Infinity reward");
    }

    // ── malformed / empty payloads ────────────────────────────────────

    #[test]
    fn test_decode_malformed_payload() {
        let garbage = b"this is not protobuf";
        let result = decode_reward_event(garbage);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_empty_payload() {
        // Empty bytes decode to a default RewardEvent (all fields zeroed/empty)
        let result = decode_reward_event(&[]);
        let event = result.unwrap();
        assert_eq!(event.experiment_id, "");
        assert_eq!(event.arm_id, "");
        assert_eq!(event.reward, 0.0);
    }
}
