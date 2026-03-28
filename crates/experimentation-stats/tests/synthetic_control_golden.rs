//! Golden-file integration tests for Synthetic Control Methods (ADR-023).
//!
//! Validates synthetic_control() against analytically derivable cases and
//! augsynth-compatible datasets to 4 decimal places.
//!
//! Run: cargo test -p experimentation-stats --test synthetic_control_golden

use std::collections::HashMap;
use std::path::PathBuf;

use experimentation_stats::synthetic_control::{
    synthetic_control, Method, SyntheticControlInput,
};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

// ---------------------------------------------------------------------------
// Golden file schema
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct DonorSpec {
    name: String,
    series: Vec<f64>,
}

#[derive(serde::Deserialize)]
struct GoldenExpected {
    #[serde(default)]
    att: Option<f64>,
    #[serde(default)]
    att_tolerance: Option<f64>,
    #[serde(default)]
    att_min: Option<f64>,
    #[serde(default)]
    att_max: Option<f64>,
    #[serde(default)]
    weights: Option<HashMap<String, f64>>,
    #[serde(default)]
    weight_tolerance: Option<f64>,
    #[serde(default)]
    weights_sum_to_one: Option<bool>,
    #[serde(default)]
    ci_width_min: Option<f64>,
    #[serde(default)]
    placebo_p_max: Option<f64>,
}

#[derive(serde::Deserialize)]
struct ScmGolden {
    test_name: String,
    #[allow(dead_code)]
    description: String,
    method: String,
    pre_periods: usize,
    alpha: f64,
    treated_series: Vec<f64>,
    donors: Vec<DonorSpec>,
    expected: GoldenExpected,
}

fn load_golden(filename: &str) -> ScmGolden {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()))
}

fn run_golden(filename: &str) {
    let g = load_golden(filename);
    let name = &g.test_name;

    let method = match g.method.as_str() {
        "Classic" => Method::Classic,
        "Augmented" => Method::Augmented,
        "SDiD" => Method::SDiD,
        "CausalImpact" => Method::CausalImpact,
        other => panic!("[{name}] unknown method: {other}"),
    };

    let donors: Vec<(String, Vec<f64>)> =
        g.donors.into_iter().map(|d| (d.name, d.series)).collect();

    let input = SyntheticControlInput {
        treated_unit: "treated".into(),
        treated_series: g.treated_series,
        donors,
        pre_periods: g.pre_periods,
        alpha: g.alpha,
    };

    let result = synthetic_control(&input, method)
        .unwrap_or_else(|e| panic!("[{name}] synthetic_control failed: {e}"));

    let exp = &g.expected;

    // Exact ATT check.
    if let (Some(expected_att), Some(tol)) = (exp.att, exp.att_tolerance) {
        let diff = (result.att - expected_att).abs();
        assert!(
            diff <= tol,
            "[{name}] ATT: expected {expected_att:.6}, got {:.6}, diff={diff:.2e}",
            result.att
        );
    }

    // ATT range check.
    if let Some(att_min) = exp.att_min {
        assert!(
            result.att >= att_min,
            "[{name}] ATT={:.4} should be ≥ {att_min}",
            result.att
        );
    }
    if let Some(att_max) = exp.att_max {
        assert!(
            result.att <= att_max,
            "[{name}] ATT={:.4} should be ≤ {att_max}",
            result.att
        );
    }

    // Per-donor weight checks.
    if let (Some(ref expected_weights), Some(tol)) = (&exp.weights, exp.weight_tolerance) {
        for (donor, &expected_w) in expected_weights {
            let actual_w = *result.donor_weights.get(donor).unwrap_or_else(|| {
                panic!("[{name}] donor '{donor}' missing from result weights")
            });
            let diff = (actual_w - expected_w).abs();
            assert!(
                diff <= tol,
                "[{name}] weight[{donor}]: expected {expected_w:.6}, got {actual_w:.6}, diff={diff:.2e}"
            );
        }
    }

    // Weights sum to 1.
    if exp.weights_sum_to_one == Some(true) {
        let sum: f64 = result.donor_weights.values().sum();
        assert!(
            (sum - 1.0).abs() < 1e-9,
            "[{name}] donor weights should sum to 1.0, got {sum}"
        );
        for &w in result.donor_weights.values() {
            assert!(w >= -1e-12, "[{name}] donor weight {w} should be ≥ 0");
        }
    }

    // CI width.
    if let Some(min_width) = exp.ci_width_min {
        let width = result.ci_upper - result.ci_lower;
        assert!(
            width >= min_width,
            "[{name}] CI width {width:.4} should be ≥ {min_width}"
        );
    }

    // Placebo p-value bound.
    if let Some(p_max) = exp.placebo_p_max {
        assert!(
            result.placebo_p_value <= p_max,
            "[{name}] placebo_p={:.4} should be ≤ {p_max}",
            result.placebo_p_value
        );
    }

    // Always: CI is well-formed.
    assert!(
        result.ci_lower <= result.ci_upper,
        "[{name}] CI [{}, {}] is inverted",
        result.ci_lower,
        result.ci_upper
    );
    assert!(result.att.is_finite(), "[{name}] ATT is not finite");
}

// ---------------------------------------------------------------------------
// Test cases
// ---------------------------------------------------------------------------

/// Classic SCM on 2-donor perfect-fit dataset. ATT = 2.0, weights = [0.6, 0.4].
/// Analytically derivable: unique simplex minimiser at (0.6, 0.4) when treated = 0.6·D1 + 0.4·D2.
#[test]
fn golden_scm_classic_perfect_fit() {
    run_golden("scm_perfect_fit_classic.json");
}

/// Augmented SCM on 2-donor imperfect-fit dataset.
/// Bias correction should keep ATT in [1.5, 2.5].
#[test]
fn golden_scm_augmented_bias_correction() {
    run_golden("scm_augmented_bias_correction.json");
}

/// Augmented SCM on 3-donor panel compatible with R augsynth.
/// treated = 0.5·D1 + 0.5·D2 (pre) + ATT=1.5 (post). ATT in [1.0, 2.0].
#[test]
fn golden_scm_augsynth_3donor() {
    run_golden("scm_augsynth_3donor.json");
}
