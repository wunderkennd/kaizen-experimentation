//! Golden-file integration tests for GROW martingale and AVLM e-values.
//!
//! GROW values are analytically derivable (see inline comments).
//! AVLM values are validated against the Gaussian mixture e-value formula
//! from Ramdas & Wang (2024) "Hypothesis Testing with E-values" to
//! 6 decimal places.
//!
//! Run:    cargo test -p experimentation-stats --test evalue_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test evalue_golden

use experimentation_stats::evalue::{e_value_avlm, e_value_grow};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

const TOLERANCE: f64 = 1e-6; // 6 decimal places

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn assert_close(actual: f64, expected: f64, field: &str, test_name: &str) {
    let diff = (actual - expected).abs();
    assert!(
        diff < TOLERANCE,
        "[{test_name}] {field}: expected {expected:.15e}, got {actual:.15e}, diff={diff:.15e} > tolerance={TOLERANCE:.0e}"
    );
}

// ---------------------------------------------------------------------------
// GROW golden tests
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenGrow {
    test_name: String,
    /// Derivation note (formula / monograph reference).
    derivation: String,
    observations: Vec<f64>,
    sigma_sq: f64,
    alpha: f64,
    expected: GrowExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GrowExpected {
    e_value: f64,
    log_e_value: f64,
    reject: bool,
    log_wealth_trajectory: Vec<f64>,
}

fn load_grow(filename: &str) -> GoldenGrow {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_grow(filename: &str, golden: &GoldenGrow) {
    let path = golden_dir().join(filename);
    let data = serde_json::to_string_pretty(golden).expect("serialization should not fail");
    std::fs::write(&path, data)
        .unwrap_or_else(|e| panic!("Failed to write golden file {}: {e}", path.display()));
}

fn run_grow_golden(filename: &str) {
    let mut golden = load_grow(filename);
    let result = e_value_grow(&golden.observations, golden.sigma_sq, golden.alpha)
        .unwrap_or_else(|e| panic!("[{}] e_value_grow failed: {e}", golden.test_name));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.expected = GrowExpected {
            e_value: result.e_value,
            log_e_value: result.log_e_value,
            reject: result.reject,
            log_wealth_trajectory: result.log_wealth_trajectory.clone(),
        };
        update_grow(filename, &golden);
        return;
    }

    let name = &golden.test_name;
    assert_close(result.e_value, golden.expected.e_value, "e_value", name);
    assert_close(
        result.log_e_value,
        golden.expected.log_e_value,
        "log_e_value",
        name,
    );
    assert_eq!(
        result.reject, golden.expected.reject,
        "[{name}] reject mismatch"
    );
    assert_eq!(
        result.log_wealth_trajectory.len(),
        golden.expected.log_wealth_trajectory.len(),
        "[{name}] trajectory length mismatch"
    );
    for (i, (&actual, &expected)) in result
        .log_wealth_trajectory
        .iter()
        .zip(golden.expected.log_wealth_trajectory.iter())
        .enumerate()
    {
        assert_close(actual, expected, &format!("trajectory[{i}]"), name);
    }
}

#[test]
fn grow_null_effect() {
    run_grow_golden("evalue_grow_null_effect.json");
}

#[test]
fn grow_constant_effect() {
    run_grow_golden("evalue_grow_constant_effect.json");
}

#[test]
fn grow_negative_observation() {
    run_grow_golden("evalue_grow_negative_observation.json");
}

#[test]
fn grow_large_sample_rejects() {
    run_grow_golden("evalue_grow_large_sample.json");
}

// ---------------------------------------------------------------------------
// AVLM golden tests
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenAvlm {
    test_name: String,
    /// Derivation note — formula reference from Ramdas & Wang monograph.
    derivation: String,
    control_y: Vec<f64>,
    treatment_y: Vec<f64>,
    control_x: Vec<f64>,
    treatment_x: Vec<f64>,
    tau_sq: f64,
    alpha: f64,
    expected: AvlmExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct AvlmExpected {
    e_value: f64,
    log_e_value: f64,
    reject: bool,
    /// Intermediate diagnostic: CUPED theta coefficient.
    theta: f64,
    /// Intermediate diagnostic: adjusted effect estimate.
    delta_adj: f64,
    /// Intermediate diagnostic: adjusted se².
    se_sq_adj: f64,
}

fn load_avlm(filename: &str) -> GoldenAvlm {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_avlm(filename: &str, golden: &GoldenAvlm) {
    let path = golden_dir().join(filename);
    let data = serde_json::to_string_pretty(golden).expect("serialization should not fail");
    std::fs::write(&path, data)
        .unwrap_or_else(|e| panic!("Failed to write golden file {}: {e}", path.display()));
}

fn run_avlm_golden(filename: &str) {
    let mut golden = load_avlm(filename);
    let result = e_value_avlm(
        &golden.control_y,
        &golden.treatment_y,
        &golden.control_x,
        &golden.treatment_x,
        golden.tau_sq,
        golden.alpha,
    )
    .unwrap_or_else(|e| panic!("[{}] e_value_avlm failed: {e}", golden.test_name));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        // Re-compute diagnostics for the golden file.
        // (theta, delta_adj, se_sq_adj derived from expected values in JSON.)
        golden.expected = AvlmExpected {
            e_value: result.e_value,
            log_e_value: result.log_e_value,
            reject: result.reject,
            theta: golden.expected.theta,       // keep existing diagnostic
            delta_adj: golden.expected.delta_adj,
            se_sq_adj: golden.expected.se_sq_adj,
        };
        update_avlm(filename, &golden);
        return;
    }

    let name = &golden.test_name;
    assert_close(result.e_value, golden.expected.e_value, "e_value", name);
    assert_close(
        result.log_e_value,
        golden.expected.log_e_value,
        "log_e_value",
        name,
    );
    assert_eq!(
        result.reject, golden.expected.reject,
        "[{name}] reject mismatch"
    );
}

#[test]
fn avlm_no_covariate_analytic() {
    run_avlm_golden("evalue_avlm_no_covariate.json");
}

#[test]
fn avlm_perfect_covariate() {
    run_avlm_golden("evalue_avlm_perfect_covariate.json");
}

#[test]
fn avlm_partial_correlation() {
    run_avlm_golden("evalue_avlm_partial_correlation.json");
}

#[test]
fn avlm_large_effect_rejects() {
    run_avlm_golden("evalue_avlm_large_effect.json");
}
