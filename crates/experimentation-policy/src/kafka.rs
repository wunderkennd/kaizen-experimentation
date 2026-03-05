//! Stub Kafka consumer for reward events.
//!
//! Full implementation pending Agent-2's event pipeline delivery.
//! Currently logs a message and returns.

use crate::types::RewardUpdate;
use tokio::sync::mpsc;
use tracing::info;

/// Start the Kafka reward consumer (stub — logs and waits).
pub async fn consume_rewards(
    _reward_tx: mpsc::Sender<RewardUpdate>,
) -> Result<(), String> {
    info!("Kafka reward consumer stub started (waiting for Agent-2 event pipeline)");
    Ok(())
}
