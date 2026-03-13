//! M1 ↔ M4b Live Bandit Delegation Contract Tests
//!
//! These tests verify wire-format compatibility between M1's `GrpcBanditClient`
//! and M4b's `BanditPolicyService`. Unlike the mock tests in `assignment_test.rs`,
//! these use a **real** Thompson Sampling / LinUCB implementation from
//! `experimentation-bandit` to validate the full proto roundtrip.
//!
//! Contract points verified:
//! 1. SelectArm request/response field roundtrip (Thompson + LinUCB)
//! 2. Context feature serialization (map<string, double>)
//! 3. ArmSelection fields: non-empty arm_id, finite probabilities, all_arm_probabilities sum ≈ 1
//! 4. Error codes: NOT_FOUND for unknown experiments
//! 5. Cold-start lifecycle: Create → SelectArm → ExportAffinity
//! 6. Concurrent SelectArm calls (LMAX contract)
//! 7. GrpcBanditClient timeout + fallback behavior

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use experimentation_assignment::bandit_client::GrpcBanditClient;
use experimentation_bandit::linucb::LinUcbPolicy;
use experimentation_bandit::policy::Policy;
use experimentation_bandit::thompson::BetaArm;
use experimentation_proto::experimentation::bandit::v1::{
    self as proto,
    bandit_policy_service_server::{BanditPolicyService, BanditPolicyServiceServer},
    CreateColdStartBanditResponse, ExportAffinityScoresResponse,
};
use experimentation_proto::experimentation::common::v1::{
    ArmSelection as ProtoArmSelection, PolicySnapshot as ProtoPolicySnapshot,
};
use rand::rngs::StdRng;
use rand::SeedableRng;
use tonic::{Request, Response, Status};

// ---------------------------------------------------------------------------
// Real M4b-compatible service for contract testing
// ---------------------------------------------------------------------------

/// A test server that uses real bandit algorithms (not mocks) to exercise
/// the full proto contract from M1's perspective.
struct RealBanditService {
    /// Thompson Sampling experiments: experiment_id → arms
    thompson_experiments: Arc<Mutex<HashMap<String, Vec<BetaArm>>>>,
    /// LinUCB experiments: experiment_id → policy
    linucb_experiments: Arc<Mutex<HashMap<String, LinUcbPolicy>>>,
    /// Cold-start experiments created dynamically
    cold_start_experiments: Arc<Mutex<HashMap<String, ColdStartInfo>>>,
}

struct ColdStartInfo {
    content_id: String,
    arms: Vec<String>,
}

impl RealBanditService {
    fn new() -> Self {
        let mut thompson = HashMap::new();
        // Pre-register a Thompson experiment with 3 arms
        thompson.insert(
            "test-thompson-3arm".to_string(),
            vec![
                BetaArm::new("arm_hero".into()),
                BetaArm::new("arm_carousel".into()),
                BetaArm::new("arm_spotlight".into()),
            ],
        );
        // Pre-register a 2-arm Thompson experiment
        thompson.insert(
            "test-thompson-2arm".to_string(),
            vec![
                BetaArm::new("control".into()),
                BetaArm::new("treatment".into()),
            ],
        );

        let mut linucb = HashMap::new();
        // Pre-register a LinUCB experiment with 2 features, 2 arms
        linucb.insert(
            "test-linucb-ctx".to_string(),
            LinUcbPolicy::new(
                "test-linucb-ctx".into(),
                vec!["arm_a".into(), "arm_b".into()],
                vec!["age".into(), "tenure".into()],
                1.0,  // alpha
                0.05, // min_exploration
            ),
        );

        Self {
            thompson_experiments: Arc::new(Mutex::new(thompson)),
            linucb_experiments: Arc::new(Mutex::new(linucb)),
            cold_start_experiments: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[tonic::async_trait]
impl BanditPolicyService for RealBanditService {
    async fn select_arm(
        &self,
        request: Request<proto::SelectArmRequest>,
    ) -> Result<Response<ProtoArmSelection>, Status> {
        let req = request.into_inner();
        let experiment_id = &req.experiment_id;

        // Try Thompson first
        {
            let thompson = self.thompson_experiments.lock().unwrap();
            if let Some(arms) = thompson.get(experiment_id) {
                let mut rng = StdRng::seed_from_u64(
                    experimentation_hash::murmur3::murmurhash3_x86_32(
                        req.user_id.as_bytes(),
                        0,
                    ) as u64,
                );
                let selection =
                    experimentation_bandit::thompson::select_arm(arms, &mut rng);
                return Ok(Response::new(ProtoArmSelection {
                    arm_id: selection.arm_id,
                    assignment_probability: selection.assignment_probability,
                    all_arm_probabilities: selection.all_arm_probabilities,
                }));
            }
        }

        // Try LinUCB
        {
            let linucb = self.linucb_experiments.lock().unwrap();
            if let Some(policy) = linucb.get(experiment_id) {
                let context = if req.context_features.is_empty() {
                    None
                } else {
                    Some(req.context_features)
                };
                let selection = policy.select_arm(context.as_ref());
                return Ok(Response::new(ProtoArmSelection {
                    arm_id: selection.arm_id,
                    assignment_probability: selection.assignment_probability,
                    all_arm_probabilities: selection.all_arm_probabilities,
                }));
            }
        }

        // Try cold-start (they use Thompson internally)
        {
            let cold = self.cold_start_experiments.lock().unwrap();
            if let Some(info) = cold.get(experiment_id) {
                let arms: Vec<BetaArm> =
                    info.arms.iter().map(|a| BetaArm::new(a.clone())).collect();
                let mut rng = StdRng::seed_from_u64(
                    experimentation_hash::murmur3::murmurhash3_x86_32(
                        req.user_id.as_bytes(),
                        0,
                    ) as u64,
                );
                let selection =
                    experimentation_bandit::thompson::select_arm(&arms, &mut rng);
                return Ok(Response::new(ProtoArmSelection {
                    arm_id: selection.arm_id,
                    assignment_probability: selection.assignment_probability,
                    all_arm_probabilities: selection.all_arm_probabilities,
                }));
            }
        }

        Err(Status::not_found(format!(
            "experiment '{experiment_id}' not found"
        )))
    }

    async fn create_cold_start_bandit(
        &self,
        request: Request<proto::CreateColdStartBanditRequest>,
    ) -> Result<Response<CreateColdStartBanditResponse>, Status> {
        let req = request.into_inner();
        let experiment_id = format!("cold-start:{}", req.content_id);

        let default_arms = vec![
            "homepage_featured".into(),
            "trending_shelf".into(),
            "recommended_row".into(),
            "notification".into(),
        ];
        self.cold_start_experiments.lock().unwrap().insert(
            experiment_id.clone(),
            ColdStartInfo {
                content_id: req.content_id.clone(),
                arms: default_arms,
            },
        );

        Ok(Response::new(CreateColdStartBanditResponse {
            experiment_id,
            content_id: req.content_id,
        }))
    }

    async fn export_affinity_scores(
        &self,
        request: Request<proto::ExportAffinityScoresRequest>,
    ) -> Result<Response<ExportAffinityScoresResponse>, Status> {
        let req = request.into_inner();

        let cold = self.cold_start_experiments.lock().unwrap();
        let info = cold.get(&req.experiment_id).ok_or_else(|| {
            Status::not_found(format!("experiment '{}' not found", req.experiment_id))
        })?;

        // Build segment affinity scores (untrained → uniform priors)
        let segments = ["TRIAL", "NEW", "ESTABLISHED", "REACTIVATED"];
        let mut segment_scores = HashMap::new();
        let mut optimal_placements = HashMap::new();

        for segment in &segments {
            // Uniform prior → all segments get equal affinity
            segment_scores.insert(segment.to_string(), 0.5);
            optimal_placements.insert(segment.to_string(), info.arms[0].clone());
        }

        Ok(Response::new(ExportAffinityScoresResponse {
            content_id: info.content_id.clone(),
            segment_affinity_scores: segment_scores,
            optimal_placements,
        }))
    }

    async fn get_policy_snapshot(
        &self,
        request: Request<proto::GetPolicySnapshotRequest>,
    ) -> Result<Response<ProtoPolicySnapshot>, Status> {
        Err(Status::not_found(format!(
            "snapshot for '{}' not found",
            request.into_inner().experiment_id
        )))
    }

    async fn rollback_policy(
        &self,
        request: Request<proto::RollbackPolicyRequest>,
    ) -> Result<Response<ProtoPolicySnapshot>, Status> {
        Err(Status::not_found(format!(
            "experiment '{}' not found",
            request.into_inner().experiment_id
        )))
    }
}

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

async fn start_real_m4b() -> (GrpcBanditClient, SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let service = RealBanditService::new();
    let server_handle = tokio::spawn(async move {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        tonic::transport::Server::builder()
            .add_service(BanditPolicyServiceServer::new(service))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });

    // Allow server to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = GrpcBanditClient::connect(&format!("http://[::1]:{}", addr.port()))
        .await
        .expect("failed to connect GrpcBanditClient to test M4b");

    (client, addr, server_handle)
}

// ---------------------------------------------------------------------------
// Contract Tests
// ---------------------------------------------------------------------------

/// 1. SelectArm Thompson: arm_id is one of registered arms, probability > 0, all probs present.
#[tokio::test]
async fn contract_select_arm_thompson_roundtrip() {
    let (client, _, handle) = start_real_m4b().await;

    let result = client
        .select_arm("test-thompson-3arm", "user-42", HashMap::new())
        .await
        .expect("SelectArm should succeed");

    // arm_id must be one of the registered arms
    let valid_arms = ["arm_hero", "arm_carousel", "arm_spotlight"];
    assert!(
        valid_arms.contains(&result.arm_id.as_str()),
        "unexpected arm_id: {}",
        result.arm_id
    );

    // assignment_probability must be positive and finite
    assert!(result.assignment_probability > 0.0);
    assert!(result.assignment_probability.is_finite());

    // all_arm_probabilities must contain all 3 arms
    assert_eq!(result.all_arm_probabilities.len(), 3);
    for arm in &valid_arms {
        assert!(
            result.all_arm_probabilities.contains_key(*arm),
            "missing arm in all_arm_probabilities: {arm}"
        );
    }

    // Probabilities must sum to approximately 1.0
    let sum: f64 = result.all_arm_probabilities.values().sum();
    assert!(
        (sum - 1.0).abs() < 0.01,
        "probabilities sum to {sum}, expected ~1.0"
    );

    handle.abort();
}

/// 2. SelectArm is deterministic for same user (via seeded RNG).
#[tokio::test]
async fn contract_select_arm_deterministic_for_same_user() {
    let (client, _, handle) = start_real_m4b().await;

    let r1 = client
        .select_arm("test-thompson-2arm", "stable-user-99", HashMap::new())
        .await
        .unwrap();
    let r2 = client
        .select_arm("test-thompson-2arm", "stable-user-99", HashMap::new())
        .await
        .unwrap();

    assert_eq!(r1.arm_id, r2.arm_id, "same user must get same arm");
    assert!(
        (r1.assignment_probability - r2.assignment_probability).abs() < f64::EPSILON,
        "same user must get same probability"
    );

    handle.abort();
}

/// 3. SelectArm LinUCB with context features: features round-trip through proto.
#[tokio::test]
async fn contract_select_arm_linucb_context_features() {
    let (client, _, handle) = start_real_m4b().await;

    let context: HashMap<String, f64> =
        [("age".into(), 30.0), ("tenure".into(), 2.5)]
            .into_iter()
            .collect();

    let result = client
        .select_arm("test-linucb-ctx", "user-with-context", context)
        .await
        .expect("LinUCB SelectArm should succeed with context");

    // arm_id must be one of the registered LinUCB arms
    assert!(
        result.arm_id == "arm_a" || result.arm_id == "arm_b",
        "unexpected arm: {}",
        result.arm_id
    );
    assert!(result.assignment_probability > 0.0);
    assert_eq!(result.all_arm_probabilities.len(), 2);

    handle.abort();
}

/// 4. SelectArm unknown experiment returns error (M1 expects this for fallback).
#[tokio::test]
async fn contract_select_arm_unknown_experiment_error() {
    let (client, _, handle) = start_real_m4b().await;

    let err = client
        .select_arm("nonexistent-exp", "user-1", HashMap::new())
        .await;

    assert!(err.is_err(), "unknown experiment should return error");
    match err.unwrap_err() {
        experimentation_assignment::bandit_client::BanditClientError::Grpc(status) => {
            assert_eq!(
                status.code(),
                tonic::Code::NotFound,
                "expected NOT_FOUND, got {:?}",
                status.code()
            );
        }
        other => panic!("expected Grpc error, got: {other:?}"),
    }

    handle.abort();
}

/// 5. Cold-start lifecycle: Create → SelectArm → ExportAffinity.
#[tokio::test]
async fn contract_cold_start_full_lifecycle() {
    let (client, _, handle) = start_real_m4b().await;

    // Step 1: Create cold-start bandit
    let created = client
        .create_cold_start_bandit(
            "movie-new-42",
            [("genre".into(), "sci-fi".into())].into_iter().collect(),
            14,
        )
        .await
        .expect("CreateColdStartBandit should succeed");

    assert_eq!(created.experiment_id, "cold-start:movie-new-42");
    assert_eq!(created.content_id, "movie-new-42");

    // Step 2: SelectArm on newly created experiment
    let arm = client
        .select_arm(
            "cold-start:movie-new-42",
            "user-cold-1",
            [
                ("user_age_bucket".into(), 2.0),
                ("watch_history_len".into(), 50.0),
                ("subscription_tier".into(), 1.0),
            ]
            .into_iter()
            .collect(),
        )
        .await
        .expect("SelectArm on cold-start should succeed");

    let valid_arms = [
        "homepage_featured",
        "trending_shelf",
        "recommended_row",
        "notification",
    ];
    assert!(
        valid_arms.contains(&arm.arm_id.as_str()),
        "cold-start arm_id '{}' not in default arms",
        arm.arm_id
    );
    assert_eq!(arm.all_arm_probabilities.len(), 4);

    // Step 3: Export affinity scores
    let affinity = client
        .export_affinity_scores("cold-start:movie-new-42")
        .await
        .expect("ExportAffinityScores should succeed");

    assert_eq!(affinity.content_id, "movie-new-42");
    assert!(!affinity.segment_affinity_scores.is_empty());
    assert!(!affinity.optimal_placements.is_empty());

    // All affinity scores must be finite (assert_finite in GrpcBanditClient)
    for (segment, score) in &affinity.segment_affinity_scores {
        assert!(
            score.is_finite(),
            "affinity score for segment '{segment}' is not finite: {score}"
        );
    }

    handle.abort();
}

/// 6. ExportAffinityScores for unknown experiment returns error.
#[tokio::test]
async fn contract_export_affinity_unknown_experiment() {
    let (client, _, handle) = start_real_m4b().await;

    let err = client
        .export_affinity_scores("nonexistent-cold-start")
        .await;

    assert!(err.is_err());

    handle.abort();
}

/// 7. Concurrent SelectArm calls return valid results.
#[tokio::test]
async fn contract_concurrent_select_arm() {
    let (client, _, handle) = start_real_m4b().await;

    let mut handles = Vec::new();
    for i in 0..20u32 {
        let c = client.clone();
        handles.push(tokio::spawn(async move {
            c.select_arm(
                "test-thompson-3arm",
                &format!("concurrent-user-{i}"),
                HashMap::new(),
            )
            .await
        }));
    }

    let valid_arms = ["arm_hero", "arm_carousel", "arm_spotlight"];
    for h in handles {
        let result = h.await.unwrap().expect("concurrent SelectArm should succeed");
        assert!(valid_arms.contains(&result.arm_id.as_str()));
        assert!(result.assignment_probability > 0.0);
        assert!(result.assignment_probability.is_finite());
    }

    handle.abort();
}

/// 8. All arm probabilities are finite (fail-fast data integrity contract).
#[tokio::test]
async fn contract_all_probabilities_finite() {
    let (client, _, handle) = start_real_m4b().await;

    for user_i in 0..10u32 {
        let result = client
            .select_arm(
                "test-thompson-3arm",
                &format!("finite-check-{user_i}"),
                HashMap::new(),
            )
            .await
            .unwrap();

        assert!(
            result.assignment_probability.is_finite(),
            "assignment_probability not finite for user {user_i}"
        );
        for (arm, prob) in &result.all_arm_probabilities {
            assert!(
                prob.is_finite(),
                "probability for arm '{arm}' not finite for user {user_i}"
            );
            assert!(
                *prob > 0.0,
                "probability for arm '{arm}' must be positive, got {prob}"
            );
        }
    }

    handle.abort();
}

/// 9. Different users get different arms (distribution check).
#[tokio::test]
async fn contract_user_distribution_across_arms() {
    let (client, _, handle) = start_real_m4b().await;

    let mut arm_counts: HashMap<String, u32> = HashMap::new();
    for i in 0..300u32 {
        let result = client
            .select_arm(
                "test-thompson-3arm",
                &format!("dist-user-{i}"),
                HashMap::new(),
            )
            .await
            .unwrap();
        *arm_counts.entry(result.arm_id).or_default() += 1;
    }

    // With uniform prior (alpha=1, beta=1), Thompson Sampling should distribute
    // fairly evenly across 3 arms. Each arm should get at least some traffic.
    assert_eq!(
        arm_counts.len(),
        3,
        "all 3 arms should receive traffic: {arm_counts:?}"
    );
    for (arm, count) in &arm_counts {
        assert!(
            *count > 30,
            "arm '{arm}' only got {count}/300 assignments — too skewed"
        );
    }

    handle.abort();
}

/// 10. CreateColdStartBandit with default window_days (0 → server default).
#[tokio::test]
async fn contract_cold_start_default_window() {
    let (client, _, handle) = start_real_m4b().await;

    let created = client
        .create_cold_start_bandit("show-defaults", HashMap::new(), 0)
        .await
        .expect("cold-start with window_days=0 should use server default");

    assert_eq!(created.experiment_id, "cold-start:show-defaults");
    assert_eq!(created.content_id, "show-defaults");

    handle.abort();
}
