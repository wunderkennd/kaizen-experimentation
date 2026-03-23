//! Kafka consumer for experiment lifecycle events (ADR-024 Phase 3).
//!
//! Subscribes to the `experiment_lifecycle` topic. Messages are JSON-encoded
//! (same convention as `surrogate_recalibration_requests`):
//!   {"experiment_id": "<uuid>", "state": "CONCLUDED"}
//!
//! On receiving a terminal state (CONCLUDED or ARCHIVED), triggers an
//! immediate reconciliation pass for the associated flag via the Reconciler.
//!
//! Consumer group: `flags-reconciler` — separate from any existing consumer
//! groups to prevent offset interference during shadow-traffic migration.

use std::sync::Arc;
use std::time::Duration;

use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::Message as KafkaMessage;
use serde::Deserialize;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::reconciler::Reconciler;
use crate::store::FlagStore;

// ---------------------------------------------------------------------------
// Event schema (JSON, same convention as surrogate_recalibration_requests)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ExperimentLifecycleEvent {
    experiment_id: String,
    state: String,
}

const TERMINAL_STATES: &[&str] = &["CONCLUDED", "ARCHIVED",
    "EXPERIMENT_STATE_CONCLUDED", "EXPERIMENT_STATE_ARCHIVED"];

// ---------------------------------------------------------------------------
// Consumer entrypoint
// ---------------------------------------------------------------------------

/// Start the Kafka consumer for experiment lifecycle events.
///
/// Connects to the broker, subscribes to the `experiment_lifecycle` topic,
/// and on each terminal event triggers the reconciler's ad-hoc reconciliation
/// for the associated flag. Returns Err only on initial setup failure.
pub async fn consume_lifecycle_events(
    store: Arc<FlagStore>,
    reconciler: Arc<tokio::sync::Mutex<Reconciler>>,
    brokers: &str,
    group_id: &str,
    topic: &str,
) -> Result<(), String> {
    info!(%brokers, %group_id, %topic, "lifecycle Kafka consumer starting");

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .set("group.id", group_id)
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false")
        .set("enable.partition.eof", "false")
        .set("session.timeout.ms", "30000")
        .set("fetch.min.bytes", "1")
        .set("fetch.wait.max.ms", "500")
        .create()
        .map_err(|e| format!("create Kafka consumer: {e}"))?;

    consumer
        .subscribe(&[topic])
        .map_err(|e| format!("subscribe to {topic}: {e}"))?;

    info!("lifecycle Kafka consumer connected");

    consume_loop(&consumer, store, reconciler).await;
    Ok(())
}

async fn consume_loop(
    consumer: &StreamConsumer,
    store: Arc<FlagStore>,
    reconciler: Arc<tokio::sync::Mutex<Reconciler>>,
) {
    let mut messages_since_commit: usize = 0;
    let mut last_commit = std::time::Instant::now();
    let commit_interval = Duration::from_secs(10);

    loop {
        // Periodic commit.
        if messages_since_commit > 0 && last_commit.elapsed() >= commit_interval {
            if let Err(e) = consumer.commit_consumer_state(CommitMode::Async) {
                error!(error = %e, "lifecycle consumer: commit failed");
            }
            messages_since_commit = 0;
            last_commit = std::time::Instant::now();
        }

        let msg = match tokio::time::timeout(Duration::from_secs(1), consumer.recv()).await {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => {
                error!(error = %e, "lifecycle consumer: recv error");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            Err(_) => continue, // timeout — periodic commit check
        };

        let payload = match msg.payload() {
            Some(b) if !b.is_empty() => b,
            _ => {
                messages_since_commit += 1;
                continue;
            }
        };

        let event: ExperimentLifecycleEvent = match serde_json::from_slice(payload) {
            Ok(e) => e,
            Err(e) => {
                warn!(
                    error = %e,
                    partition = msg.partition(),
                    offset = msg.offset(),
                    "lifecycle consumer: JSON decode failed, skipping"
                );
                messages_since_commit += 1;
                continue;
            }
        };

        if !TERMINAL_STATES.contains(&event.state.as_str()) {
            messages_since_commit += 1;
            continue;
        }

        let experiment_id = match Uuid::parse_str(&event.experiment_id) {
            Ok(id) => id,
            Err(_) => {
                warn!(
                    experiment_id = %event.experiment_id,
                    "lifecycle consumer: invalid experiment_id UUID, skipping"
                );
                messages_since_commit += 1;
                continue;
            }
        };

        // Look up the flag linked to this experiment and trigger reconciliation.
        match store.get_flag_by_experiment(experiment_id).await {
            Ok(flag) => {
                info!(
                    %experiment_id,
                    flag_id = %flag.flag_id,
                    state = %event.state,
                    "lifecycle consumer: triggering reconciliation"
                );
                let mut rec = reconciler.lock().await;
                rec.reconcile_flag(flag.flag_id, experiment_id).await;
            }
            Err(crate::store::StoreError::NotFound(_)) => {
                // No flag linked to this experiment — nothing to do.
            }
            Err(e) => {
                error!(
                    error = %e,
                    %experiment_id,
                    "lifecycle consumer: store lookup failed"
                );
            }
        }

        messages_since_commit += 1;
    }
}
