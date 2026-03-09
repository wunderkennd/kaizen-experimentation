//! LMAX-inspired single-threaded policy core (ADR-002).
//!
//! All policy state mutations happen on this dedicated thread.
//! The gRPC and Kafka threads communicate via bounded channels.
//! Zero mutexes. Zero shared mutable state.

use crate::config::PolicyConfig;
use crate::snapshot::SnapshotStore;
use crate::types::{
    CreateColdStartRequest, CreateColdStartResponse, ExportAffinityRequest, ExportAffinityResponse,
    GetSnapshotRequest, GetSnapshotResponse, PolicyError, RewardUpdate, RollbackPolicyRequest,
    SelectArmRequest, SelectArmResponse,
};
use experimentation_bandit::cold_start;
use experimentation_bandit::linucb::LinUcbPolicy;
use experimentation_bandit::policy::AnyPolicy;
use experimentation_bandit::thompson::ThompsonSamplingPolicy;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Management commands sent to the policy core (cold-start, snapshots, rollback).
pub enum ManagementCommand {
    CreateColdStart(CreateColdStartRequest),
    ExportAffinity(ExportAffinityRequest),
    GetSnapshot(GetSnapshotRequest),
    RollbackPolicy(RollbackPolicyRequest),
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

        policy.update(&update.arm_id, update.reward, update.context.as_ref());
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

        let envelope = SnapshotStore::make_envelope(
            experiment_id.to_string(),
            policy.policy_type().to_string(),
            policy.serialize(),
            policy.total_rewards(),
            kafka_offset,
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
}
