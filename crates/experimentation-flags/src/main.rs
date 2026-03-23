//! Experimentation Feature Flag Service (M7) — Rust port (ADR-024).

use experimentation_flags::config::FlagsConfig;
use experimentation_flags::grpc;
use experimentation_flags::store::FlagStore;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "experimentation_flags=info".into()),
        )
        .json()
        .init();

    tracing::info!("starting experimentation-flags service (ADR-024 Rust port)");

    let config = FlagsConfig::from_env();
    tracing::info!(grpc_addr = %config.grpc_addr, "configuration loaded");

    let store = FlagStore::connect(&config.database_url)
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
