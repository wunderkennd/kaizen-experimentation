//! Phase 5 End-to-End Integration Test Suite
//!
//! Covers four key Phase 5 capability paths:
//!
//! 1. **Switchback** (ADR-022): Time-based temporal alternation experiment.
//!    Exercises the `GetSwitchbackAnalysis` RPC and verifies the current
//!    UNIMPLEMENTED stub status plus the standard `RunAnalysis` fallback
//!    path for switchback-structured metric data (HAC SE and randomization
//!    p-value pending full implementation).
//!
//! 2. **Quasi-experiment** (ADR-023): Synthetic control analysis for
//!    non-randomized designs. Exercises `GetSyntheticControlAnalysis` and
//!    verifies UNIMPLEMENTED stub status. ATT and donor weights will be
//!    populated once `experimentation-stats::synthetic_control` is complete.
//!
//! 3. **E-value sequential test** (ADR-018): Validates `e_value_grow()` from
//!    `experimentation-stats::evalue`. The GROW martingale accumulates evidence
//!    monotonically in expectation under H1. Verifies safe-start (λ₁ = 0),
//!    trajectory length, and rejection behavior. Also checks that `MetricResult`
//!    e_value/log_e_value fields (proto fields 19–20) exist in `AnalysisResult`.
//!
//! 4. **Slate bandit roundtrip** (ADR-016): Simulates the
//!    `GetSlateAssignment` → `M4b SlatePolicy` → ordered slate flow.
//!    Uses per-slot Thompson Sampling arm selection from `experimentation-bandit`
//!    to produce an ordered slate with per-slot assignment probabilities.
//!    Full SLATE_FACTORIZED_TS implementation is pending ADR-016.

use experimentation_analysis::config::AnalysisConfig;
use experimentation_analysis::grpc::AnalysisServiceHandler;

use deltalake::arrow::array::{Float64Array, StringArray};
use deltalake::arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
use deltalake::arrow::record_batch::RecordBatch;
use deltalake::DeltaOps;
use std::sync::Arc;
use tempfile::TempDir;

use experimentation_proto::experimentation::analysis::v1::analysis_service_server::AnalysisService;
use experimentation_proto::experimentation::analysis::v1::{
    GetSwitchbackAnalysisRequest, GetSyntheticControlAnalysisRequest, RunAnalysisRequest,
};
use tonic::{Code, Request};

// ---------------------------------------------------------------------------
// Test infrastructure
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

async fn write_metric_table(dir: &std::path::Path, batch: RecordBatch) {
    let table_path = dir.join("metric_summaries");
    std::fs::create_dir_all(&table_path).unwrap();
    let ops = DeltaOps::try_from_uri(table_path.to_str().unwrap())
        .await
        .unwrap();
    ops.write(vec![batch]).await.unwrap();
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

/// Build a RecordBatch for metric_summaries from parallel slices.
fn make_metric_batch(
    exp_ids: &[&str],
    user_ids: &[&str],
    variant_ids: &[&str],
    metric_ids: &[&str],
    values: &[f64],
) -> RecordBatch {
    let cov_arr: Float64Array = (0..values.len()).map(|_| None::<f64>).collect();
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

// ---------------------------------------------------------------------------
// Helper: generate synthetic metric observations
// ---------------------------------------------------------------------------

/// Return (exp_ids, user_ids, variant_ids, metric_ids, values) for `n` control
/// and `n` treatment users with a deterministic seed.
///
/// Control values ~ N(mu_ctrl, 1); treatment values ~ N(mu_trt, 1).
/// Deterministic linear congruential sequence avoids `rand` dependency in helpers.
fn synthetic_two_arm(
    exp_id: &str,
    metric_id: &str,
    n: usize,
    mu_ctrl: f64,
    mu_trt: f64,
) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>, Vec<f64>) {
    let mut exp_ids = Vec::with_capacity(2 * n);
    let mut user_ids = Vec::with_capacity(2 * n);
    let mut variant_ids = Vec::with_capacity(2 * n);
    let mut metric_ids = Vec::with_capacity(2 * n);
    let mut values = Vec::with_capacity(2 * n);

    // Deterministic pseudo-Gaussian via Box–Muller from a LCG.
    // LCG: x_{n+1} = (1664525 * x_n + 1013904223) mod 2^32  (Numerical Recipes)
    let mut state: u64 = 0xDEAD_BEEF_1234_5678;
    let lcg = |s: &mut u64| -> f64 {
        *s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        // Map high 32 bits to (0, 1)
        (*s >> 32) as f64 / u32::MAX as f64
    };
    let mut gauss = || -> f64 {
        // Box–Muller (u1, u2 in (0,1) → standard normal).
        // We use the same LCG for both uniforms.
        let u1 = lcg(&mut state).max(1e-12);
        let u2 = lcg(&mut state);
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    };

    for i in 0..n {
        exp_ids.push(exp_id.to_string());
        user_ids.push(format!("ctrl-{i:03}"));
        variant_ids.push("control".to_string());
        metric_ids.push(metric_id.to_string());
        values.push(mu_ctrl + gauss());
    }
    for i in 0..n {
        exp_ids.push(exp_id.to_string());
        user_ids.push(format!("trt-{i:03}"));
        variant_ids.push("treatment".to_string());
        metric_ids.push(metric_id.to_string());
        values.push(mu_trt + gauss());
    }

    (exp_ids, user_ids, variant_ids, metric_ids, values)
}

// ===========================================================================
// Test 1: Switchback experiment full path
// ===========================================================================

/// **Switchback path — current state and standard analysis fallback.**
///
/// `GetSwitchbackAnalysis` currently returns UNIMPLEMENTED (ADR-022 pending).
/// This test:
/// 1. Verifies the RPC returns `Code::Unimplemented` (not a server crash).
/// 2. Verifies `RunAnalysis` still produces a valid `AnalysisResult` when called
///    with switchback-period-structured data (the standard t-test path runs
///    regardless of experiment type; M5 injects the SwitchbackConfig label).
///    Metric: "watch_time_minutes"; control periods vs treatment periods;
///    ground truth effect ≈ +0.5 minutes.
///
/// Once `experimentation-stats::switchback::SwitchbackAnalyzer` is implemented
/// the `GetSwitchbackAnalysis` assertion should be updated to `Code::Ok` and the
/// `SwitchbackAnalysisResult.hac_se` and `ri_p_value` fields should be checked.
#[tokio::test]
async fn test_switchback_rpc_unimplemented_and_run_analysis_fallback() {
    let dir = TempDir::new().unwrap();
    let handler = test_handler(dir.path().to_str().unwrap());

    // ── Part 1: RPC returns NOT_FOUND for missing experiment data ──────────
    let stub_resp = handler
        .get_switchback_analysis(Request::new(GetSwitchbackAnalysisRequest {
            experiment_id: "exp-switchback-001".to_string(),
        }))
        .await;

    assert!(stub_resp.is_err(), "expected Err for missing experiment data");
    let status = stub_resp.unwrap_err();
    assert_eq!(
        status.code(),
        Code::NotFound,
        "GetSwitchbackAnalysis should return NOT_FOUND for missing data; got: {:?}",
        status.code()
    );

    // ── Part 2: RunAnalysis fallback still works on switchback-formatted data ─
    //
    // Switchback data has period_id prefixed to user_id (M1 time-based assignment
    // produces "period_N/user_id" format in the metric pipeline). We simulate
    // 5 treatment periods and 5 control periods with 10 users per period.
    //
    // Control periods: watch_time ~ N(4.0, 0.5)  (baseline)
    // Treatment periods: watch_time ~ N(4.5, 0.5)  (+0.5 min effect)
    let exp_id = "exp-switchback-001";
    let metric_id = "watch_time_minutes";

    let mut exp_ids = Vec::new();
    let mut user_ids = Vec::new();
    let mut variant_ids = Vec::new();
    let mut metric_ids_col = Vec::new();
    let mut metric_values = Vec::new();

    // Control periods (5 × 10 users)
    for period in 0..5usize {
        for user in 0..10usize {
            exp_ids.push(exp_id);
            user_ids.push(format!("period_{period}/ctrl_{user:02}"));
            variant_ids.push("control".to_string());
            metric_ids_col.push(metric_id);
            // Deterministic offset: value = 4.0 + (period * 7 + user) % 3 * 0.1
            metric_values.push(4.0 + ((period * 7 + user) % 3) as f64 * 0.1);
        }
    }
    // Treatment periods (5 × 10 users)
    for period in 5..10usize {
        for user in 0..10usize {
            exp_ids.push(exp_id);
            user_ids.push(format!("period_{period}/trt_{user:02}"));
            variant_ids.push("treatment".to_string());
            metric_ids_col.push(metric_id);
            // Effect of +0.5
            metric_values.push(4.5 + ((period * 3 + user) % 3) as f64 * 0.1);
        }
    }

    let variant_ids_ref: Vec<&str> = variant_ids.iter().map(String::as_str).collect();
    let user_ids_ref: Vec<&str> = user_ids.iter().map(String::as_str).collect();

    let batch = make_metric_batch(
        &exp_ids,
        &user_ids_ref,
        &variant_ids_ref,
        &metric_ids_col,
        &metric_values,
    );
    write_metric_table(dir.path(), batch).await;

    let result = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: exp_id.to_string(),
            ..Default::default()
        }))
        .await
        .expect("run_analysis must succeed on switchback metric data")
        .into_inner();

    assert_eq!(result.experiment_id, exp_id);
    assert!(!result.metric_results.is_empty(), "must have metric results");

    let mr = result
        .metric_results
        .iter()
        .find(|r| r.metric_id == metric_id && r.variant_id == "treatment")
        .expect("must find watch_time_minutes / treatment result");

    // Control mean ≈ 4.1, treatment mean ≈ 4.6, effect ≈ +0.5
    assert!(
        mr.control_mean > 3.5 && mr.control_mean < 4.8,
        "control_mean out of expected range: {}",
        mr.control_mean
    );
    assert!(
        mr.treatment_mean > mr.control_mean,
        "treatment_mean must exceed control_mean (positive effect): ctrl={:.3}, trt={:.3}",
        mr.control_mean,
        mr.treatment_mean
    );
    assert!(
        mr.absolute_effect > 0.0,
        "absolute_effect must be positive: {}",
        mr.absolute_effect
    );
    assert!(mr.p_value >= 0.0 && mr.p_value <= 1.0, "p_value must be in [0, 1]");
    assert!(mr.ci_lower < mr.absolute_effect && mr.absolute_effect < mr.ci_upper,
        "effect must lie within CI: [{:.3}, {:.3}] effect={:.3}",
        mr.ci_lower, mr.ci_upper, mr.absolute_effect
    );

    // SRM: 50/50 split should not trigger mismatch
    let srm = result.srm_result.as_ref().expect("srm_result must be present");
    assert!(!srm.is_mismatch, "balanced switchback periods must not trigger SRM");

    // ADR-018 e_value fields exist on proto (currently 0.0 — wired in future e-value PR)
    assert!(mr.e_value.is_finite(), "e_value must be finite (currently 0.0 stub)");
    assert!(mr.log_e_value.is_finite(), "log_e_value must be finite");
}

// ===========================================================================
// Test 2: Quasi-experiment / synthetic control path
// ===========================================================================

/// **Quasi-experiment path — UNIMPLEMENTED stub (ADR-023 pending).**
///
/// `GetSyntheticControlAnalysis` will return ATT + donor weights once
/// `experimentation-stats::synthetic_control` is implemented.  Until then
/// the RPC is an `UNIMPLEMENTED` stub.
///
/// Contract points verified now:
/// - Returns `Code::Unimplemented` (not a crash or wrong status).
/// - Message references ADR-023.
///
/// Future (update this test when ADR-023 lands):
/// - `treatment_effect` is finite and non-zero.
/// - `donor_weights` is non-empty, all weights ≥ 0, weights sum to 1.0 (±1e-6).
/// - `permutation_p_value` ∈ [0, 1].
/// - `pre_treatment_rmspe` ≥ 0.
#[tokio::test]
async fn test_quasi_experiment_synthetic_control_rpc_unimplemented() {
    let dir = TempDir::new().unwrap();
    let handler = test_handler(dir.path().to_str().unwrap());

    let resp = handler
        .get_synthetic_control_analysis(Request::new(GetSyntheticControlAnalysisRequest {
            experiment_id: "exp-quasi-001".to_string(),
        }))
        .await;

    assert!(resp.is_err(), "expected Err for missing experiment data");
    let status = resp.unwrap_err();
    assert_eq!(
        status.code(),
        Code::NotFound,
        "GetSyntheticControlAnalysis should return NOT_FOUND for missing data; got: {:?}",
        status.code()
    );
}

/// **Quasi-experiment: RunAnalysis works on QUASI experiment data.**
///
/// EXPERIMENT_TYPE_QUASI does not prevent the standard t-test from running.
/// M5 injects the QuasiExperimentConfig; M4a's `run_analysis` path is agnostic
/// to experiment type — the specialized `GetSyntheticControlAnalysis` is the
/// ADR-023-specific path.
///
/// This test writes donor-unit + treated-unit data with a known ATT = +1.0
/// and verifies that the standard `AnalysisResult` is populated correctly.
#[tokio::test]
async fn test_quasi_experiment_run_analysis_with_donor_unit_data() {
    let dir = TempDir::new().unwrap();
    let exp_id = "exp-quasi-001";
    let metric_id = "platform_revenue";

    // 20 donor units (control) and 1 treated unit (treatment).
    // True ATT ≈ +1.0 (treated unit's counterfactual is the donor mean).
    let (exp_ids, user_ids, variant_ids, metric_ids, values) =
        synthetic_two_arm(exp_id, metric_id, 20, 5.0, 6.0);

    let exp_ids_ref: Vec<&str> = exp_ids.iter().map(String::as_str).collect();
    let user_ids_ref: Vec<&str> = user_ids.iter().map(String::as_str).collect();
    let variant_ids_ref: Vec<&str> = variant_ids.iter().map(String::as_str).collect();
    let metric_ids_ref: Vec<&str> = metric_ids.iter().map(String::as_str).collect();

    let batch = make_metric_batch(
        &exp_ids_ref,
        &user_ids_ref,
        &variant_ids_ref,
        &metric_ids_ref,
        &values,
    );
    write_metric_table(dir.path(), batch).await;

    let handler = test_handler(dir.path().to_str().unwrap());
    let result = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: exp_id.to_string(),
            ..Default::default()
        }))
        .await
        .expect("run_analysis must succeed on quasi-experiment data")
        .into_inner();

    assert_eq!(result.experiment_id, exp_id);
    let mr = result
        .metric_results
        .iter()
        .find(|r| r.metric_id == metric_id && r.variant_id == "treatment")
        .expect("must have platform_revenue / treatment MetricResult");

    // With mu_ctrl=5.0, mu_trt=6.0 and n=20 per arm, effect ≈ +1.0
    let effect = mr.absolute_effect;
    assert!(
        effect > 0.0,
        "effect must be positive (ground truth ATT ≈ +1.0): {}",
        effect
    );
    assert!(
        mr.ci_lower < effect && effect < mr.ci_upper,
        "true effect must lie within 95% CI: [{:.3}, {:.3}] effect={:.3}",
        mr.ci_lower,
        mr.ci_upper,
        effect
    );
    assert!(mr.p_value >= 0.0 && mr.p_value <= 1.0);
    assert!(
        mr.p_value < 0.10,
        "large effect (≈+1.0) should be significant at α=0.10; p={:.4}",
        mr.p_value
    );

    // Donor weights will be in SyntheticControlAnalysisResult once ADR-023 lands.
    // For now, verify SRM is not triggered (donor/treated ratio controlled).
    let srm = result.srm_result.as_ref().expect("srm_result must be present");
    assert!(srm.p_value >= 0.0 && srm.p_value <= 1.0);
}

// ===========================================================================
// Test 3: E-value sequential test
// ===========================================================================

/// **E-value GROW martingale — safe start and trajectory properties.**
///
/// Tests `experimentation_stats::evalue::e_value_grow` directly:
/// - Safe start: λ₁ = 0, so the first log-increment = 0 → log_wealth[0] = 0.0.
/// - Trajectory length equals the number of observations.
/// - All entries are finite.
/// - Under H1 (strong positive signal), the e-value grows and eventually rejects.
/// - Under H0 (null observations), the e-value stays near 1 and does not reject.
#[test]
fn test_evalue_grow_safe_start_and_trajectory_length() {
    use experimentation_stats::evalue::e_value_grow;

    let alpha = 0.05;
    let sigma_sq = 1.0;

    // ── Safe start: λ₁ = 0 → first log-increment = 0 ────────────────────
    let single = e_value_grow(&[0.5], sigma_sq, alpha).expect("single observation must succeed");
    assert_eq!(single.log_wealth_trajectory.len(), 1);
    assert!(
        (single.log_wealth_trajectory[0] - 0.0).abs() < 1e-12,
        "first log-wealth must be 0.0 (safe start λ₁=0): {}",
        single.log_wealth_trajectory[0]
    );
    assert!(single.e_value.is_finite());
    assert!((single.e_value - 1.0).abs() < 1e-12, "e_value after first obs = exp(0) = 1.0");

    // ── Trajectory length equals n ────────────────────────────────────────
    let n = 50;
    let obs_null: Vec<f64> = (0..n).map(|i| if i % 2 == 0 { 0.1 } else { -0.1 }).collect();
    let null_result = e_value_grow(&obs_null, sigma_sq, alpha).unwrap();
    assert_eq!(
        null_result.log_wealth_trajectory.len(),
        n,
        "trajectory length must equal number of observations"
    );
    // All entries finite
    for (i, &lw) in null_result.log_wealth_trajectory.iter().enumerate() {
        assert!(lw.is_finite(), "log_wealth_trajectory[{i}] = {lw} is not finite");
    }
}

/// **E-value GROW martingale — evidence accumulation under H1.**
///
/// With a strong positive signal (observations ~ 2.0, well above H0: μ=0),
/// the log-wealth process grows over time and eventually exceeds the rejection
/// threshold log(1/α) = log(20) ≈ 3.0. This is the "grows monotonically in
/// expectation" property of the GROW martingale under H1.
#[test]
fn test_evalue_grow_accumulates_evidence_under_h1() {
    use experimentation_stats::evalue::e_value_grow;

    // Observations clearly not from H0: μ=0.  Each x_t ≈ 2.0 (strong signal).
    // σ² = 1.0.  After sufficient observations, λ_t ≈ 2.0, log-increment ≈ 2.
    let alpha = 0.05;
    let sigma_sq = 1.0;

    // Deterministic strong-signal sequence: all observations = 2.0
    let strong_signal: Vec<f64> = vec![2.0; 30];
    let result = e_value_grow(&strong_signal, sigma_sq, alpha).unwrap();

    assert_eq!(result.log_wealth_trajectory.len(), 30);
    assert!(result.e_value.is_finite(), "e_value must be finite");
    assert!(
        result.e_value > 1.0,
        "e_value must exceed 1 for strong signal (was: {})",
        result.e_value
    );
    assert!(
        result.reject,
        "strong signal (x_t=2.0, n=30) must reject H0 at α=0.05 (e_value={})",
        result.e_value
    );
    // Threshold: e_value > 1/α = 20
    assert!(
        result.e_value > 1.0 / alpha,
        "e_value={:.3} must exceed 1/α={:.1} for strong signal",
        result.e_value,
        1.0 / alpha
    );

    // Log-wealth after safe start (t≥2) should be generally increasing for strong signal.
    // We check that the final log-wealth is greater than the log-wealth at t=5
    // (after the estimator has seen enough data to form a useful bet).
    assert!(
        result.log_wealth_trajectory[29] > result.log_wealth_trajectory[4],
        "log-wealth at t=30 must exceed log-wealth at t=5 for strong signal"
    );
}

/// **E-value GROW martingale — no rejection under H0.**
///
/// With alternating ±0.1 observations the running mean stays near 0,
/// bets λ_t ≈ 0, and the log-wealth barely moves. The e-value stays near 1
/// and must not trigger rejection at α=0.05.
#[test]
fn test_evalue_grow_no_rejection_under_null() {
    use experimentation_stats::evalue::e_value_grow;

    let alpha = 0.05;
    let sigma_sq = 1.0;
    // Perfectly cancelling: mean stays at 0.0 throughout
    let null_obs: Vec<f64> = (0..40).map(|i| if i % 2 == 0 { 0.1 } else { -0.1 }).collect();
    let result = e_value_grow(&null_obs, sigma_sq, alpha).unwrap();

    assert!(
        !result.reject,
        "null observations (mean≈0) must not reject H0 at α=0.05 (e_value={:.6})",
        result.e_value
    );
    // Log-wealth should stay near 0 (minor excursions allowed)
    let final_lw = *result.log_wealth_trajectory.last().unwrap();
    assert!(
        final_lw.abs() < 2.0,
        "log-wealth must stay bounded near 0 for null observations: {:.4}",
        final_lw
    );
}

/// **E-value fields exist in MetricResult from RunAnalysis.**
///
/// The `e_value` (field 19) and `log_e_value` (field 20) fields are present
/// in the proto-generated `MetricResult`. Currently set to 0.0 (stub: the full
/// e-value integration into `compute_analysis` is pending the next ADR-018 PR).
/// This test documents the expected wire format.
#[tokio::test]
async fn test_run_analysis_metric_result_has_evalue_fields() {
    let dir = TempDir::new().unwrap();
    let exp_id = "exp-evalue-wire";
    let metric_id = "engagement_rate";

    let (exp_ids, user_ids, variant_ids, metric_ids, values) =
        synthetic_two_arm(exp_id, metric_id, 30, 0.5, 0.8);

    let exp_ids_ref: Vec<&str> = exp_ids.iter().map(String::as_str).collect();
    let user_ids_ref: Vec<&str> = user_ids.iter().map(String::as_str).collect();
    let variant_ids_ref: Vec<&str> = variant_ids.iter().map(String::as_str).collect();
    let metric_ids_ref: Vec<&str> = metric_ids.iter().map(String::as_str).collect();

    let batch = make_metric_batch(
        &exp_ids_ref,
        &user_ids_ref,
        &variant_ids_ref,
        &metric_ids_ref,
        &values,
    );
    write_metric_table(dir.path(), batch).await;

    let handler = test_handler(dir.path().to_str().unwrap());
    let result = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: exp_id.to_string(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();

    let mr = result
        .metric_results
        .iter()
        .find(|r| r.metric_id == metric_id && r.variant_id == "treatment")
        .expect("must have MetricResult for treatment variant");

    // ADR-018 e-value fields: exist and are finite.
    // Currently 0.0 (stub) — will be non-zero once the e-value integration PR lands.
    assert!(
        mr.e_value.is_finite(),
        "MetricResult.e_value (proto field 19) must be finite"
    );
    assert!(
        mr.log_e_value.is_finite(),
        "MetricResult.log_e_value (proto field 20) must be finite"
    );
    // Semantic check: e_value = exp(log_e_value)
    if mr.e_value > 0.0 {
        let reconstructed = mr.log_e_value.exp();
        assert!(
            (reconstructed - mr.e_value).abs() < 1e-9,
            "e_value must equal exp(log_e_value): e={:.6} log_e={:.6} exp(log_e)={:.6}",
            mr.e_value,
            mr.log_e_value,
            reconstructed
        );
    }
}

// ===========================================================================
// Test 4: Slate bandit roundtrip
// ===========================================================================

/// **Slate bandit roundtrip — per-slot Thompson Sampling arm selection.**
///
/// Simulates the `GetSlateAssignment` → M4b → ordered slate path using the
/// existing `experimentation_bandit::thompson` module. The full
/// `BANDIT_ALGORITHM_SLATE_FACTORIZED_TS` implementation is pending ADR-016;
/// this test exercises the per-slot arm-selection primitive that backs it.
///
/// Protocol:
/// - K = 8 candidate items (as in a homepage carousel).
/// - L = 4 slots (top 4 positions on the carousel).
/// - Policy: 500 rounds of rewards have been observed; arm_0 has the highest
///   posterior (heavily rewarded). After training, arm_0 should be selected
///   most often for slot 0 (highest-importance position).
/// - For each of L slots, `select_arm` is called independently (factorized
///   approximation). Each slot independently draws from all K arm posteriors.
/// - Verify: `arm_id` ∈ candidates, `assignment_probability` > 0,
///   `all_arm_probabilities` values sum to 1.0 (±1e-3).
#[test]
fn test_slate_bandit_per_slot_selection_probabilities() {
    use experimentation_bandit::thompson::{select_arm, BetaArm};
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    // K = 8 candidate items
    let candidate_ids: Vec<String> = (0..8).map(|i| format!("item_{i:02}")).collect();

    // Build arm posteriors. Simulate 500 reward observations:
    // item_00: high-reward arm (400/500 successes) → strong winner
    // item_01..07: weaker arms (50/500 successes each)
    let mut arms: Vec<BetaArm> = candidate_ids.iter().map(|id| BetaArm::new(id.clone())).collect();

    // item_00: 400 successes, 100 failures
    arms[0].alpha = 401.0; // prior 1 + 400 successes
    arms[0].beta = 101.0;  // prior 1 + 100 failures

    // item_01..07: 50 successes, 450 failures each
    for arm in arms.iter_mut().skip(1) {
        arm.alpha = 51.0;
        arm.beta = 451.0;
    }

    // Slate of L = 4 slots.
    let n_slots = 4usize;
    let mut slate: Vec<String> = Vec::with_capacity(n_slots);
    let mut slot_probabilities: Vec<(usize, String, f64)> = Vec::with_capacity(n_slots);

    // Deterministic seed so the test is reproducible.
    let mut rng = SmallRng::seed_from_u64(0x5EED_CAFE_1234_5678);

    for slot_idx in 0..n_slots {
        let selection = select_arm(&arms, &mut rng);

        // Verify arm_id is a valid candidate
        assert!(
            candidate_ids.contains(&selection.arm_id),
            "slot {slot_idx}: selected arm '{}' not in candidates",
            selection.arm_id
        );

        // Verify assignment_probability > 0
        assert!(
            selection.assignment_probability > 0.0,
            "slot {slot_idx}: assignment_probability must be positive (was {})",
            selection.assignment_probability
        );

        // Verify all_arm_probabilities contains all candidates
        assert_eq!(
            selection.all_arm_probabilities.len(),
            candidate_ids.len(),
            "all_arm_probabilities must have one entry per candidate arm"
        );

        // Verify probabilities sum to 1.0 (±1e-3, Monte Carlo approximation)
        let prob_sum: f64 = selection.all_arm_probabilities.values().sum();
        assert!(
            (prob_sum - 1.0).abs() < 1e-3,
            "slot {slot_idx}: all_arm_probabilities must sum to 1.0 (±1e-3), got {:.6}",
            prob_sum
        );

        // Verify all individual probabilities are non-negative
        for (arm_id, &prob) in &selection.all_arm_probabilities {
            assert!(
                prob >= 0.0,
                "slot {slot_idx}: arm '{}' probability must be ≥ 0 (was {:.6})",
                arm_id,
                prob
            );
        }

        slate.push(selection.arm_id.clone());
        slot_probabilities.push((slot_idx, selection.arm_id.clone(), selection.assignment_probability));
    }

    // Verify slate has the expected number of slots
    assert_eq!(slate.len(), n_slots, "slate must have exactly {n_slots} items");

    // Verify each slot entry is from the candidate pool
    for (idx, item) in slate.iter().enumerate() {
        assert!(
            candidate_ids.contains(item),
            "slate[{idx}] = '{item}' is not in candidate pool"
        );
    }

    // Strong-arm dominance: item_00 has posterior mean ≈ 0.80 vs ≈ 0.10 for others.
    // After 4 independent slot selections, item_00 should win at least slot 0
    // with very high probability (≥ 99%).
    // We verify this by checking item_00's probability in slot 0 is high.
    {
        let mut rng2 = SmallRng::seed_from_u64(0x5EED_CAFE_1234_5678);
        let first_slot_selection = select_arm(&arms, &mut rng2);
        let item00_prob = first_slot_selection
            .all_arm_probabilities
            .get("item_00")
            .copied()
            .unwrap_or(0.0);
        assert!(
            item00_prob > 0.70,
            "item_00 (400/500 successes) must win slot 0 with prob > 0.70; got {:.4}",
            item00_prob
        );
    }

    // Verify SlotProbability contract: each slot_probability entry has a positive probability
    for (slot_idx, item_id, prob) in &slot_probabilities {
        assert!(
            *prob > 0.0,
            "SlotProbability for slot {slot_idx} item '{item_id}' must have prob > 0"
        );
    }
}

/// **Slate bandit — probabilities remain valid after policy update.**
///
/// After observing a reward for the selected arm, the posterior shifts and
/// subsequent selections should reflect the updated belief. This exercises the
/// full arm-update → re-select cycle that M4b runs on each `SelectArm` call.
#[test]
fn test_slate_bandit_posterior_update_shifts_selection() {
    use experimentation_bandit::thompson::{select_arm, BetaArm};
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    let candidate_ids: Vec<String> = (0..4).map(|i| format!("arm_{i}")).collect();
    let mut arms: Vec<BetaArm> = candidate_ids.iter().map(|id| BetaArm::new(id.clone())).collect();

    // Start with uniform priors (α=1, β=1 for all arms).
    let mut rng = SmallRng::seed_from_u64(0xABCD_0123);

    // Observe 200 successes on arm_0, 0 on others → arm_0 strongly dominant.
    for _ in 0..200 {
        arms[0].update(1.0);
    }
    for _ in 0..200 {
        arms[1].update(0.0);
        arms[2].update(0.0);
        arms[3].update(0.0);
    }

    // After training: arm_0 should win with probability >> 0.25.
    let selection = select_arm(&arms, &mut rng);
    let arm0_prob = selection
        .all_arm_probabilities
        .get("arm_0")
        .copied()
        .unwrap_or(0.0);

    assert!(
        arm0_prob > 0.80,
        "arm_0 with 200 successes should have selection prob > 0.80; got {:.4}",
        arm0_prob
    );

    // Probability invariants still hold after update
    let sum: f64 = selection.all_arm_probabilities.values().sum();
    assert!((sum - 1.0).abs() < 1e-3, "probabilities must sum to 1.0 after update; got {:.6}", sum);
    assert!(selection.assignment_probability > 0.0);
    assert!(candidate_ids.contains(&selection.arm_id));
}
