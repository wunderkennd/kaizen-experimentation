//! Golden-file integration tests for CUPED variance reduction.
//!
//! Expected values computed via numpy/scipy (equivalent to R's var()/cov() with ddof=1).
//!
//! Run: cargo test -p experimentation-stats --test cuped_golden
//! Update golden files: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test cuped_golden

use experimentation_stats::cuped::cuped_adjust;
use std::path::PathBuf;

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenCuped {
    test_name: String,
    r_command: String,
    control_y: Vec<f64>,
    treatment_y: Vec<f64>,
    control_x: Vec<f64>,
    treatment_x: Vec<f64>,
    alpha: f64,
    expected: GoldenExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenExpected {
    raw_effect: f64,
    adjusted_effect: f64,
    theta: f64,
    raw_se: f64,
    adjusted_se: f64,
    variance_reduction: f64,
    ci_lower: f64,
    ci_upper: f64,
    control_adjusted_mean: f64,
    treatment_adjusted_mean: f64,
}

const TOLERANCE: f64 = 1e-6;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_golden(filename: &str) -> GoldenCuped {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_golden(filename: &str, golden: &GoldenCuped) {
    let path = golden_dir().join(filename);
    let data = serde_json::to_string_pretty(golden).expect("serialization should not fail");
    std::fs::write(&path, data)
        .unwrap_or_else(|e| panic!("Failed to write golden file {}: {e}", path.display()));
}

fn assert_close(actual: f64, expected: f64, field: &str, test_name: &str) {
    let diff = (actual - expected).abs();
    assert!(
        diff < TOLERANCE,
        "[{test_name}] {field}: expected {expected:.15e}, got {actual:.15e}, diff={diff:.15e} > tolerance={TOLERANCE:.0e}"
    );
}

fn run_golden_test(filename: &str) {
    let mut golden = load_golden(filename);
    let result = cuped_adjust(
        &golden.control_y,
        &golden.treatment_y,
        &golden.control_x,
        &golden.treatment_x,
        golden.alpha,
    )
    .unwrap_or_else(|e| panic!("[{}] cuped_adjust failed: {e}", golden.test_name));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.expected = GoldenExpected {
            raw_effect: result.raw_effect,
            adjusted_effect: result.adjusted_effect,
            theta: result.theta,
            raw_se: result.raw_se,
            adjusted_se: result.adjusted_se,
            variance_reduction: result.variance_reduction,
            ci_lower: result.ci_lower,
            ci_upper: result.ci_upper,
            control_adjusted_mean: result.control_adjusted_mean,
            treatment_adjusted_mean: result.treatment_adjusted_mean,
        };
        update_golden(filename, &golden);
        eprintln!("[{}] Updated golden file: {}", golden.test_name, filename);
        return;
    }

    let expected = &golden.expected;
    let name = &golden.test_name;

    assert_close(result.raw_effect, expected.raw_effect, "raw_effect", name);
    assert_close(result.adjusted_effect, expected.adjusted_effect, "adjusted_effect", name);
    assert_close(result.theta, expected.theta, "theta", name);
    assert_close(result.raw_se, expected.raw_se, "raw_se", name);
    assert_close(result.adjusted_se, expected.adjusted_se, "adjusted_se", name);
    assert_close(result.variance_reduction, expected.variance_reduction, "variance_reduction", name);
    assert_close(result.ci_lower, expected.ci_lower, "ci_lower", name);
    assert_close(result.ci_upper, expected.ci_upper, "ci_upper", name);
    assert_close(result.control_adjusted_mean, expected.control_adjusted_mean, "control_adjusted_mean", name);
    assert_close(result.treatment_adjusted_mean, expected.treatment_adjusted_mean, "treatment_adjusted_mean", name);
}

#[test]
fn golden_cuped_high_correlation() {
    run_golden_test("cuped_high_correlation.json");
}

#[test]
fn golden_cuped_low_correlation() {
    run_golden_test("cuped_low_correlation.json");
}

#[test]
fn golden_cuped_near_perfect_correlation() {
    run_golden_test("cuped_near_perfect_correlation.json");
}

#[test]
fn golden_cuped_unequal_groups() {
    run_golden_test("cuped_unequal_groups.json");
}

#[test]
fn golden_cuped_negative_correlation() {
    run_golden_test("cuped_negative_correlation.json");
}

/// Verify that high-correlation covariates produce significant variance reduction.
#[test]
fn cuped_variance_reduction_threshold() {
    let golden = load_golden("cuped_high_correlation.json");
    let result = cuped_adjust(
        &golden.control_y,
        &golden.treatment_y,
        &golden.control_x,
        &golden.treatment_x,
        golden.alpha,
    )
    .unwrap();

    assert!(
        result.variance_reduction > 0.30,
        "High-correlation CUPED should reduce variance by >30%, got {:.4}",
        result.variance_reduction
    );
    assert!(
        result.adjusted_se < result.raw_se,
        "Adjusted SE ({}) should be smaller than raw SE ({})",
        result.adjusted_se,
        result.raw_se
    );
}
