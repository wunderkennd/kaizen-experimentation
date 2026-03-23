//! Feature flag service configuration from environment variables.

/// Configuration for the feature flag service (M7).
#[derive(Debug, Clone)]
pub struct FlagsConfig {
    /// gRPC + tonic-web listen address (default: [::]:50057).
    pub grpc_addr: String,
    /// PostgreSQL connection URL — required.
    pub database_url: String,
    /// M5 Management service gRPC address for PromoteToExperiment (Phase 2).
    pub m5_addr: Option<String>,
    /// Default layer ID for promoted experiments (default: "default").
    pub default_layer_id: String,

    // --- Phase 3: Kafka reconciler ---
    /// Kafka broker list (default: localhost:9092).
    pub kafka_brokers: String,
    /// Kafka consumer group for the lifecycle topic (default: flags-reconciler).
    pub kafka_group_id: String,
    /// Kafka topic for experiment lifecycle events (default: experiment_lifecycle).
    pub kafka_lifecycle_topic: String,
    /// Whether to start the Kafka consumer (default: false — polling reconciler is primary).
    pub kafka_enabled: bool,
    /// Polling reconciler interval in seconds (default: 60).
    pub reconciler_interval_secs: u64,
    /// Default resolution action: rollout_full | rollback | keep (default: rollout_full).
    pub reconciler_default_action: String,

    // --- Admin HTTP server ---
    /// Internal HTTP admin server address (default: [::]:9090).
    pub admin_addr: String,
}

impl FlagsConfig {
    pub fn from_env() -> Self {
        Self {
            grpc_addr: std::env::var("FLAGS_GRPC_ADDR")
                .unwrap_or_else(|_| "[::]:50057".into()),
            database_url: std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set for the flags service"),
            m5_addr: std::env::var("M5_ADDR").ok(),
            default_layer_id: std::env::var("FLAGS_DEFAULT_LAYER_ID")
                .unwrap_or_else(|_| "default".into()),

            kafka_brokers: std::env::var("KAFKA_BROKERS")
                .unwrap_or_else(|_| "localhost:9092".into()),
            kafka_group_id: std::env::var("FLAGS_KAFKA_GROUP_ID")
                .unwrap_or_else(|_| "flags-reconciler".into()),
            kafka_lifecycle_topic: std::env::var("FLAGS_KAFKA_LIFECYCLE_TOPIC")
                .unwrap_or_else(|_| "experiment_lifecycle".into()),
            kafka_enabled: std::env::var("FLAGS_KAFKA_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            reconciler_interval_secs: std::env::var("FLAGS_RECONCILER_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            reconciler_default_action: std::env::var("FLAGS_RECONCILER_DEFAULT_ACTION")
                .unwrap_or_else(|_| "rollout_full".into()),

            admin_addr: std::env::var("FLAGS_ADMIN_ADDR")
                .unwrap_or_else(|_| "[::]:9090".into()),
        }
    }
}
