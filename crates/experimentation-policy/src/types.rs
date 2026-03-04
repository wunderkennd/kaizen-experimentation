//! Channel message types for the LMAX policy core.

use std::collections::HashMap;
use tokio::sync::oneshot;

/// Request sent from the gRPC thread into the policy core.
pub struct SelectArmRequest {
    /// Experiment to select an arm for.
    pub experiment_id: String,
    /// Optional context features for contextual bandits.
    pub context: Option<HashMap<String, f64>>,
    /// Channel to send the response back to the gRPC handler.
    pub reply_tx: oneshot::Sender<Result<SelectArmResponse, PolicyError>>,
}

/// Response from the policy core back to the gRPC handler.
#[derive(Debug, Clone)]
pub struct SelectArmResponse {
    pub arm_id: String,
    pub assignment_probability: f64,
    pub all_arm_probabilities: HashMap<String, f64>,
}

/// Reward event sent from the Kafka consumer into the policy core.
#[derive(Debug, Clone)]
pub struct RewardUpdate {
    /// Experiment that received the reward.
    pub experiment_id: String,
    /// Arm that was shown to the user.
    pub arm_id: String,
    /// Observed reward value (0.0 or 1.0 for binary).
    pub reward: f64,
    /// Optional context features for contextual bandits.
    pub context: Option<HashMap<String, f64>>,
    /// Kafka offset for snapshot bookmarking.
    pub kafka_offset: i64,
}

/// Errors originating from the policy core.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PolicyError {
    #[error("experiment not found: {0}")]
    ExperimentNotFound(String),
    #[error("internal error: {0}")]
    Internal(String),
}
