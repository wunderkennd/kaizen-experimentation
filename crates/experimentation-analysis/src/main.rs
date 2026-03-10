//! Experimentation Analysis Service (M4a) — Statistical inference.
//!
//! Batch analysis service that reads metric data from Delta Lake,
//! runs statistical tests via the experimentation-stats crate,
//! and serves results over gRPC.

mod config;
mod delta_reader;
mod grpc;

use config::AnalysisConfig;
use tracing::info;

#[tokio::main]
async fn main() {
    // 1. Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "experimentation_analysis=info".into()),
        )
        .json()
        .init();

    info!("Starting experimentation-analysis service");

    // 2. Load configuration
    let config = AnalysisConfig::from_env();
    info!(?config, "Configuration loaded");

    // 3. Start gRPC server
    if let Err(e) = grpc::serve_grpc(config).await {
        tracing::error!(error = %e, "gRPC server failed");
        std::process::exit(1);
    }
}
