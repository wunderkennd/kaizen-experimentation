//! Surrogate metric validation and projection adjustment.
//!
//! Validates surrogate models that predict long-term outcomes from
//! short-term metrics. M3 computes raw projections; this module
//! validates calibration quality and adjusts confidence intervals.
//!
//! # Workflow
//! 1. M3 trains surrogate model (e.g., 7d watch time → 90d churn)
//! 2. M3 computes `SurrogateProjection` per variant
//! 3. **M4a (this module)** validates calibration and adjusts CIs
//! 4. M6 displays observed + projected results with confidence badge
//!
//! See design doc section 7.5 and `proto/common/v1/surrogate.proto`.

use experimentation_core::error::{assert_finite, Error, Result};

/// Confidence badge for surrogate model quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConfidenceBadge {
    /// R² > 0.7 — high confidence in surrogate projections.
    Green,
    /// R² ∈ [0.5, 0.7] — moderate confidence, interpret with caution.
    Yellow,
    /// R² < 0.5 — low confidence, projections are unreliable.
    Red,
}

/// Historical experiment outcome for calibration validation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CalibrationPoint {
    /// Experiment identifier.
    pub experiment_id: String,
    /// Model's predicted effect on the target metric.
    pub predicted_effect: f64,
    /// Actually observed long-term effect.
    pub actual_effect: f64,
}

/// Result of surrogate model calibration validation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CalibrationResult {
    /// R² — proportion of variance explained.
    pub r_squared: f64,
    /// Root mean squared error of predictions.
    pub rmse: f64,
    /// Mean prediction bias (predicted - actual).
    pub mean_bias: f64,
    /// Standard deviation of prediction errors.
    pub error_std: f64,
    /// Number of historical experiments used.
    pub n_experiments: usize,
    /// Confidence badge based on R².
    pub badge: ConfidenceBadge,
    /// Per-experiment prediction errors (predicted - actual).
    pub prediction_errors: Vec<f64>,
}

/// Input for a surrogate projection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectionInput {
    /// Observed short-term treatment effect.
    pub observed_effect: f64,
    /// Standard error of the observed effect.
    pub observed_se: f64,
    /// Model's projected long-term effect (from M3).
    pub projected_effect: f64,
    /// Model's projected CI lower bound (from M3).
    pub projection_ci_lower: f64,
    /// Model's projected CI upper bound (from M3).
    pub projection_ci_upper: f64,
    /// Calibration R² at time of projection.
    pub calibration_r_squared: f64,
}

/// Adjusted surrogate projection with validation-aware CIs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdjustedProjection {
    /// Bias-corrected projected effect.
    pub adjusted_effect: f64,
    /// Adjusted CI lower bound (wider than model-only CI).
    pub ci_lower: f64,
    /// Adjusted CI upper bound (wider than model-only CI).
    pub ci_upper: f64,
    /// Calibration R² (snapshot).
    pub calibration_r_squared: f64,
    /// Confidence badge.
    pub badge: ConfidenceBadge,
    /// CI width inflation factor vs. model-only CI.
    pub ci_inflation_factor: f64,
}

/// Backtesting result for a surrogate model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BacktestResult {
    /// Fraction of actual outcomes within projected 95% CI.
    pub coverage_rate: f64,
    /// Mean absolute error.
    pub mae: f64,
    /// Median absolute percentage error.
    pub median_ape: f64,
    /// Fraction of projections within ±25% of actual.
    pub within_25_pct: f64,
    /// Number of experiments backtested.
    pub n_experiments: usize,
    /// Whether the model passes acceptance criteria.
    pub passes_acceptance: bool,
}

/// Backtest input: model projection vs. observed outcome.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BacktestPoint {
    pub experiment_id: String,
    pub projected_effect: f64,
    pub projection_ci_lower: f64,
    pub projection_ci_upper: f64,
    pub actual_effect: f64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Validate a surrogate model's calibration against historical data.
///
/// Computes R², RMSE, bias, and assigns a confidence badge.
pub fn validate_calibration(points: &[CalibrationPoint]) -> Result<CalibrationResult> {
    if points.is_empty() {
        return Err(Error::Validation(
            "calibration requires at least 1 data point".into(),
        ));
    }

    let n = points.len();

    for (i, p) in points.iter().enumerate() {
        assert_finite(p.predicted_effect, &format!("predicted_effect[{i}]"));
        assert_finite(p.actual_effect, &format!("actual_effect[{i}]"));
    }

    let errors: Vec<f64> = points
        .iter()
        .map(|p| p.predicted_effect - p.actual_effect)
        .collect();

    let mean_bias = errors.iter().sum::<f64>() / n as f64;
    assert_finite(mean_bias, "mean_bias");

    let error_variance = if n > 1 {
        errors.iter().map(|e| (e - mean_bias).powi(2)).sum::<f64>() / (n - 1) as f64
    } else {
        0.0
    };
    assert_finite(error_variance, "error_variance");
    let error_std = error_variance.sqrt();

    let mse = errors.iter().map(|e| e * e).sum::<f64>() / n as f64;
    assert_finite(mse, "mse");
    let rmse = mse.sqrt();

    // R²: 1 - SS_res / SS_tot
    let actual_mean = points.iter().map(|p| p.actual_effect).sum::<f64>() / n as f64;
    assert_finite(actual_mean, "actual_mean");

    let ss_res: f64 = points
        .iter()
        .map(|p| (p.actual_effect - p.predicted_effect).powi(2))
        .sum();
    let ss_tot: f64 = points
        .iter()
        .map(|p| (p.actual_effect - actual_mean).powi(2))
        .sum();

    let r_squared = if ss_tot > 1e-30 {
        1.0 - ss_res / ss_tot
    } else {
        // All actual values are identical — model can't explain variance.
        if ss_res < 1e-30 { 1.0 } else { 0.0 }
    };
    assert_finite(r_squared, "r_squared");

    let badge = r_squared_to_badge(r_squared);

    Ok(CalibrationResult {
        r_squared,
        rmse,
        mean_bias,
        error_std,
        n_experiments: n,
        badge,
        prediction_errors: errors,
    })
}

/// Adjust a surrogate projection's CIs based on calibration uncertainty.
///
/// Widens the model's CI to account for calibration error, and applies
/// bias correction if calibration data is available.
pub fn adjust_projection(
    input: &ProjectionInput,
    calibration: Option<&CalibrationResult>,
    alpha: f64,
) -> Result<AdjustedProjection> {
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }

    assert_finite(input.observed_effect, "observed_effect");
    assert_finite(input.observed_se, "observed_se");
    assert_finite(input.projected_effect, "projected_effect");
    assert_finite(input.projection_ci_lower, "projection_ci_lower");
    assert_finite(input.projection_ci_upper, "projection_ci_upper");

    let z = normal_quantile(1.0 - alpha / 2.0);

    // Model CI half-width.
    let model_hw = (input.projection_ci_upper - input.projection_ci_lower) / 2.0;
    assert_finite(model_hw, "model_ci_halfwidth");

    // Apply bias correction and CI inflation if calibration data available.
    let (adjusted_effect, additional_var) = if let Some(cal) = calibration {
        // Debias: subtract mean prediction bias.
        let debiased = input.projected_effect - cal.mean_bias;
        assert_finite(debiased, "debiased_effect");

        // Additional variance from calibration uncertainty.
        let cal_var = cal.error_std.powi(2);
        assert_finite(cal_var, "calibration_variance");

        (debiased, cal_var)
    } else {
        (input.projected_effect, 0.0)
    };

    // Combined CI: sqrt(model_var + calibration_var).
    let model_var = if model_hw > 0.0 {
        (model_hw / z).powi(2)
    } else {
        input.observed_se.powi(2)
    };
    assert_finite(model_var, "model_variance");

    let total_se = (model_var + additional_var).sqrt();
    assert_finite(total_se, "total_se");

    let ci_lower = adjusted_effect - z * total_se;
    let ci_upper = adjusted_effect + z * total_se;

    let adjusted_hw = z * total_se;
    let ci_inflation = if model_hw > 1e-30 {
        adjusted_hw / model_hw
    } else {
        1.0
    };

    let badge = r_squared_to_badge(input.calibration_r_squared);

    Ok(AdjustedProjection {
        adjusted_effect,
        ci_lower,
        ci_upper,
        calibration_r_squared: input.calibration_r_squared,
        badge,
        ci_inflation_factor: ci_inflation,
    })
}

/// Backtest a surrogate model against experiments with known outcomes.
///
/// Checks CI coverage, MAE, and whether projections fall within ±25%
/// of actual outcomes (acceptance criterion from design doc).
pub fn backtest_surrogate(points: &[BacktestPoint]) -> Result<BacktestResult> {
    if points.is_empty() {
        return Err(Error::Validation(
            "backtest requires at least 1 data point".into(),
        ));
    }

    let n = points.len();

    for (i, p) in points.iter().enumerate() {
        assert_finite(p.projected_effect, &format!("bt_projected[{i}]"));
        assert_finite(p.actual_effect, &format!("bt_actual[{i}]"));
        assert_finite(p.projection_ci_lower, &format!("bt_ci_lower[{i}]"));
        assert_finite(p.projection_ci_upper, &format!("bt_ci_upper[{i}]"));
    }

    // CI coverage: fraction of actuals within projected CI.
    let covered = points
        .iter()
        .filter(|p| p.actual_effect >= p.projection_ci_lower && p.actual_effect <= p.projection_ci_upper)
        .count();
    let coverage_rate = covered as f64 / n as f64;
    assert_finite(coverage_rate, "coverage_rate");

    // MAE.
    let mae: f64 = points
        .iter()
        .map(|p| (p.projected_effect - p.actual_effect).abs())
        .sum::<f64>()
        / n as f64;
    assert_finite(mae, "mae");

    // Median absolute percentage error (skip zero actuals).
    let mut apes: Vec<f64> = points
        .iter()
        .filter(|p| p.actual_effect.abs() > 1e-10)
        .map(|p| ((p.projected_effect - p.actual_effect) / p.actual_effect).abs())
        .collect();
    apes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_ape = if apes.is_empty() {
        0.0
    } else if apes.len().is_multiple_of(2) {
        (apes[apes.len() / 2 - 1] + apes[apes.len() / 2]) / 2.0
    } else {
        apes[apes.len() / 2]
    };
    assert_finite(median_ape, "median_ape");

    // Within ±25% of actual.
    let within_25 = points
        .iter()
        .filter(|p| {
            if p.actual_effect.abs() < 1e-10 {
                // For near-zero actuals, check absolute closeness.
                (p.projected_effect - p.actual_effect).abs() < 0.25
            } else {
                ((p.projected_effect - p.actual_effect) / p.actual_effect).abs() < 0.25
            }
        })
        .count();
    let within_25_pct = within_25 as f64 / n as f64;

    // Acceptance: coverage ≥ 0.85 AND within_25_pct ≥ 0.7
    let passes = coverage_rate >= 0.85 && within_25_pct >= 0.70;

    Ok(BacktestResult {
        coverage_rate,
        mae,
        median_ape,
        within_25_pct,
        n_experiments: n,
        passes_acceptance: passes,
    })
}

/// Compute a linear surrogate projection from short-term effects.
///
/// For a linear surrogate model: projected = intercept + sum(coefficients * effects).
pub fn linear_projection(
    effects: &[f64],
    coefficients: &[f64],
    intercept: f64,
    effect_ses: &[f64],
) -> Result<LinearProjectionResult> {
    if effects.len() != coefficients.len() || effects.len() != effect_ses.len() {
        return Err(Error::Validation(
            "effects, coefficients, and effect_ses must have the same length".into(),
        ));
    }
    if effects.is_empty() {
        return Err(Error::Validation(
            "need at least 1 input metric".into(),
        ));
    }

    assert_finite(intercept, "intercept");

    let mut projected = intercept;
    let mut projected_var = 0.0;

    for (i, ((&eff, &coef), &se)) in effects
        .iter()
        .zip(coefficients.iter())
        .zip(effect_ses.iter())
        .enumerate()
    {
        assert_finite(eff, &format!("effect[{i}]"));
        assert_finite(coef, &format!("coefficient[{i}]"));
        assert_finite(se, &format!("effect_se[{i}]"));

        projected += coef * eff;
        // Variance propagation (delta method): Var(coef * X) = coef² * Var(X)
        projected_var += coef.powi(2) * se.powi(2);
    }
    assert_finite(projected, "projected_effect");
    assert_finite(projected_var, "projected_variance");

    let projected_se = projected_var.sqrt();

    Ok(LinearProjectionResult {
        projected_effect: projected,
        projected_se,
    })
}

/// Result of a linear surrogate projection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LinearProjectionResult {
    pub projected_effect: f64,
    pub projected_se: f64,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn r_squared_to_badge(r_squared: f64) -> ConfidenceBadge {
    if r_squared > 0.7 {
        ConfidenceBadge::Green
    } else if r_squared >= 0.5 {
        ConfidenceBadge::Yellow
    } else {
        ConfidenceBadge::Red
    }
}

fn normal_quantile(p: f64) -> f64 {
    use statrs::distribution::{ContinuousCDF, Normal};
    let n = Normal::new(0.0, 1.0).unwrap();
    n.inverse_cdf(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cal_points(
        predicted: &[f64],
        actual: &[f64],
    ) -> Vec<CalibrationPoint> {
        predicted
            .iter()
            .zip(actual.iter())
            .enumerate()
            .map(|(i, (&p, &a))| CalibrationPoint {
                experiment_id: format!("exp_{i}"),
                predicted_effect: p,
                actual_effect: a,
            })
            .collect()
    }

    #[test]
    fn test_calibration_perfect_model() {
        let points = make_cal_points(&[1.0, 2.0, 3.0, 4.0, 5.0], &[1.0, 2.0, 3.0, 4.0, 5.0]);
        let result = validate_calibration(&points).unwrap();
        assert!((result.r_squared - 1.0).abs() < 1e-10);
        assert!(result.rmse < 1e-10);
        assert!(result.mean_bias.abs() < 1e-10);
        assert_eq!(result.badge, ConfidenceBadge::Green);
    }

    #[test]
    fn test_calibration_biased_model() {
        // Model consistently over-predicts by 0.5.
        let points = make_cal_points(&[1.5, 2.5, 3.5, 4.5, 5.5], &[1.0, 2.0, 3.0, 4.0, 5.0]);
        let result = validate_calibration(&points).unwrap();
        assert!((result.mean_bias - 0.5).abs() < 1e-10);
        // R² = 1 - SS_res/SS_tot = 1 - 1.25/10 = 0.875 (bias reduces R²).
        assert!(result.r_squared > 0.8, "R² should be > 0.8 for biased but correlated model: {}", result.r_squared);
    }

    #[test]
    fn test_calibration_poor_model() {
        // Random predictions unrelated to actual.
        let points = make_cal_points(&[5.0, 1.0, 4.0, 2.0, 3.0], &[1.0, 2.0, 3.0, 4.0, 5.0]);
        let result = validate_calibration(&points).unwrap();
        assert!(result.r_squared < 0.5);
        assert_eq!(result.badge, ConfidenceBadge::Red);
    }

    #[test]
    fn test_calibration_badges() {
        // High R²
        let points = make_cal_points(&[1.0, 2.0, 3.0], &[1.1, 2.05, 2.95]);
        let result = validate_calibration(&points).unwrap();
        assert_eq!(result.badge, ConfidenceBadge::Green);

        // Single point
        let points = make_cal_points(&[1.0], &[1.0]);
        let result = validate_calibration(&points).unwrap();
        // Single point with exact match → R² = 1 or degenerate.
        assert!(result.r_squared >= 0.0);
    }

    #[test]
    fn test_calibration_empty() {
        assert!(validate_calibration(&[]).is_err());
    }

    #[test]
    fn test_adjust_projection_no_calibration() {
        let input = ProjectionInput {
            observed_effect: 0.5,
            observed_se: 0.1,
            projected_effect: 0.3,
            projection_ci_lower: 0.1,
            projection_ci_upper: 0.5,
            calibration_r_squared: 0.8,
        };
        let result = adjust_projection(&input, None, 0.05).unwrap();
        assert_eq!(result.badge, ConfidenceBadge::Green);
        // Without calibration, CI should be approximately the same.
        assert!((result.ci_inflation_factor - 1.0).abs() < 0.01);
        assert!((result.adjusted_effect - 0.3).abs() < 1e-10);
    }

    #[test]
    fn test_adjust_projection_with_calibration() {
        let cal_points = make_cal_points(
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &[0.8, 1.6, 2.4, 3.2, 4.0],
        );
        let calibration = validate_calibration(&cal_points).unwrap();

        let input = ProjectionInput {
            observed_effect: 0.5,
            observed_se: 0.1,
            projected_effect: 0.3,
            projection_ci_lower: 0.1,
            projection_ci_upper: 0.5,
            calibration_r_squared: 0.8,
        };
        let result = adjust_projection(&input, Some(&calibration), 0.05).unwrap();
        // CI should be inflated due to calibration uncertainty.
        assert!(result.ci_inflation_factor > 1.0, "CI should be inflated: {}", result.ci_inflation_factor);
        // Effect should be debiased.
        assert!((result.adjusted_effect - (0.3 - calibration.mean_bias)).abs() < 1e-10);
    }

    #[test]
    fn test_adjust_projection_ci_contains_estimate() {
        let input = ProjectionInput {
            observed_effect: 0.5,
            observed_se: 0.1,
            projected_effect: 0.3,
            projection_ci_lower: 0.1,
            projection_ci_upper: 0.5,
            calibration_r_squared: 0.6,
        };
        let result = adjust_projection(&input, None, 0.05).unwrap();
        assert!(
            result.ci_lower <= result.adjusted_effect && result.adjusted_effect <= result.ci_upper,
            "CI [{}, {}] should contain estimate {}",
            result.ci_lower, result.ci_upper, result.adjusted_effect,
        );
    }

    #[test]
    fn test_backtest_perfect() {
        let points: Vec<BacktestPoint> = (0..10)
            .map(|i| {
                let effect = 1.0 + i as f64 * 0.5;
                BacktestPoint {
                    experiment_id: format!("exp_{i}"),
                    projected_effect: effect,
                    projection_ci_lower: effect - 0.5,
                    projection_ci_upper: effect + 0.5,
                    actual_effect: effect,
                }
            })
            .collect();
        let result = backtest_surrogate(&points).unwrap();
        assert!((result.coverage_rate - 1.0).abs() < 1e-10);
        assert!(result.mae < 1e-10);
        assert!(result.within_25_pct > 0.99);
        assert!(result.passes_acceptance);
    }

    #[test]
    fn test_backtest_poor_coverage() {
        let points: Vec<BacktestPoint> = (0..10)
            .map(|i| {
                let actual = 1.0 + i as f64 * 0.5;
                BacktestPoint {
                    experiment_id: format!("exp_{i}"),
                    projected_effect: actual + 2.0, // Way off.
                    projection_ci_lower: actual + 1.5,
                    projection_ci_upper: actual + 2.5,
                    actual_effect: actual,
                }
            })
            .collect();
        let result = backtest_surrogate(&points).unwrap();
        assert!(result.coverage_rate < 0.5);
        assert!(!result.passes_acceptance);
    }

    #[test]
    fn test_backtest_empty() {
        assert!(backtest_surrogate(&[]).is_err());
    }

    #[test]
    fn test_linear_projection_single_input() {
        let result = linear_projection(&[2.0], &[1.5], 0.5, &[0.1]).unwrap();
        assert!((result.projected_effect - 3.5).abs() < 1e-10); // 0.5 + 1.5*2.0
        assert!((result.projected_se - 0.15).abs() < 1e-10); // sqrt(1.5^2 * 0.1^2) = 0.15
    }

    #[test]
    fn test_linear_projection_multiple_inputs() {
        let result = linear_projection(
            &[1.0, 2.0],
            &[0.5, 0.3],
            0.1,
            &[0.05, 0.08],
        )
        .unwrap();
        let expected = 0.1 + 0.5 * 1.0 + 0.3 * 2.0; // 1.2
        assert!((result.projected_effect - expected).abs() < 1e-10);
        let expected_var = 0.5_f64.powi(2) * 0.05_f64.powi(2) + 0.3_f64.powi(2) * 0.08_f64.powi(2);
        assert!((result.projected_se - expected_var.sqrt()).abs() < 1e-10);
    }

    #[test]
    fn test_linear_projection_mismatched_lengths() {
        assert!(linear_projection(&[1.0, 2.0], &[0.5], 0.0, &[0.1]).is_err());
    }

    #[test]
    fn test_linear_projection_empty() {
        assert!(linear_projection(&[], &[], 0.0, &[]).is_err());
    }

    mod proptest_surrogate {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn calibration_r_squared_bounded(
                n in 3usize..20,
                scale in 0.1f64..10.0,
            ) {
                let points: Vec<CalibrationPoint> = (0..n)
                    .map(|i| CalibrationPoint {
                        experiment_id: format!("exp_{i}"),
                        predicted_effect: i as f64 * scale,
                        actual_effect: i as f64 * scale + 0.1,
                    })
                    .collect();
                let result = validate_calibration(&points).unwrap();
                // R² can be negative for terrible models, but should be bounded.
                prop_assert!(result.r_squared <= 1.0 + 1e-10, "R² > 1: {}", result.r_squared);
                prop_assert!(result.rmse.is_finite());
                prop_assert!(result.mean_bias.is_finite());
            }

            #[test]
            fn projection_ci_contains_estimate(
                effect in -10.0f64..10.0,
                se in 0.01f64..2.0,
            ) {
                let input = ProjectionInput {
                    observed_effect: effect,
                    observed_se: se,
                    projected_effect: effect * 0.8,
                    projection_ci_lower: effect * 0.8 - 1.96 * se,
                    projection_ci_upper: effect * 0.8 + 1.96 * se,
                    calibration_r_squared: 0.75,
                };
                let result = adjust_projection(&input, None, 0.05).unwrap();
                prop_assert!(
                    result.ci_lower <= result.adjusted_effect,
                    "CI lower {} > estimate {}", result.ci_lower, result.adjusted_effect
                );
                prop_assert!(
                    result.adjusted_effect <= result.ci_upper,
                    "estimate {} > CI upper {}", result.adjusted_effect, result.ci_upper
                );
            }

            #[test]
            fn linear_projection_all_finite(
                n in 1usize..5,
                intercept in -10.0f64..10.0,
            ) {
                let effects: Vec<f64> = (0..n).map(|i| i as f64 + 1.0).collect();
                let coefficients: Vec<f64> = (0..n).map(|i| (i as f64 + 1.0) * 0.5).collect();
                let ses: Vec<f64> = (0..n).map(|_| 0.1).collect();
                let result = linear_projection(&effects, &coefficients, intercept, &ses).unwrap();
                prop_assert!(result.projected_effect.is_finite());
                prop_assert!(result.projected_se.is_finite());
                prop_assert!(result.projected_se >= 0.0);
            }

            #[test]
            fn backtest_rates_in_unit_interval(n in 1usize..20) {
                let points: Vec<BacktestPoint> = (0..n)
                    .map(|i| {
                        let eff = 1.0 + i as f64;
                        BacktestPoint {
                            experiment_id: format!("exp_{i}"),
                            projected_effect: eff + 0.1,
                            projection_ci_lower: eff - 0.5,
                            projection_ci_upper: eff + 0.5,
                            actual_effect: eff,
                        }
                    })
                    .collect();
                let result = backtest_surrogate(&points).unwrap();
                prop_assert!(result.coverage_rate >= 0.0 && result.coverage_rate <= 1.0);
                prop_assert!(result.within_25_pct >= 0.0 && result.within_25_pct <= 1.0);
                prop_assert!(result.mae.is_finite() && result.mae >= 0.0);
            }
        }
    }
}
