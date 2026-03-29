//! Experimentation Management Service (M5) — Rust port binary (ADR-025).
//!
//! Startup sequence:
//!   1. Load config from env.
//!   2. Connect PostgreSQL (sqlx pool).
//!   3. Optionally start guardrail Kafka consumer.
//!   4. Serve gRPC + tonic-web (blocking).

use std::sync::Arc;

use tracing::info;

use experimentation_management::config::ManagementConfig;
use experimentation_management::grpc;
use experimentation_management::kafka;
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

    info!("starting experimentation-management service (ADR-025 Phase 2)");

    let config = ManagementConfig::from_env();
    info!(
        grpc_addr = %config.grpc_addr,
        kafka_enabled = %config.kafka_enabled,
        "configuration loaded"
    );

    let store = ManagementStore::connect(&config.database_url)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to connect to PostgreSQL");
            std::process::exit(1);
        });

    let store_arc = Arc::new(store.clone());

    // --- Guardrail Kafka consumer (optional) ---
    if config.kafka_enabled {
        let store_k = store_arc.clone();
        let brokers = config.kafka_brokers.clone();
        let group = config.kafka_group_id.clone();
        let alert_topic = config.kafka_guardrail_topic.clone();
        let lifecycle_topic = config.kafka_lifecycle_topic.clone();

        tokio::spawn(async move {
            if let Err(e) = kafka::consume_guardrail_alerts(
                store_k,
                &brokers,
                &group,
                &alert_topic,
                &lifecycle_topic,
            )
            .await
            {
                tracing::error!(error = %e, "guardrail Kafka consumer failed");
            }
        });
    }

    // --- gRPC server (blocking) ---
    if let Err(e) = grpc::serve(&config, store).await {
        tracing::error!(error = %e, "gRPC server failed");
        std::process::exit(1);
    }
}
