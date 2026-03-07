//! Golden-file integration tests for surrogate metric validation.
//!
//! Tests validate_calibration, adjust_projection, backtest_surrogate,
//! and linear_projection against manually computed reference values.
//!
//! Run: cargo test -p experimentation-stats --test surrogate_golden
//! Update golden files: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test surrogate_golden

use experimentation_stats::surrogate::*;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Golden file data structures
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenSurrogate {
    test_name: String,
    description: String,
    calibration: GoldenCalibration,
    backtest: GoldenBacktest,
    projection: GoldenProjection,
    #[serde(skip_serializing_if = "Option::is_none")]
    linear_projection: Option<GoldenLinearProjection>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenCalibration {
    points: Vec<CalibrationPoint>,
    expected: GoldenCalibrationExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenCalibrationExpected {
    #[serde(skip_serializing_if = "Option::is_none")]
    r_squared: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rmse: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mean_bias: Option<f64>,
    badge: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenBacktest {
    points: Vec<BacktestPoint>,
    expected: GoldenBacktestExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenBacktestExpected {
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mae: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    within_25_pct: Option<f64>,
    passes_acceptance: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenProjection {
    input: ProjectionInput,
    alpha: f64,
    expected: GoldenProjectionExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenProjectionExpected {
    #[serde(skip_serializing_if = "Option::is_none")]
    adjusted_effect: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ci_lower: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ci_upper: Option<f64>,
    badge: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ci_inflation_factor: Option<f64>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenLinearProjection {
    effects: Vec<f64>,
    coefficients: Vec<f64>,
    intercept: f64,
    effect_ses: Vec<f64>,
    expected: GoldenLinearExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenLinearExpected {
    projected_effect: f64,
    projected_se: f64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const TOLERANCE: f64 = 1e-6;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_golden(filename: &str) -> GoldenSurrogate {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_golden(filename: &str, golden: &GoldenSurrogate) {
    let path = golden_dir().join(filename);
    let data = serde_json::to_string_pretty(golden).expect("serialization should not fail");
    std::fs::write(&path, data)
        .unwrap_or_else(|e| panic!("Failed to write golden file {}: {e}", path.display()));
}

fn assert_close(actual: f64, expected: f64, field: &str, test_name: &str) {
    let diff = (actual - expected).abs();
    assert!(
        diff < TOLERANCE,
        "[{test_name}] {field}: expected {expected:.15e}, got {actual:.15e}, diff={diff:.15e} > tol={TOLERANCE:.0e}"
    );
}

fn badge_str(badge: ConfidenceBadge) -> &'static str {
    match badge {
        ConfidenceBadge::Green => "Green",
        ConfidenceBadge::Yellow => "Yellow",
        ConfidenceBadge::Red => "Red",
    }
}

fn run_golden_test(filename: &str) {
    let mut golden = load_golden(filename);
    let name = golden.test_name.clone();

    // --- Calibration ---
    let cal_result = validate_calibration(&golden.calibration.points)
        .unwrap_or_else(|e| panic!("[{name}] validate_calibration failed: {e}"));

    // --- Backtest ---
    let bt_result = backtest_surrogate(&golden.backtest.points)
        .unwrap_or_else(|e| panic!("[{name}] backtest_surrogate failed: {e}"));

    // --- Projection (with calibration) ---
    let proj_result = adjust_projection(
        &golden.projection.input,
        Some(&cal_result),
        golden.projection.alpha,
    )
    .unwrap_or_else(|e| panic!("[{name}] adjust_projection failed: {e}"));

    // --- Linear projection (if present) ---
    let lin_result = golden.linear_projection.as_ref().map(|lp| {
        linear_projection(&lp.effects, &lp.coefficients, lp.intercept, &lp.effect_ses)
            .unwrap_or_else(|e| panic!("[{name}] linear_projection failed: {e}"))
    });

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.calibration.expected = GoldenCalibrationExpected {
            r_squared: Some(cal_result.r_squared),
            rmse: Some(cal_result.rmse),
            mean_bias: Some(cal_result.mean_bias),
            badge: badge_str(cal_result.badge).to_string(),
        };
        golden.backtest.expected = GoldenBacktestExpected {
            coverage_rate: Some(bt_result.coverage_rate),
            mae: Some(bt_result.mae),
            within_25_pct: Some(bt_result.within_25_pct),
            passes_acceptance: bt_result.passes_acceptance,
        };
        golden.projection.expected = GoldenProjectionExpected {
            adjusted_effect: Some(proj_result.adjusted_effect),
            ci_lower: Some(proj_result.ci_lower),
            ci_upper: Some(proj_result.ci_upper),
            badge: badge_str(proj_result.badge).to_string(),
            ci_inflation_factor: Some(proj_result.ci_inflation_factor),
        };
        if let (Some(lp), Some(lr)) = (golden.linear_projection.as_mut(), &lin_result) {
            lp.expected = GoldenLinearExpected {
                projected_effect: lr.projected_effect,
                projected_se: lr.projected_se,
            };
        }
        update_golden(filename, &golden);
        eprintln!("[{name}] Updated golden file: {filename}");
        return;
    }

    // --- Verify calibration ---
    let cal_exp = &golden.calibration.expected;
    assert_eq!(
        badge_str(cal_result.badge), cal_exp.badge,
        "[{name}] calibration badge: expected {}, got {}",
        cal_exp.badge, badge_str(cal_result.badge)
    );
    if let Some(expected_r2) = cal_exp.r_squared {
        assert_close(cal_result.r_squared, expected_r2, "r_squared", &name);
    }
    if let Some(expected_rmse) = cal_exp.rmse {
        assert_close(cal_result.rmse, expected_rmse, "rmse", &name);
    }
    if let Some(expected_bias) = cal_exp.mean_bias {
        assert_close(cal_result.mean_bias, expected_bias, "mean_bias", &name);
    }

    // --- Verify backtest ---
    let bt_exp = &golden.backtest.expected;
    assert_eq!(
        bt_result.passes_acceptance, bt_exp.passes_acceptance,
        "[{name}] passes_acceptance: expected {}, got {}",
        bt_exp.passes_acceptance, bt_result.passes_acceptance
    );
    if let Some(expected_cov) = bt_exp.coverage_rate {
        assert_close(bt_result.coverage_rate, expected_cov, "coverage_rate", &name);
    }
    if let Some(expected_mae) = bt_exp.mae {
        assert_close(bt_result.mae, expected_mae, "mae", &name);
    }
    if let Some(expected_w25) = bt_exp.within_25_pct {
        assert_close(bt_result.within_25_pct, expected_w25, "within_25_pct", &name);
    }

    // --- Verify projection ---
    let proj_exp = &golden.projection.expected;
    assert_eq!(
        badge_str(proj_result.badge), proj_exp.badge,
        "[{name}] projection badge: expected {}, got {}",
        proj_exp.badge, badge_str(proj_result.badge)
    );
    if let Some(adj_eff) = proj_exp.adjusted_effect {
        assert_close(proj_result.adjusted_effect, adj_eff, "adjusted_effect", &name);
    }
    if let Some(ci_lo) = proj_exp.ci_lower {
        assert_close(proj_result.ci_lower, ci_lo, "ci_lower", &name);
    }
    if let Some(ci_hi) = proj_exp.ci_upper {
        assert_close(proj_result.ci_upper, ci_hi, "ci_upper", &name);
    }
    if let Some(infl) = proj_exp.ci_inflation_factor {
        assert_close(proj_result.ci_inflation_factor, infl, "ci_inflation_factor", &name);
    }

    // --- Verify linear projection ---
    if let (Some(lp), Some(lr)) = (&golden.linear_projection, &lin_result) {
        assert_close(lr.projected_effect, lp.expected.projected_effect, "linear_projected_effect", &name);
        assert_close(lr.projected_se, lp.expected.projected_se, "linear_projected_se", &name);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn golden_surrogate_well_calibrated() {
    run_golden_test("surrogate_well_calibrated.json");
}

#[test]
fn golden_surrogate_biased_model() {
    run_golden_test("surrogate_biased_model.json");
}

#[test]
fn golden_surrogate_poor_model() {
    run_golden_test("surrogate_poor_model.json");
}
