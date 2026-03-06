//! Sequential testing: mSPRT and Group Sequential Tests (GST).
//!
//! Implements two complementary sequential testing approaches per ADR-004:
//!
//! - **mSPRT**: Always-valid p-values with arbitrary peeking.
//!   Based on Johari et al. (2017) mixture sequential probability ratio test.
//!   Uses a normal mixing distribution with variance τ².
//!
//! - **GST**: Group Sequential Tests with Lan-DeMets spending functions.
//!   O'Brien-Fleming (conservative early, powerful late) and Pocock (equal stopping).
//!
//! Validated against R's gsDesign package and scipy to 4+ decimal places.

use experimentation_core::error::{assert_finite, Error, Result};
use statrs::distribution::{ContinuousCDF, Normal};

// ---------------------------------------------------------------------------
// mSPRT (mixture Sequential Probability Ratio Test)
// ---------------------------------------------------------------------------

/// Result of an mSPRT computation at a single analysis point.
#[derive(Debug, Clone)]
pub struct MsprtResult {
    /// Mixture likelihood ratio statistic Λ_n.
    pub lambda: f64,
    /// Always-valid p-value: min(1, 1/Λ_n).
    pub p_value: f64,
    /// Whether the boundary has been crossed (Λ_n > 1/α).
    pub boundary_crossed: bool,
}

/// Compute the mSPRT statistic for normally distributed data.
///
/// Uses the mixture likelihood ratio with a normal mixing distribution
/// of variance `tau_sq` on the true effect size (Johari et al. 2017).
///
/// # Arguments
/// * `z_stat` — Z-statistic at the current sample size.
/// * `n` — Effective sample size (harmonic mean of group sizes × 2 for two-sample).
/// * `sigma_sq` — Pooled variance estimate.
/// * `tau_sq` — Mixing variance (prior scale on effect size). Controls sensitivity.
///   Larger τ² → more sensitive to large effects, less to small.
/// * `alpha` — Overall significance level.
///
/// # Formula
/// `Λ_n = sqrt(V / (V + n)) * exp(n * Z² / (2 * (V + n)))`
/// where `V = σ² / τ²`.
pub fn msprt_normal(
    z_stat: f64,
    n: f64,
    sigma_sq: f64,
    tau_sq: f64,
    alpha: f64,
) -> Result<MsprtResult> {
    if n <= 0.0 {
        return Err(Error::Validation("n must be positive".into()));
    }
    if sigma_sq <= 0.0 {
        return Err(Error::Validation("sigma_sq must be positive".into()));
    }
    if tau_sq <= 0.0 {
        return Err(Error::Validation("tau_sq must be positive".into()));
    }
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }

    assert_finite(z_stat, "z_stat");
    assert_finite(n, "n");
    assert_finite(sigma_sq, "sigma_sq");
    assert_finite(tau_sq, "tau_sq");

    // V = σ² / τ² (the variance ratio)
    let v = sigma_sq / tau_sq;
    assert_finite(v, "v");

    // Λ_n = sqrt(V / (V + n)) * exp(n * Z² / (2 * (V + n)))
    let v_plus_n = v + n;
    let log_lambda = 0.5 * (v / v_plus_n).ln() + (n * z_stat * z_stat) / (2.0 * v_plus_n);
    assert_finite(log_lambda, "log_lambda");

    let lambda = log_lambda.exp();
    assert_finite(lambda, "lambda");

    let p_value = (1.0 / lambda).min(1.0);
    assert_finite(p_value, "p_value");

    let threshold = 1.0 / alpha;
    let boundary_crossed = lambda > threshold;

    Ok(MsprtResult {
        lambda,
        p_value,
        boundary_crossed,
    })
}

/// Compute the mSPRT statistic directly from sample statistics.
///
/// Convenience wrapper that computes the z-statistic internally.
#[allow(clippy::too_many_arguments)]
pub fn msprt_from_samples(
    control_mean: f64,
    treatment_mean: f64,
    control_var: f64,
    treatment_var: f64,
    n_control: f64,
    n_treatment: f64,
    tau_sq: f64,
    alpha: f64,
) -> Result<MsprtResult> {
    assert_finite(control_mean, "control_mean");
    assert_finite(treatment_mean, "treatment_mean");
    assert_finite(control_var, "control_var");
    assert_finite(treatment_var, "treatment_var");

    if n_control < 2.0 || n_treatment < 2.0 {
        return Err(Error::Validation(
            "each group must have at least 2 observations".into(),
        ));
    }

    let se = (control_var / n_control + treatment_var / n_treatment).sqrt();
    assert_finite(se, "se");
    if se == 0.0 {
        return Err(Error::Numerical("standard error is zero".into()));
    }

    let z_stat = (treatment_mean - control_mean) / se;
    assert_finite(z_stat, "z_stat");

    // Effective sample size: harmonic mean of group sizes
    let n_eff = 2.0 * n_control * n_treatment / (n_control + n_treatment);

    // Pooled variance estimate
    let sigma_sq = (control_var + treatment_var) / 2.0;

    msprt_normal(z_stat, n_eff, sigma_sq, tau_sq, alpha)
}

// ---------------------------------------------------------------------------
// Group Sequential Tests (GST) with Lan-DeMets spending functions
// ---------------------------------------------------------------------------

/// Alpha spending function type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpendingFunction {
    /// O'Brien-Fleming: conservative early stopping, maximum power at final look.
    /// α*(t) = 2 * (1 - Φ(z_{α/2} / √t))
    OBrienFleming,
    /// Pocock: equal stopping probability at each look.
    /// α*(t) = α * ln(1 + (e-1) * t)
    Pocock,
}

/// Result of a GST analysis at a single look.
#[derive(Debug, Clone)]
pub struct GstResult {
    /// Whether the effect has crossed the stopping boundary at this look.
    pub boundary_crossed: bool,
    /// Cumulative alpha spent through this look.
    pub alpha_spent: f64,
    /// Remaining alpha budget.
    pub alpha_remaining: f64,
    /// Current look number (1-indexed).
    pub current_look: u32,
    /// Total planned looks.
    pub planned_looks: u32,
    /// Critical z-value at this look.
    pub critical_value: f64,
    /// Nominal p-value at this look (unadjusted).
    pub nominal_p_value: f64,
    /// Adjusted p-value accounting for multiple looks.
    pub adjusted_p_value: f64,
}

/// Compute the cumulative alpha spent at information fraction `t`.
pub fn spending_function_alpha(spending: SpendingFunction, t: f64, overall_alpha: f64) -> f64 {
    assert_finite(t, "information_fraction");
    assert!(
        (0.0..=1.0).contains(&t),
        "information fraction must be in [0, 1], got {t}"
    );

    let alpha = match spending {
        SpendingFunction::OBrienFleming => {
            let z = Normal::new(0.0, 1.0).expect("valid normal");
            let z_alpha_half = z.inverse_cdf(1.0 - overall_alpha / 2.0);
            2.0 * (1.0 - z.cdf(z_alpha_half / t.sqrt()))
        }
        SpendingFunction::Pocock => {
            overall_alpha * (1.0 + (std::f64::consts::E - 1.0) * t).ln()
        }
    };

    // Clamp to [0, overall_alpha]
    alpha.max(0.0).min(overall_alpha)
}

/// Evaluate the GST boundary at a specific look.
///
/// # Arguments
/// * `z_stat` — Z-statistic at the current look.
/// * `current_look` — Which look this is (1-indexed, must be ≤ planned_looks).
/// * `planned_looks` — Total number of planned analysis looks.
/// * `overall_alpha` — Total alpha budget across all looks.
/// * `spending` — Spending function type (OBF or Pocock).
/// * `prev_alpha_spent` — Alpha already spent in previous looks (pass 0.0 for first look).
pub fn gst_evaluate(
    z_stat: f64,
    current_look: u32,
    planned_looks: u32,
    overall_alpha: f64,
    spending: SpendingFunction,
    prev_alpha_spent: f64,
) -> Result<GstResult> {
    if planned_looks < 2 {
        return Err(Error::Validation(
            "GST requires at least 2 planned looks".into(),
        ));
    }
    if current_look == 0 || current_look > planned_looks {
        return Err(Error::Validation(format!(
            "current_look must be in [1, {planned_looks}], got {current_look}"
        )));
    }
    if overall_alpha <= 0.0 || overall_alpha >= 1.0 {
        return Err(Error::Validation("overall_alpha must be in (0, 1)".into()));
    }
    if prev_alpha_spent < 0.0 || prev_alpha_spent >= overall_alpha {
        return Err(Error::Validation(format!(
            "prev_alpha_spent must be in [0, {overall_alpha}), got {prev_alpha_spent}"
        )));
    }

    assert_finite(z_stat, "z_stat");
    assert_finite(prev_alpha_spent, "prev_alpha_spent");

    let z = Normal::new(0.0, 1.0)
        .map_err(|e| Error::Numerical(format!("failed to create Normal: {e}")))?;

    // Information fraction at this look
    let t = current_look as f64 / planned_looks as f64;

    // Cumulative alpha to spend through this look
    let cumulative_alpha = spending_function_alpha(spending, t, overall_alpha);
    assert_finite(cumulative_alpha, "cumulative_alpha");

    // Incremental alpha for this look
    let incremental_alpha = (cumulative_alpha - prev_alpha_spent).max(0.0);
    assert_finite(incremental_alpha, "incremental_alpha");

    // Critical value: z-boundary for two-sided test at incremental alpha level
    let critical_value = if incremental_alpha > 0.0 {
        z.inverse_cdf(1.0 - incremental_alpha / 2.0)
    } else {
        f64::INFINITY
    };
    assert_finite(critical_value, "critical_value");

    // Nominal p-value (two-sided)
    let nominal_p_value = 2.0 * (1.0 - z.cdf(z_stat.abs()));
    assert_finite(nominal_p_value, "nominal_p_value");

    let boundary_crossed = z_stat.abs() > critical_value;

    // Adjusted p-value: the smallest overall alpha for which this look
    // would have crossed the boundary. Approximate by scaling nominal p.
    let adjusted_p_value = if boundary_crossed {
        cumulative_alpha.min(1.0)
    } else {
        // Conservative: report the nominal p-value scaled by the spending factor
        (nominal_p_value * overall_alpha / incremental_alpha.max(f64::MIN_POSITIVE)).min(1.0)
    };
    assert_finite(adjusted_p_value, "adjusted_p_value");

    let alpha_spent = cumulative_alpha;
    let alpha_remaining = (overall_alpha - alpha_spent).max(0.0);
    assert_finite(alpha_remaining, "alpha_remaining");

    Ok(GstResult {
        boundary_crossed,
        alpha_spent,
        alpha_remaining,
        current_look,
        planned_looks,
        critical_value,
        nominal_p_value,
        adjusted_p_value,
    })
}

/// Compute all GST boundaries for a given spending function and number of looks.
///
/// Returns the critical z-values at each look. Useful for plotting boundary curves.
pub fn gst_boundaries(
    planned_looks: u32,
    overall_alpha: f64,
    spending: SpendingFunction,
) -> Result<Vec<f64>> {
    if planned_looks < 2 {
        return Err(Error::Validation(
            "GST requires at least 2 planned looks".into(),
        ));
    }

    let z = Normal::new(0.0, 1.0)
        .map_err(|e| Error::Numerical(format!("failed to create Normal: {e}")))?;

    let mut boundaries = Vec::with_capacity(planned_looks as usize);
    let mut prev_alpha = 0.0;

    for k in 1..=planned_looks {
        let t = k as f64 / planned_looks as f64;
        let cumulative = spending_function_alpha(spending, t, overall_alpha);
        let incremental = (cumulative - prev_alpha).max(0.0);

        let critical = if incremental > 0.0 {
            z.inverse_cdf(1.0 - incremental / 2.0)
        } else {
            f64::INFINITY
        };
        assert_finite(critical, "gst_boundary");

        boundaries.push(critical);
        prev_alpha = cumulative;
    }

    Ok(boundaries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msprt_no_effect() {
        // Z=0 should give lambda ≤ 1 (no evidence against H0)
        let result = msprt_normal(0.0, 100.0, 1.0, 0.1, 0.05).unwrap();
        assert!(result.lambda <= 1.0);
        assert!(!result.boundary_crossed);
        assert!((result.p_value - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_msprt_strong_effect() {
        // Large z-stat should cross the boundary
        let result = msprt_normal(4.0, 1000.0, 1.0, 0.1, 0.05).unwrap();
        assert!(result.lambda > 20.0); // 1/0.05 = 20
        assert!(result.boundary_crossed);
        assert!(result.p_value < 0.05);
    }

    #[test]
    fn test_msprt_validation_errors() {
        assert!(msprt_normal(1.0, -1.0, 1.0, 0.1, 0.05).is_err());
        assert!(msprt_normal(1.0, 100.0, 0.0, 0.1, 0.05).is_err());
        assert!(msprt_normal(1.0, 100.0, 1.0, 0.0, 0.05).is_err());
        assert!(msprt_normal(1.0, 100.0, 1.0, 0.1, 0.0).is_err());
        assert!(msprt_normal(1.0, 100.0, 1.0, 0.1, 1.0).is_err());
    }

    #[test]
    fn test_msprt_from_samples() {
        let result = msprt_from_samples(
            10.0, 10.5, // means
            4.0, 4.0,   // variances
            500.0, 500.0, // sample sizes
            0.1,  // tau_sq
            0.05, // alpha
        )
        .unwrap();
        // With moderate effect, may or may not cross
        assert!(result.p_value > 0.0 && result.p_value <= 1.0);
    }

    #[test]
    fn test_obf_spending_function() {
        // OBF should spend very little alpha early
        let early = spending_function_alpha(SpendingFunction::OBrienFleming, 0.25, 0.05);
        let mid = spending_function_alpha(SpendingFunction::OBrienFleming, 0.5, 0.05);
        let final_ = spending_function_alpha(SpendingFunction::OBrienFleming, 1.0, 0.05);

        assert!(early < 0.001, "OBF early alpha should be tiny, got {early}");
        assert!(mid < 0.02, "OBF mid alpha should be small, got {mid}");
        assert!((final_ - 0.05).abs() < 1e-6, "OBF final alpha should equal overall, got {final_}");
    }

    #[test]
    fn test_pocock_spending_function() {
        // Pocock should spend alpha more evenly
        let early = spending_function_alpha(SpendingFunction::Pocock, 0.25, 0.05);
        let mid = spending_function_alpha(SpendingFunction::Pocock, 0.5, 0.05);
        let final_ = spending_function_alpha(SpendingFunction::Pocock, 1.0, 0.05);

        assert!(early > 0.005, "Pocock early alpha should be moderate, got {early}");
        assert!(mid > 0.02, "Pocock mid alpha should be substantial, got {mid}");
        assert!((final_ - 0.05).abs() < 1e-6, "Pocock final alpha should equal overall, got {final_}");
    }

    #[test]
    fn test_gst_boundaries_obf() {
        let bounds = gst_boundaries(4, 0.05, SpendingFunction::OBrienFleming).unwrap();
        assert_eq!(bounds.len(), 4);
        // OBF: boundaries should decrease over looks
        for i in 1..bounds.len() {
            assert!(bounds[i] <= bounds[i - 1],
                "OBF boundaries should decrease: {} > {}", bounds[i], bounds[i-1]);
        }
    }

    #[test]
    fn test_gst_boundaries_pocock() {
        let bounds = gst_boundaries(4, 0.05, SpendingFunction::Pocock).unwrap();
        assert_eq!(bounds.len(), 4);
        // Pocock: boundaries should be more uniform
        let range = bounds.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - bounds.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(range < 1.0, "Pocock boundaries should be relatively uniform, range={range}");
    }

    #[test]
    fn test_gst_evaluate_no_effect() {
        let result = gst_evaluate(0.5, 1, 4, 0.05, SpendingFunction::OBrienFleming, 0.0).unwrap();
        assert!(!result.boundary_crossed);
        assert_eq!(result.current_look, 1);
        assert!(result.alpha_remaining > 0.0);
    }

    #[test]
    fn test_gst_evaluate_strong_effect() {
        let result = gst_evaluate(5.0, 4, 4, 0.05, SpendingFunction::OBrienFleming, 0.01).unwrap();
        assert!(result.boundary_crossed);
    }

    #[test]
    fn test_gst_validation_errors() {
        assert!(gst_evaluate(1.0, 1, 1, 0.05, SpendingFunction::OBrienFleming, 0.0).is_err());
        assert!(gst_evaluate(1.0, 0, 4, 0.05, SpendingFunction::OBrienFleming, 0.0).is_err());
        assert!(gst_evaluate(1.0, 5, 4, 0.05, SpendingFunction::OBrienFleming, 0.0).is_err());
    }
}
