//! LMAX-inspired single-threaded policy core (ADR-002).
//!
//! All policy state mutations happen on this dedicated thread.
//! The gRPC and Kafka threads communicate via bounded channels.
//! Zero mutexes. Zero shared mutable state.

use crate::config::PolicyConfig;
use crate::snapshot::SnapshotStore;
use crate::types::{
    CreateColdStartRequest, CreateColdStartResponse, ExportAffinityRequest, ExportAffinityResponse,
    GetSnapshotRequest, GetSnapshotResponse, PolicyError, RegisterMetaExperimentRequest,
    RegisterMetaExperimentResponse, RewardUpdate, RollbackPolicyRequest, SelectArmRequest,
    SelectArmResponse,
};
use experimentation_bandit::cold_start;
use experimentation_bandit::linucb::LinUcbPolicy;
use experimentation_bandit::reward_composer::{sigmoid, CompositionMethod, Objective, RewardComposer};
use experimentation_bandit::policy::AnyPolicy;
use experimentation_bandit::thompson::ThompsonSamplingPolicy;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Management commands sent to the policy core (cold-start, snapshots, rollback, meta).
pub enum ManagementCommand {
    CreateColdStart(CreateColdStartRequest),
    ExportAffinity(ExportAffinityRequest),
    GetSnapshot(GetSnapshotRequest),
    RollbackPolicy(RollbackPolicyRequest),
    /// ADR-013: register isolated bandit policies for each variant of a META experiment.
    RegisterMetaExperiment(RegisterMetaExperimentRequest),
}

/// The single-threaded policy core that owns all mutable bandit state.
pub struct PolicyCore {
    /// All experiment policies, keyed by experiment_id.
    policies: HashMap<String, AnyPolicy>,
    /// Cold-start configs, keyed by experiment_id.
    cold_start_configs: HashMap<String, cold_start::ColdStartConfig>,
    /// RocksDB snapshot store.
    snapshot_store: SnapshotStore,
    /// Configuration.
    config: PolicyConfig,
    /// Track rewards since last snapshot per experiment.
    rewards_since_snapshot: HashMap<String, u64>,
    /// Last Kafka offset processed per experiment.
    last_kafka_offset: HashMap<String, i64>,
    /// Multi-objective reward composers (ADR-011), keyed by experiment_id.
    /// Present only for experiments registered with `reward_objectives`.
    reward_composers: HashMap<String, RewardComposer>,
}

impl PolicyCore {
    /// Create a new policy core with the given snapshot store and config.
    pub fn new(snapshot_store: SnapshotStore, config: PolicyConfig) -> Self {
        Self {
            policies: HashMap::new(),
            cold_start_configs: HashMap::new(),
            snapshot_store,
            config,
            rewards_since_snapshot: HashMap::new(),
            last_kafka_offset: HashMap::new(),
            reward_composers: HashMap::new(),
        }
    }

    /// Restore all policies from the latest RocksDB snapshots.
    /// Called once at startup before the event loop begins.
    pub fn restore_from_snapshots(&mut self) -> Result<usize, String> {
        let envelopes = self
            .snapshot_store
            .load_all_latest()
            .map_err(|e| format!("failed to load snapshots: {e}"))?;

        let count = envelopes.len();
        for envelope in envelopes {
            let policy = AnyPolicy::deserialize(&envelope.policy_type, &envelope.policy_state);
            info!(
                experiment_id = %envelope.experiment_id,
                policy_type = %envelope.policy_type,
                total_rewards = envelope.total_rewards_processed,
                kafka_offset = envelope.kafka_offset,
                "Restored policy from snapshot"
            );
            self.last_kafka_offset
                .insert(envelope.experiment_id.clone(), envelope.kafka_offset);

            // ADR-011: restore RewardComposer state if present.
            if let Some(composer_bytes) = &envelope.reward_composer_state {
                let composer = RewardComposer::from_bytes(composer_bytes);
                self.reward_composers
                    .insert(envelope.experiment_id.clone(), composer);
            }

            self.policies
                .insert(envelope.experiment_id, policy);
        }

        info!(restored_count = count, "Policy core startup restore complete");
        Ok(count)
    }

    /// Register a new Thompson Sampling experiment with the given arm IDs.
    /// If the experiment already exists, this is a no-op.
    #[allow(dead_code)]
    pub fn register_experiment(&mut self, experiment_id: String, arm_ids: Vec<String>) {
        self.policies
            .entry(experiment_id.clone())
            .or_insert_with(|| {
                info!(%experiment_id, arms = ?arm_ids, "Registered new Thompson experiment");
                AnyPolicy::Thompson(ThompsonSamplingPolicy::new(experiment_id, arm_ids))
            });
    }

    /// Register a new LinUCB experiment.
    /// If the experiment already exists, this is a no-op.
    #[allow(dead_code)]
    pub fn register_linucb_experiment(
        &mut self,
        experiment_id: String,
        arm_ids: Vec<String>,
        feature_keys: Vec<String>,
        alpha: f64,
        min_exploration_fraction: f64,
    ) {
        self.policies
            .entry(experiment_id.clone())
            .or_insert_with(|| {
                info!(
                    %experiment_id,
                    arms = ?arm_ids,
                    features = ?feature_keys,
                    alpha,
                    min_exploration_fraction,
                    "Registered new LinUCB experiment"
                );
                AnyPolicy::LinUcb(LinUcbPolicy::new(
                    experiment_id,
                    arm_ids,
                    feature_keys,
                    alpha,
                    min_exploration_fraction,
                ))
            });
    }

    /// Register a Thompson Sampling experiment with multi-objective reward composition
    /// (ADR-011).  If the experiment already exists, only the composer is updated.
    ///
    /// `composer` encapsulates all objective weights, composition method, and the
    /// running [`MetricNormalizer`].  Reward events for this experiment must carry
    /// `metric_values` in their [`RewardUpdate`]; the core will call
    /// `composer.compose()` before updating the bandit posterior.
    #[allow(dead_code)]
    pub fn register_multi_objective_experiment(
        &mut self,
        experiment_id: String,
        arm_ids: Vec<String>,
        composer: RewardComposer,
    ) {
        self.policies
            .entry(experiment_id.clone())
            .or_insert_with(|| {
                info!(%experiment_id, arms = ?arm_ids, "Registered new multi-objective Thompson experiment");
                AnyPolicy::Thompson(ThompsonSamplingPolicy::new(
                    experiment_id.clone(),
                    arm_ids,
                ))
            });
        self.reward_composers.insert(experiment_id, composer);
    }

    /// Run the single-threaded event loop.
    ///
    /// Uses `tokio::select!` to multiplex arm selection requests, reward updates,
    /// and management commands on a single thread. This is the LMAX pattern.
    pub async fn run(
        mut self,
        mut policy_rx: mpsc::Receiver<SelectArmRequest>,
        mut reward_rx: mpsc::Receiver<RewardUpdate>,
        mut mgmt_rx: mpsc::Receiver<ManagementCommand>,
    ) {
        info!("Policy core event loop started");

        loop {
            tokio::select! {
                // Arm selection requests from gRPC (Thread 1)
                Some(req) = policy_rx.recv() => {
                    let response = self.handle_select_arm(&req.experiment_id, req.context.as_ref());
                    let _ = req.reply_tx.send(response);
                }
                // Reward updates from Kafka (Thread 2)
                Some(update) = reward_rx.recv() => {
                    self.handle_reward_update(update);
                }
                // Management commands (cold-start, snapshots, rollback)
                Some(cmd) = mgmt_rx.recv() => {
                    self.handle_management_command(cmd);
                }
                // All channels closed → shutdown
                else => {
                    info!("All channels closed, policy core shutting down");
                    break;
                }
            }
        }
    }

    fn handle_management_command(&mut self, cmd: ManagementCommand) {
        match cmd {
            ManagementCommand::CreateColdStart(req) => {
                let result = self.handle_create_cold_start(req.config);
                let _ = req.reply_tx.send(result);
            }
            ManagementCommand::ExportAffinity(req) => {
                let result =
                    self.handle_export_affinity(&req.experiment_id, &req.segment_contexts);
                let _ = req.reply_tx.send(result);
            }
            ManagementCommand::GetSnapshot(req) => {
                let result = self.handle_get_snapshot(&req.experiment_id);
                let _ = req.reply_tx.send(result);
            }
            ManagementCommand::RollbackPolicy(req) => {
                let result =
                    self.handle_rollback_policy(&req.experiment_id, req.target_snapshot_epoch_ms);
                let _ = req.reply_tx.send(result);
            }
            ManagementCommand::RegisterMetaExperiment(req) => {
                let result = self.handle_register_meta_experiment(
                    &req.experiment_id,
                    &req.variant_policies,
                );
                let _ = req.reply_tx.send(result);
            }
        }
    }

    fn handle_select_arm(
        &self,
        experiment_id: &str,
        context: Option<&HashMap<String, f64>>,
    ) -> Result<SelectArmResponse, PolicyError> {
        let policy = self
            .policies
            .get(experiment_id)
            .ok_or_else(|| PolicyError::ExperimentNotFound(experiment_id.to_string()))?;

        let selection = policy.select_arm(context);
        Ok(SelectArmResponse {
            arm_id: selection.arm_id,
            assignment_probability: selection.assignment_probability,
            all_arm_probabilities: selection.all_arm_probabilities,
        })
    }

    fn handle_reward_update(&mut self, update: RewardUpdate) {
        // ADR-011: if metric_values are present and a RewardComposer is
        // registered for this experiment, compose the scalar reward first.
        let scalar_reward = if let Some(metrics) = &update.metric_values {
            if let Some(composer) = self.reward_composers.get_mut(&update.experiment_id) {
                let composed = composer.compose(metrics);
                // Beta-Bernoulli (Thompson) expects reward ∈ [0, 1].
                // Map the real-valued composed score via sigmoid.
                match self.policies.get(&update.experiment_id) {
                    Some(AnyPolicy::Thompson(_)) => sigmoid(composed),
                    _ => composed,
                }
            } else {
                update.reward
            }
        } else {
            update.reward
        };

        let policy = match self.policies.get_mut(&update.experiment_id) {
            Some(p) => p,
            None => {
                warn!(
                    experiment_id = %update.experiment_id,
                    "Received reward for unknown experiment, ignoring"
                );
                return;
            }
        };

        policy.update(&update.arm_id, scalar_reward, update.context.as_ref());
        self.last_kafka_offset
            .insert(update.experiment_id.clone(), update.kafka_offset);

        // Track rewards since last snapshot
        let count = self
            .rewards_since_snapshot
            .entry(update.experiment_id.clone())
            .or_insert(0);
        *count += 1;

        // Snapshot on every N-th reward
        if *count >= self.config.snapshot_interval {
            *count = 0;
            self.write_snapshot(&update.experiment_id);
        }
    }

    fn handle_create_cold_start(
        &mut self,
        config: cold_start::ColdStartConfig,
    ) -> Result<CreateColdStartResponse, PolicyError> {
        let (experiment_id, policy) = cold_start::create_cold_start_policy(&config);

        if self.policies.contains_key(&experiment_id) {
            info!(
                %experiment_id,
                content_id = %config.content_id,
                "Cold-start bandit already exists, returning existing"
            );
        } else {
            info!(
                %experiment_id,
                content_id = %config.content_id,
                window_days = config.window_days,
                arms = ?config.arm_ids,
                "Created cold-start bandit"
            );
            self.policies.insert(experiment_id.clone(), policy);
        }

        let content_id = config.content_id.clone();
        self.cold_start_configs
            .insert(experiment_id.clone(), config);

        Ok(CreateColdStartResponse {
            experiment_id,
            content_id,
        })
    }

    fn handle_export_affinity(
        &self,
        experiment_id: &str,
        segment_contexts: &HashMap<String, HashMap<String, f64>>,
    ) -> Result<ExportAffinityResponse, PolicyError> {
        let policy = self
            .policies
            .get(experiment_id)
            .ok_or_else(|| PolicyError::ExperimentNotFound(experiment_id.to_string()))?;

        let linucb = match policy {
            AnyPolicy::LinUcb(p) => p,
            other => {
                return Err(PolicyError::WrongPolicyType {
                    expected: "linucb".to_string(),
                    actual: other.policy_type().to_string(),
                })
            }
        };

        let config = self.cold_start_configs.get(experiment_id);
        let content_id = config
            .map(|c| c.content_id.clone())
            .unwrap_or_else(|| experiment_id.to_string());

        let scores = cold_start::export_affinity_scores(linucb, &content_id, segment_contexts);

        Ok(ExportAffinityResponse {
            content_id: scores.content_id,
            segment_affinity_scores: scores.segment_affinity_scores,
            optimal_placements: scores.optimal_placements,
        })
    }

    fn handle_get_snapshot(
        &self,
        experiment_id: &str,
    ) -> Result<GetSnapshotResponse, PolicyError> {
        let envelope = self
            .snapshot_store
            .load_latest(experiment_id)
            .map_err(|e| PolicyError::Internal(format!("RocksDB error: {e}")))?
            .ok_or_else(|| {
                PolicyError::SnapshotNotFound(format!("no snapshot for {experiment_id}"))
            })?;

        Ok(GetSnapshotResponse {
            experiment_id: envelope.experiment_id,
            policy_state: envelope.policy_state,
            total_rewards_processed: envelope.total_rewards_processed,
            kafka_offset: envelope.kafka_offset,
            snapshot_at_epoch_ms: envelope.snapshot_at_epoch_ms,
        })
    }

    fn handle_rollback_policy(
        &mut self,
        experiment_id: &str,
        target_snapshot_epoch_ms: i64,
    ) -> Result<GetSnapshotResponse, PolicyError> {
        // Find the specific snapshot by scanning
        let envelope = self
            .snapshot_store
            .load_latest(experiment_id)
            .map_err(|e| PolicyError::Internal(format!("RocksDB error: {e}")))?
            .ok_or_else(|| {
                PolicyError::SnapshotNotFound(format!("no snapshot for {experiment_id}"))
            })?;

        // For now, rollback to the latest available snapshot.
        // A more granular implementation would scan for the specific timestamp.
        if envelope.snapshot_at_epoch_ms != target_snapshot_epoch_ms {
            warn!(
                %experiment_id,
                requested = target_snapshot_epoch_ms,
                available = envelope.snapshot_at_epoch_ms,
                "Exact snapshot timestamp not found, using latest"
            );
        }

        let policy_type = envelope.policy_type.clone();
        let policy = AnyPolicy::deserialize(&policy_type, &envelope.policy_state);
        self.policies.insert(experiment_id.to_string(), policy);

        info!(
            %experiment_id,
            snapshot_at = envelope.snapshot_at_epoch_ms,
            total_rewards = envelope.total_rewards_processed,
            "Rolled back policy to snapshot"
        );

        Ok(GetSnapshotResponse {
            experiment_id: envelope.experiment_id,
            policy_state: envelope.policy_state,
            total_rewards_processed: envelope.total_rewards_processed,
            kafka_offset: envelope.kafka_offset,
            snapshot_at_epoch_ms: envelope.snapshot_at_epoch_ms,
        })
    }

    /// Build the compound policy ID for a META experiment variant.
    /// Format: `{experiment_id}::v::{variant_id}`
    pub fn meta_variant_policy_id(experiment_id: &str, variant_id: &str) -> String {
        format!("{experiment_id}::v::{variant_id}")
    }

    /// ADR-013: Register isolated bandit policies for each variant of a META experiment.
    ///
    /// Each variant gets its own Thompson Sampling policy with a dedicated
    /// `RewardComposer` configured from the variant's reward weights.
    /// Policy IDs use the compound format `{experiment_id}::v::{variant_id}`.
    fn handle_register_meta_experiment(
        &mut self,
        experiment_id: &str,
        variant_policies: &[crate::types::MetaVariantPolicyConfig],
    ) -> Result<RegisterMetaExperimentResponse, PolicyError> {
        let mut policy_ids = Vec::with_capacity(variant_policies.len());

        for vp in variant_policies {
            let policy_id = Self::meta_variant_policy_id(experiment_id, &vp.variant_id);

            // Build Objectives from reward_weights for the RewardComposer.
            let objectives: Vec<_> = vp
                .reward_weights
                .iter()
                .map(|(metric_id, &weight)| Objective {
                    metric_id: metric_id.clone(),
                    weight,
                    floor: 0.0,
                    is_primary: false,
                })
                .collect();

            let composer = RewardComposer::new(
                objectives,
                CompositionMethod::WeightedScalarization,
            );

            // Create isolated policy (Thompson Sampling) with variant-specific composer.
            self.policies
                .entry(policy_id.clone())
                .or_insert_with(|| {
                    info!(
                        %experiment_id,
                        variant_id = %vp.variant_id,
                        %policy_id,
                        arms = ?vp.arm_ids,
                        "Registered META variant policy"
                    );
                    AnyPolicy::Thompson(ThompsonSamplingPolicy::new(
                        policy_id.clone(),
                        vp.arm_ids.clone(),
                    ))
                });

            self.reward_composers.insert(policy_id.clone(), composer);
            policy_ids.push(policy_id);
        }

        info!(
            %experiment_id,
            variant_count = variant_policies.len(),
            "Registered META experiment with isolated per-variant policies"
        );

        Ok(RegisterMetaExperimentResponse {
            experiment_id: experiment_id.to_string(),
            policy_ids,
        })
    }

    fn write_snapshot(&self, experiment_id: &str) {
        let policy = match self.policies.get(experiment_id) {
            Some(p) => p,
            None => return,
        };

        let kafka_offset = self
            .last_kafka_offset
            .get(experiment_id)
            .copied()
            .unwrap_or(-1);

        // ADR-011: persist RewardComposer state alongside policy posteriors.
        let reward_composer_state = self
            .reward_composers
            .get(experiment_id)
            .map(|c| c.to_bytes());

        let envelope = SnapshotStore::make_envelope_with_composer(
            experiment_id.to_string(),
            policy.policy_type().to_string(),
            policy.serialize(),
            policy.total_rewards(),
            kafka_offset,
            reward_composer_state,
        );

        if let Err(e) = self.snapshot_store.write_snapshot(&envelope) {
            error!(
                %experiment_id,
                error = %e,
                "Failed to write RocksDB snapshot"
            );
            return;
        }

        // Prune old snapshots
        if let Err(e) = self
            .snapshot_store
            .prune_old_snapshots(experiment_id, self.config.max_snapshots_per_experiment)
        {
            warn!(
                %experiment_id,
                error = %e,
                "Failed to prune old snapshots"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use experimentation_bandit::cold_start::ColdStartConfig;
    use experimentation_bandit::policy::Policy;
    use std::path::PathBuf;

    fn temp_db_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("test-core-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    fn test_config(db_path: &str) -> PolicyConfig {
        PolicyConfig {
            grpc_addr: "[::1]:0".into(),
            rocksdb_path: db_path.into(),
            policy_channel_depth: 100,
            reward_channel_depth: 100,
            snapshot_interval: 5,
            max_snapshots_per_experiment: 3,
            kafka_brokers: "localhost:9092".into(),
            kafka_group_id: "test-group".into(),
            kafka_reward_topic: "reward_events".into(),
            kafka_auto_offset_reset: "earliest".into(),
            kafka_commit_batch_size: 100,
            kafka_commit_interval_secs: 5,
        }
    }

    /// Helper to spawn a core with all three channels.
    fn spawn_core(
        core: PolicyCore,
    ) -> (
        mpsc::Sender<SelectArmRequest>,
        mpsc::Sender<RewardUpdate>,
        mpsc::Sender<ManagementCommand>,
        tokio::task::JoinHandle<()>,
    ) {
        let (policy_tx, policy_rx) = mpsc::channel(100);
        let (reward_tx, reward_rx) = mpsc::channel(100);
        let (mgmt_tx, mgmt_rx) = mpsc::channel(100);
        let handle = tokio::spawn(core.run(policy_rx, reward_rx, mgmt_rx));
        (policy_tx, reward_tx, mgmt_tx, handle)
    }

    #[tokio::test]
    async fn test_select_arm_and_reward_via_channels() {
        let db_path = temp_db_path("channel-test");
        let store = SnapshotStore::open(&db_path).unwrap();
        let config = test_config(db_path.to_str().unwrap());

        let mut core = PolicyCore::new(store, config);
        core.register_experiment("exp-1".into(), vec!["a".into(), "b".into()]);

        let (policy_tx, reward_tx, mgmt_tx, handle) = spawn_core(core);

        // Send a SelectArm request
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        policy_tx
            .send(SelectArmRequest {
                experiment_id: "exp-1".into(),
                context: None,
                reply_tx,
            })
            .await
            .unwrap();

        let response = reply_rx.await.unwrap().unwrap();
        assert!(response.arm_id == "a" || response.arm_id == "b");

        // Send reward updates
        for _ in 0..10 {
            reward_tx
                .send(RewardUpdate {
                    experiment_id: "exp-1".into(),
                    arm_id: "a".into(),
                    reward: 1.0,
                    context: None,
                    kafka_offset: 1,
                    metric_values: None,
                })
                .await
                .unwrap();
        }

        // Send another SelectArm — should still work
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        policy_tx
            .send(SelectArmRequest {
                experiment_id: "exp-1".into(),
                context: None,
                reply_tx,
            })
            .await
            .unwrap();
        let response = reply_rx.await.unwrap().unwrap();
        assert!(response.arm_id == "a" || response.arm_id == "b");

        // Test unknown experiment
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        policy_tx
            .send(SelectArmRequest {
                experiment_id: "unknown".into(),
                context: None,
                reply_tx,
            })
            .await
            .unwrap();
        let response = reply_rx.await.unwrap();
        assert!(response.is_err());

        drop(policy_tx);
        drop(reward_tx);
        drop(mgmt_tx);
        handle.await.unwrap();

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_crash_recovery() {
        let db_path = temp_db_path("crash-recovery");

        // Phase 1: Create a core, process rewards, generate snapshots
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());

            let mut core = PolicyCore::new(store, config);
            core.register_experiment("exp-1".into(), vec!["a".into(), "b".into()]);

            let (_policy_tx, reward_tx, _mgmt_tx, handle) = spawn_core(core);

            for i in 0..100 {
                reward_tx
                    .send(RewardUpdate {
                        experiment_id: "exp-1".into(),
                        arm_id: "a".into(),
                        reward: 1.0,
                        context: None,
                        kafka_offset: i,
                        metric_values: None,
                    })
                    .await
                    .unwrap();
            }

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            drop(reward_tx);
            drop(_policy_tx);
            drop(_mgmt_tx);
            handle.await.unwrap();
        }

        // Phase 2: Reopen RocksDB and restore
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());

            let mut core = PolicyCore::new(store, config);
            let restored = core.restore_from_snapshots().unwrap();
            assert_eq!(restored, 1, "should restore 1 experiment");

            let (policy_tx, _reward_tx, _mgmt_tx, handle) = spawn_core(core);

            let mut a_count = 0;
            for _ in 0..10 {
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                policy_tx
                    .send(SelectArmRequest {
                        experiment_id: "exp-1".into(),
                        context: None,
                        reply_tx,
                    })
                    .await
                    .unwrap();
                let response = reply_rx.await.unwrap().unwrap();
                if response.arm_id == "a" {
                    a_count += 1;
                }
            }
            assert!(
                a_count >= 8,
                "restored policy should prefer arm 'a' after 100 successes, got {a_count}/10 selections"
            );

            drop(policy_tx);
            drop(_reward_tx);
            drop(_mgmt_tx);
            handle.await.unwrap();
        }

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_linucb_select_arm_via_channels() {
        let db_path = temp_db_path("linucb-channel");
        let store = SnapshotStore::open(&db_path).unwrap();
        let config = test_config(db_path.to_str().unwrap());

        let mut core = PolicyCore::new(store, config);
        core.register_linucb_experiment(
            "linucb-exp".into(),
            vec!["a".into(), "b".into()],
            vec!["f0".into(), "f1".into()],
            1.0,
            0.05,
        );

        let (policy_tx, reward_tx, _mgmt_tx, handle) = spawn_core(core);

        let ctx: HashMap<String, f64> = [("f0".into(), 1.0), ("f1".into(), 0.5)]
            .into_iter()
            .collect();

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        policy_tx
            .send(SelectArmRequest {
                experiment_id: "linucb-exp".into(),
                context: Some(ctx.clone()),
                reply_tx,
            })
            .await
            .unwrap();

        let response = reply_rx.await.unwrap().unwrap();
        assert!(response.arm_id == "a" || response.arm_id == "b");
        assert_eq!(response.all_arm_probabilities.len(), 2);

        for _ in 0..10 {
            reward_tx
                .send(RewardUpdate {
                    experiment_id: "linucb-exp".into(),
                    arm_id: "a".into(),
                    reward: 1.0,
                    context: Some(ctx.clone()),
                    kafka_offset: 1,
                    metric_values: None,
                })
                .await
                .unwrap();
        }

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        policy_tx
            .send(SelectArmRequest {
                experiment_id: "linucb-exp".into(),
                context: Some(ctx),
                reply_tx,
            })
            .await
            .unwrap();
        let response = reply_rx.await.unwrap().unwrap();
        assert!(response.arm_id == "a" || response.arm_id == "b");

        drop(policy_tx);
        drop(reward_tx);
        drop(_mgmt_tx);
        handle.await.unwrap();

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_linucb_crash_recovery() {
        let db_path = temp_db_path("linucb-crash");
        let ctx: HashMap<String, f64> = [("f0".into(), 1.0), ("f1".into(), 0.5)]
            .into_iter()
            .collect();

        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());

            let mut core = PolicyCore::new(store, config);
            core.register_linucb_experiment(
                "linucb-exp".into(),
                vec!["a".into(), "b".into()],
                vec!["f0".into(), "f1".into()],
                1.0,
                0.05,
            );

            let (_policy_tx, reward_tx, _mgmt_tx, handle) = spawn_core(core);

            for i in 0..50 {
                reward_tx
                    .send(RewardUpdate {
                        experiment_id: "linucb-exp".into(),
                        arm_id: "a".into(),
                        reward: 1.0,
                        context: Some(ctx.clone()),
                        kafka_offset: i,
                        metric_values: None,
                    })
                    .await
                    .unwrap();
            }

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            drop(reward_tx);
            drop(_policy_tx);
            drop(_mgmt_tx);
            handle.await.unwrap();
        }

        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());

            let mut core = PolicyCore::new(store, config);
            let restored = core.restore_from_snapshots().unwrap();
            assert_eq!(restored, 1, "should restore 1 LinUCB experiment");

            let (policy_tx, _reward_tx, _mgmt_tx, handle) = spawn_core(core);

            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            policy_tx
                .send(SelectArmRequest {
                    experiment_id: "linucb-exp".into(),
                    context: Some(ctx),
                    reply_tx,
                })
                .await
                .unwrap();

            let response = reply_rx.await.unwrap().unwrap();
            assert_eq!(
                response.arm_id, "a",
                "restored LinUCB should prefer arm 'a' after training"
            );

            drop(policy_tx);
            drop(_reward_tx);
            drop(_mgmt_tx);
            handle.await.unwrap();
        }

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_cold_start_lifecycle() {
        let db_path = temp_db_path("cold-start-lifecycle");
        let store = SnapshotStore::open(&db_path).unwrap();
        let config = test_config(db_path.to_str().unwrap());
        let core = PolicyCore::new(store, config);

        let (policy_tx, reward_tx, mgmt_tx, handle) = spawn_core(core);

        // Step 1: Create cold-start bandit
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        mgmt_tx
            .send(ManagementCommand::CreateColdStart(CreateColdStartRequest {
                config: ColdStartConfig {
                    content_id: "movie-42".into(),
                    content_metadata: [("genre".into(), "comedy".into())]
                        .into_iter()
                        .collect(),
                    window_days: 7,
                    arm_ids: vec!["homepage".into(), "trending".into()],
                    feature_keys: vec!["age_bucket".into(), "watch_count".into()],
                    alpha: 1.0,
                    min_exploration_fraction: 0.05,
                },
                reply_tx,
            }))
            .await
            .unwrap();

        let resp = reply_rx.await.unwrap().unwrap();
        assert_eq!(resp.experiment_id, "cold-start:movie-42");
        assert_eq!(resp.content_id, "movie-42");

        // Step 2: Select arms and train with rewards
        let ctx: HashMap<String, f64> = [("age_bucket".into(), 2.0), ("watch_count".into(), 50.0)]
            .into_iter()
            .collect();

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        policy_tx
            .send(SelectArmRequest {
                experiment_id: "cold-start:movie-42".into(),
                context: Some(ctx.clone()),
                reply_tx,
            })
            .await
            .unwrap();
        let selection = reply_rx.await.unwrap().unwrap();
        assert!(selection.arm_id == "homepage" || selection.arm_id == "trending");

        // Train: homepage gets high rewards
        for i in 0..50 {
            reward_tx
                .send(RewardUpdate {
                    experiment_id: "cold-start:movie-42".into(),
                    arm_id: "homepage".into(),
                    reward: 1.0,
                    context: Some(ctx.clone()),
                    kafka_offset: i,
                    metric_values: None,
                })
                .await
                .unwrap();
            reward_tx
                .send(RewardUpdate {
                    experiment_id: "cold-start:movie-42".into(),
                    arm_id: "trending".into(),
                    reward: 0.2,
                    context: Some(ctx.clone()),
                    kafka_offset: 50 + i,
                    metric_values: None,
                })
                .await
                .unwrap();
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Step 3: Export affinity scores
        let segments: HashMap<String, HashMap<String, f64>> = [(
            "young_watchers".to_string(),
            [("age_bucket".into(), 2.0), ("watch_count".into(), 50.0)]
                .into_iter()
                .collect(),
        )]
        .into_iter()
        .collect();

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        mgmt_tx
            .send(ManagementCommand::ExportAffinity(ExportAffinityRequest {
                experiment_id: "cold-start:movie-42".into(),
                segment_contexts: segments,
                reply_tx,
            }))
            .await
            .unwrap();

        let affinity = reply_rx.await.unwrap().unwrap();
        assert_eq!(affinity.content_id, "movie-42");
        assert_eq!(
            affinity.optimal_placements.get("young_watchers").unwrap(),
            "homepage",
            "homepage should win after training with higher rewards"
        );

        // Step 4: Get policy snapshot
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        mgmt_tx
            .send(ManagementCommand::GetSnapshot(GetSnapshotRequest {
                experiment_id: "cold-start:movie-42".into(),
                reply_tx,
            }))
            .await
            .unwrap();
        let snapshot = reply_rx.await.unwrap().unwrap();
        assert_eq!(snapshot.experiment_id, "cold-start:movie-42");
        assert!(snapshot.total_rewards_processed > 0);

        drop(policy_tx);
        drop(reward_tx);
        drop(mgmt_tx);
        handle.await.unwrap();

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_cold_start_crash_recovery() {
        let db_path = temp_db_path("cold-start-crash");
        let ctx: HashMap<String, f64> = [("age_bucket".into(), 2.0), ("watch_count".into(), 50.0)]
            .into_iter()
            .collect();

        // Phase 1: Create and train
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());
            let core = PolicyCore::new(store, config);
            let (_policy_tx, reward_tx, mgmt_tx, handle) = spawn_core(core);

            // Create cold-start bandit
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            mgmt_tx
                .send(ManagementCommand::CreateColdStart(
                    CreateColdStartRequest {
                        config: ColdStartConfig {
                            content_id: "show-99".into(),
                            content_metadata: HashMap::new(),
                            window_days: 7,
                            arm_ids: vec!["a".into(), "b".into()],
                            feature_keys: vec!["age_bucket".into(), "watch_count".into()],
                            alpha: 0.01,
                            min_exploration_fraction: 0.05,
                        },
                        reply_tx,
                    },
                ))
                .await
                .unwrap();
            reply_rx.await.unwrap().unwrap();

            // Train: arm "a" gets high rewards, arm "b" gets low rewards
            for i in 0..50 {
                reward_tx
                    .send(RewardUpdate {
                        experiment_id: "cold-start:show-99".into(),
                        arm_id: "a".into(),
                        reward: 1.0,
                        context: Some(ctx.clone()),
                        kafka_offset: i * 2,
                        metric_values: None,
                    })
                    .await
                    .unwrap();
                reward_tx
                    .send(RewardUpdate {
                        experiment_id: "cold-start:show-99".into(),
                        arm_id: "b".into(),
                        reward: 0.0,
                        context: Some(ctx.clone()),
                        kafka_offset: i * 2 + 1,
                        metric_values: None,
                    })
                    .await
                    .unwrap();
            }

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            drop(reward_tx);
            drop(_policy_tx);
            drop(mgmt_tx);
            handle.await.unwrap();
        }

        // Phase 2: Restore and verify
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());
            let mut core = PolicyCore::new(store, config);
            let restored = core.restore_from_snapshots().unwrap();
            assert_eq!(restored, 1);

            let (policy_tx, _reward_tx, _mgmt_tx, handle) = spawn_core(core);

            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            policy_tx
                .send(SelectArmRequest {
                    experiment_id: "cold-start:show-99".into(),
                    context: Some(ctx),
                    reply_tx,
                })
                .await
                .unwrap();
            let response = reply_rx.await.unwrap().unwrap();
            assert_eq!(
                response.arm_id, "a",
                "restored cold-start should prefer arm 'a'"
            );

            drop(policy_tx);
            drop(_reward_tx);
            drop(_mgmt_tx);
            handle.await.unwrap();
        }

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_multi_experiment_concurrent_crash_recovery() {
        // Verify that Thompson, LinUCB, and cold-start experiments all restore
        // correctly from the same RocksDB instance after a simulated crash.
        let db_path = temp_db_path("multi-crash");
        let ctx: HashMap<String, f64> = [("f0".into(), 1.0), ("f1".into(), 0.5)]
            .into_iter()
            .collect();

        // Phase 1: Register three different policy types and train them
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());
            let mut core = PolicyCore::new(store, config);

            // Thompson Sampling
            core.register_experiment("thompson-1".into(), vec!["a".into(), "b".into()]);

            // LinUCB
            core.register_linucb_experiment(
                "linucb-1".into(),
                vec!["x".into(), "y".into()],
                vec!["f0".into(), "f1".into()],
                1.0,
                0.05,
            );

            let (_policy_tx, reward_tx, mgmt_tx, handle) = spawn_core(core);

            // Create cold-start bandit via management channel
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            mgmt_tx
                .send(ManagementCommand::CreateColdStart(CreateColdStartRequest {
                    config: ColdStartConfig {
                        content_id: "multi-movie".into(),
                        content_metadata: HashMap::new(),
                        window_days: 7,
                        arm_ids: vec!["p".into(), "q".into()],
                        feature_keys: vec!["f0".into(), "f1".into()],
                        alpha: 1.0,
                        min_exploration_fraction: 0.05,
                    },
                    reply_tx,
                }))
                .await
                .unwrap();
            reply_rx.await.unwrap().unwrap();

            // Train all three: arm "a"/"x"/"p" always wins
            for i in 0..30 {
                // Thompson
                reward_tx
                    .send(RewardUpdate {
                        experiment_id: "thompson-1".into(),
                        arm_id: "a".into(),
                        reward: 1.0,
                        context: None,
                        kafka_offset: i * 3,
                        metric_values: None,
                    })
                    .await
                    .unwrap();
                // LinUCB
                reward_tx
                    .send(RewardUpdate {
                        experiment_id: "linucb-1".into(),
                        arm_id: "x".into(),
                        reward: 1.0,
                        context: Some(ctx.clone()),
                        kafka_offset: i * 3 + 1,
                        metric_values: None,
                    })
                    .await
                    .unwrap();
                // Cold-start
                reward_tx
                    .send(RewardUpdate {
                        experiment_id: "cold-start:multi-movie".into(),
                        arm_id: "p".into(),
                        reward: 1.0,
                        context: Some(ctx.clone()),
                        kafka_offset: i * 3 + 2,
                        metric_values: None,
                    })
                    .await
                    .unwrap();
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            drop(reward_tx);
            drop(_policy_tx);
            drop(mgmt_tx);
            handle.await.unwrap();
        }

        // Phase 2: Reopen and restore all three experiments
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());
            let mut core = PolicyCore::new(store, config);
            let restored = core.restore_from_snapshots().unwrap();
            assert_eq!(restored, 3, "should restore all 3 experiments");

            let (policy_tx, _reward_tx, _mgmt_tx, handle) = spawn_core(core);

            // Verify Thompson prefers "a" (multi-trial: Thompson is probabilistic)
            let mut a_count = 0;
            for _ in 0..20 {
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                policy_tx
                    .send(SelectArmRequest {
                        experiment_id: "thompson-1".into(),
                        context: None,
                        reply_tx,
                    })
                    .await
                    .unwrap();
                if reply_rx.await.unwrap().unwrap().arm_id == "a" {
                    a_count += 1;
                }
            }
            assert!(
                a_count >= 14,
                "Thompson should pick 'a' at least 14/20 times after 30 rewards, got {a_count}"
            );

            // Verify LinUCB prefers "x"
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            policy_tx
                .send(SelectArmRequest {
                    experiment_id: "linucb-1".into(),
                    context: Some(ctx.clone()),
                    reply_tx,
                })
                .await
                .unwrap();
            let resp = reply_rx.await.unwrap().unwrap();
            assert_eq!(resp.arm_id, "x", "LinUCB should prefer arm 'x'");

            // Verify cold-start prefers "p"
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            policy_tx
                .send(SelectArmRequest {
                    experiment_id: "cold-start:multi-movie".into(),
                    context: Some(ctx),
                    reply_tx,
                })
                .await
                .unwrap();
            let resp = reply_rx.await.unwrap().unwrap();
            assert_eq!(resp.arm_id, "p", "Cold-start should prefer arm 'p'");

            drop(policy_tx);
            drop(_reward_tx);
            drop(_mgmt_tx);
            handle.await.unwrap();
        }

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_snapshot_kafka_offset_preserved() {
        // Verify that the Kafka offset stored in the snapshot matches the last
        // offset processed, so that replay after crash resumes from the right point.
        let db_path = temp_db_path("offset-verify");
        let expected_last_offset: i64 = 999;

        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());
            let mut core = PolicyCore::new(store, config);
            core.register_experiment("offset-exp".into(), vec!["a".into(), "b".into()]);

            let (_policy_tx, reward_tx, _mgmt_tx, handle) = spawn_core(core);

            // Send rewards with incrementing offsets, ending at expected_last_offset
            for i in 0..=expected_last_offset {
                reward_tx
                    .send(RewardUpdate {
                        experiment_id: "offset-exp".into(),
                        arm_id: if i % 2 == 0 { "a" } else { "b" }.into(),
                        reward: 1.0,
                        context: None,
                        kafka_offset: i,
                        metric_values: None,
                    })
                    .await
                    .unwrap();
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            drop(reward_tx);
            drop(_policy_tx);
            drop(_mgmt_tx);
            handle.await.unwrap();
        }

        // Verify the snapshot contains the correct offset
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let envelope = store.load_latest("offset-exp").unwrap().unwrap();
            // The last snapshot might not be exactly at expected_last_offset because
            // snapshots fire every `snapshot_interval` (5) rewards. But it should be
            // close and <= expected_last_offset.
            assert!(
                envelope.kafka_offset <= expected_last_offset,
                "snapshot offset {} should be <= last processed {}",
                envelope.kafka_offset,
                expected_last_offset
            );
            // With 1000 rewards and snapshot_interval=5, the last snapshot is at reward 1000
            // (1000 % 5 == 0), so offset should be exactly 999.
            assert_eq!(
                envelope.kafka_offset, expected_last_offset,
                "snapshot offset should be exactly {expected_last_offset} since 1000 is divisible by snapshot_interval=5"
            );
            assert_eq!(
                envelope.total_rewards_processed, 1000,
                "total rewards should reflect all 1000 processed rewards"
            );
        }

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_high_volume_crash_recovery() {
        // Process 2000+ rewards across 2 experiments, crash, restore, and verify
        // that the learned preferences are maintained.
        let db_path = temp_db_path("high-volume");
        let ctx: HashMap<String, f64> = [("f0".into(), 1.0)].into_iter().collect();

        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());
            let mut core = PolicyCore::new(store, config);
            core.register_experiment(
                "ts-high".into(),
                vec!["a".into(), "b".into(), "c".into()],
            );
            core.register_linucb_experiment(
                "lu-high".into(),
                vec!["x".into(), "y".into(), "z".into()],
                vec!["f0".into()],
                0.5,
                0.05,
            );

            let (_policy_tx, reward_tx, _mgmt_tx, handle) = spawn_core(core);

            // 1200 rewards for Thompson (arm "a" wins heavily)
            for i in 0..1200i64 {
                let (arm, reward) = if i % 3 == 0 {
                    ("a", 1.0)
                } else if i % 3 == 1 {
                    ("b", 0.1)
                } else {
                    ("c", 0.0)
                };
                reward_tx
                    .send(RewardUpdate {
                        experiment_id: "ts-high".into(),
                        arm_id: arm.into(),
                        reward,
                        context: None,
                        kafka_offset: i,
                        metric_values: None,
                    })
                    .await
                    .unwrap();
            }

            // 1000 rewards for LinUCB (arm "x" wins heavily)
            for i in 0..1000i64 {
                let (arm, reward) = if i % 3 == 0 {
                    ("x", 1.0)
                } else if i % 3 == 1 {
                    ("y", 0.1)
                } else {
                    ("z", 0.0)
                };
                reward_tx
                    .send(RewardUpdate {
                        experiment_id: "lu-high".into(),
                        arm_id: arm.into(),
                        reward,
                        context: Some(ctx.clone()),
                        kafka_offset: 1200 + i,
                        metric_values: None,
                    })
                    .await
                    .unwrap();
            }

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            drop(reward_tx);
            drop(_policy_tx);
            drop(_mgmt_tx);
            handle.await.unwrap();
        }

        // Restore and verify learned preferences survive crash
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());
            let mut core = PolicyCore::new(store, config);
            let restored = core.restore_from_snapshots().unwrap();
            assert_eq!(restored, 2);

            let (policy_tx, _reward_tx, _mgmt_tx, handle) = spawn_core(core);

            // Thompson: "a" should dominate after 400 wins
            let mut a_count = 0;
            for _ in 0..20 {
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                policy_tx
                    .send(SelectArmRequest {
                        experiment_id: "ts-high".into(),
                        context: None,
                        reply_tx,
                    })
                    .await
                    .unwrap();
                if reply_rx.await.unwrap().unwrap().arm_id == "a" {
                    a_count += 1;
                }
            }
            assert!(
                a_count >= 16,
                "Thompson should pick 'a' at least 16/20 times after heavy training, got {a_count}"
            );

            // LinUCB: "x" should dominate
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            policy_tx
                .send(SelectArmRequest {
                    experiment_id: "lu-high".into(),
                    context: Some(ctx),
                    reply_tx,
                })
                .await
                .unwrap();
            let resp = reply_rx.await.unwrap().unwrap();
            assert_eq!(
                resp.arm_id, "x",
                "LinUCB should prefer 'x' after heavy training"
            );

            drop(policy_tx);
            drop(_reward_tx);
            drop(_mgmt_tx);
            handle.await.unwrap();
        }

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_recovery_timing_under_10s() {
        // Verify that restoring from RocksDB completes within the 10s SLA,
        // even with multiple experiments and many snapshots.
        let db_path = temp_db_path("recovery-timing");

        // Pre-populate RocksDB with snapshots for 10 experiments using real policies
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            for exp_idx in 0..10 {
                let exp_id = format!("timing-exp-{exp_idx}");
                let policy = ThompsonSamplingPolicy::new(
                    exp_id.clone(),
                    vec!["a".into(), "b".into(), "c".into()],
                );
                let state = policy.serialize();
                for snap_idx in 0..3u64 {
                    let envelope = SnapshotStore::make_envelope(
                        exp_id.clone(),
                        "thompson_sampling".into(),
                        state.clone(),
                        (snap_idx + 1) * 100,
                        (snap_idx * 50) as i64,
                    );
                    store.write_snapshot(&envelope).unwrap();
                }
            }
        }

        // Measure restore time
        let start = std::time::Instant::now();
        let store = SnapshotStore::open(&db_path).unwrap();
        let config = test_config(db_path.to_str().unwrap());
        let mut core = PolicyCore::new(store, config);
        let restored = core.restore_from_snapshots().unwrap();
        let elapsed = start.elapsed();

        assert_eq!(restored, 10, "should restore all 10 experiments");
        assert!(
            elapsed.as_secs() < 10,
            "Recovery took {:?}, exceeds 10s SLA",
            elapsed
        );
        // In practice, this should be well under 1 second for 10 experiments
        assert!(
            elapsed.as_millis() < 1000,
            "Recovery took {:?}, should be under 1s for 10 experiments",
            elapsed
        );

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_export_affinity_wrong_policy_type() {
        let db_path = temp_db_path("wrong-type");
        let store = SnapshotStore::open(&db_path).unwrap();
        let config = test_config(db_path.to_str().unwrap());
        let mut core = PolicyCore::new(store, config);
        core.register_experiment("thompson-exp".into(), vec!["a".into(), "b".into()]);

        let (_policy_tx, _reward_tx, mgmt_tx, handle) = spawn_core(core);

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        mgmt_tx
            .send(ManagementCommand::ExportAffinity(ExportAffinityRequest {
                experiment_id: "thompson-exp".into(),
                segment_contexts: HashMap::new(),
                reply_tx,
            }))
            .await
            .unwrap();

        let result = reply_rx.await.unwrap();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("wrong policy type"),
            "expected WrongPolicyType error, got: {err}"
        );

        drop(_policy_tx);
        drop(_reward_tx);
        drop(mgmt_tx);
        handle.await.unwrap();

        let _ = std::fs::remove_dir_all(&db_path);
    }

    /// ADR-011: verify that RewardComposer state (normaliser mean/variance)
    /// survives a crash-and-restore cycle via RocksDB.
    #[tokio::test]
    async fn test_multi_objective_composer_crash_recovery() {
        use experimentation_bandit::reward_composer::{CompositionMethod, Objective, RewardComposer};

        let db_path = temp_db_path("multi-obj-crash");

        // Phase 1: train a multi-objective experiment for 50 rewards then crash.
        let saved_n_obs;
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());

            let composer = RewardComposer::new(
                vec![
                    Objective { metric_id: "engagement".into(), weight: 0.7, floor: 0.0, is_primary: false },
                    Objective { metric_id: "quality".into(), weight: 0.3, floor: 0.0, is_primary: false },
                ],
                CompositionMethod::WeightedScalarization,
            );

            let mut core = PolicyCore::new(store, config);
            core.register_multi_objective_experiment(
                "mo-exp".into(),
                vec!["arm_a".into(), "arm_b".into()],
                composer,
            );

            let (_policy_tx, reward_tx, _mgmt_tx, handle) = spawn_core(core);

            // Send enough rewards to trigger snapshots (snapshot_interval = 5).
            for i in 0..50i64 {
                reward_tx
                    .send(RewardUpdate {
                        experiment_id: "mo-exp".into(),
                        arm_id: "arm_a".into(),
                        reward: 0.0, // unused — composer takes over
                        context: None,
                        kafka_offset: i,
                        metric_values: Some(
                            [("engagement".into(), 0.8_f64), ("quality".into(), 0.6_f64)]
                                .into_iter()
                                .collect(),
                        ),
                    })
                    .await
                    .unwrap();
            }

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            // Peek at the composer state before crash so we can verify restoration.
            // We can't inspect core directly after spawn_core (it's moved), so we
            // rely on the snapshot. Just store the expected n_obs.
            saved_n_obs = 50u64;

            drop(reward_tx);
            drop(_policy_tx);
            drop(_mgmt_tx);
            handle.await.unwrap();
        }

        // Phase 2: restore and verify composer state was persisted.
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());

            let mut core = PolicyCore::new(store, config);
            let restored = core.restore_from_snapshots().unwrap();
            assert_eq!(restored, 1, "should restore 1 multi-objective experiment");

            // The composer should have been restored — check it is present.
            assert!(
                core.reward_composers.contains_key("mo-exp"),
                "RewardComposer should be restored from RocksDB snapshot"
            );

            let composer = &core.reward_composers["mo-exp"];
            // Normaliser should have accumulated at least some observations for
            // the snapshotted batches (snapshot_interval = 5 → ≥ 45 obs in last snap).
            let eng_n_obs = composer
                .normalizer
                .stats
                .get("engagement")
                .map(|s| s.n)
                .unwrap_or(0);
            assert!(
                eng_n_obs >= 45,
                "engagement normaliser should have ≥ 45 obs after restore, got {eng_n_obs} (saved {saved_n_obs})"
            );
        }

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_register_meta_experiment_isolated_policies() {
        use crate::types::{MetaVariantPolicyConfig, RegisterMetaExperimentRequest};

        let db_path = temp_db_path("meta-experiment");
        let store = SnapshotStore::open(&db_path).unwrap();
        let config = test_config(db_path.to_str().unwrap());
        let core = PolicyCore::new(store, config);

        let (policy_tx, reward_tx, mgmt_tx, handle) = spawn_core(core);

        // Register a META experiment with two variants, each with different reward weights.
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        mgmt_tx
            .send(ManagementCommand::RegisterMetaExperiment(
                RegisterMetaExperimentRequest {
                    experiment_id: "meta-exp-1".into(),
                    variant_policies: vec![
                        MetaVariantPolicyConfig {
                            variant_id: "obj_watch_time".into(),
                            arm_ids: vec!["content-a".into(), "content-b".into()],
                            reward_weights: [("watch_time".into(), 1.0)]
                                .into_iter()
                                .collect(),
                        },
                        MetaVariantPolicyConfig {
                            variant_id: "obj_engagement".into(),
                            arm_ids: vec!["content-a".into(), "content-b".into()],
                            reward_weights: [
                                ("watch_time".into(), 0.4),
                                ("engagement".into(), 0.6),
                            ]
                            .into_iter()
                            .collect(),
                        },
                    ],
                    reply_tx,
                },
            ))
            .await
            .unwrap();

        let resp = reply_rx.await.unwrap().unwrap();
        assert_eq!(resp.experiment_id, "meta-exp-1");
        assert_eq!(resp.policy_ids.len(), 2);
        assert!(resp
            .policy_ids
            .contains(&"meta-exp-1::v::obj_watch_time".to_string()));
        assert!(resp
            .policy_ids
            .contains(&"meta-exp-1::v::obj_engagement".to_string()));

        // Each variant policy should be independently selectable.
        for policy_id in &resp.policy_ids {
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            policy_tx
                .send(SelectArmRequest {
                    experiment_id: policy_id.clone(),
                    context: None,
                    reply_tx,
                })
                .await
                .unwrap();
            let arm = reply_rx.await.unwrap().unwrap();
            assert!(arm.arm_id == "content-a" || arm.arm_id == "content-b");
        }

        // Train variant "obj_watch_time" — content-a gets all rewards.
        for i in 0..20 {
            reward_tx
                .send(RewardUpdate {
                    experiment_id: "meta-exp-1::v::obj_watch_time".into(),
                    arm_id: "content-a".into(),
                    reward: 1.0,
                    context: None,
                    kafka_offset: i,
                    metric_values: Some(
                        [("watch_time".into(), 10.0)]
                            .into_iter()
                            .collect(),
                    ),
                })
                .await
                .unwrap();
        }

        // Train variant "obj_engagement" — content-b gets all rewards.
        for i in 0..20 {
            reward_tx
                .send(RewardUpdate {
                    experiment_id: "meta-exp-1::v::obj_engagement".into(),
                    arm_id: "content-b".into(),
                    reward: 1.0,
                    context: None,
                    kafka_offset: 20 + i,
                    metric_values: Some(
                        [("watch_time".into(), 2.0), ("engagement".into(), 8.0)]
                            .into_iter()
                            .collect(),
                    ),
                })
                .await
                .unwrap();
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // The parent experiment ID should NOT be registered as a policy.
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        policy_tx
            .send(SelectArmRequest {
                experiment_id: "meta-exp-1".into(),
                context: None,
                reply_tx,
            })
            .await
            .unwrap();
        assert!(
            reply_rx.await.unwrap().is_err(),
            "parent meta experiment should not have a policy; only variants do"
        );

        drop(policy_tx);
        drop(reward_tx);
        drop(mgmt_tx);
        handle.await.unwrap();

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[test]
    fn test_meta_variant_policy_id_format() {
        assert_eq!(
            PolicyCore::meta_variant_policy_id("exp-123", "variant-a"),
            "exp-123::v::variant-a"
        );
    }
}
