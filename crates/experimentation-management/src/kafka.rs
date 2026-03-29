//! Guardrail Kafka consumer for M5 Management Service (ADR-008).
//!
//! Subscribes to the `guardrail_alerts` topic (published by M3 Metric Engine).
//! On guardrail breach, automatically pauses the experiment via a TOCTOU-safe
//! RUNNING→PAUSED transition. Publishes a lifecycle event to `experiment_lifecycle`
//! for downstream consumers (M7 flags reconciler).
//!
//! ## Message format (JSON)
//!
//! M3 publishes:
//! ```json
//! {
//!   "experiment_id": "<uuid>",
//!   "metric_id": "<metric-id>",
//!   "threshold": 0.05,
//!   "observed_value": 0.09,
//!   "breach_direction": "ABOVE|BELOW",
//!   "consecutive_breaches": 3
//! }
//! ```
//!
//! On receipt, M5:
//! 1. Executes `UPDATE experiments SET state='PAUSED', paused_at=NOW(), pause_reason=...
//!    WHERE experiment_id=$id AND state='RUNNING'` — rows_affected must be 1.
//! 2. Writes an audit trail entry with action='guardrail_auto_pause'.
//! 3. Publishes `{"experiment_id": ..., "state": "PAUSED"}` to experiment_lifecycle.

use std::sync::Arc;
use std::time::Duration;

use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::Message as KafkaMessage;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::store::ManagementStore;

// ---------------------------------------------------------------------------
// Guardrail alert schema
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct GuardrailAlert {
    pub experiment_id: String,
    pub metric_id: String,
    pub threshold: f64,
    pub observed_value: f64,
    pub breach_direction: Option<String>,
    pub consecutive_breaches: Option<i32>,
}

#[derive(Debug, Serialize)]
struct LifecycleEvent {
    experiment_id: String,
    state: &'static str,
    reason: String,
}

// ---------------------------------------------------------------------------
// Consumer entrypoint
// ---------------------------------------------------------------------------

/// Start the guardrail Kafka consumer.
///
/// Blocks until the broker becomes unavailable or the process exits.
/// Returns `Err` only on initial setup failure.
pub async fn consume_guardrail_alerts(
    store: Arc<ManagementStore>,
    brokers: &str,
    group_id: &str,
    alert_topic: &str,
    lifecycle_topic: &str,
) -> Result<(), String> {
    info!(%brokers, %group_id, %alert_topic, "guardrail alert consumer starting");

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .set("group.id", group_id)
        .set("auto.offset.reset", "latest") // don't replay old alerts on restart
        .set("enable.auto.commit", "false")
        .set("enable.partition.eof", "false")
        .set("session.timeout.ms", "30000")
        .set("fetch.min.bytes", "1")
        .set("fetch.wait.max.ms", "200")
        .create()
        .map_err(|e| format!("create guardrail consumer: {e}"))?;

    consumer
        .subscribe(&[alert_topic])
        .map_err(|e| format!("subscribe to {alert_topic}: {e}"))?;

    // Lifecycle event producer (best-effort — failure is non-fatal).
    let producer: Option<FutureProducer> = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .set("message.timeout.ms", "5000")
        .create()
        .ok();

    info!("guardrail alert consumer connected");
    consume_loop(&consumer, store, producer, lifecycle_topic).await;
    Ok(())
}

async fn consume_loop(
    consumer: &StreamConsumer,
    store: Arc<ManagementStore>,
    producer: Option<FutureProducer>,
    lifecycle_topic: &str,
) {
    let mut messages_since_commit: usize = 0;
    let commit_interval = Duration::from_secs(10);
    let mut last_commit = std::time::Instant::now();

    loop {
        // Periodic commit.
        if messages_since_commit > 0 && last_commit.elapsed() >= commit_interval {
            if let Err(e) = consumer.commit_consumer_state(CommitMode::Async) {
                error!(error = %e, "guardrail consumer: commit failed");
            }
            messages_since_commit = 0;
            last_commit = std::time::Instant::now();
        }

        let msg = match tokio::time::timeout(Duration::from_secs(1), consumer.recv()).await {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => {
                error!(error = %e, "guardrail consumer: recv error");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            Err(_) => continue, // timeout — periodic commit check
        };

        let Some(payload) = msg.payload() else {
            messages_since_commit += 1;
            continue;
        };

        let alert: GuardrailAlert = match serde_json::from_slice(payload) {
            Ok(a) => a,
            Err(e) => {
                warn!(
                    error = %e,
                    partition = msg.partition(),
                    offset = msg.offset(),
                    "guardrail consumer: JSON decode failed, skipping"
                );
                messages_since_commit += 1;
                continue;
            }
        };

        let experiment_id = match Uuid::parse_str(&alert.experiment_id) {
            Ok(id) => id,
            Err(_) => {
                warn!(
                    experiment_id = %alert.experiment_id,
                    "guardrail consumer: invalid experiment_id UUID"
                );
                messages_since_commit += 1;
                continue;
            }
        };

        let reason = format!(
            "guardrail_auto_pause: metric={} threshold={:.4} observed={:.4} breaches={}",
            alert.metric_id,
            alert.threshold,
            alert.observed_value,
            alert.consecutive_breaches.unwrap_or(1)
        );

        // TOCTOU-safe RUNNING→PAUSED transition.
        match store.pause_transition(experiment_id, &reason).await {
            Ok(1) => {
                info!(
                    %experiment_id,
                    metric_id = %alert.metric_id,
                    "guardrail auto-pause executed"
                );

                // Append audit trail (non-fatal on failure).
                let details = serde_json::json!({
                    "metric_id": alert.metric_id,
                    "threshold": alert.threshold,
                    "observed_value": alert.observed_value,
                    "breach_direction": alert.breach_direction,
                    "consecutive_breaches": alert.consecutive_breaches,
                });
                if let Err(e) = store
                    .record_audit(
                        experiment_id,
                        "guardrail_auto_pause",
                        "system",
                        Some("RUNNING"),
                        Some("PAUSED"),
                        &details,
                    )
                    .await
                {
                    warn!(error = %e, %experiment_id, "audit write failed (non-fatal)");
                }

                // Publish lifecycle event.
                publish_lifecycle_event(
                    &producer,
                    lifecycle_topic,
                    experiment_id,
                    "PAUSED",
                    &reason,
                )
                .await;
            }
            Ok(_) => {
                // rows_affected == 0: already paused, concluded, or concurrent winner.
                warn!(
                    %experiment_id,
                    "guardrail pause skipped (experiment not in RUNNING state)"
                );
            }
            Err(e) => {
                error!(error = %e, %experiment_id, "guardrail pause DB error");
            }
        }

        messages_since_commit += 1;
    }
}

async fn publish_lifecycle_event(
    producer: &Option<FutureProducer>,
    topic: &str,
    experiment_id: Uuid,
    state: &'static str,
    reason: &str,
) {
    let Some(producer) = producer else { return };

    let event = LifecycleEvent {
        experiment_id: experiment_id.to_string(),
        state,
        reason: reason.to_string(),
    };

    let Ok(payload) = serde_json::to_vec(&event) else {
        return;
    };

    let key = experiment_id.to_string();
    let record = FutureRecord::to(topic).key(&key).payload(&payload);

    if let Err((e, _)) = producer.send(record, Duration::from_secs(5)).await {
        warn!(
            error = %e,
            %experiment_id,
            "lifecycle event publish failed (non-fatal)"
        );
    }
}
