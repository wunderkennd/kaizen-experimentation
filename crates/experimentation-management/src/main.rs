//! Experimentation Management Service (M5) — Rust port (ADR-025).
//!
//! Startup sequence:
//!   1. Load config from environment variables.
//!   2. Connect PostgreSQL pool.
//!   3. Start gRPC server with RBAC interceptor + tonic-web (blocking).

use tracing::info;

use experimentation_management::config::ManagementConfig;
use experimentation_management::grpc;
use experimentation_management::store::ManagementStore;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "experimentation_management=info".into()),
        )
        .json()
        .init();

    info!("starting experimentation-management service (ADR-025 Phase 1 Rust port)");

    let config = ManagementConfig::from_env();
    info!(
        grpc_addr = %config.grpc_addr,
        db_pool_max = %config.db_pool_max,
        "configuration loaded"
    );

    let store = ManagementStore::connect(&config.database_url, config.db_pool_max)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to connect to PostgreSQL");
            std::process::exit(1);
        });

    if let Err(e) = grpc::serve(config, store).await {
        tracing::error!(error = %e, "gRPC server failed");
        std::process::exit(1);
    }
}
