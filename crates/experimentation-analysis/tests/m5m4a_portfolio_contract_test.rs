//! M5 ↔ M4a GetPortfolioAllocation Contract Tests (ADR-019)
//!
//! These tests verify the wire-format contract between M5's portfolio optimization
//! requests and M4a's `GetPortfolioAllocation` RPC. M5 submits a set of running
//! experiment IDs and a total traffic budget; M4a returns optimal per-experiment
//! traffic allocations and conclusion signals.
//!
//! Contract points verified:
//! 1. Response has exactly one ExperimentAllocation per requested experiment_id
//! 2. Each allocation's experiment_id matches one from the request
//! 3. All allocated_fraction values are in [0.0, 1.0] and finite
//! 4. sum(allocated_fraction) <= total_traffic_budget
//! 5. estimated_power is in [0.0, 1.0] and finite for each allocation
//! 6. computed_at timestamp is populated
//! 7. total_allocated matches sum of individual allocated_fractions
//! 8. Empty experiment_ids returns empty allocations (not an error)
//! 9. Invalid total_traffic_budget (> 1.0 or <= 0) returns INVALID_ARGUMENT
//! 10. Allocations respect budget even under high-traffic experiments
//! 11. can_conclude flag is set when sample size exceeds stopping rule

use experimentation_proto::experimentation::analysis::v1::{
    analysis_service_server::{AnalysisService, AnalysisServiceServer},
    AnalysisResult, ExperimentAllocation, GetAnalysisResultRequest,
    GetInterferenceAnalysisRequest, GetInterleavingAnalysisRequest, GetNoveltyAnalysisRequest,
    GetPortfolioAllocationRequest, GetPortfolioAllocationResponse, GetSwitchbackAnalysisRequest,
    GetSyntheticControlAnalysisRequest, InterleavingAnalysisResult, InterferenceAnalysisResult,
    GetPortfolioPowerAnalysisRequest, NoveltyAnalysisResult, PortfolioPowerAnalysisResult,
    RunAnalysisRequest, SwitchbackAnalysisResult, SyntheticControlAnalysisResult,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use tonic::{Request, Response, Status};

// ---------------------------------------------------------------------------
// Test M4a implementation for portfolio allocation
// ---------------------------------------------------------------------------

/// Per-experiment metadata tracked by the test service.
struct ExpInfo {
    /// Sample size accumulated so far (simulates M4a reading from Delta Lake).
    sample_size: u64,
    /// Target sample size for full power (from power calculation at design time).
    target_sample_size: u64,
    /// Observed effect size (absolute, in effect units).
    observed_effect: f64,
    /// Minimum detectable effect (from experiment design).
    mde: f64,
}

/// Test implementation of M4a's AnalysisService for portfolio allocation.
struct PortfolioTestService {
    /// Registered experiment metadata (simulates Delta Lake reads).
    experiments: HashMap<String, ExpInfo>,
}

impl PortfolioTestService {
    fn new() -> Self {
        let mut exps = HashMap::new();

        // Mature experiment: has 90% of target sample, borderline concludable.
        exps.insert(
            "exp-mature".to_string(),
            ExpInfo {
                sample_size: 9000,
                target_sample_size: 10_000,
                observed_effect: 0.05,
                mde: 0.03,
            },
        );

        // Ready-to-conclude: has 120% of target, effect > MDE, high power.
        exps.insert(
            "exp-ready-conclude".to_string(),
            ExpInfo {
                sample_size: 12_000,
                target_sample_size: 10_000,
                observed_effect: 0.08,
                mde: 0.03,
            },
        );

        // Early-stage: only 20% of target sample size, needs more traffic.
        exps.insert(
            "exp-early".to_string(),
            ExpInfo {
                sample_size: 2_000,
                target_sample_size: 10_000,
                observed_effect: 0.01,
                mde: 0.03,
            },
        );

        // High-traffic incumbent: large ongoing experiment.
        exps.insert(
            "exp-incumbent".to_string(),
            ExpInfo {
                sample_size: 50_000,
                target_sample_size: 50_000,
                observed_effect: 0.02,
                mde: 0.01,
            },
        );

        Self { experiments: exps }
    }

    /// Portfolio optimizer: maximizes information gain within the traffic budget.
    ///
    /// Simple heuristic that mirrors the ADR-019 objective: prioritize experiments
    /// with the lowest completion ratio (most room to gain information).
    fn compute_portfolio_allocation(
        &self,
        experiment_ids: &[String],
        total_budget: f64,
    ) -> Result<GetPortfolioAllocationResponse, Status> {
        if experiment_ids.is_empty() {
            return Ok(GetPortfolioAllocationResponse {
                allocations: vec![],
                total_allocated: 0.0,
                computed_at: Some(now_timestamp()),
            });
        }

        // Validate all experiments exist.
        for exp_id in experiment_ids {
            if !self.experiments.contains_key(exp_id.as_str()) {
                return Err(Status::not_found(format!(
                    "experiment '{exp_id}' not found in M4a analysis store"
                )));
            }
        }

        // Compute completion ratio and target allocation for each experiment.
        // Lower completion → higher priority for traffic allocation.
        let mut weights: Vec<(usize, f64)> = experiment_ids
            .iter()
            .enumerate()
            .map(|(i, exp_id)| {
                let info = &self.experiments[exp_id.as_str()];
                let completion = (info.sample_size as f64 / info.target_sample_size as f64).min(1.0);
                // Experiments closer to target get less additional traffic.
                let weight = (1.0 - completion).max(0.05); // min 5% weight to avoid starvation
                (i, weight)
            })
            .collect();

        // Normalize weights to sum to 1.0, then scale by total_budget.
        let total_weight: f64 = weights.iter().map(|(_, w)| w).sum();
        for (_, w) in &mut weights {
            *w = (*w / total_weight) * total_budget;
        }

        let mut allocations = vec![
            ExperimentAllocation {
                experiment_id: String::new(),
                allocated_fraction: 0.0,
                can_conclude: false,
                estimated_power: 0.0,
            };
            experiment_ids.len()
        ];

        let mut total_allocated = 0.0;
        for (i, fraction) in weights {
            let exp_id = &experiment_ids[i];
            let info = &self.experiments[exp_id.as_str()];

            // Statistical power estimate: Cohen's power approximation.
            // Power ≈ Φ(|effect| / mde × √(sample/target) − z_alpha/2)
            // Simplified: clamp completion × (effect/mde) to [0, 1].
            let completion = (info.sample_size as f64 / info.target_sample_size as f64).min(1.0);
            let effect_ratio = if info.mde > 0.0 {
                (info.observed_effect.abs() / info.mde).min(2.0)
            } else {
                0.0
            };
            let estimated_power = (completion * effect_ratio * 0.9).clamp(0.0, 1.0);

            // Experiment is concludable if: sample >= target AND effect > MDE.
            let can_conclude =
                info.sample_size >= info.target_sample_size && info.observed_effect.abs() > info.mde;

            allocations[i] = ExperimentAllocation {
                experiment_id: exp_id.clone(),
                allocated_fraction: fraction,
                can_conclude,
                estimated_power,
            };
            total_allocated += fraction;
        }

        Ok(GetPortfolioAllocationResponse {
            allocations,
            total_allocated,
            computed_at: Some(now_timestamp()),
        })
    }
}

fn now_timestamp() -> prost_types::Timestamp {
    let now = chrono::Utc::now();
    prost_types::Timestamp {
        seconds: now.timestamp(),
        nanos: now.timestamp_subsec_nanos() as i32,
    }
}

#[tonic::async_trait]
impl AnalysisService for PortfolioTestService {
    async fn run_analysis(
        &self,
        _r: Request<RunAnalysisRequest>,
    ) -> Result<Response<AnalysisResult>, Status> {
        Err(Status::unimplemented("not used in portfolio contract tests"))
    }

    async fn get_analysis_result(
        &self,
        _r: Request<GetAnalysisResultRequest>,
    ) -> Result<Response<AnalysisResult>, Status> {
        Err(Status::unimplemented("not used in portfolio contract tests"))
    }

    async fn get_interleaving_analysis(
        &self,
        _r: Request<GetInterleavingAnalysisRequest>,
    ) -> Result<Response<InterleavingAnalysisResult>, Status> {
        Err(Status::unimplemented("not used in portfolio contract tests"))
    }

    async fn get_novelty_analysis(
        &self,
        _r: Request<GetNoveltyAnalysisRequest>,
    ) -> Result<Response<NoveltyAnalysisResult>, Status> {
        Err(Status::unimplemented("not used in portfolio contract tests"))
    }

    async fn get_interference_analysis(
        &self,
        _r: Request<GetInterferenceAnalysisRequest>,
    ) -> Result<Response<InterferenceAnalysisResult>, Status> {
        Err(Status::unimplemented("not used in portfolio contract tests"))
    }

    async fn get_synthetic_control_analysis(
        &self,
        _r: Request<GetSyntheticControlAnalysisRequest>,
    ) -> Result<Response<SyntheticControlAnalysisResult>, Status> {
        Err(Status::unimplemented("not used in portfolio contract tests"))
    }

    async fn get_switchback_analysis(
        &self,
        _r: Request<GetSwitchbackAnalysisRequest>,
    ) -> Result<Response<SwitchbackAnalysisResult>, Status> {
        Err(Status::unimplemented("not used in portfolio contract tests"))
    }

    async fn get_portfolio_allocation(
        &self,
        request: Request<GetPortfolioAllocationRequest>,
    ) -> Result<Response<GetPortfolioAllocationResponse>, Status> {
        let req = request.into_inner();

        // Validate budget: must be in (0.0, 1.0].
        if req.total_traffic_budget <= 0.0 || req.total_traffic_budget > 1.0 {
            return Err(Status::invalid_argument(format!(
                "total_traffic_budget must be in (0.0, 1.0], got {}",
                req.total_traffic_budget
            )));
        }

        let resp = self
            .compute_portfolio_allocation(&req.experiment_ids, req.total_traffic_budget)?;

        Ok(Response::new(resp))
    }

    async fn get_portfolio_power_analysis(
        &self,
        _r: Request<GetPortfolioPowerAnalysisRequest>,
    ) -> Result<Response<PortfolioPowerAnalysisResult>, Status> {
        Err(Status::unimplemented("not used in portfolio contract tests"))
    }
}

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

type PortfolioClient =
    experimentation_proto::experimentation::analysis::v1::analysis_service_client::AnalysisServiceClient<
        tonic::transport::Channel,
    >;

async fn start_portfolio_server() -> (PortfolioClient, SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let service = PortfolioTestService::new();
    let handle = tokio::spawn(async move {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        tonic::transport::Server::builder()
            .add_service(AnalysisServiceServer::new(service))
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

    let client = experimentation_proto::experimentation::analysis::v1::analysis_service_client::AnalysisServiceClient::new(channel);

    (client, addr, handle)
}

// ---------------------------------------------------------------------------
// Contract Tests
// ---------------------------------------------------------------------------

/// 1. Response has exactly one ExperimentAllocation per requested experiment_id.
#[tokio::test]
async fn contract_portfolio_one_allocation_per_experiment() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let exp_ids = vec![
        "exp-mature".to_string(),
        "exp-early".to_string(),
        "exp-ready-conclude".to_string(),
    ];

    let resp = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: exp_ids.clone(),
            total_traffic_budget: 0.6,
        })
        .await
        .expect("GetPortfolioAllocation should succeed")
        .into_inner();

    assert_eq!(
        resp.allocations.len(),
        exp_ids.len(),
        "should have one allocation per requested experiment"
    );

    // Each allocation's experiment_id must be in the request set.
    let request_ids: std::collections::HashSet<_> = exp_ids.iter().collect();
    for alloc in &resp.allocations {
        assert!(
            request_ids.contains(&alloc.experiment_id),
            "allocation for unknown experiment_id '{}'",
            alloc.experiment_id
        );
    }

    handle.abort();
}

/// 2. All allocated_fraction values are in [0.0, 1.0] and finite.
#[tokio::test]
async fn contract_portfolio_allocated_fractions_valid() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let resp = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: vec![
                "exp-mature".to_string(),
                "exp-early".to_string(),
                "exp-ready-conclude".to_string(),
                "exp-incumbent".to_string(),
            ],
            total_traffic_budget: 0.8,
        })
        .await
        .unwrap()
        .into_inner();

    for alloc in &resp.allocations {
        assert!(
            alloc.allocated_fraction >= 0.0,
            "allocated_fraction for '{}' must be >= 0, got {}",
            alloc.experiment_id,
            alloc.allocated_fraction
        );
        assert!(
            alloc.allocated_fraction <= 1.0,
            "allocated_fraction for '{}' must be <= 1.0, got {}",
            alloc.experiment_id,
            alloc.allocated_fraction
        );
        assert!(
            alloc.allocated_fraction.is_finite(),
            "allocated_fraction for '{}' must be finite",
            alloc.experiment_id
        );
    }

    handle.abort();
}

/// 3. sum(allocated_fraction) <= total_traffic_budget.
#[tokio::test]
async fn contract_portfolio_total_within_budget() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let budget = 0.5;
    let resp = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: vec![
                "exp-mature".to_string(),
                "exp-early".to_string(),
                "exp-ready-conclude".to_string(),
            ],
            total_traffic_budget: budget,
        })
        .await
        .unwrap()
        .into_inner();

    let sum: f64 = resp.allocations.iter().map(|a| a.allocated_fraction).sum();
    assert!(
        sum <= budget + 1e-9,
        "sum of allocated_fractions {:.4} exceeds total_traffic_budget {:.4}",
        sum,
        budget
    );

    handle.abort();
}

/// 4. total_allocated field matches sum of individual allocated_fractions.
#[tokio::test]
async fn contract_portfolio_total_allocated_matches_sum() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let resp = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: vec!["exp-mature".to_string(), "exp-early".to_string()],
            total_traffic_budget: 0.4,
        })
        .await
        .unwrap()
        .into_inner();

    let expected_total: f64 = resp.allocations.iter().map(|a| a.allocated_fraction).sum();
    assert!(
        (resp.total_allocated - expected_total).abs() < 1e-9,
        "total_allocated {:.6} != sum of fractions {:.6}",
        resp.total_allocated,
        expected_total
    );

    handle.abort();
}

/// 5. estimated_power is in [0.0, 1.0] and finite for each allocation.
#[tokio::test]
async fn contract_portfolio_estimated_power_valid() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let resp = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: vec![
                "exp-mature".to_string(),
                "exp-ready-conclude".to_string(),
                "exp-early".to_string(),
            ],
            total_traffic_budget: 0.6,
        })
        .await
        .unwrap()
        .into_inner();

    for alloc in &resp.allocations {
        assert!(
            alloc.estimated_power >= 0.0 && alloc.estimated_power <= 1.0,
            "estimated_power for '{}' must be in [0,1], got {}",
            alloc.experiment_id,
            alloc.estimated_power
        );
        assert!(
            alloc.estimated_power.is_finite(),
            "estimated_power for '{}' must be finite",
            alloc.experiment_id
        );
    }

    handle.abort();
}

/// 6. computed_at timestamp is populated.
#[tokio::test]
async fn contract_portfolio_computed_at_present() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let resp = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: vec!["exp-mature".to_string()],
            total_traffic_budget: 0.3,
        })
        .await
        .unwrap()
        .into_inner();

    assert!(
        resp.computed_at.is_some(),
        "computed_at must be present in GetPortfolioAllocationResponse"
    );

    let ts = resp.computed_at.unwrap();
    // timestamp must be a recent epoch (after year 2020).
    assert!(
        ts.seconds > 1_580_000_000,
        "computed_at.seconds looks wrong: {}",
        ts.seconds
    );

    handle.abort();
}

/// 7. Empty experiment_ids returns empty allocations without error.
#[tokio::test]
async fn contract_portfolio_empty_request_returns_empty() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let resp = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: vec![],
            total_traffic_budget: 0.5,
        })
        .await
        .expect("empty experiment_ids should not error")
        .into_inner();

    assert!(
        resp.allocations.is_empty(),
        "empty request should produce empty allocations"
    );
    assert!(
        (resp.total_allocated - 0.0).abs() < 1e-9,
        "total_allocated should be 0.0 for empty request"
    );

    handle.abort();
}

/// 8. total_traffic_budget > 1.0 returns INVALID_ARGUMENT.
#[tokio::test]
async fn contract_portfolio_budget_above_one_rejected() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let err = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: vec!["exp-mature".to_string()],
            total_traffic_budget: 1.5,
        })
        .await
        .unwrap_err();

    assert_eq!(
        err.code(),
        tonic::Code::InvalidArgument,
        "budget > 1.0 must return INVALID_ARGUMENT, got: {:?}",
        err.code()
    );

    handle.abort();
}

/// 9. total_traffic_budget <= 0 returns INVALID_ARGUMENT.
#[tokio::test]
async fn contract_portfolio_budget_zero_rejected() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let err = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: vec!["exp-mature".to_string()],
            total_traffic_budget: 0.0,
        })
        .await
        .unwrap_err();

    assert_eq!(
        err.code(),
        tonic::Code::InvalidArgument,
        "budget = 0 must return INVALID_ARGUMENT, got: {:?}",
        err.code()
    );

    handle.abort();
}

/// 10. Unknown experiment returns NOT_FOUND.
#[tokio::test]
async fn contract_portfolio_unknown_experiment_not_found() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let err = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: vec!["exp-mature".to_string(), "nonexistent-exp".to_string()],
            total_traffic_budget: 0.5,
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

/// 11. can_conclude is true for the experiment that has met all stopping criteria.
#[tokio::test]
async fn contract_portfolio_can_conclude_signal() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let resp = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: vec![
                "exp-ready-conclude".to_string(), // should be concludable
                "exp-early".to_string(),          // should NOT be concludable
                "exp-mature".to_string(),         // borderline — not concludable (9000 < 10000)
            ],
            total_traffic_budget: 0.6,
        })
        .await
        .unwrap()
        .into_inner();

    let allocs: HashMap<&str, &ExperimentAllocation> = resp
        .allocations
        .iter()
        .map(|a| (a.experiment_id.as_str(), a))
        .collect();

    assert!(
        allocs["exp-ready-conclude"].can_conclude,
        "exp-ready-conclude should have can_conclude=true (sample=12000>target=10000, effect=0.08>mde=0.03)",
    );
    assert!(
        !allocs["exp-early"].can_conclude,
        "exp-early should have can_conclude=false (sample=2000 << target=10000)"
    );
    assert!(
        !allocs["exp-mature"].can_conclude,
        "exp-mature should have can_conclude=false (sample=9000 < target=10000)"
    );

    handle.abort();
}

/// 12. Single experiment request works correctly.
#[tokio::test]
async fn contract_portfolio_single_experiment() {
    let (mut client, _, handle) = start_portfolio_server().await;

    let resp = client
        .get_portfolio_allocation(GetPortfolioAllocationRequest {
            experiment_ids: vec!["exp-incumbent".to_string()],
            total_traffic_budget: 0.3,
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(resp.allocations.len(), 1);
    assert_eq!(resp.allocations[0].experiment_id, "exp-incumbent");
    // Single experiment gets the full budget allocated.
    assert!(
        (resp.allocations[0].allocated_fraction - 0.3).abs() < 1e-9,
        "single experiment should receive full budget, got: {}",
        resp.allocations[0].allocated_fraction
    );

    handle.abort();
}
