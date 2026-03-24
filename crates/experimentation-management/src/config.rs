//! Configuration for the Management Service (M5) loaded from environment variables.

/// Configuration for the Management Service.
#[derive(Debug, Clone)]
pub struct ManagementConfig {
    /// gRPC listen address (e.g. "0.0.0.0:50055").
    pub grpc_addr: String,
    /// PostgreSQL connection URL.
    pub database_url: String,
    /// Maximum PostgreSQL pool connections.
    pub db_pool_max: u32,
}

impl ManagementConfig {
    /// Load configuration from environment variables.
    ///
    /// Required: `DATABASE_URL`
    /// Optional: `MANAGEMENT_GRPC_ADDR` (default: "0.0.0.0:50055")
    pub fn from_env() -> Self {
        Self {
            grpc_addr: std::env::var("MANAGEMENT_GRPC_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:50055".to_string()),
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://localhost/experimentation".to_string()),
            db_pool_max: std::env::var("DB_POOL_MAX")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
        }
    }
}
