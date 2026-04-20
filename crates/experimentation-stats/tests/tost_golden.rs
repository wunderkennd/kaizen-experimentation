//! Golden-file integration tests for TOST equivalence testing (ADR-027).
//!
//! Each fixture encodes a precise R command (TOSTER::t_TOST with var.equal = FALSE)
//! whose output is the canonical reference. Because both `tost_equivalence_test`
//! and TOSTER share the Welch t-test internals, the numeric outputs agree to
//! well beyond the 6-decimal tolerance this harness enforces.
//!
//! Run: `cargo test -p experimentation-stats --test tost_golden`
//! Update goldens: `UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test tost_golden`

use experimentation_stats::tost::{
    tost_cuped_equivalence_test, tost_equivalence_test, TostConfig,
};
use std::path::PathBuf;

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenTost {
    test_name: String,
    r_command: String,
    mode: String, // "tost" or "tost_cuped"
    control: Vec<f64>,
    treatment: Vec<f64>,
    #[serde(default)]
    control_x: Vec<f64>,
    #[serde(default)]
    treatment_x: Vec<f64>,
    delta: f64,
    alpha: f64,
    expected: GoldenExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenExpected {
    point_estimate: f64,
    std_error: f64,
    df: f64,
    p_lower: f64,
    p_upper: f64,
    p_tost: f64,
    ci_lower: f64,
    ci_upper: f64,
    equivalent: bool,
    control_mean: f64,
    treatment_mean: f64,
}

const TOLERANCE: f64 = 1e-6;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_golden(filename: &str) -> GoldenTost {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_golden(filename: &str, golden: &GoldenTost) {
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

fn run_golden(filename: &str) {
    let mut golden = load_golden(filename);
    let cfg = TostConfig {
        delta: golden.delta,
        alpha: golden.alpha,
    };

    let result = match golden.mode.as_str() {
        "tost" => tost_equivalence_test(&golden.control, &golden.treatment, &cfg)
            .unwrap_or_else(|e| panic!("[{}] tost_equivalence_test failed: {e}", golden.test_name)),
        "tost_cuped" => tost_cuped_equivalence_test(
            &golden.control,
            &golden.treatment,
            &golden.control_x,
            &golden.treatment_x,
            &cfg,
        )
        .unwrap_or_else(|e| panic!("[{}] tost_cuped_equivalence_test failed: {e}", golden.test_name)),
        other => panic!("[{}] unknown mode: {other}", golden.test_name),
    };

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.expected = GoldenExpected {
            point_estimate: result.point_estimate,
            std_error: result.std_error,
            df: result.df,
            p_lower: result.p_lower,
            p_upper: result.p_upper,
            p_tost: result.p_tost,
            ci_lower: result.ci_lower,
            ci_upper: result.ci_upper,
            equivalent: result.equivalent,
            control_mean: result.control_mean,
            treatment_mean: result.treatment_mean,
        };
        update_golden(filename, &golden);
        eprintln!("[{}] Updated golden file: {}", golden.test_name, filename);
        return;
    }

    let e = &golden.expected;
    let name = &golden.test_name;
    assert_close(result.point_estimate, e.point_estimate, "point_estimate", name);
    assert_close(result.std_error, e.std_error, "std_error", name);
    assert_close(result.df, e.df, "df", name);
    assert_close(result.p_lower, e.p_lower, "p_lower", name);
    assert_close(result.p_upper, e.p_upper, "p_upper", name);
    assert_close(result.p_tost, e.p_tost, "p_tost", name);
    assert_close(result.ci_lower, e.ci_lower, "ci_lower", name);
    assert_close(result.ci_upper, e.ci_upper, "ci_upper", name);
    assert_close(result.control_mean, e.control_mean, "control_mean", name);
    assert_close(result.treatment_mean, e.treatment_mean, "treatment_mean", name);
    assert_eq!(
        result.equivalent, e.equivalent,
        "[{name}] equivalent: expected {}, got {}",
        e.equivalent, result.equivalent
    );
}

#[test]
fn golden_tost_clear_equivalence() {
    run_golden("tost_clear_equivalence.json");
}

#[test]
fn golden_tost_not_equivalent() {
    run_golden("tost_not_equivalent.json");
}

#[test]
fn golden_tost_narrow_margin_equivalence() {
    run_golden("tost_narrow_margin_equivalence.json");
}

#[test]
fn golden_tost_unequal_n_unequal_variance() {
    run_golden("tost_unequal_n_unequal_variance.json");
}

#[test]
fn golden_tost_cuped_high_correlation() {
    run_golden("tost_cuped_high_correlation.json");
}
