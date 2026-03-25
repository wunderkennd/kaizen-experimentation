//! M1 ↔ M4b GetSlateAssignment Contract Tests (ADR-016)
//!
//! These tests verify the wire-format contract between M1's slate assignment path
//! and M4b's new `GetSlateAssignment` RPC. The slate interface supports
//! SLATE_FACTORIZED_TS experiments where M4b returns an ordered list of arm IDs
//! rather than a single arm.
//!
//! Contract points verified:
//! 1. GetSlateAssignmentRequest fields round-trip: experiment_id, user_id, context_features, num_slots
//! 2. SlateAssignmentResponse: arm_ids length equals num_slots, all unique, all valid candidates
//! 3. slot_probabilities: length equals num_slots, all positive and finite
//! 4. joint_probability: positive, finite, approximately product of slot_probabilities
//! 5. Different users produce different orderings (distribution check)
//! 6. Same user + same experiment produces the same slate (deterministic)
//! 7. Unknown experiment returns NOT_FOUND
//! 8. Slate with context_features (contextual variant)
//! 9. num_slots > candidate pool → capped at pool size
//! 10. Concurrent slate requests — valid results under load

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use experimentation_bandit::thompson::BetaArm;
use experimentation_proto::experimentation::bandit::v1::{
    self as proto,
    bandit_policy_service_server::{BanditPolicyService, BanditPolicyServiceServer},
    CreateColdStartBanditResponse, ExportAffinityScoresResponse, SelectSlateRequest, SlateAssignmentResponse, SlateSelection,
};
use experimentation_proto::experimentation::common::v1::{
    ArmSelection as ProtoArmSelection, PolicySnapshot as ProtoPolicySnapshot,
};
use rand::rngs::StdRng;
use rand::SeedableRng;
use tonic::{Request, Response, Status};

// ---------------------------------------------------------------------------
// Test M4b implementation with real slate selection (factorized Thompson)
// ---------------------------------------------------------------------------

/// Slate experiment config used by the test service.
struct SlateExperiment {
    /// Candidate arm IDs (all eligible items).
    candidates: Vec<BetaArm>,
    /// Default number of slots if num_slots == 0 in request.
    default_slots: usize,
}

struct SlateTestService {
    experiments: Arc<Mutex<HashMap<String, SlateExperiment>>>,
}

impl SlateTestService {
    fn new() -> Self {
        let mut exps = HashMap::new();

        // 6-candidate, 3-slot slate: content ranking experiment
        exps.insert(
            "slate-content-3slot".to_string(),
            SlateExperiment {
                candidates: vec![
                    BetaArm::new("drama_series".into()),
                    BetaArm::new("comedy_film".into()),
                    BetaArm::new("documentary".into()),
                    BetaArm::new("action_blockbuster".into()),
                    BetaArm::new("kids_animation".into()),
                    BetaArm::new("thriller".into()),
                ],
                default_slots: 3,
            },
        );

        // 4-candidate, 2-slot slate: homepage hero banners
        exps.insert(
            "slate-hero-2slot".to_string(),
            SlateExperiment {
                candidates: vec![
                    BetaArm::new("hero_a".into()),
                    BetaArm::new("hero_b".into()),
                    BetaArm::new("hero_c".into()),
                    BetaArm::new("hero_d".into()),
                ],
                default_slots: 2,
            },
        );

        // Small pool for the "num_slots > pool" test
        exps.insert(
            "slate-small-pool".to_string(),
            SlateExperiment {
                candidates: vec![
                    BetaArm::new("item_x".into()),
                    BetaArm::new("item_y".into()),
                ],
                default_slots: 5, // exceeds pool size of 2
            },
        );

        Self {
            experiments: Arc::new(Mutex::new(exps)),
        }
    }
}

/// Factorized Thompson Sampling slate selection.
///
/// For each slot, sample from the remaining candidates using Thompson's Beta
/// sampling, remove the selected arm, and recurse. Returns (arm_ids, per_slot_probs).
fn select_slate_factorized(
    candidates: &[BetaArm],
    num_slots: usize,
    rng: &mut StdRng,
) -> (Vec<String>, Vec<f64>) {
    let actual_slots = num_slots.min(candidates.len());
    let mut remaining: Vec<BetaArm> = candidates.to_vec();
    let mut arm_ids = Vec::with_capacity(actual_slots);
    let mut slot_probs = Vec::with_capacity(actual_slots);

    for _ in 0..actual_slots {
        if remaining.is_empty() {
            break;
        }
        let selection = experimentation_bandit::thompson::select_arm(&remaining, rng);
        let prob = selection.assignment_probability;
        let arm_id = selection.arm_id.clone();

        // Per-slot probability: marginal probability of this arm at this slot
        // given the remaining candidate pool.
        slot_probs.push(prob.max(1e-9)); // avoid exact zero for IPW stability
        arm_ids.push(arm_id.clone());
        remaining.retain(|a| a.arm_id != arm_id);
    }

    (arm_ids, slot_probs)
}

#[tonic::async_trait]
impl BanditPolicyService for SlateTestService {
    async fn select_arm(
        &self,
        _request: Request<proto::SelectArmRequest>,
    ) -> Result<Response<ProtoArmSelection>, Status> {
        Err(Status::unimplemented("use GetSlateAssignment for slate tests"))
    }

    async fn get_slate_assignment(
        &self,
        request: Request<proto::GetSlateAssignmentRequest>,
    ) -> Result<Response<SlateAssignmentResponse>, Status> {
        let req = request.into_inner();

        if req.experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        if req.user_id.is_empty() {
            return Err(Status::invalid_argument("user_id is required"));
        }

        let (candidates, default_slots) = {
            let exps = self.experiments.lock().unwrap();
            let exp = exps.get(&req.experiment_id).ok_or_else(|| {
                Status::not_found(format!("experiment '{}' not found", req.experiment_id))
            })?;
            (exp.candidates.clone(), exp.default_slots)
        };

        let num_slots = if req.num_slots > 0 {
            req.num_slots as usize
        } else {
            default_slots
        };

        // Seed RNG deterministically per (user_id, experiment_id) for reproducibility.
        let seed = experimentation_hash::murmur3::murmurhash3_x86_32(
            format!("{}:{}", req.user_id, req.experiment_id).as_bytes(),
            0,
        ) as u64;
        let mut rng = StdRng::seed_from_u64(seed);

        let (arm_ids, slot_probs) = select_slate_factorized(&candidates, num_slots, &mut rng);

        // Joint probability: product of per-slot marginals (factorized model approximation).
        let joint_probability: f64 = slot_probs.iter().product();

        Ok(Response::new(SlateAssignmentResponse {
            arm_ids,
            slot_probabilities: slot_probs,
            joint_probability,
        }))
    }

    async fn create_cold_start_bandit(
        &self,
        _request: Request<proto::CreateColdStartBanditRequest>,
    ) -> Result<Response<CreateColdStartBanditResponse>, Status> {
        Err(Status::unimplemented("not used in slate contract tests"))
    }

    async fn export_affinity_scores(
        &self,
        _request: Request<proto::ExportAffinityScoresRequest>,
    ) -> Result<Response<ExportAffinityScoresResponse>, Status> {
        Err(Status::unimplemented("not used in slate contract tests"))
    }

    async fn get_policy_snapshot(
        &self,
        _request: Request<proto::GetPolicySnapshotRequest>,
    ) -> Result<Response<ProtoPolicySnapshot>, Status> {
        Err(Status::unimplemented("not used in slate contract tests"))
    }

    async fn rollback_policy(
        &self,
        _request: Request<proto::RollbackPolicyRequest>,
    ) -> Result<Response<ProtoPolicySnapshot>, Status> {
        Err(Status::unimplemented("not used in slate contract tests"))
    }

    async fn select_slate(
        &self,
        _request: Request<SelectSlateRequest>,
    ) -> Result<Response<SlateSelection>, Status> {
        Err(Status::unimplemented("not used in slate contract tests"))
    }
}

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

type SlateClient =
    proto::bandit_policy_service_client::BanditPolicyServiceClient<tonic::transport::Channel>;

async fn start_slate_server() -> (SlateClient, SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let service = SlateTestService::new();
    let handle = tokio::spawn(async move {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        tonic::transport::Server::builder()
            .add_service(BanditPolicyServiceServer::new(service))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let channel = tonic::transport::Channel::from_shared(format!("http://[::1]:{}", addr.port()))
        .unwrap()
        .connect()
        .await
        .unwrap();

    let client =
        proto::bandit_policy_service_client::BanditPolicyServiceClient::new(channel);

    (client, addr, handle)
}

// ---------------------------------------------------------------------------
// Contract Tests
// ---------------------------------------------------------------------------

/// 1. Basic slate: arm_ids.len == num_slots, all arms are unique, all from candidate pool.
#[tokio::test]
async fn contract_slate_arm_ids_length_and_uniqueness() {
    let (mut client, _, handle) = start_slate_server().await;

    let resp = client
        .get_slate_assignment(proto::GetSlateAssignmentRequest {
            experiment_id: "slate-content-3slot".into(),
            user_id: "user-slate-1".into(),
            context_features: HashMap::new(),
            num_slots: 3,
        })
        .await
        .expect("GetSlateAssignment should succeed")
        .into_inner();

    assert_eq!(resp.arm_ids.len(), 3, "expected exactly 3 arm_ids");

    // All arm IDs must be unique within the slate.
    let unique: HashSet<_> = resp.arm_ids.iter().collect();
    assert_eq!(
        unique.len(),
        resp.arm_ids.len(),
        "slate must not repeat arm IDs: {:?}",
        resp.arm_ids
    );

    // All arm IDs must be from the known candidate pool.
    let valid_candidates = [
        "drama_series",
        "comedy_film",
        "documentary",
        "action_blockbuster",
        "kids_animation",
        "thriller",
    ];
    for arm_id in &resp.arm_ids {
        assert!(
            valid_candidates.contains(&arm_id.as_str()),
            "unknown arm_id '{arm_id}' not in candidate pool"
        );
    }

    handle.abort();
}

/// 2. slot_probabilities: len == num_slots, all positive and finite.
#[tokio::test]
async fn contract_slate_slot_probabilities_valid() {
    let (mut client, _, handle) = start_slate_server().await;

    let resp = client
        .get_slate_assignment(proto::GetSlateAssignmentRequest {
            experiment_id: "slate-content-3slot".into(),
            user_id: "user-slate-prob".into(),
            context_features: HashMap::new(),
            num_slots: 3,
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(
        resp.slot_probabilities.len(),
        resp.arm_ids.len(),
        "slot_probabilities length must equal arm_ids length"
    );

    for (i, &prob) in resp.slot_probabilities.iter().enumerate() {
        assert!(
            prob > 0.0,
            "slot_probabilities[{i}] must be positive, got {prob}"
        );
        assert!(
            prob.is_finite(),
            "slot_probabilities[{i}] must be finite, got {prob}"
        );
        assert!(
            prob <= 1.0,
            "slot_probabilities[{i}] must be <= 1.0, got {prob}"
        );
    }

    handle.abort();
}

/// 3. joint_probability is positive, finite, and approximately the product of slot probs.
#[tokio::test]
async fn contract_slate_joint_probability_valid() {
    let (mut client, _, handle) = start_slate_server().await;

    let resp = client
        .get_slate_assignment(proto::GetSlateAssignmentRequest {
            experiment_id: "slate-content-3slot".into(),
            user_id: "user-joint-prob".into(),
            context_features: HashMap::new(),
            num_slots: 3,
        })
        .await
        .unwrap()
        .into_inner();

    assert!(
        resp.joint_probability > 0.0,
        "joint_probability must be positive"
    );
    assert!(
        resp.joint_probability.is_finite(),
        "joint_probability must be finite"
    );
    assert!(
        resp.joint_probability <= 1.0,
        "joint_probability must be <= 1.0"
    );

    // Joint probability must equal product of per-slot marginals (factorized model).
    let expected_joint: f64 = resp.slot_probabilities.iter().product();
    assert!(
        (resp.joint_probability - expected_joint).abs() < 1e-9,
        "joint_probability {:.6} != product of slot_probs {:.6}",
        resp.joint_probability,
        expected_joint
    );

    handle.abort();
}

/// 4. Deterministic: same user + experiment produces the same slate.
#[tokio::test]
async fn contract_slate_deterministic_for_same_user() {
    let (mut client, _, handle) = start_slate_server().await;

    let req = || proto::GetSlateAssignmentRequest {
        experiment_id: "slate-content-3slot".into(),
        user_id: "stable-slate-user-42".into(),
        context_features: HashMap::new(),
        num_slots: 3,
    };

    let r1 = client.get_slate_assignment(req()).await.unwrap().into_inner();
    let r2 = client.get_slate_assignment(req()).await.unwrap().into_inner();

    assert_eq!(
        r1.arm_ids, r2.arm_ids,
        "same user must get the same slate ordering"
    );
    for (p1, p2) in r1.slot_probabilities.iter().zip(&r2.slot_probabilities) {
        assert!(
            (p1 - p2).abs() < f64::EPSILON,
            "same user must get the same slot probabilities"
        );
    }

    handle.abort();
}

/// 5. Different users produce different orderings (distribution check).
#[tokio::test]
async fn contract_slate_distribution_across_users() {
    let (mut client, _, handle) = start_slate_server().await;

    // Collect top-position arm choices across many users.
    let mut top_arm_counts: HashMap<String, u32> = HashMap::new();
    for i in 0..200u32 {
        let resp = client
            .get_slate_assignment(proto::GetSlateAssignmentRequest {
                experiment_id: "slate-content-3slot".into(),
                user_id: format!("dist-slate-user-{i}"),
                context_features: HashMap::new(),
                num_slots: 3,
            })
            .await
            .unwrap()
            .into_inner();

        let top_arm = resp.arm_ids.first().unwrap().clone();
        *top_arm_counts.entry(top_arm).or_default() += 1;
    }

    // With uniform Thompson priors, multiple arms should reach the top position.
    assert!(
        top_arm_counts.len() >= 3,
        "at least 3 different arms should appear at position 0 across 200 users, \
         got: {top_arm_counts:?}"
    );

    handle.abort();
}

/// 6. Unknown experiment returns NOT_FOUND.
#[tokio::test]
async fn contract_slate_unknown_experiment_not_found() {
    let (mut client, _, handle) = start_slate_server().await;

    let err = client
        .get_slate_assignment(proto::GetSlateAssignmentRequest {
            experiment_id: "nonexistent-slate-exp".into(),
            user_id: "user-1".into(),
            context_features: HashMap::new(),
            num_slots: 3,
        })
        .await
        .unwrap_err();

    assert_eq!(
        err.code(),
        tonic::Code::NotFound,
        "unknown experiment must return NOT_FOUND, got: {:?}",
        err.code()
    );

    handle.abort();
}

/// 7. num_slots=0 falls back to experiment default.
#[tokio::test]
async fn contract_slate_num_slots_zero_uses_default() {
    let (mut client, _, handle) = start_slate_server().await;

    let resp = client
        .get_slate_assignment(proto::GetSlateAssignmentRequest {
            experiment_id: "slate-hero-2slot".into(),
            user_id: "user-default-slots".into(),
            context_features: HashMap::new(),
            num_slots: 0, // should use experiment default of 2
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(
        resp.arm_ids.len(),
        2,
        "num_slots=0 should use experiment default (2), got: {:?}",
        resp.arm_ids
    );

    handle.abort();
}

/// 8. num_slots > candidate pool is capped at pool size.
#[tokio::test]
async fn contract_slate_num_slots_capped_at_pool_size() {
    let (mut client, _, handle) = start_slate_server().await;

    // "slate-small-pool" has 2 candidates but default_slots=5.
    // Requesting 10 slots should return only 2 (pool size).
    let resp = client
        .get_slate_assignment(proto::GetSlateAssignmentRequest {
            experiment_id: "slate-small-pool".into(),
            user_id: "user-overflow-slots".into(),
            context_features: HashMap::new(),
            num_slots: 10,
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(
        resp.arm_ids.len(),
        2,
        "num_slots must be capped at pool size (2), got: {:?}",
        resp.arm_ids
    );
    assert_eq!(
        resp.slot_probabilities.len(),
        resp.arm_ids.len(),
        "slot_probabilities length must match arm_ids length"
    );

    handle.abort();
}

/// 9. Context features are accepted without error (contextual variant path).
#[tokio::test]
async fn contract_slate_with_context_features_accepted() {
    let (mut client, _, handle) = start_slate_server().await;

    let context: HashMap<String, f64> = [
        ("user_age_bucket".into(), 3.0),
        ("watch_history_len".into(), 150.0),
        ("subscription_tier".into(), 2.0),
    ]
    .into_iter()
    .collect();

    let resp = client
        .get_slate_assignment(proto::GetSlateAssignmentRequest {
            experiment_id: "slate-content-3slot".into(),
            user_id: "contextual-user-1".into(),
            context_features: context,
            num_slots: 3,
        })
        .await
        .expect("GetSlateAssignment with context should succeed")
        .into_inner();

    assert_eq!(resp.arm_ids.len(), 3);
    assert_eq!(resp.slot_probabilities.len(), 3);
    assert!(resp.joint_probability > 0.0);

    handle.abort();
}

/// 10. Concurrent slate requests return valid results under load.
#[tokio::test]
async fn contract_slate_concurrent_requests() {
    let (client, _, handle) = start_slate_server().await;

    let per_task_timeout = if std::env::var("CI").is_ok() {
        std::time::Duration::from_secs(30)
    } else {
        std::time::Duration::from_secs(10)
    };

    let mut handles = Vec::new();
    for i in 0..30u32 {
        let mut c = client.clone();
        handles.push(tokio::spawn(async move {
            tokio::time::timeout(per_task_timeout, async move {
                c.get_slate_assignment(proto::GetSlateAssignmentRequest {
                    experiment_id: "slate-content-3slot".into(),
                    user_id: format!("concurrent-slate-{i}"),
                    context_features: HashMap::new(),
                    num_slots: 3,
                })
                .await
            })
            .await
            .expect("concurrent GetSlateAssignment timed out")
        }));
    }

    let valid_candidates: HashSet<&str> = [
        "drama_series",
        "comedy_film",
        "documentary",
        "action_blockbuster",
        "kids_animation",
        "thriller",
    ]
    .into_iter()
    .collect();

    for h in handles {
        let resp = h
            .await
            .unwrap()
            .expect("concurrent GetSlateAssignment should succeed")
            .into_inner();

        assert_eq!(resp.arm_ids.len(), 3);
        assert_eq!(resp.slot_probabilities.len(), 3);
        assert!(resp.joint_probability > 0.0 && resp.joint_probability.is_finite());

        let unique: HashSet<_> = resp.arm_ids.iter().collect();
        assert_eq!(unique.len(), 3, "concurrent slate must have unique arm IDs");

        for arm_id in &resp.arm_ids {
            assert!(
                valid_candidates.contains(arm_id.as_str()),
                "concurrent: unknown arm_id '{arm_id}'"
            );
        }
    }

    handle.abort();
}

/// 11. All returned slot_probabilities are finite across many users (fail-fast contract).
#[tokio::test]
async fn contract_slate_all_probabilities_finite() {
    let (mut client, _, handle) = start_slate_server().await;

    for i in 0..20u32 {
        let resp = client
            .get_slate_assignment(proto::GetSlateAssignmentRequest {
                experiment_id: "slate-content-3slot".into(),
                user_id: format!("finite-check-slate-{i}"),
                context_features: HashMap::new(),
                num_slots: 3,
            })
            .await
            .unwrap()
            .into_inner();

        for (j, &prob) in resp.slot_probabilities.iter().enumerate() {
            assert!(
                prob.is_finite(),
                "user {i}, slot {j}: probability is not finite: {prob}"
            );
        }
        assert!(
            resp.joint_probability.is_finite(),
            "user {i}: joint_probability is not finite"
        );
    }

    handle.abort();
}
