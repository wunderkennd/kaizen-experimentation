//! Golden-file integration tests for Bayesian analysis.
//!
//! Beta-Binomial: validated against R qbeta() + Monte Carlo P(B1 > B2).
//! Normal-Normal: validated against R pnorm() closed-form.
//!
//! Run: cargo test -p experimentation-stats --test bayesian_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test bayesian_golden

use experimentation_stats::bayesian::{bayesian_beta_binomial, bayesian_normal, BayesianModel};
use std::path::PathBuf;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

// ---------------------------------------------------------------------------
// Beta-Binomial golden tests
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct BetaBinomialGolden {
    test_name: String,
    control_successes: u64,
    control_total: u64,
    treatment_successes: u64,
    treatment_total: u64,
    credible_level: f64,
    seed: u64,
    expected: BetaBinomialExpected,
    tolerance_normal: f64,
    tolerance_mc: f64,
}

#[derive(serde::Deserialize)]
struct BetaBinomialExpected {
    posterior_mean_control: f64,
    posterior_mean_treatment: f64,
    effect: f64,
    probability_of_superiority: f64,
    model: String,
}

fn run_beta_binomial_golden(filename: &str) {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
    let golden: BetaBinomialGolden = serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));

    let result = bayesian_beta_binomial(
        golden.control_successes,
        golden.control_total,
        golden.treatment_successes,
        golden.treatment_total,
        golden.credible_level,
        golden.seed,
    )
    .unwrap_or_else(|e| panic!("[{}] bayesian_beta_binomial failed: {e}", golden.test_name));

    let name = &golden.test_name;
    let tol = golden.tolerance_normal;
    let mc_tol = golden.tolerance_mc;

    assert_eq!(result.model, BayesianModel::BetaBinomial, "[{name}] model");

    assert!(
        (result.posterior_mean_control - golden.expected.posterior_mean_control).abs() < tol,
        "[{name}] posterior_mean_control: expected {}, got {}",
        golden.expected.posterior_mean_control,
        result.posterior_mean_control
    );

    assert!(
        (result.posterior_mean_treatment - golden.expected.posterior_mean_treatment).abs() < tol,
        "[{name}] posterior_mean_treatment: expected {}, got {}",
        golden.expected.posterior_mean_treatment,
        result.posterior_mean_treatment
    );

    assert!(
        (result.effect - golden.expected.effect).abs() < tol,
        "[{name}] effect: expected {}, got {}",
        golden.expected.effect,
        result.effect
    );

    assert!(
        (result.probability_of_superiority - golden.expected.probability_of_superiority).abs()
            < mc_tol,
        "[{name}] P(superiority): expected {}, got {} (MC tolerance={})",
        golden.expected.probability_of_superiority,
        result.probability_of_superiority,
        mc_tol
    );

    // Credible interval sanity.
    assert!(
        result.credible_lower <= result.credible_upper,
        "[{name}] credible_lower ({}) > credible_upper ({})",
        result.credible_lower,
        result.credible_upper
    );
}

#[test]
fn golden_bayesian_beta_binomial_clear_effect() {
    run_beta_binomial_golden("bayesian_beta_binomial_clear_effect.json");
}

#[test]
fn golden_bayesian_beta_binomial_no_effect() {
    run_beta_binomial_golden("bayesian_beta_binomial_no_effect.json");
}

// ---------------------------------------------------------------------------
// Normal-Normal golden tests
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct NormalGolden {
    test_name: String,
    control: Vec<f64>,
    treatment: Vec<f64>,
    credible_level: f64,
    expected: NormalExpected,
}

#[derive(serde::Deserialize)]
struct NormalExpected {
    #[serde(default)]
    probability_of_superiority_min: Option<f64>,
    #[serde(default)]
    probability_of_superiority_range: Option<[f64; 2]>,
    #[serde(default)]
    effect_sign: Option<String>,
    #[serde(default)]
    credible_lower_positive: Option<bool>,
    #[serde(default)]
    credible_lower_negative: Option<bool>,
    #[serde(default)]
    credible_upper_positive: Option<bool>,
    model: String,
}

fn run_normal_golden(filename: &str) {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
    let golden: NormalGolden = serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));

    let result = bayesian_normal(&golden.control, &golden.treatment, golden.credible_level)
        .unwrap_or_else(|e| panic!("[{}] bayesian_normal failed: {e}", golden.test_name));

    let name = &golden.test_name;

    assert_eq!(result.model, BayesianModel::NormalNormal, "[{name}] model");

    if let Some(min_prob) = golden.expected.probability_of_superiority_min {
        assert!(
            result.probability_of_superiority >= min_prob,
            "[{name}] P(superiority) {} < min {}",
            result.probability_of_superiority,
            min_prob
        );
    }

    if let Some([lo, hi]) = golden.expected.probability_of_superiority_range {
        assert!(
            result.probability_of_superiority >= lo && result.probability_of_superiority <= hi,
            "[{name}] P(superiority) {} not in [{lo}, {hi}]",
            result.probability_of_superiority
        );
    }

    if let Some(ref sign) = golden.expected.effect_sign {
        match sign.as_str() {
            "positive" => assert!(result.effect > 0.0, "[{name}] effect should be positive"),
            "negative" => assert!(result.effect < 0.0, "[{name}] effect should be negative"),
            _ => {}
        }
    }

    if golden.expected.credible_lower_positive == Some(true) {
        assert!(
            result.credible_lower > 0.0,
            "[{name}] credible_lower should be positive, got {}",
            result.credible_lower
        );
    }

    if golden.expected.credible_lower_negative == Some(true) {
        assert!(
            result.credible_lower < 0.0,
            "[{name}] credible_lower should be negative, got {}",
            result.credible_lower
        );
    }

    if golden.expected.credible_upper_positive == Some(true) {
        assert!(
            result.credible_upper > 0.0,
            "[{name}] credible_upper should be positive, got {}",
            result.credible_upper
        );
    }

    assert!(
        result.credible_lower <= result.credible_upper,
        "[{name}] credible_lower ({}) > credible_upper ({})",
        result.credible_lower,
        result.credible_upper
    );
}

#[test]
fn golden_bayesian_normal_clear_effect() {
    run_normal_golden("bayesian_normal_clear_effect.json");
}

#[test]
fn golden_bayesian_normal_no_effect() {
    run_normal_golden("bayesian_normal_no_effect.json");
}
