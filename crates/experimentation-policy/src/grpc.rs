//! gRPC server for the BanditPolicyService.
//!
//! Translates proto messages into internal channel types, sends them to
//! the PolicyCore via bounded mpsc channels, and awaits oneshot responses.
//! All mutable state lives on the single-threaded PolicyCore (LMAX pattern).

use crate::core::ManagementCommand;
use crate::types::{
    CreateColdStartRequest, ExportAffinityRequest, GetSnapshotRequest, PolicyError,
    RollbackPolicyRequest, SelectArmRequest,
};
use experimentation_bandit::cold_start::ColdStartConfig;
use experimentation_proto::experimentation::bandit::v1::bandit_policy_service_server::BanditPolicyService;
use experimentation_proto::experimentation::bandit::v1::{
    self as proto, CreateColdStartBanditResponse, ExportAffinityScoresResponse,
};
use experimentation_proto::experimentation::common::v1::{
    ArmSelection as ProtoArmSelection, PolicySnapshot as ProtoPolicySnapshot,
};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
use tonic::{Request, Response, Status};
use tracing::info;

/// Default arm IDs for cold-start bandits when not specified.
const DEFAULT_COLD_START_ARMS: &[&str] = &[
    "homepage_featured",
    "trending_shelf",
    "recommended_row",
    "notification",
];

/// Default feature keys for cold-start bandits.
const DEFAULT_COLD_START_FEATURES: &[&str] = &[
    "user_age_bucket",
    "watch_history_len",
    "subscription_tier",
];

/// gRPC handler that bridges proto messages to the PolicyCore via channels.
#[derive(Clone)]
pub struct BanditPolicyServiceHandler {
    policy_tx: mpsc::Sender<SelectArmRequest>,
    mgmt_tx: mpsc::Sender<ManagementCommand>,
}

impl BanditPolicyServiceHandler {
    pub fn new(
        policy_tx: mpsc::Sender<SelectArmRequest>,
        mgmt_tx: mpsc::Sender<ManagementCommand>,
    ) -> Self {
        Self {
            policy_tx,
            mgmt_tx,
        }
    }
}

/// Map internal PolicyError to tonic Status.
fn policy_error_to_status(err: PolicyError) -> Status {
    match &err {
        PolicyError::ExperimentNotFound(_) => Status::not_found(err.to_string()),
        PolicyError::SnapshotNotFound(_) => Status::not_found(err.to_string()),
        PolicyError::WrongPolicyType { .. } => Status::failed_precondition(err.to_string()),
        PolicyError::Internal(_) => Status::internal(err.to_string()),
    }
}

/// Convert internal snapshot response to proto PolicySnapshot.
fn snapshot_to_proto(resp: crate::types::GetSnapshotResponse) -> ProtoPolicySnapshot {
    let seconds = resp.snapshot_at_epoch_ms / 1000;
    let nanos = ((resp.snapshot_at_epoch_ms % 1000) * 1_000_000) as i32;

    ProtoPolicySnapshot {
        experiment_id: resp.experiment_id,
        policy_state: resp.policy_state,
        total_rewards_processed: resp.total_rewards_processed as i64,
        kafka_offset: resp.kafka_offset,
        snapshot_at: Some(prost_types::Timestamp { seconds, nanos }),
    }
}

#[tonic::async_trait]
impl BanditPolicyService for BanditPolicyServiceHandler {
    async fn select_arm(
        &self,
        request: Request<proto::SelectArmRequest>,
    ) -> Result<Response<ProtoArmSelection>, Status> {
        let req = request.into_inner();

        let context = if req.context_features.is_empty() {
            None
        } else {
            Some(req.context_features)
        };

        let (reply_tx, reply_rx) = oneshot::channel();
        self.policy_tx
            .send(SelectArmRequest {
                experiment_id: req.experiment_id,
                context,
                reply_tx,
            })
            .await
            .map_err(|_| Status::unavailable("policy core is shutting down"))?;

        let response = reply_rx
            .await
            .map_err(|_| Status::internal("policy core dropped response channel"))?
            .map_err(policy_error_to_status)?;

        Ok(Response::new(ProtoArmSelection {
            arm_id: response.arm_id,
            assignment_probability: response.assignment_probability,
            all_arm_probabilities: response.all_arm_probabilities,
        }))
    }

    async fn get_slate_assignment(
        &self,
        _request: Request<proto::GetSlateAssignmentRequest>,
    ) -> Result<Response<proto::SlateAssignmentResponse>, Status> {
        // ADR-016: Full slate bandit implementation is in-progress.
        // The contract is defined; the PolicyCore integration is Phase 5 work.
        Err(Status::unimplemented(
            "GetSlateAssignment is not yet wired to PolicyCore; \
             use the contract test service for wire-format validation",
        ))
    }

    /// Stub for ADR-016 slate bandit selection.
    ///
    /// Returns `unimplemented` until `experimentation-bandit` ships a SlatePolicy.
    /// M1 falls back to random slate ordering on gRPC error per the proto contract.
    async fn select_slate(
        &self,
        _request: Request<proto::SelectSlateRequest>,
    ) -> Result<Response<proto::SlateSelection>, Status> {
        Err(Status::unimplemented(
            "slate bandit selection (ADR-016) not yet implemented",
        ))
    }

    async fn create_cold_start_bandit(
        &self,
        request: Request<proto::CreateColdStartBanditRequest>,
    ) -> Result<Response<CreateColdStartBanditResponse>, Status> {
        let req = request.into_inner();

        let config = ColdStartConfig {
            content_id: req.content_id,
            content_metadata: req.content_metadata,
            window_days: if req.window_days > 0 {
                req.window_days
            } else {
                ColdStartConfig::DEFAULT_WINDOW_DAYS
            },
            arm_ids: DEFAULT_COLD_START_ARMS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            feature_keys: DEFAULT_COLD_START_FEATURES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            alpha: 1.0,
            min_exploration_fraction: 0.05,
        };

        let (reply_tx, reply_rx) = oneshot::channel();
        self.mgmt_tx
            .send(ManagementCommand::CreateColdStart(CreateColdStartRequest {
                config,
                reply_tx,
            }))
            .await
            .map_err(|_| Status::unavailable("policy core is shutting down"))?;

        let response = reply_rx
            .await
            .map_err(|_| Status::internal("policy core dropped response channel"))?
            .map_err(policy_error_to_status)?;

        Ok(Response::new(CreateColdStartBanditResponse {
            experiment_id: response.experiment_id,
            content_id: response.content_id,
        }))
    }

    async fn export_affinity_scores(
        &self,
        request: Request<proto::ExportAffinityScoresRequest>,
    ) -> Result<Response<ExportAffinityScoresResponse>, Status> {
        let req = request.into_inner();

        // Build default segment contexts for lifecycle segments.
        // In production, these would come from the feature store or request metadata.
        let segment_contexts = default_segment_contexts();

        let (reply_tx, reply_rx) = oneshot::channel();
        self.mgmt_tx
            .send(ManagementCommand::ExportAffinity(ExportAffinityRequest {
                experiment_id: req.experiment_id,
                segment_contexts,
                reply_tx,
            }))
            .await
            .map_err(|_| Status::unavailable("policy core is shutting down"))?;

        let response = reply_rx
            .await
            .map_err(|_| Status::internal("policy core dropped response channel"))?
            .map_err(policy_error_to_status)?;

        Ok(Response::new(ExportAffinityScoresResponse {
            content_id: response.content_id,
            segment_affinity_scores: response.segment_affinity_scores,
            optimal_placements: response.optimal_placements,
        }))
    }

    async fn get_policy_snapshot(
        &self,
        request: Request<proto::GetPolicySnapshotRequest>,
    ) -> Result<Response<ProtoPolicySnapshot>, Status> {
        let req = request.into_inner();

        let (reply_tx, reply_rx) = oneshot::channel();
        self.mgmt_tx
            .send(ManagementCommand::GetSnapshot(GetSnapshotRequest {
                experiment_id: req.experiment_id,
                reply_tx,
            }))
            .await
            .map_err(|_| Status::unavailable("policy core is shutting down"))?;

        let response = reply_rx
            .await
            .map_err(|_| Status::internal("policy core dropped response channel"))?
            .map_err(policy_error_to_status)?;

        Ok(Response::new(snapshot_to_proto(response)))
    }

    async fn rollback_policy(
        &self,
        request: Request<proto::RollbackPolicyRequest>,
    ) -> Result<Response<ProtoPolicySnapshot>, Status> {
        let req = request.into_inner();

        let (reply_tx, reply_rx) = oneshot::channel();
        self.mgmt_tx
            .send(ManagementCommand::RollbackPolicy(RollbackPolicyRequest {
                experiment_id: req.experiment_id,
                target_snapshot_epoch_ms: req.target_snapshot_epoch_ms,
                reply_tx,
            }))
            .await
            .map_err(|_| Status::unavailable("policy core is shutting down"))?;

        let response = reply_rx
            .await
            .map_err(|_| Status::internal("policy core dropped response channel"))?
            .map_err(policy_error_to_status)?;

        Ok(Response::new(snapshot_to_proto(response)))
    }
}

/// Start the gRPC server serving the BanditPolicyService.
pub async fn serve_grpc(
    addr: String,
    policy_tx: mpsc::Sender<SelectArmRequest>,
    mgmt_tx: mpsc::Sender<ManagementCommand>,
) -> Result<(), String> {
    let addr = addr
        .parse()
        .map_err(|e| format!("invalid gRPC address '{addr}': {e}"))?;

    let handler = BanditPolicyServiceHandler::new(policy_tx, mgmt_tx);

    info!(%addr, "gRPC server starting");

    tonic::transport::Server::builder()
        .add_service(
            experimentation_proto::experimentation::bandit::v1::bandit_policy_service_server::BanditPolicyServiceServer::new(handler),
        )
        .serve(addr)
        .await
        .map_err(|e| format!("gRPC server error: {e}"))
}

/// Build default lifecycle segment contexts for affinity export.
///
/// In production these would be loaded from the feature store. For now,
/// we provide representative feature vectors for the 6 lifecycle segments.
fn default_segment_contexts() -> HashMap<String, HashMap<String, f64>> {
    let mut segments = HashMap::new();

    segments.insert(
        "TRIAL".into(),
        [
            ("user_age_bucket".into(), 1.0),
            ("watch_history_len".into(), 5.0),
            ("subscription_tier".into(), 0.0),
        ]
        .into_iter()
        .collect(),
    );
    segments.insert(
        "NEW".into(),
        [
            ("user_age_bucket".into(), 2.0),
            ("watch_history_len".into(), 20.0),
            ("subscription_tier".into(), 1.0),
        ]
        .into_iter()
        .collect(),
    );
    segments.insert(
        "ESTABLISHED".into(),
        [
            ("user_age_bucket".into(), 3.0),
            ("watch_history_len".into(), 100.0),
            ("subscription_tier".into(), 2.0),
        ]
        .into_iter()
        .collect(),
    );
    segments.insert(
        "REACTIVATED".into(),
        [
            ("user_age_bucket".into(), 3.0),
            ("watch_history_len".into(), 50.0),
            ("subscription_tier".into(), 1.0),
        ]
        .into_iter()
        .collect(),
    );

    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::PolicyCore;
    use crate::snapshot::SnapshotStore;
    use experimentation_proto::experimentation::bandit::v1::bandit_policy_service_server::BanditPolicyServiceServer;
    use std::net::SocketAddr;
    use tokio::sync::mpsc;

    /// Monotonic counter for unique test DB paths.
    static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Helper: spin up PolicyCore + gRPC server on an ephemeral port, return client + cleanup.
    async fn start_test_server() -> (
        experimentation_proto::experimentation::bandit::v1::bandit_policy_service_client::BanditPolicyServiceClient<
            tonic::transport::Channel,
        >,
        SocketAddr,
        tokio::task::JoinHandle<()>,
        std::path::PathBuf,
    ) {
        let counter = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let db_path = std::env::temp_dir().join(format!(
            "test-grpc-{}-{}-{}",
            std::process::id(),
            counter,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&db_path);

        let store = SnapshotStore::open(&db_path).unwrap();
        let config = crate::config::PolicyConfig {
            grpc_addr: "[::1]:0".into(),
            rocksdb_path: db_path.to_str().unwrap().into(),
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
        };

        let (policy_tx, policy_rx) = mpsc::channel(100);
        let (reward_tx, reward_rx) = mpsc::channel(100);
        let (mgmt_tx, mgmt_rx) = mpsc::channel(100);

        // Register a Thompson experiment for tests
        let mut core = PolicyCore::new(store, config);
        core.register_experiment("test-thompson".into(), vec!["a".into(), "b".into()]);
        core.register_linucb_experiment(
            "test-linucb".into(),
            vec!["arm_0".into(), "arm_1".into()],
            vec!["f0".into(), "f1".into()],
            1.0,
            0.05,
        );

        // Spawn core
        tokio::spawn(async move {
            core.run(policy_rx, reward_rx, mgmt_rx).await;
        });

        // Bind to ephemeral port
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let handler = BanditPolicyServiceHandler::new(policy_tx, mgmt_tx);

        let server_handle = tokio::spawn(async move {
            let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
            tonic::transport::Server::builder()
                .add_service(BanditPolicyServiceServer::new(handler))
                .serve_with_incoming(incoming)
                .await
                .unwrap();
        });

        // Small delay for server startup
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let channel = tonic::transport::Channel::from_shared(format!("http://[::1]:{}", addr.port()))
            .unwrap()
            .connect()
            .await
            .unwrap();

        let client = experimentation_proto::experimentation::bandit::v1::bandit_policy_service_client::BanditPolicyServiceClient::new(channel);

        // Keep reward_tx alive for the duration of tests
        std::mem::forget(reward_tx);

        (client, addr, server_handle, db_path)
    }

    #[tokio::test]
    async fn test_select_arm_thompson() {
        let (mut client, _, handle, db_path) = start_test_server().await;

        let resp = client
            .select_arm(proto::SelectArmRequest {
                experiment_id: "test-thompson".into(),
                user_id: "user-1".into(),
                context_features: HashMap::new(),
            })
            .await
            .unwrap()
            .into_inner();

        assert!(resp.arm_id == "a" || resp.arm_id == "b");
        assert!(resp.assignment_probability > 0.0);
        assert_eq!(resp.all_arm_probabilities.len(), 2);

        handle.abort();
        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_select_arm_linucb() {
        let (mut client, _, handle, db_path) = start_test_server().await;

        let resp = client
            .select_arm(proto::SelectArmRequest {
                experiment_id: "test-linucb".into(),
                user_id: "user-1".into(),
                context_features: [("f0".into(), 1.0), ("f1".into(), 0.5)]
                    .into_iter()
                    .collect(),
            })
            .await
            .unwrap()
            .into_inner();

        assert!(resp.arm_id == "arm_0" || resp.arm_id == "arm_1");
        assert_eq!(resp.all_arm_probabilities.len(), 2);

        handle.abort();
        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_select_arm_unknown_experiment() {
        let (mut client, _, handle, db_path) = start_test_server().await;

        let err = client
            .select_arm(proto::SelectArmRequest {
                experiment_id: "nonexistent".into(),
                user_id: "user-1".into(),
                context_features: HashMap::new(),
            })
            .await
            .unwrap_err();

        assert_eq!(err.code(), tonic::Code::NotFound);

        handle.abort();
        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_create_cold_start_bandit() {
        let (mut client, _, handle, db_path) = start_test_server().await;

        let resp = client
            .create_cold_start_bandit(proto::CreateColdStartBanditRequest {
                content_id: "movie-42".into(),
                content_metadata: [("genre".into(), "comedy".into())]
                    .into_iter()
                    .collect(),
                window_days: 7,
            })
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.experiment_id, "cold-start:movie-42");
        assert_eq!(resp.content_id, "movie-42");

        // Should be able to select an arm from the newly created experiment
        let arm_resp = client
            .select_arm(proto::SelectArmRequest {
                experiment_id: "cold-start:movie-42".into(),
                user_id: "user-1".into(),
                context_features: [
                    ("user_age_bucket".into(), 2.0),
                    ("watch_history_len".into(), 50.0),
                    ("subscription_tier".into(), 1.0),
                ]
                .into_iter()
                .collect(),
            })
            .await
            .unwrap()
            .into_inner();

        assert!(DEFAULT_COLD_START_ARMS.contains(&arm_resp.arm_id.as_str()));

        handle.abort();
        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_export_affinity_scores() {
        let (mut client, _, handle, db_path) = start_test_server().await;

        // Create cold-start bandit first
        client
            .create_cold_start_bandit(proto::CreateColdStartBanditRequest {
                content_id: "show-99".into(),
                content_metadata: HashMap::new(),
                window_days: 7,
            })
            .await
            .unwrap();

        // Export affinity scores (untrained → all zeros, but should not error)
        let resp = client
            .export_affinity_scores(proto::ExportAffinityScoresRequest {
                experiment_id: "cold-start:show-99".into(),
            })
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.content_id, "show-99");
        assert!(!resp.segment_affinity_scores.is_empty());
        assert!(!resp.optimal_placements.is_empty());

        handle.abort();
        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[tokio::test]
    async fn test_get_policy_snapshot_not_found() {
        let (mut client, _, handle, db_path) = start_test_server().await;

        let err = client
            .get_policy_snapshot(proto::GetPolicySnapshotRequest {
                experiment_id: "nonexistent".into(),
            })
            .await
            .unwrap_err();

        assert_eq!(err.code(), tonic::Code::NotFound);

        handle.abort();
        let _ = std::fs::remove_dir_all(&db_path);
    }
}
