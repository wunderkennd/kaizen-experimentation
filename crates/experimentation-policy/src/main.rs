//! Experimentation Policy Service (M4b) — Bandit arm selection.
//!
//! Architecture: LMAX-inspired single-threaded core (ADR-002).
//! - Thread 1 (tokio): gRPC server sends SelectArmRequests via channel
//! - Thread 2 (tokio): Kafka consumer sends RewardUpdates via channel
//! - Thread 3 (dedicated): PolicyCore event loop owns all mutable state
//!
//! Crash recovery: RocksDB snapshots (ADR-003).

mod config;
mod core;
mod grpc;
mod kafka;
pub mod snapshot;
pub mod types;

use config::PolicyConfig;
use core::PolicyCore;
use snapshot::SnapshotStore;
use std::path::Path;
use tokio::sync::mpsc;
use tracing::info;

#[tokio::main]
async fn main() {
    // 1. Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "experimentation_policy=info".into()),
        )
        .json()
        .init();

    info!("Starting experimentation-policy service");

    // 2. Load configuration
    let config = PolicyConfig::from_env();
    info!(?config, "Configuration loaded");

    // 3. Create bounded channels (gRPC -> core, Kafka -> core)
    let (policy_tx, policy_rx) = mpsc::channel(config.policy_channel_depth);
    let (reward_tx, reward_rx) = mpsc::channel(config.reward_channel_depth);

    // 4. Open RocksDB and create PolicyCore
    let snapshot_store = SnapshotStore::open(Path::new(&config.rocksdb_path))
        .expect("failed to open RocksDB snapshot store");

    let mut policy_core = PolicyCore::new(snapshot_store, config.clone());

    // 5. Restore from snapshots (crash recovery)
    match policy_core.restore_from_snapshots() {
        Ok(count) => info!(count, "Restored policies from RocksDB"),
        Err(e) => tracing::error!(error = %e, "Failed to restore from snapshots"),
    }

    // 6. Spawn dedicated thread for PolicyCore event loop
    let core_handle = std::thread::Builder::new()
        .name("policy-core".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build single-threaded runtime for policy core");
            rt.block_on(policy_core.run(policy_rx, reward_rx));
        })
        .expect("failed to spawn policy core thread");

    // 7. Spawn Kafka consumer (stub)
    let kafka_handle = tokio::spawn(kafka::consume_rewards(reward_tx));

    // 8. Start gRPC server (stub)
    let grpc_addr = config.grpc_addr.clone();
    let grpc_handle = tokio::spawn(grpc::serve_grpc(grpc_addr));

    info!("Policy service running. Press Ctrl-C to shutdown.");

    // 9. Wait for shutdown signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install ctrl-c handler");
    info!("Shutdown signal received");

    // Drop the policy_tx sender to signal the core to stop
    drop(policy_tx);

    // Wait for tasks
    let _ = kafka_handle.await;
    let _ = grpc_handle.await;

    // Wait for the core thread to finish
    core_handle.join().expect("policy core thread panicked");

    info!("Policy service stopped");
}
