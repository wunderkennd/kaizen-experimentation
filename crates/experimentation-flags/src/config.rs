//! Feature flag service configuration from environment variables.

/// Configuration for the feature flag service (M7).
#[derive(Debug, Clone)]
pub struct FlagsConfig {
    /// gRPC + tonic-web listen address (default: [::]:50057).
    pub grpc_addr: String,
    /// PostgreSQL connection URL — required for the store.
    pub database_url: String,
    /// M5 Management service address for PromoteToExperiment (Phase 2).
    pub m5_addr: Option<String>,
}

impl FlagsConfig {
    pub fn from_env() -> Self {
        Self {
            grpc_addr: std::env::var("FLAGS_GRPC_ADDR")
                .unwrap_or_else(|_| "[::]:50057".into()),
            database_url: std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set for the flags service"),
            m5_addr: std::env::var("M5_ADDR").ok(),
        }
    }
}
