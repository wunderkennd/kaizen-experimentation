//! Experimentation Feature Flag Service (M7) — Rust port (ADR-024).
//!
//! Startup sequence:
//!   1. Load config from env.
//!   2. Connect PostgreSQL (sqlx pool, shared via PgPool clone-by-arc).
//!   3. Build AuditStore from same pool.
//!   4. Start admin HTTP server (axum, separate port).
//!   5. Build polling reconciler (if M5_ADDR configured).
//!   6. Optionally start Kafka lifecycle consumer (FLAGS_KAFKA_ENABLED=true).
//!   7. Serve gRPC + tonic-web (blocking).

use std::sync::Arc;
use std::time::Duration;

use tracing::info;

use experimentation_flags::admin::{admin_router, AdminState};
use experimentation_flags::audit::AuditStore;
use experimentation_flags::config::FlagsConfig;
use experimentation_flags::grpc;
use experimentation_flags::reconciler::{Reconciler, ResolutionAction};
use experimentation_flags::store::FlagStore;
use experimentation_proto::experimentation::management::v1::experiment_management_service_client::ExperimentManagementServiceClient;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "experimentation_flags=info".into()),
        )
        .json()
        .init();

    info!("starting experimentation-flags service (ADR-024 Rust port)");

    let config = FlagsConfig::from_env();
    info!(
        grpc_addr = %config.grpc_addr,
        admin_addr = %config.admin_addr,
        kafka_enabled = %config.kafka_enabled,
        "configuration loaded"
    );

    let store = FlagStore::connect(&config.database_url)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to connect to PostgreSQL");
            std::process::exit(1);
        });

    // PgPool is Clone (Arc-backed). Share the same pool across all components.
    let pool = store.pool().clone();
    let store = Arc::new(store);
    let audit = Arc::new(AuditStore::new(pool.clone()));

    // --- Admin HTTP server ---
    {
        let admin_state = AdminState {
            store: store.clone(),
            audit: Some(audit.clone()),
        };
        let router = admin_router(admin_state);
        let admin_addr: std::net::SocketAddr = config.admin_addr.parse().unwrap_or_else(|e| {
            tracing::error!(error = %e, "invalid FLAGS_ADMIN_ADDR");
            std::process::exit(1);
        });
        tokio::spawn(async move {
            info!(%admin_addr, "admin HTTP server starting");
            match tokio::net::TcpListener::bind(admin_addr).await {
                Ok(listener) => {
                    if let Err(e) = axum::serve(listener, router).await {
                        tracing::error!(error = %e, "admin server error");
                    }
                }
                Err(e) => tracing::error!(error = %e, "failed to bind admin addr"),
            }
        });
    }

    // --- Reconciler + Kafka (only when M5_ADDR is configured) ---
    if let Some(ref m5_addr) = config.m5_addr {
        let channel = match tonic::transport::Channel::from_shared(m5_addr.clone()) {
            Ok(ep) => match ep.connect().await {
                Ok(ch) => {
                    info!(%m5_addr, "connected to M5 management service");
                    Some(ch)
                }
                Err(e) => {
                    tracing::warn!(error = %e, %m5_addr, "M5 connection failed — reconciler disabled");
                    None
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "invalid M5_ADDR — reconciler disabled");
                None
            }
        };

        if let Some(channel) = channel {
            let m5_client = ExperimentManagementServiceClient::new(channel);
            let default_action =
                ResolutionAction::from_str(&config.reconciler_default_action)
                    .unwrap_or(ResolutionAction::RolloutFull);
            let interval = Duration::from_secs(config.reconciler_interval_secs);

            // Polling reconciler (primary).
            let reconciler = Reconciler::new(
                store.clone(),
                Some(audit.clone()),
                m5_client.clone(),
                interval,
                default_action,
            );
            tokio::spawn(reconciler.run());

            // Kafka lifecycle consumer (optional).
            if config.kafka_enabled {
                let store_k = store.clone();
                let audit_k = audit.clone();
                let brokers = config.kafka_brokers.clone();
                let group = config.kafka_group_id.clone();
                let topic = config.kafka_lifecycle_topic.clone();
                let reconciler_k = Arc::new(tokio::sync::Mutex::new(Reconciler::new(
                    store_k.clone(),
                    Some(audit_k),
                    m5_client,
                    interval,
                    default_action,
                )));
                tokio::spawn(async move {
                    if let Err(e) = experimentation_flags::kafka::consume_lifecycle_events(
                        store_k,
                        reconciler_k,
                        &brokers,
                        &group,
                        &topic,
                    )
                    .await
                    {
                        tracing::error!(error = %e, "Kafka lifecycle consumer failed");
                    }
                });
            }
        }
    }

    // --- gRPC server (blocking) ---
    // Re-use the shared pool so we don't open a second connection.
    let grpc_store = FlagStore::from_pool(pool);
    if let Err(e) = grpc::serve(config, grpc_store, Some(audit)).await {
        tracing::error!(error = %e, "gRPC server failed");
        std::process::exit(1);
    }
}
