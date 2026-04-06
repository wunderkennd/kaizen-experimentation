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
    /// Scalar reward (0.0–1.0 for binary rewards; used when `metric_values` is
    /// absent or no composer is registered for this experiment).
    pub reward: f64,
    /// Optional context features for contextual bandits.
    pub context: Option<HashMap<String, f64>>,
    /// Kafka offset for snapshot bookmarking.
    pub kafka_offset: i64,
    /// Per-metric observed values for multi-objective composition (ADR-011).
    /// When non-empty and a [`RewardComposer`] is registered for this
    /// experiment, the policy core composes these into a scalar reward before
    /// calling `policy.update()`.
    pub metric_values: Option<HashMap<String, f64>>,
}

/// Request to create a cold-start bandit.
pub struct CreateColdStartRequest {
    pub config: experimentation_bandit::cold_start::ColdStartConfig,
    pub reply_tx: oneshot::Sender<Result<CreateColdStartResponse, PolicyError>>,
}

/// Response from creating a cold-start bandit.
#[derive(Debug, Clone)]
pub struct CreateColdStartResponse {
    pub experiment_id: String,
    pub content_id: String,
}

/// Request to export affinity scores from a trained cold-start bandit.
pub struct ExportAffinityRequest {
    pub experiment_id: String,
    /// Per-segment context feature vectors for affinity computation.
    pub segment_contexts: HashMap<String, HashMap<String, f64>>,
    pub reply_tx: oneshot::Sender<Result<ExportAffinityResponse, PolicyError>>,
}

/// Response with exported affinity scores.
#[derive(Debug, Clone)]
pub struct ExportAffinityResponse {
    pub content_id: String,
    pub segment_affinity_scores: HashMap<String, f64>,
    pub optimal_placements: HashMap<String, String>,
}

/// Request to get a policy snapshot.
pub struct GetSnapshotRequest {
    pub experiment_id: String,
    pub reply_tx: oneshot::Sender<Result<GetSnapshotResponse, PolicyError>>,
}

/// Response with policy snapshot data.
#[derive(Debug, Clone)]
pub struct GetSnapshotResponse {
    pub experiment_id: String,
    pub policy_state: Vec<u8>,
    pub total_rewards_processed: u64,
    pub kafka_offset: i64,
    pub snapshot_at_epoch_ms: i64,
}

/// Request to register isolated bandit policies for a META experiment (ADR-013).
/// Each variant gets its own policy keyed as `{experiment_id}::v::{variant_id}`.
pub struct RegisterMetaExperimentRequest {
    /// Parent META experiment ID.
    pub experiment_id: String,
    /// Per-variant configurations. Each entry creates an isolated policy.
    pub variant_policies: Vec<MetaVariantPolicyConfig>,
    /// Channel to send the result back to the caller.
    pub reply_tx: oneshot::Sender<Result<RegisterMetaExperimentResponse, PolicyError>>,
}

/// Per-variant policy configuration for a META experiment.
#[derive(Debug, Clone)]
pub struct MetaVariantPolicyConfig {
    /// Variant ID from the experiment's variant definitions.
    pub variant_id: String,
    /// Arm IDs for this variant's bandit policy.
    pub arm_ids: Vec<String>,
    /// Reward weight map: metric_id → weight (must sum to 1.0).
    pub reward_weights: HashMap<String, f64>,
}

/// Response from registering a META experiment.
#[derive(Debug, Clone)]
pub struct RegisterMetaExperimentResponse {
    /// Parent experiment ID.
    pub experiment_id: String,
    /// Compound policy IDs created (one per variant).
    pub policy_ids: Vec<String>,
}

/// Request to rollback policy to a previous snapshot.
pub struct RollbackPolicyRequest {
    pub experiment_id: String,
    pub target_snapshot_epoch_ms: i64,
    pub reply_tx: oneshot::Sender<Result<GetSnapshotResponse, PolicyError>>,
}

/// Errors originating from the policy core.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PolicyError {
    #[error("experiment not found: {0}")]
    ExperimentNotFound(String),
    #[error("snapshot not found: {0}")]
    SnapshotNotFound(String),
    #[error("wrong policy type: expected {expected}, got {actual}")]
    WrongPolicyType { expected: String, actual: String },
    #[error("internal error: {0}")]
    Internal(String),
}
