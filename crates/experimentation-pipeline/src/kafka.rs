//! Kafka producer wrapper with idempotent delivery and backpressure handling.

use rdkafka::config::ClientConfig;
use rdkafka::error::KafkaError;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::time::Duration;
use tracing::info;

/// Kafka topic names matching topic_configs.sh.
pub const TOPIC_EXPOSURES: &str = "exposures";
pub const TOPIC_METRIC_EVENTS: &str = "metric_events";
pub const TOPIC_REWARD_EVENTS: &str = "reward_events";
pub const TOPIC_QOE_EVENTS: &str = "qoe_events";

/// Configuration for the Kafka producer.
pub struct KafkaConfig {
    pub brokers: String,
    pub linger_ms: u32,
    pub queue_buffering_max_messages: u32,
}

impl Default for KafkaConfig {
    fn default() -> Self {
        Self {
            brokers: "localhost:9092".to_string(),
            linger_ms: 0, // p99 < 10ms target; configurable via env
            queue_buffering_max_messages: 100_000,
        }
    }
}

/// Wraps an rdkafka FutureProducer with idempotent delivery.
pub struct EventProducer {
    producer: FutureProducer,
}

/// Errors from the produce path.
#[derive(Debug)]
pub enum ProduceError {
    /// Kafka internal queue is full — client should back off.
    QueueFull,
    /// Other Kafka error.
    Kafka(String),
}

impl std::fmt::Display for ProduceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProduceError::QueueFull => write!(f, "Kafka producer queue full"),
            ProduceError::Kafka(msg) => write!(f, "Kafka error: {msg}"),
        }
    }
}

impl EventProducer {
    /// Create a new idempotent Kafka producer.
    pub fn new(config: &KafkaConfig) -> Result<Self, KafkaError> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .set("enable.idempotence", "true")
            .set("acks", "all")
            .set("compression.type", "lz4")
            .set("queue.buffering.max.ms", config.linger_ms.to_string())
            .set(
                "queue.buffering.max.messages",
                config.queue_buffering_max_messages.to_string(),
            )
            .create()?;

        info!(
            brokers = %config.brokers,
            linger_ms = config.linger_ms,
            "Kafka producer initialized with idempotent delivery"
        );

        Ok(Self { producer })
    }

    /// Publish a serialized protobuf payload to a topic.
    ///
    /// `key` determines the Kafka partition (e.g. experiment_id for exposures,
    /// user_id for metric_events).
    pub async fn publish(
        &self,
        topic: &str,
        key: &str,
        payload: &[u8],
    ) -> Result<(), ProduceError> {
        let record = FutureRecord::to(topic).key(key).payload(payload);

        match self.producer.send(record, Duration::from_secs(0)).await {
            Ok(_) => Ok(()),
            Err((
                KafkaError::MessageProduction(rdkafka::types::RDKafkaErrorCode::QueueFull),
                _,
            )) => Err(ProduceError::QueueFull),
            Err((e, _)) => Err(ProduceError::Kafka(e.to_string())),
        }
    }
}
