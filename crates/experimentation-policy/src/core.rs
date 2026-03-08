//! LMAX-inspired single-threaded policy core (ADR-002).
//!
//! All policy state mutations happen on this dedicated thread.
//! The gRPC and Kafka threads communicate via bounded channels.
//! Zero mutexes. Zero shared mutable state.

use crate::config::PolicyConfig;
use crate::snapshot::SnapshotStore;
use crate::types::{PolicyError, RewardUpdate, SelectArmRequest, SelectArmResponse};
use experimentation_bandit::linucb::LinUcbPolicy;
use experimentation_bandit::policy::AnyPolicy;
use experimentation_bandit::thompson::ThompsonSamplingPolicy;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// The single-threaded policy core that owns all mutable bandit state.
pub struct PolicyCore {
    /// All experiment policies, keyed by experiment_id.
    policies: HashMap<String, AnyPolicy>,
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
    /// Uses `tokio::select!` to multiplex arm selection requests and reward updates
    /// on a single thread. This is the LMAX pattern — no locks, no shared state.
    pub async fn run(
        mut self,
        mut policy_rx: mpsc::Receiver<SelectArmRequest>,
        mut reward_rx: mpsc::Receiver<RewardUpdate>,
    ) {
        info!("Policy core event loop started");

        loop {
            tokio::select! {
                // Arm selection requests from gRPC (Thread 1)
                Some(req) = policy_rx.recv() => {
                    let response = self.handle_select_arm(&req.experiment_id, req.context.as_ref());
                    // Ignore send errors — caller may have timed out.
                    let _ = req.reply_tx.send(response);
                }
                // Reward updates from Kafka (Thread 2)
                Some(update) = reward_rx.recv() => {
                    self.handle_reward_update(update);
                }
                // Both channels closed → shutdown
                else => {
                    info!("All channels closed, policy core shutting down");
                    break;
                }
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

    #[tokio::test]
    async fn test_select_arm_and_reward_via_channels() {
        let db_path = temp_db_path("channel-test");
        let store = SnapshotStore::open(&db_path).unwrap();
        let config = test_config(db_path.to_str().unwrap());

        let (policy_tx, policy_rx) = mpsc::channel(100);
        let (reward_tx, reward_rx) = mpsc::channel(100);

        let mut core = PolicyCore::new(store, config);
        core.register_experiment("exp-1".into(), vec!["a".into(), "b".into()]);

        // Spawn the core on a task (simulating the dedicated thread)
        let handle = tokio::spawn(core.run(policy_rx, reward_rx));

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

        // Drop senders to trigger shutdown
        drop(policy_tx);
        drop(reward_tx);
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

            let (_policy_tx, policy_rx) = mpsc::channel(100);
            let (reward_tx, reward_rx) = mpsc::channel(100);

            let mut core = PolicyCore::new(store, config);
            core.register_experiment("exp-1".into(), vec!["a".into(), "b".into()]);

            let handle = tokio::spawn(core.run(policy_rx, reward_rx));

            // Send enough rewards to trigger snapshots (interval=5).
            // Use 100 successes for arm "a" to make selection deterministic.
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

            // Allow time for processing
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            // Simulate crash — drop everything
            drop(reward_tx);
            drop(_policy_tx);
            handle.await.unwrap();
        }

        // Phase 2: Reopen RocksDB and restore
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());

            let mut core = PolicyCore::new(store, config);
            let restored = core.restore_from_snapshots().unwrap();
            assert_eq!(restored, 1, "should restore 1 experiment");

            // Verify we can select arms from the restored policy
            let (policy_tx, policy_rx) = mpsc::channel(100);
            let (_reward_tx, reward_rx) = mpsc::channel(100);

            let handle = tokio::spawn(core.run(policy_rx, reward_rx));

            // After 100 rewards to arm "a" (all successes), arm "a" should dominate.
            // Sample 10 times to reduce flakiness.
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
            handle.await.unwrap();
        }

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_linucb_select_arm_via_channels() {
        let db_path = temp_db_path("linucb-channel");
        let store = SnapshotStore::open(&db_path).unwrap();
        let config = test_config(db_path.to_str().unwrap());

        let (policy_tx, policy_rx) = mpsc::channel(100);
        let (reward_tx, reward_rx) = mpsc::channel(100);

        let mut core = PolicyCore::new(store, config);
        core.register_linucb_experiment(
            "linucb-exp".into(),
            vec!["a".into(), "b".into()],
            vec!["f0".into(), "f1".into()],
            1.0,
            0.05,
        );

        let handle = tokio::spawn(core.run(policy_rx, reward_rx));

        // Send a SelectArm request with context
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

        // Send reward updates with context
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

        // Verify selection still works after updates
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
        handle.await.unwrap();

        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_linucb_crash_recovery() {
        let db_path = temp_db_path("linucb-crash");
        let ctx: HashMap<String, f64> = [("f0".into(), 1.0), ("f1".into(), 0.5)]
            .into_iter()
            .collect();

        // Phase 1: Train LinUCB, generate snapshots
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());

            let (_policy_tx, policy_rx) = mpsc::channel(100);
            let (reward_tx, reward_rx) = mpsc::channel(100);

            let mut core = PolicyCore::new(store, config);
            core.register_linucb_experiment(
                "linucb-exp".into(),
                vec!["a".into(), "b".into()],
                vec!["f0".into(), "f1".into()],
                1.0,
                0.05,
            );

            let handle = tokio::spawn(core.run(policy_rx, reward_rx));

            // Send rewards — arm "a" always gets reward 1.0
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
            handle.await.unwrap();
        }

        // Phase 2: Restore and verify state
        {
            let store = SnapshotStore::open(&db_path).unwrap();
            let config = test_config(db_path.to_str().unwrap());

            let mut core = PolicyCore::new(store, config);
            let restored = core.restore_from_snapshots().unwrap();
            assert_eq!(restored, 1, "should restore 1 LinUCB experiment");

            let (policy_tx, policy_rx) = mpsc::channel(100);
            let (_reward_tx, reward_rx) = mpsc::channel(100);

            let handle = tokio::spawn(core.run(policy_rx, reward_rx));

            // Verify the restored policy selects arms correctly
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
            // After 50 rewards to arm "a", it should be strongly preferred
            assert_eq!(
                response.arm_id, "a",
                "restored LinUCB should prefer arm 'a' after training"
            );

            drop(policy_tx);
            drop(_reward_tx);
            handle.await.unwrap();
        }

        let _ = std::fs::remove_dir_all(&db_path);
    }
}
