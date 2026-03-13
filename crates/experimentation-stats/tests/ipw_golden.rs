//! Golden-file integration tests for IPW-adjusted analysis.
//!
//! Validated against R's survey::svymean().
//!
//! Run: cargo test -p experimentation-stats --test ipw_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test ipw_golden

use experimentation_stats::ipw::{ipw_estimate, IpwObservation};
use std::path::PathBuf;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

#[derive(serde::Deserialize)]
struct IpwGolden {
    test_name: String,
    observations: Vec<IpwObservation>,
    alpha: f64,
    min_probability: f64,
    expected: IpwExpected,
    tolerance: f64,
}

#[derive(serde::Deserialize)]
struct IpwExpected {
    #[serde(default)]
    effect: Option<f64>,
    #[serde(default)]
    effect_positive: Option<bool>,
    n_observations: usize,
    n_clipped: Option<usize>,
    #[serde(default)]
    effective_sample_size: Option<f64>,
    #[serde(default)]
    ess_less_than_n: Option<bool>,
    #[serde(default)]
    p_value_significant: Option<bool>,
}

fn run_ipw_golden(filename: &str) {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
    let golden: IpwGolden = serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));

    let result = ipw_estimate(&golden.observations, golden.alpha, golden.min_probability)
        .unwrap_or_else(|e| panic!("[{}] ipw_estimate failed: {e}", golden.test_name));

    let name = &golden.test_name;
    let tol = golden.tolerance;

    // Check n_observations.
    assert_eq!(
        result.n_observations, golden.expected.n_observations,
        "[{name}] n_observations"
    );

    // Check exact effect if specified.
    if let Some(expected_effect) = golden.expected.effect {
        assert!(
            (result.effect - expected_effect).abs() < tol,
            "[{name}] effect: expected {expected_effect}, got {}",
            result.effect
        );
    }

    // Check effect sign.
    if golden.expected.effect_positive == Some(true) {
        assert!(
            result.effect > 0.0,
            "[{name}] effect should be positive, got {}",
            result.effect
        );
    }

    // Check clipping count.
    if let Some(expected_clipped) = golden.expected.n_clipped {
        assert_eq!(result.n_clipped, expected_clipped, "[{name}] n_clipped");
    }

    // Check ESS.
    if let Some(expected_ess) = golden.expected.effective_sample_size {
        assert!(
            (result.effective_sample_size - expected_ess).abs() < tol,
            "[{name}] ESS: expected {expected_ess}, got {}",
            result.effective_sample_size
        );
    }

    if golden.expected.ess_less_than_n == Some(true) {
        assert!(
            result.effective_sample_size < result.n_observations as f64,
            "[{name}] ESS ({}) should be < N ({})",
            result.effective_sample_size,
            result.n_observations
        );
    }

    // Check p-value significance.
    if golden.expected.p_value_significant == Some(true) {
        assert!(
            result.p_value < golden.alpha,
            "[{name}] p_value ({}) should be < alpha ({})",
            result.p_value,
            golden.alpha
        );
    }

    // General sanity checks.
    assert!(
        result.p_value >= 0.0 && result.p_value <= 1.0,
        "[{name}] p_value in [0,1]"
    );
    assert!(result.se >= 0.0, "[{name}] se >= 0");
    assert!(
        result.ci_lower <= result.ci_upper,
        "[{name}] CI: lower ({}) > upper ({})",
        result.ci_lower,
        result.ci_upper
    );
    assert!(
        result.effective_sample_size > 0.0
            && result.effective_sample_size <= result.n_observations as f64,
        "[{name}] ESS ({}) should be in (0, N]",
        result.effective_sample_size
    );
}

#[test]
fn golden_ipw_uniform_assignment() {
    run_ipw_golden("ipw_uniform_assignment.json");
}

#[test]
fn golden_ipw_adaptive_assignment() {
    run_ipw_golden("ipw_adaptive_assignment.json");
}

#[test]
fn golden_ipw_heavy_clipping() {
    run_ipw_golden("ipw_heavy_clipping.json");
}
