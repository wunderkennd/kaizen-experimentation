//! Golden-file integration tests for TC/JIVE K-fold IV surrogate calibration.
//!
//! Validates kfold_iv_calibrate against Netflix KDD 2024 Table 2 scenarios:
//! - Scenario A: strong surrogate, no confounding  → OLS = JIVE = true γ
//! - Scenario B: strong surrogate, confounded       → OLS biased, JIVE corrects
//! - Scenario C: weak surrogate                     → instrument_strength ≠ Strong
//!
//! Run:    cargo test -p experimentation-stats --test orl_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test orl_golden

use experimentation_stats::orl::{kfold_iv_calibrate, InstrumentStrength, KFoldIvConfig, OrlObservation};
use std::path::PathBuf;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

// ---------------------------------------------------------------------------
// Shared structs for golden file deserialization
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct OrlGolden {
    test_name: String,
    #[allow(dead_code)]
    description: String,
    n_folds: usize,
    alpha: f64,
    observations: Vec<OrlObservation>,
    expected: OrlExpected,
    tolerance: f64,
}

#[derive(serde::Deserialize)]
struct OrlExpected {
    // Scenario A fields
    #[serde(default)]
    iv_estimate: Option<f64>,
    #[serde(default)]
    ols_estimate: Option<f64>,
    #[serde(default)]
    bias_correction_abs_max: Option<f64>,
    #[serde(default)]
    true_gamma: Option<f64>,
    #[serde(default)]
    ci_contains_truth: Option<bool>,
    #[serde(default)]
    first_stage_f_stat_min: Option<f64>,

    // Scenario B fields
    #[serde(default)]
    ols_biased_up: Option<bool>,
    #[serde(default)]
    jive_closer_to_truth: Option<bool>,

    // Scenario C fields
    #[serde(default)]
    instrument_strength_not_strong: Option<bool>,
    #[serde(default)]
    first_stage_f_stat_max: Option<f64>,

    // Common
    #[serde(default)]
    instrument_strength: Option<String>,
}

fn load_golden(filename: &str) -> OrlGolden {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()))
}

fn run_golden_test(filename: &str) {
    let golden = load_golden(filename);
    let name = &golden.test_name;

    let config = KFoldIvConfig { n_folds: golden.n_folds, alpha: golden.alpha };
    let result = kfold_iv_calibrate(&golden.observations, &config)
        .unwrap_or_else(|e| panic!("[{name}] kfold_iv_calibrate failed: {e}"));

    let tol = golden.tolerance;
    let exp = &golden.expected;

    // --- Exact iv_estimate ---
    if let Some(expected_iv) = exp.iv_estimate {
        let diff = (result.iv_estimate - expected_iv).abs();
        assert!(
            diff <= tol,
            "[{name}] iv_estimate: expected {expected_iv:.15e}, got {:.15e}, diff={diff:.15e}",
            result.iv_estimate
        );
    }

    // --- Exact ols_estimate ---
    if let Some(expected_ols) = exp.ols_estimate {
        let diff = (result.ols_estimate - expected_ols).abs();
        assert!(
            diff <= tol,
            "[{name}] ols_estimate: expected {expected_ols:.15e}, got {:.15e}, diff={diff:.15e}",
            result.ols_estimate
        );
    }

    // --- Bias correction small ---
    if let Some(max_bc) = exp.bias_correction_abs_max {
        assert!(
            result.bias_correction.abs() <= max_bc,
            "[{name}] |bias_correction| should be ≤ {max_bc}: got {}",
            result.bias_correction
        );
    }

    // --- CI contains truth ---
    if let (Some(true), Some(truth)) = (exp.ci_contains_truth, exp.true_gamma) {
        assert!(
            result.ci_lower <= truth && truth <= result.ci_upper,
            "[{name}] CI [{}, {}] should contain γ={truth}",
            result.ci_lower,
            result.ci_upper
        );
    }

    // --- First-stage F minimum ---
    if let Some(f_min) = exp.first_stage_f_stat_min {
        assert!(
            result.first_stage_f_stat >= f_min,
            "[{name}] first_stage_f_stat should be ≥ {f_min}: got {}",
            result.first_stage_f_stat
        );
    }

    // --- First-stage F maximum ---
    if let Some(f_max) = exp.first_stage_f_stat_max {
        assert!(
            result.first_stage_f_stat <= f_max,
            "[{name}] first_stage_f_stat should be ≤ {f_max}: got {}",
            result.first_stage_f_stat
        );
    }

    // --- Instrument strength string ---
    if let Some(ref strength_str) = exp.instrument_strength {
        let actual_str = match result.instrument_strength {
            InstrumentStrength::Strong => "Strong",
            InstrumentStrength::Moderate => "Moderate",
            InstrumentStrength::Weak => "Weak",
        };
        assert_eq!(
            actual_str, strength_str.as_str(),
            "[{name}] instrument_strength: expected {strength_str}, got {actual_str}"
        );
    }

    // --- OLS biased up (Scenario B) ---
    if exp.ols_biased_up == Some(true) {
        assert!(
            result.ols_estimate > result.iv_estimate,
            "[{name}] OLS should be biased upward vs JIVE: OLS={:.4} IV={:.4}",
            result.ols_estimate,
            result.iv_estimate
        );
    }

    // --- JIVE closer to truth (Scenario B) ---
    if let (Some(true), Some(truth)) = (exp.jive_closer_to_truth, exp.true_gamma) {
        let jive_err = (result.iv_estimate - truth).abs();
        let ols_err = (result.ols_estimate - truth).abs();
        assert!(
            jive_err < ols_err,
            "[{name}] JIVE ({:.4}) should be closer to γ={truth} than OLS ({:.4}): JIVE_err={jive_err:.4} OLS_err={ols_err:.4}",
            result.iv_estimate,
            result.ols_estimate
        );
    }

    // --- Instrument not strong (Scenario C) ---
    if exp.instrument_strength_not_strong == Some(true) {
        assert_ne!(
            result.instrument_strength,
            InstrumentStrength::Strong,
            "[{name}] instrument should not be Strong (F={})",
            result.first_stage_f_stat
        );
    }
}

// ---------------------------------------------------------------------------
// Test cases — Netflix KDD 2024 Table 2 scenarios
// ---------------------------------------------------------------------------

/// Scenario A: strong surrogate, no confounding. JIVE = OLS = true γ = 0.3.
#[test]
fn golden_orl_kdd2024_no_confounding() {
    run_golden_test("orl_kdd2024_no_confounding.json");
}

/// Scenario B: strong surrogate, positive confounding. OLS biased up; JIVE corrects.
#[test]
fn golden_orl_kdd2024_confounded() {
    run_golden_test("orl_kdd2024_confounded.json");
}

/// Scenario C: weak surrogate. First-stage F below Stock-Yogo threshold.
#[test]
fn golden_orl_kdd2024_weak_instrument() {
    run_golden_test("orl_kdd2024_weak_instrument.json");
}
