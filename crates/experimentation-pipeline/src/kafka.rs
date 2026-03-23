//! Kafka producer wrapper with idempotent delivery and backpressure handling.

use prometheus::Histogram;
use rdkafka::config::ClientConfig;
use rdkafka::error::KafkaError;
use rdkafka::message::OwnedHeaders;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::info;

/// Kafka header key for ingest timestamp (epoch milliseconds).
/// Downstream consumers use this to measure end-to-end pipeline latency.
pub const HEADER_INGEST_TS_MS: &str = "x-ingest-ts-ms";

/// Kafka header key for event type (exposure, metric, reward, qoe).
pub const HEADER_EVENT_TYPE: &str = "x-event-type";

/// W3C Trace Context header. Propagated from incoming gRPC metadata to Kafka
/// so downstream consumers (M3, M4a) can correlate traces back to ingest.
pub const HEADER_TRACEPARENT: &str = "traceparent";

/// Kafka topic names matching topic_configs.sh.
pub const TOPIC_EXPOSURES: &str = "exposures";
pub const TOPIC_METRIC_EVENTS: &str = "metric_events";
pub const TOPIC_REWARD_EVENTS: &str = "reward_events";
pub const TOPIC_QOE_EVENTS: &str = "qoe_events";
/// ADR-021: Model retraining events for feedback loop interference detection.
pub const TOPIC_MODEL_RETRAINING_EVENTS: &str = "model_retraining_events";

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

/// Trait abstracting the Kafka publish path.
///
/// Implemented by `EventProducer` for production use. Tests inject a mock
/// implementation to validate the service layer without a running Kafka broker.
#[tonic::async_trait]
pub trait Producer: Send + Sync {
    async fn publish(
        &self,
        topic: &str,
        key: &str,
        payload: &[u8],
        latency_histogram: Option<&Histogram>,
    ) -> Result<(), ProduceError>;

    async fn publish_with_headers(
        &self,
        topic: &str,
        key: &str,
        payload: &[u8],
        latency_histogram: Option<&Histogram>,
        event_type: Option<&str>,
        traceparent: Option<&str>,
    ) -> Result<(), ProduceError>;
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
    /// Other Kafka error (broker unreachable, serialization, etc.).
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

impl ProduceError {
    /// Returns true if this error indicates Kafka broker is unreachable
    /// (as opposed to a transient queue-full condition).
    pub fn is_broker_unreachable(&self) -> bool {
        matches!(self, ProduceError::Kafka(_))
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
}

#[tonic::async_trait]
impl Producer for EventProducer {
    async fn publish(
        &self,
        topic: &str,
        key: &str,
        payload: &[u8],
        latency_histogram: Option<&Histogram>,
    ) -> Result<(), ProduceError> {
        self.publish_with_headers(topic, key, payload, latency_histogram, None, None)
            .await
    }

    async fn publish_with_headers(
        &self,
        topic: &str,
        key: &str,
        payload: &[u8],
        latency_histogram: Option<&Histogram>,
        event_type: Option<&str>,
        traceparent: Option<&str>,
    ) -> Result<(), ProduceError> {
        let ingest_ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string();

        let mut headers = OwnedHeaders::new().insert(rdkafka::message::Header {
            key: HEADER_INGEST_TS_MS,
            value: Some(ingest_ts_ms.as_bytes()),
        });
        if let Some(et) = event_type {
            headers = headers.insert(rdkafka::message::Header {
                key: HEADER_EVENT_TYPE,
                value: Some(et.as_bytes()),
            });
        }
        if let Some(tp) = traceparent {
            headers = headers.insert(rdkafka::message::Header {
                key: HEADER_TRACEPARENT,
                value: Some(tp.as_bytes()),
            });
        }

        let record = FutureRecord::to(topic)
            .key(key)
            .payload(payload)
            .headers(headers);

        let start = Instant::now();
        let result = match self.producer.send(record, Duration::from_secs(0)).await {
            Ok(_) => Ok(()),
            Err((
                KafkaError::MessageProduction(rdkafka::types::RDKafkaErrorCode::QueueFull),
                _,
            )) => Err(ProduceError::QueueFull),
            Err((e, _)) => Err(ProduceError::Kafka(e.to_string())),
        };

        if let Some(histogram) = latency_histogram {
            histogram.observe(start.elapsed().as_secs_f64());
        }

        result
    }
}
