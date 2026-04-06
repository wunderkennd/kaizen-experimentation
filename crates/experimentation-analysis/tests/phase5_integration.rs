//! Phase 5 Cross-Cutting Integration Test Suite (Sprint 5.5, Task 5.5.5)
//!
//! Four end-to-end integration tests covering the major Phase 5 capabilities
//! across multiple modules.  Each test exercises a complete capability path
//! from data generation through statistical analysis or bandit policy execution.
//!
//! ## Tests
//!
//! 1. **Multi-objective bandit + AVLM analysis** (ADR-011 + ADR-015):
//!    Two-objective reward composition (engagement + retention) feeding into
//!    a Thompson Sampling policy, followed by AVLM sequential analysis of the
//!    resulting per-variant outcomes with pre-experiment covariates.
//!
//! 2. **Switchback experiment lifecycle** (ADR-022):
//!    Full switchback pipeline: block-level data generation with washout
//!    periods → `SwitchbackAnalyzer` → HAC SE, randomization inference,
//!    and carryover diagnostic → `RunAnalysis` RPC for the same data.
//!
//! 3. **Synthetic control method** (ADR-023):
//!    Panel data with 1 treated unit + 5 donors → Classic SCM → Augmented SCM
//!    → verify ATT, donor weights, placebo p-value, and CI properties.
//!
//! 4. **Meta-experiment with isolated bandit policies** (ADR-013 + ADR-016):
//!    Two meta-experiment variants with different reward objectives, each running
//!    an isolated slate bandit policy with distinct attribution models.
//!    Verifies policy isolation, posterior divergence, and LIPS OPE validity.
//!
//! ## Cross-cutting ADRs
//! ADR-011 (multi-objective), ADR-013 (meta-experiment), ADR-015 (AVLM),
//! ADR-016 (slate bandit), ADR-022 (switchback), ADR-023 (synthetic control).

use experimentation_analysis::config::AnalysisConfig;
use experimentation_analysis::grpc::AnalysisServiceHandler;

use deltalake::arrow::array::{Float64Array, StringArray};
use deltalake::arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
use deltalake::arrow::record_batch::RecordBatch;
use deltalake::DeltaOps;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;

use experimentation_proto::experimentation::analysis::v1::analysis_service_server::AnalysisService;
use experimentation_proto::experimentation::analysis::v1::RunAnalysisRequest;
use tonic::Request;

// ---------------------------------------------------------------------------
// Test infrastructure (shared with phase5_e2e.rs patterns)
// ---------------------------------------------------------------------------

fn test_config(path: &str) -> AnalysisConfig {
    AnalysisConfig {
        grpc_addr: "[::1]:0".into(),
        delta_lake_path: path.into(),
        default_alpha: 0.05,
        default_js_threshold: 0.05,
        database_url: None,
        default_tau_sq: 0.5,
    }
}

fn test_handler(path: &str) -> AnalysisServiceHandler {
    AnalysisServiceHandler::new(test_config(path), None)
}

fn metric_summaries_schema() -> Arc<ArrowSchema> {
    Arc::new(ArrowSchema::new(vec![
        Field::new("experiment_id", DataType::Utf8, false),
        Field::new("user_id", DataType::Utf8, false),
        Field::new("variant_id", DataType::Utf8, false),
        Field::new("metric_id", DataType::Utf8, false),
        Field::new("metric_value", DataType::Float64, false),
        Field::new("cuped_covariate", DataType::Float64, true),
    ]))
}

fn make_metric_batch(
    exp_ids: &[&str],
    user_ids: &[&str],
    variant_ids: &[&str],
    metric_ids: &[&str],
    values: &[f64],
    covariates: &[Option<f64>],
) -> RecordBatch {
    let cov_arr: Float64Array = covariates.iter().copied().collect();
    RecordBatch::try_new(
        metric_summaries_schema(),
        vec![
            Arc::new(StringArray::from(exp_ids.to_vec())),
            Arc::new(StringArray::from(user_ids.to_vec())),
            Arc::new(StringArray::from(variant_ids.to_vec())),
            Arc::new(StringArray::from(metric_ids.to_vec())),
            Arc::new(Float64Array::from(values.to_vec())),
            Arc::new(cov_arr),
        ],
    )
    .unwrap()
}

async fn write_metric_table(dir: &std::path::Path, batch: RecordBatch) {
    let table_path = dir.join("metric_summaries");
    std::fs::create_dir_all(&table_path).unwrap();
    let ops = DeltaOps::try_from_uri(table_path.to_str().unwrap())
        .await
        .unwrap();
    ops.write(vec![batch]).await.unwrap();
}

/// Deterministic pseudo-Gaussian via Box–Muller from a LCG.
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_uniform(&mut self) -> f64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 32) as f64 / u32::MAX as f64
    }

    fn next_gaussian(&mut self) -> f64 {
        let u1 = self.next_uniform().max(1e-12);
        let u2 = self.next_uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

// ===========================================================================
// Test 1: Multi-objective bandit + AVLM analysis
// ===========================================================================

/// **Cross-cutting: Multi-objective bandit reward composition → AVLM sequential analysis.**
///
/// This test exercises the ADR-011 + ADR-015 pipeline:
///
/// 1. **ADR-011**: A `RewardComposer` with two objectives (engagement_rate at 60%,
///    retention_rate at 40%) using `WeightedScalarization` composes scalar rewards
///    from per-metric observations.
///
/// 2. **Thompson Sampling**: Two arms receive rewards from the composer. Arm "treatment"
///    gets higher engagement (0.8) and retention (0.6); arm "control" gets lower values.
///    After 200 rounds, Thompson Sampling must strongly prefer "treatment".
///
/// 3. **ADR-015**: AVLM sequential analysis runs on the resulting per-user metric data
///    with pre-experiment covariates. The regression-adjusted CI should be narrower
///    than the raw CI (variance reduction > 0) and should detect the treatment effect.
///
/// 4. **RunAnalysis RPC**: The same data is analyzed via the gRPC `RunAnalysis` path,
///    verifying that the metric pipeline processes multi-objective experiment data.
#[tokio::test]
async fn test_multi_objective_bandit_plus_avlm_analysis() {
    use experimentation_bandit::reward_composer::{CompositionMethod, Objective, RewardComposer};
    use experimentation_bandit::thompson::{select_arm, BetaArm};
    use experimentation_stats::avlm::AvlmSequentialTest;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    // ── Phase 1: Multi-objective reward composition (ADR-011) ───────────
    let objectives = vec![
        Objective {
            metric_id: "engagement_rate".into(),
            weight: 0.6,
            floor: 0.0,
            is_primary: false,
        },
        Objective {
            metric_id: "retention_rate".into(),
            weight: 0.4,
            floor: 0.0,
            is_primary: false,
        },
    ];
    let mut composer =
        RewardComposer::new(objectives, CompositionMethod::WeightedScalarization);

    // Compose rewards for treatment arm (higher engagement + retention).
    let mut rng_data = DeterministicRng::new(0xCAFE_BABE_1234);
    let mut treatment_rewards = Vec::new();
    let mut control_rewards = Vec::new();

    for _ in 0..200 {
        // Treatment: engagement ~ 0.8, retention ~ 0.6
        let trt_metrics: HashMap<String, f64> = [
            ("engagement_rate".into(), 0.8 + 0.05 * rng_data.next_gaussian()),
            ("retention_rate".into(), 0.6 + 0.05 * rng_data.next_gaussian()),
        ]
        .into();
        let trt_reward = composer.compose(&trt_metrics);
        assert!(trt_reward.is_finite(), "composed treatment reward must be finite");
        treatment_rewards.push(trt_reward);

        // Control: engagement ~ 0.4, retention ~ 0.3
        let ctrl_metrics: HashMap<String, f64> = [
            ("engagement_rate".into(), 0.4 + 0.05 * rng_data.next_gaussian()),
            ("retention_rate".into(), 0.3 + 0.05 * rng_data.next_gaussian()),
        ]
        .into();
        let ctrl_reward = composer.compose(&ctrl_metrics);
        assert!(ctrl_reward.is_finite(), "composed control reward must be finite");
        control_rewards.push(ctrl_reward);
    }

    // Verify that treatment rewards are consistently higher.
    let trt_mean: f64 = treatment_rewards.iter().sum::<f64>() / treatment_rewards.len() as f64;
    let ctrl_mean: f64 = control_rewards.iter().sum::<f64>() / control_rewards.len() as f64;
    assert!(
        trt_mean > ctrl_mean,
        "treatment composed reward mean ({trt_mean:.4}) must exceed control ({ctrl_mean:.4})"
    );

    // ── Phase 2: Thompson Sampling with composed rewards ────────────────
    let mut arms = vec![
        BetaArm::new("treatment".into()),
        BetaArm::new("control".into()),
    ];

    // Feed composed rewards as Beta updates (clamped to [0, 1]).
    for (trt_r, ctrl_r) in treatment_rewards.iter().zip(control_rewards.iter()) {
        arms[0].update(trt_r.clamp(0.0, 1.0));
        arms[1].update(ctrl_r.clamp(0.0, 1.0));
    }

    let mut rng = SmallRng::seed_from_u64(0x1234_5678_ABCD);
    let selection = select_arm(&arms, &mut rng);
    let trt_prob = selection
        .all_arm_probabilities
        .get("treatment")
        .copied()
        .unwrap_or(0.0);
    assert!(
        trt_prob > 0.80,
        "after 200 rounds of higher composed rewards, treatment must dominate (prob={trt_prob:.4})"
    );

    // ── Phase 3: AVLM sequential analysis (ADR-015) ─────────────────────
    let mut avlm = AvlmSequentialTest::new(0.5, 0.05).unwrap();

    // Simulate per-user outcomes with pre-experiment covariates.
    // Covariate: pre-experiment engagement (correlated with outcome).
    let mut rng_avlm = DeterministicRng::new(0xA71A_5EED_0001);
    let n_per_arm = 100;

    for _ in 0..n_per_arm {
        // Control user: pre-engagement x ~ N(5.0, 1.0), outcome y = 0.5x + noise
        let x = 5.0 + rng_avlm.next_gaussian();
        let y = 0.5 * x + 0.3 * rng_avlm.next_gaussian();
        avlm.update(y, x, false).unwrap();
    }
    for _ in 0..n_per_arm {
        // Treatment user: pre-engagement x ~ N(5.0, 1.0), outcome y = 0.5x + 0.5 (effect) + noise
        let x = 5.0 + rng_avlm.next_gaussian();
        let y = 0.5 * x + 0.5 + 0.3 * rng_avlm.next_gaussian();
        avlm.update(y, x, true).unwrap();
    }

    let cs = avlm
        .confidence_sequence()
        .expect("AVLM query must succeed")
        .expect("must have enough data for CI");

    // Regression adjustment should reduce variance (compared to no covariate).
    assert!(
        cs.variance_reduction > 0.0,
        "AVLM variance reduction must be positive (covariate is correlated with outcome): {:.4}",
        cs.variance_reduction
    );

    // Adjusted effect should detect the +0.5 treatment effect.
    assert!(
        cs.adjusted_effect > 0.0,
        "adjusted effect must be positive (treatment adds +0.5): {:.4}",
        cs.adjusted_effect
    );

    // CI should contain the true effect and exclude zero (given n=100 per arm
    // and strong signal relative to noise).
    assert!(
        cs.ci_lower < cs.adjusted_effect && cs.adjusted_effect < cs.ci_upper,
        "adjusted effect must lie within CI: [{:.4}, {:.4}] effect={:.4}",
        cs.ci_lower,
        cs.ci_upper,
        cs.adjusted_effect
    );

    // With strong covariate correlation (ρ ≈ 0.85) and large n, the CI should
    // be tight enough to make the effect significant.
    assert!(
        cs.is_significant,
        "with n=100/arm and +0.5 effect, AVLM should detect significance \
         (CI=[{:.4}, {:.4}], effect={:.4})",
        cs.ci_lower,
        cs.ci_upper,
        cs.adjusted_effect
    );

    // ── Phase 4: RunAnalysis RPC with the same data ─────────────────────
    let dir = TempDir::new().unwrap();

    let mut exp_ids = Vec::new();
    let mut user_ids = Vec::new();
    let mut variant_ids = Vec::new();
    let mut metric_ids = Vec::new();
    let mut values = Vec::new();
    let mut covariates = Vec::new();

    // Re-generate the same data for the Delta Lake table.
    let mut rng_table = DeterministicRng::new(0xA71A_5EED_0001);
    for i in 0..n_per_arm {
        let x = 5.0 + rng_table.next_gaussian();
        let y = 0.5 * x + 0.3 * rng_table.next_gaussian();
        exp_ids.push("exp-mo-avlm-001");
        user_ids.push(format!("ctrl-{i:03}"));
        variant_ids.push("control".to_string());
        metric_ids.push("engagement_rate");
        values.push(y);
        covariates.push(Some(x));
    }
    for i in 0..n_per_arm {
        let x = 5.0 + rng_table.next_gaussian();
        let y = 0.5 * x + 0.5 + 0.3 * rng_table.next_gaussian();
        exp_ids.push("exp-mo-avlm-001");
        user_ids.push(format!("trt-{i:03}"));
        variant_ids.push("treatment".to_string());
        metric_ids.push("engagement_rate");
        values.push(y);
        covariates.push(Some(x));
    }

    let variant_ids_ref: Vec<&str> = variant_ids.iter().map(String::as_str).collect();
    let user_ids_ref: Vec<&str> = user_ids.iter().map(String::as_str).collect();

    let batch = make_metric_batch(
        &exp_ids,
        &user_ids_ref,
        &variant_ids_ref,
        &metric_ids,
        &values,
        &covariates,
    );
    write_metric_table(dir.path(), batch).await;

    let handler = test_handler(dir.path().to_str().unwrap());
    let result = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-mo-avlm-001".to_string(),
            ..Default::default()
        }))
        .await
        .expect("RunAnalysis must succeed")
        .into_inner();

    assert_eq!(result.experiment_id, "exp-mo-avlm-001");
    assert!(!result.metric_results.is_empty(), "must produce metric results");

    let mr = result
        .metric_results
        .iter()
        .find(|r| r.metric_id == "engagement_rate" && r.variant_id == "treatment")
        .expect("must have engagement_rate / treatment MetricResult");

    assert!(
        mr.absolute_effect > 0.0,
        "RunAnalysis must detect positive effect: {:.4}",
        mr.absolute_effect
    );
    assert!(
        mr.p_value < 0.10,
        "large effect with n=100/arm should be significant at α=0.10; p={:.4}",
        mr.p_value
    );
}

// ===========================================================================
// Test 2: Switchback experiment lifecycle
// ===========================================================================

/// **Switchback full lifecycle: block generation → SwitchbackAnalyzer → RunAnalysis.**
///
/// This test exercises the ADR-022 pipeline end to end:
///
/// 1. **Block generation**: 20 alternating treatment/control blocks (10 each)
///    with 2 washout blocks at transitions. Treatment effect = +2.0 on top of
///    baseline ~ N(10.0, 1.0).
///
/// 2. **SwitchbackAnalyzer**: Computes HAC SE (Newey-West), randomization
///    inference p-value, and carryover diagnostic from block-level outcomes.
///    Verifies effect direction, HAC SE positivity, RI p-value validity,
///    and carryover diagnostic output.
///
/// 3. **RunAnalysis RPC**: The same block-period data (formatted as
///    `period_N/user_M`) flows through the standard analysis path, verifying
///    that the t-test fallback still produces valid results for switchback data.
#[tokio::test]
async fn test_switchback_experiment_lifecycle() {
    use experimentation_stats::switchback::{BlockOutcome, SwitchbackAnalyzer};

    let mut rng = DeterministicRng::new(0x5A1C_CBAC_0001);
    let baseline = 10.0;
    let effect = 2.0;

    // ── Phase 1: Generate block-level outcomes ──────────────────────────
    // 20 blocks: blocks 0,2,4,6,8 = control; blocks 1,3,5,7,9 = treatment
    // (first 10), then repeat pattern for blocks 10–19.
    // Washout blocks at positions 4 and 14 (transition blocks).
    let mut blocks = Vec::new();
    for i in 0..20u64 {
        let is_washout = i == 4 || i == 14;
        let is_treatment = i % 2 == 1;
        let base = if is_treatment {
            baseline + effect
        } else {
            baseline
        };
        blocks.push(BlockOutcome {
            block_index: i,
            cluster_id: "global".into(),
            is_treatment,
            metric_value: base + rng.next_gaussian() * 0.5,
            user_count: 1000,
            in_washout: is_washout,
        });
    }

    // ── Phase 2: SwitchbackAnalyzer ─────────────────────────────────────
    let analyzer = SwitchbackAnalyzer::new(blocks.clone())
        .expect("must construct analyzer with 18 non-washout blocks");

    let result = analyzer
        .analyze(0.05, 10_000, 42)
        .expect("switchback analysis must succeed");

    // Effect should be close to +2.0 (treatment blocks are baseline + 2.0).
    assert!(
        result.effect > 0.0,
        "switchback effect must be positive (ground truth +2.0): {:.4}",
        result.effect
    );
    assert!(
        (result.effect - effect).abs() < 2.0,
        "switchback effect should be within 2.0 of ground truth (2.0): {:.4}",
        result.effect
    );

    // HAC SE must be positive and finite.
    assert!(
        result.hac_se > 0.0 && result.hac_se.is_finite(),
        "HAC SE must be positive and finite: {:.4}",
        result.hac_se
    );

    // CI must contain the effect estimate.
    assert!(
        result.ci_lower < result.effect && result.effect < result.ci_upper,
        "effect must lie within CI: [{:.4}, {:.4}] effect={:.4}",
        result.ci_lower,
        result.ci_upper,
        result.effect
    );

    // Randomization inference p-value must be in [0, 1].
    assert!(
        result.randomization_p_value >= 0.0 && result.randomization_p_value <= 1.0,
        "RI p-value must be in [0, 1]: {:.4}",
        result.randomization_p_value
    );

    // With a strong effect (+2.0 on σ=0.5 noise, 18 blocks), RI should detect it.
    assert!(
        result.randomization_p_value < 0.10,
        "strong switchback effect should yield RI p < 0.10: {:.4}",
        result.randomization_p_value
    );

    // Effective blocks = 18 (20 total − 2 washout).
    assert_eq!(
        result.effective_blocks, 18,
        "18 effective blocks (20 total − 2 washout)"
    );

    // Carryover diagnostic: p-value must be in [0, 1].
    assert!(
        result.carryover_test_p_value >= 0.0 && result.carryover_test_p_value <= 1.0,
        "carryover p-value must be in [0, 1]: {:.4}",
        result.carryover_test_p_value
    );

    // Lag-1 autocorrelation must be finite.
    assert!(
        result.lag1_autocorrelation.is_finite(),
        "lag-1 autocorrelation must be finite: {:.4}",
        result.lag1_autocorrelation
    );

    // HAC bandwidth must be > 0 (Andrews automatic selection with temporal data).
    assert!(
        result.hac_bandwidth > 0,
        "HAC bandwidth must be positive for temporal data: {}",
        result.hac_bandwidth
    );

    // ── Phase 3: RunAnalysis RPC with switchback-formatted data ─────────
    let dir = TempDir::new().unwrap();
    let exp_id = "exp-switchback-lifecycle";
    let metric_id = "watch_time_minutes";

    let mut exp_ids_v = Vec::new();
    let mut user_ids_v = Vec::new();
    let mut variant_ids_v = Vec::new();
    let mut metric_ids_v = Vec::new();
    let mut values_v = Vec::new();
    let mut covariates_v = Vec::new();

    let mut rng_rpc = DeterministicRng::new(0x5A1C_CBAC_0002);
    // Generate period-prefixed user data matching the block structure.
    for block in &blocks {
        if block.in_washout {
            continue;
        }
        let variant = if block.is_treatment {
            "treatment"
        } else {
            "control"
        };
        for user in 0..10u32 {
            exp_ids_v.push(exp_id);
            user_ids_v.push(format!("period_{}/user_{user:03}", block.block_index));
            variant_ids_v.push(variant.to_string());
            metric_ids_v.push(metric_id);
            values_v.push(block.metric_value + rng_rpc.next_gaussian() * 0.2);
            covariates_v.push(None);
        }
    }

    let variant_ids_ref: Vec<&str> = variant_ids_v.iter().map(String::as_str).collect();
    let user_ids_ref: Vec<&str> = user_ids_v.iter().map(String::as_str).collect();

    let batch = make_metric_batch(
        &exp_ids_v,
        &user_ids_ref,
        &variant_ids_ref,
        &metric_ids_v,
        &values_v,
        &covariates_v,
    );
    write_metric_table(dir.path(), batch).await;

    let handler = test_handler(dir.path().to_str().unwrap());
    let rpc_result = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: exp_id.to_string(),
            ..Default::default()
        }))
        .await
        .expect("RunAnalysis must succeed on switchback data")
        .into_inner();

    assert_eq!(rpc_result.experiment_id, exp_id);
    let mr = rpc_result
        .metric_results
        .iter()
        .find(|r| r.metric_id == metric_id && r.variant_id == "treatment")
        .expect("must have watch_time_minutes / treatment result");

    // Treatment mean > control mean (positive effect).
    assert!(
        mr.treatment_mean > mr.control_mean,
        "treatment_mean must exceed control_mean: trt={:.3}, ctrl={:.3}",
        mr.treatment_mean,
        mr.control_mean
    );
    assert!(
        mr.absolute_effect > 0.0,
        "absolute_effect must be positive: {:.4}",
        mr.absolute_effect
    );
}

// ===========================================================================
// Test 3: Synthetic control method
// ===========================================================================

/// **Synthetic control: Classic SCM + Augmented SCM on panel data.**
///
/// This test exercises the ADR-023 pipeline:
///
/// 1. **Panel data**: One treated unit ("region:de") + 5 donor units
///    ("region:fr", "region:uk", "region:es", "region:it", "region:nl").
///    Pre-treatment: 20 periods with parallel trends.
///    Post-treatment: 10 periods where the treated unit gets a +3.0 effect.
///
/// 2. **Classic SCM**: Constrained convex optimization finds donor weights
///    on the simplex (w ≥ 0, Σw = 1). ATT should be close to +3.0.
///    Donor weights must be non-negative and sum to 1.0.
///
/// 3. **Augmented SCM**: Ridge bias correction on top of Classic SCM.
///    ATT should also be close to +3.0, potentially with tighter CIs.
///
/// 4. **RunAnalysis RPC**: Panel data formatted as user-level observations
///    flows through the standard t-test path.
#[tokio::test]
async fn test_synthetic_control_method() {
    use experimentation_stats::synthetic_control::{
        synthetic_control, Method, SyntheticControlInput,
    };

    let pre_periods = 20usize;
    let post_periods = 10usize;
    let total_periods = pre_periods + post_periods;
    let treatment_effect = 3.0;

    let mut rng = DeterministicRng::new(0x5111_C1A1_0001);

    // Generate parallel trends for all units (pre-treatment).
    // Base trend: y_t = 10.0 + 0.5 * t + noise.
    let base_trend: Vec<f64> = (0..total_periods)
        .map(|t| 10.0 + 0.5 * t as f64)
        .collect();

    // Treated unit: follows base trend pre-treatment, adds +3.0 post-treatment.
    let treated_series: Vec<f64> = (0..total_periods)
        .map(|t| {
            let base = base_trend[t] + rng.next_gaussian() * 0.3;
            if t >= pre_periods {
                base + treatment_effect
            } else {
                base
            }
        })
        .collect();

    // Donors: each follows the base trend with a unit-specific intercept + noise.
    let donor_names = ["region:fr", "region:uk", "region:es", "region:it", "region:nl"];
    let donor_offsets = [0.5, -0.3, 0.1, -0.2, 0.4];
    let donors: Vec<(String, Vec<f64>)> = donor_names
        .iter()
        .zip(donor_offsets.iter())
        .map(|(name, offset)| {
            let series: Vec<f64> = (0..total_periods)
                .map(|t| base_trend[t] + offset + rng.next_gaussian() * 0.3)
                .collect();
            (name.to_string(), series)
        })
        .collect();

    let input = SyntheticControlInput::new(
        "region:de",
        treated_series.clone(),
        donors.clone(),
        pre_periods,
    );

    // ── Classic SCM ─────────────────────────────────────────────────────
    let classic = synthetic_control(&input, Method::Classic)
        .expect("Classic SCM must succeed");

    assert_eq!(classic.method, Method::Classic);

    // ATT should be close to +3.0 (treatment effect).
    assert!(
        classic.att > 0.0,
        "Classic SCM ATT must be positive (ground truth +3.0): {:.4}",
        classic.att
    );
    assert!(
        (classic.att - treatment_effect).abs() < 2.0,
        "Classic SCM ATT should be within 2.0 of ground truth (3.0): {:.4}",
        classic.att
    );

    // Donor weights: non-negative, sum to 1.0.
    assert!(
        !classic.donor_weights.is_empty(),
        "donor weights must be non-empty"
    );
    for (name, &w) in &classic.donor_weights {
        assert!(
            w >= -1e-9,
            "Classic SCM weight for '{name}' must be non-negative: {w:.6}"
        );
    }
    let weight_sum: f64 = classic.donor_weights.values().sum();
    assert!(
        (weight_sum - 1.0).abs() < 1e-6,
        "Classic SCM donor weights must sum to 1.0 (±1e-6): {weight_sum:.8}"
    );

    // CI must contain the ATT estimate.
    assert!(
        classic.ci_lower < classic.att && classic.att < classic.ci_upper,
        "ATT must lie within CI: [{:.4}, {:.4}] att={:.4}",
        classic.ci_lower,
        classic.ci_upper,
        classic.att
    );

    // Placebo p-value must be in [0, 1].
    assert!(
        classic.placebo_p_value >= 0.0 && classic.placebo_p_value <= 1.0,
        "placebo p-value must be in [0, 1]: {:.4}",
        classic.placebo_p_value
    );

    // ── Augmented SCM ───────────────────────────────────────────────────
    let augmented = synthetic_control(&input, Method::Augmented)
        .expect("Augmented SCM must succeed");

    assert_eq!(augmented.method, Method::Augmented);

    // ATT should also be close to +3.0.
    assert!(
        augmented.att > 0.0,
        "Augmented SCM ATT must be positive: {:.4}",
        augmented.att
    );
    assert!(
        (augmented.att - treatment_effect).abs() < 2.0,
        "Augmented SCM ATT should be within 2.0 of ground truth (3.0): {:.4}",
        augmented.att
    );

    // Augmented donor weights may not be on the simplex (Ridge correction
    // relaxes the convexity constraint), but should still sum close to 1.0.
    assert!(
        !augmented.donor_weights.is_empty(),
        "Augmented SCM donor weights must be non-empty"
    );

    // CI must be valid.
    assert!(
        augmented.ci_lower < augmented.ci_upper,
        "Augmented SCM CI must have lower < upper: [{:.4}, {:.4}]",
        augmented.ci_lower,
        augmented.ci_upper
    );

    // ── Phase 3: RunAnalysis RPC with panel data ────────────────────────
    let dir = TempDir::new().unwrap();
    let exp_id = "exp-scm-001";
    let metric_id = "platform_revenue";

    let mut exp_ids_v = Vec::new();
    let mut user_ids_v = Vec::new();
    let mut variant_ids_v = Vec::new();
    let mut metric_ids_v = Vec::new();
    let mut values_v = Vec::new();
    let mut covariates_v = Vec::new();

    // Treated unit post-treatment observations → "treatment" variant.
    for t in pre_periods..total_periods {
        exp_ids_v.push(exp_id);
        user_ids_v.push(format!("region_de_t{t:02}"));
        variant_ids_v.push("treatment".to_string());
        metric_ids_v.push(metric_id);
        values_v.push(treated_series[t]);
        covariates_v.push(None);
    }

    // Donor units post-treatment observations → "control" variant.
    for (name, series) in &donors {
        for t in pre_periods..total_periods {
            exp_ids_v.push(exp_id);
            user_ids_v.push(format!("{}_t{t:02}", name.replace(':', "_")));
            variant_ids_v.push("control".to_string());
            metric_ids_v.push(metric_id);
            values_v.push(series[t]);
            covariates_v.push(None);
        }
    }

    let variant_ids_ref: Vec<&str> = variant_ids_v.iter().map(String::as_str).collect();
    let user_ids_ref: Vec<&str> = user_ids_v.iter().map(String::as_str).collect();

    let batch = make_metric_batch(
        &exp_ids_v,
        &user_ids_ref,
        &variant_ids_ref,
        &metric_ids_v,
        &values_v,
        &covariates_v,
    );
    write_metric_table(dir.path(), batch).await;

    let handler = test_handler(dir.path().to_str().unwrap());
    let rpc_result = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: exp_id.to_string(),
            ..Default::default()
        }))
        .await
        .expect("RunAnalysis must succeed on SCM data")
        .into_inner();

    assert_eq!(rpc_result.experiment_id, exp_id);
    let mr = rpc_result
        .metric_results
        .iter()
        .find(|r| r.metric_id == metric_id && r.variant_id == "treatment")
        .expect("must have platform_revenue / treatment result");

    // Treatment mean should exceed control mean (ATT ≈ +3.0).
    assert!(
        mr.absolute_effect > 0.0,
        "SCM RunAnalysis effect must be positive (ground truth +3.0): {:.4}",
        mr.absolute_effect
    );
}

// ===========================================================================
// Test 4: Meta-experiment with isolated bandit policies
// ===========================================================================

/// **Meta-experiment: isolated slate bandit policies with different objectives.**
///
/// This test exercises ADR-013 + ADR-016:
///
/// 1. **ADR-013 setup**: Two meta-experiment variants ("variant_engagement" and
///    "variant_retention") with different reward weight profiles. Each variant
///    runs an independent slate bandit policy.
///
/// 2. **ADR-016**: Each variant's policy is a `SlatePolicy` with 3 slots, 6
///    candidate items, and different attribution models:
///    - Variant A: `ClickedSlotOnly` attribution (no position bias)
///    - Variant B: `LeaveOneOut` attribution with Cascade position bias (γ=0.8)
///
/// 3. **Policy isolation**: After 100 rounds of different reward signals, the
///    two policies must have divergent posteriors (different arm rankings).
///
/// 4. **LIPS OPE**: Off-policy evaluation of each policy's logged slates
///    produces finite, non-negative estimates.
///
/// 5. **Serialization roundtrip**: Both policies survive `to_bytes` → `from_bytes`
///    without losing state (crash-recovery requirement from ADR-016).
#[test]
fn test_meta_experiment_isolated_bandit_policies() {
    use experimentation_bandit::slate::{
        lips_estimate, AttributionModel, PositionBiasModel, SlateLog, SlatePolicy,
    };
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    let n_slots = 3;
    let candidates: Vec<String> = (0..6).map(|i| format!("item_{i:02}")).collect();

    // ── Variant A: ClickedSlotOnly, no position bias ────────────────────
    let mut policy_a = SlatePolicy::new(
        "exp-meta-001/variant_engagement".into(),
        candidates.clone(),
        n_slots,
        AttributionModel::ClickedSlotOnly,
    );

    // ── Variant B: LeaveOneOut, Cascade position bias ───────────────────
    let mut policy_b = SlatePolicy::with_position_bias(
        "exp-meta-001/variant_retention".into(),
        candidates.clone(),
        n_slots,
        AttributionModel::LeaveOneOut,
        PositionBiasModel::Cascade { gamma: 0.8 },
    );

    let mut rng_a = SmallRng::seed_from_u64(0xAE1A_A001);
    let mut rng_b = SmallRng::seed_from_u64(0xAE1A_B002);
    let mut rng_data = DeterministicRng::new(0xAE1A_DA1A_03);

    let mut logs_a: Vec<SlateLog> = Vec::new();
    let mut logs_b: Vec<SlateLog> = Vec::new();

    // ── Phase 1: Run 100 rounds with different reward distributions ─────
    //
    // Variant A (engagement focus): item_00 has high engagement (click rate 80%),
    //   item_01–05 have lower engagement (20%).
    // Variant B (retention focus): item_03 has high retention (click rate 70%),
    //   others have lower retention (15%).
    for _ in 0..100 {
        // Variant A
        let slate_a = policy_a.select_slate(&candidates, n_slots, &mut rng_a);
        assert_eq!(slate_a.len(), n_slots, "slate_a must have {n_slots} items");
        // Simulate click: item_00 gets clicked 80% of the time if present.
        let click_pos_a = slate_a.iter().position(|id| id == "item_00");
        let clicked_a = if let Some(pos) = click_pos_a {
            if rng_data.next_uniform() < 0.8 {
                Some(pos)
            } else {
                None
            }
        } else {
            // Random click with 20% probability on any item.
            if rng_data.next_uniform() < 0.2 {
                Some(0)
            } else {
                None
            }
        };
        let reward_a = if clicked_a.is_some() { 1.0 } else { 0.0 };
        policy_a.update(&slate_a, clicked_a, reward_a, 0.5);
        logs_a.push(SlateLog {
            slate: slate_a,
            clicked: clicked_a.map(|p| candidates[p.min(candidates.len() - 1)].clone()),
            clicked_position: clicked_a,
            propensity: 0.5,
            reward: reward_a,
        });

        // Variant B
        let slate_b = policy_b.select_slate(&candidates, n_slots, &mut rng_b);
        assert_eq!(slate_b.len(), n_slots, "slate_b must have {n_slots} items");
        // Simulate click: item_03 gets clicked 70% of the time if present.
        let click_pos_b = slate_b.iter().position(|id| id == "item_03");
        let clicked_b = if let Some(pos) = click_pos_b {
            if rng_data.next_uniform() < 0.7 {
                Some(pos)
            } else {
                None
            }
        } else {
            if rng_data.next_uniform() < 0.15 {
                Some(0)
            } else {
                None
            }
        };
        let reward_b = if clicked_b.is_some() { 1.0 } else { 0.0 };
        policy_b.update(&slate_b, clicked_b, reward_b, 0.5);
        logs_b.push(SlateLog {
            slate: slate_b,
            clicked: clicked_b.map(|p| candidates[p.min(candidates.len() - 1)].clone()),
            clicked_position: clicked_b,
            propensity: 0.5,
            reward: reward_b,
        });
    }

    // ── Phase 2: Verify policy isolation (divergent posteriors) ──────────
    // After 100 rounds, policy_a should favor item_00 (engagement winner),
    // while policy_b should favor item_03 (retention winner).

    // Run 50 selection trials and count which item appears in slot 0 most often.
    let mut a_slot0_counts: HashMap<String, u32> = HashMap::new();
    let mut b_slot0_counts: HashMap<String, u32> = HashMap::new();

    let mut rng_check_a = SmallRng::seed_from_u64(0xC0EC_A000);
    let mut rng_check_b = SmallRng::seed_from_u64(0xC0EC_B000);

    for _ in 0..50 {
        let sa = policy_a.select_slate(&candidates, n_slots, &mut rng_check_a);
        *a_slot0_counts.entry(sa[0].clone()).or_default() += 1;

        let sb = policy_b.select_slate(&candidates, n_slots, &mut rng_check_b);
        *b_slot0_counts.entry(sb[0].clone()).or_default() += 1;
    }

    // Policy A should select item_00 most often in slot 0.
    let a_best = a_slot0_counts
        .iter()
        .max_by_key(|(_, &v)| v)
        .map(|(k, _)| k.clone())
        .unwrap();
    assert_eq!(
        a_best, "item_00",
        "variant_engagement policy should favor item_00 in slot 0 (got '{a_best}')"
    );

    // Policy B should select item_03 most often in slot 0.
    let b_best = b_slot0_counts
        .iter()
        .max_by_key(|(_, &v)| v)
        .map(|(k, _)| k.clone())
        .unwrap();
    assert_eq!(
        b_best, "item_03",
        "variant_retention policy should favor item_03 in slot 0 (got '{b_best}')"
    );

    // The two policies must have different top arms (policy isolation).
    assert_ne!(
        a_best, b_best,
        "meta-experiment variants must have divergent policy preferences"
    );

    // ── Phase 3: LIPS off-policy evaluation ─────────────────────────────
    let lips_a = lips_estimate(&logs_a);
    let lips_b = lips_estimate(&logs_b);

    assert!(
        lips_a.is_finite() && lips_a >= 0.0,
        "LIPS estimate for variant_engagement must be finite and non-negative: {lips_a:.4}"
    );
    assert!(
        lips_b.is_finite() && lips_b >= 0.0,
        "LIPS estimate for variant_retention must be finite and non-negative: {lips_b:.4}"
    );

    // Both policies received reward signal, so LIPS should be non-zero.
    assert!(
        lips_a > 0.0,
        "variant_engagement LIPS must be positive (received clicks): {lips_a:.4}"
    );
    assert!(
        lips_b > 0.0,
        "variant_retention LIPS must be positive (received clicks): {lips_b:.4}"
    );

    // ── Phase 4: Serialization roundtrip (crash recovery) ───────────────
    let bytes_a = policy_a.to_bytes();
    let restored_a = SlatePolicy::from_bytes(&bytes_a);
    assert_eq!(
        restored_a.experiment_id(),
        "exp-meta-001/variant_engagement",
        "restored policy_a must preserve experiment_id"
    );
    assert_eq!(
        restored_a.total_updates(),
        100,
        "restored policy_a must preserve update count"
    );

    // Verify restored policy produces the same selection as the original.
    let mut rng_restore = SmallRng::seed_from_u64(0xAE51_0AE1);
    let mut rng_original = SmallRng::seed_from_u64(0xAE51_0AE1);
    let slate_restored = restored_a.select_slate(&candidates, n_slots, &mut rng_restore);
    let slate_original = policy_a.select_slate(&candidates, n_slots, &mut rng_original);
    assert_eq!(
        slate_restored, slate_original,
        "restored policy must produce identical slate selection with same RNG seed"
    );

    let bytes_b = policy_b.to_bytes();
    let restored_b = SlatePolicy::from_bytes(&bytes_b);
    assert_eq!(
        restored_b.experiment_id(),
        "exp-meta-001/variant_retention"
    );
    assert_eq!(restored_b.total_updates(), 100);
    assert_eq!(
        *restored_b.position_bias(),
        PositionBiasModel::Cascade { gamma: 0.8 },
        "restored policy_b must preserve position bias model"
    );
    assert_eq!(
        *restored_b.attribution(),
        AttributionModel::LeaveOneOut,
        "restored policy_b must preserve attribution model"
    );
}
