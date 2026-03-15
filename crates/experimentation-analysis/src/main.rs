//! Experimentation Analysis Service (M4a) — Statistical inference.
//!
//! Batch analysis service that reads metric data from Delta Lake,
//! runs statistical tests via the experimentation-stats crate,
//! and serves results over gRPC.

use experimentation_analysis::config::AnalysisConfig;
use experimentation_analysis::grpc;
use experimentation_analysis::store::AnalysisStore;
use std::sync::Arc;
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

    // 3. Connect to PostgreSQL (optional — caching disabled if DATABASE_URL not set)
    let store = match &config.database_url {
        Some(url) => match AnalysisStore::connect(url).await {
            Ok(s) => {
                info!("PostgreSQL cache connected");
                Some(Arc::new(s))
            }
            Err(e) => {
                tracing::warn!(error = %e, "PostgreSQL cache unavailable, running without cache");
                None
            }
        },
        None => {
            info!("DATABASE_URL not set, running without PostgreSQL cache");
            None
        }
    };

    // 4. Start gRPC server
    if let Err(e) = grpc::serve_grpc(config, store).await {
        tracing::error!(error = %e, "gRPC server failed");
        std::process::exit(1);
    }
}
