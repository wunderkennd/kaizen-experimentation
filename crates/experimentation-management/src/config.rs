//! Configuration loaded from environment variables.

pub struct ManagementConfig {
    /// PostgreSQL connection URL.
    pub database_url: String,
    /// gRPC listen address (host:port).
    pub grpc_addr: String,
    /// Kafka broker list (comma-separated). Used by guardrail consumer.
    pub kafka_brokers: String,
    /// Kafka consumer group ID for the guardrail consumer.
    pub kafka_group_id: String,
    /// Topic for guardrail alerts published by M3.
    pub kafka_guardrail_topic: String,
    /// Topic for lifecycle events published by M5 (for M7 reconciler).
    pub kafka_lifecycle_topic: String,
    /// Whether the guardrail Kafka consumer is enabled.
    pub kafka_enabled: bool,
}

impl ManagementConfig {
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/experimentation".into()),
            grpc_addr: std::env::var("MANAGEMENT_GRPC_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:50055".into()),
            kafka_brokers: std::env::var("KAFKA_BROKERS")
                .unwrap_or_else(|_| "localhost:9092".into()),
            kafka_group_id: std::env::var("KAFKA_GROUP_ID")
                .unwrap_or_else(|_| "management-guardrail".into()),
            kafka_guardrail_topic: std::env::var("KAFKA_GUARDRAIL_TOPIC")
                .unwrap_or_else(|_| "guardrail_alerts".into()),
            kafka_lifecycle_topic: std::env::var("KAFKA_LIFECYCLE_TOPIC")
                .unwrap_or_else(|_| "experiment_lifecycle".into()),
            kafka_enabled: std::env::var("KAFKA_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
        }
    }
}
