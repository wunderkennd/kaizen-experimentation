//! Portfolio alpha allocation and annualized impact estimation (ADR-019).
//!
//! Provides functions for computing the optimal per-experiment significance
//! threshold across a portfolio of simultaneous experiments, and for estimating
//! the annualized business impact of a detected effect.
//!
//! # Functions
//!
//! - [`optimal_alpha`]: Bonferroni correction — divides the platform alpha budget
//!   equally across all simultaneously running experiments, controlling the
//!   family-wise error rate (FWER).
//!
//! - [`annualized_impact`]: Projects the per-user-per-day treatment effect to
//!   an annual aggregate assuming the winning variant is permanently shipped.
//!
//! # References
//!
//! Bonferroni (1936) — multiple comparison correction.
//! ADR-019 (Portfolio Optimization) — platform-level alpha management.

use experimentation_core::error::{Error, Result};

// ---------------------------------------------------------------------------
// Optimal alpha allocation (Bonferroni)
// ---------------------------------------------------------------------------

/// Compute the optimal per-experiment significance level via Bonferroni correction.
///
/// Divides the platform-wide alpha budget equally across `n_experiments`
/// simultaneous tests. This is the simplest multiple-comparison correction that
/// controls the family-wise error rate (FWER) at `alpha_budget`:
///
/// ```text
/// alpha_per_experiment = alpha_budget / n_experiments
/// ```
///
/// This is conservative — real power is higher when experiments are independent
/// (Sidak correction gives (1 − (1 − α)^{1/k})), but Bonferroni is widely
/// understood, audit-friendly, and appropriate for correlated experiments on
/// shared user populations.
///
/// For portfolio-level FDR control (allowing some false positives across the
/// portfolio while controlling the proportion), see the `evalue` module's
/// `e_value_grow` / `e_value_avlm` combined with online FDR via the
/// `OnlineFdrController` in `experimentation-management`.
///
/// # Arguments
/// * `alpha_budget` — Platform-wide family-wise error rate target. Must be in (0, 1).
/// * `n_experiments` — Number of simultaneously running experiments. Must be ≥ 1.
///
/// # Errors
/// Returns `Error::Validation` for invalid inputs.
///
/// # Examples
/// ```
/// use experimentation_stats::portfolio::optimal_alpha;
///
/// // 5 simultaneous experiments with a 5% platform budget.
/// let alpha = optimal_alpha(0.05, 5).unwrap();
/// assert!((alpha - 0.01).abs() < 1e-12);
/// ```
pub fn optimal_alpha(alpha_budget: f64, n_experiments: usize) -> Result<f64> {
    if alpha_budget <= 0.0 || alpha_budget >= 1.0 {
        return Err(Error::Validation(
            "alpha_budget must be in (0, 1)".into(),
        ));
    }
    if n_experiments == 0 {
        return Err(Error::Validation(
            "n_experiments must be at least 1".into(),
        ));
    }
    Ok(alpha_budget / n_experiments as f64)
}

// ---------------------------------------------------------------------------
// Annualized impact
// ---------------------------------------------------------------------------

/// Project a per-user-per-day treatment effect to an annualized aggregate impact.
///
/// Assumes the winning variant is permanently shipped after the experiment
/// concludes. The deployed impact is:
///
/// ```text
/// annualized_impact = effect * daily_users * 365
/// ```
///
/// The experiment `duration_days` does not scale the output because `effect` is
/// a per-user-per-day delta (e.g. +0.003 watch-hours per user per day). Once
/// shipped, all `daily_users` receive the improvement every day.
///
/// `duration_days` is validated (> 0) to catch misconfigured inputs but is not
/// used in the final formula.
///
/// # Arguments
/// * `effect` — Point estimate of the treatment effect, per user per day
///   (e.g. incremental watch-hours, incremental click-through rate).
///   May be negative for harmful treatments.
/// * `daily_users` — Daily active users exposed to the change post-ship.
///   Must be > 0.
/// * `duration_days` — Experiment duration in calendar days. Must be > 0.
///   (Used for input validation; does not scale the output.)
///
/// # Errors
/// Returns `Error::Validation` for invalid inputs (non-finite effect, non-positive
/// `daily_users`, non-positive `duration_days`).
///
/// # Examples
/// ```
/// use experimentation_stats::portfolio::annualized_impact;
///
/// // +0.01 watch-hours/user/day, 10M daily users.
/// let impact = annualized_impact(0.01, 10_000_000.0, 14.0).unwrap();
/// assert!((impact - 36_500_000.0).abs() < 1.0);
/// ```
pub fn annualized_impact(effect: f64, daily_users: f64, duration_days: f64) -> Result<f64> {
    if !effect.is_finite() {
        return Err(Error::Validation("effect must be finite".into()));
    }
    if daily_users <= 0.0 {
        return Err(Error::Validation("daily_users must be positive".into()));
    }
    if duration_days <= 0.0 {
        return Err(Error::Validation("duration_days must be positive".into()));
    }
    Ok(effect * daily_users * 365.0)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- optimal_alpha ------------------------------------------------------

    #[test]
    fn test_optimal_alpha_bonferroni_five_experiments() {
        let alpha = optimal_alpha(0.05, 5).unwrap();
        assert!((alpha - 0.01).abs() < 1e-12, "alpha={alpha}");
    }

    #[test]
    fn test_optimal_alpha_single_experiment() {
        let alpha = optimal_alpha(0.05, 1).unwrap();
        assert!((alpha - 0.05).abs() < 1e-12);
    }

    #[test]
    fn test_optimal_alpha_ten_experiments() {
        let alpha = optimal_alpha(0.10, 10).unwrap();
        assert!((alpha - 0.01).abs() < 1e-12, "alpha={alpha}");
    }

    #[test]
    fn test_optimal_alpha_validation_errors() {
        assert!(optimal_alpha(0.0, 5).is_err(), "zero budget");
        assert!(optimal_alpha(1.0, 5).is_err(), "budget=1");
        assert!(optimal_alpha(-0.05, 5).is_err(), "negative budget");
        assert!(optimal_alpha(1.5, 5).is_err(), "budget>1");
        assert!(optimal_alpha(0.05, 0).is_err(), "zero experiments");
    }

    #[test]
    fn test_optimal_alpha_monotone_in_n() {
        // More experiments → smaller per-experiment alpha.
        let a1 = optimal_alpha(0.05, 1).unwrap();
        let a5 = optimal_alpha(0.05, 5).unwrap();
        let a10 = optimal_alpha(0.05, 10).unwrap();
        assert!(a1 > a5, "a1={a1} a5={a5}");
        assert!(a5 > a10, "a5={a5} a10={a10}");
    }

    // --- annualized_impact --------------------------------------------------

    #[test]
    fn test_annualized_impact_basic() {
        // 0.01 effect * 10M users * 365 days = 36.5M
        let impact = annualized_impact(0.01, 10_000_000.0, 14.0).unwrap();
        assert!((impact - 36_500_000.0).abs() < 1.0, "impact={impact}");
    }

    #[test]
    fn test_annualized_impact_zero_effect() {
        let impact = annualized_impact(0.0, 1_000_000.0, 7.0).unwrap();
        assert!(impact.abs() < 1e-12, "impact={impact}");
    }

    #[test]
    fn test_annualized_impact_negative_effect() {
        // Harmful treatment should produce negative impact.
        let impact = annualized_impact(-0.005, 1_000_000.0, 30.0).unwrap();
        assert!(impact < 0.0, "impact={impact}");
        assert!((impact - (-0.005 * 1_000_000.0 * 365.0)).abs() < 1.0);
    }

    #[test]
    fn test_annualized_impact_duration_does_not_scale() {
        // Same effect + users: different duration → same annualized impact.
        let i7 = annualized_impact(0.01, 1_000_000.0, 7.0).unwrap();
        let i30 = annualized_impact(0.01, 1_000_000.0, 30.0).unwrap();
        assert!((i7 - i30).abs() < 1e-6, "i7={i7} i30={i30}");
    }

    #[test]
    fn test_annualized_impact_validation_errors() {
        assert!(annualized_impact(f64::NAN, 1e6, 14.0).is_err(), "NaN effect");
        assert!(annualized_impact(f64::INFINITY, 1e6, 14.0).is_err(), "inf effect");
        assert!(annualized_impact(0.01, 0.0, 14.0).is_err(), "zero daily_users");
        assert!(annualized_impact(0.01, -1.0, 14.0).is_err(), "neg daily_users");
        assert!(annualized_impact(0.01, 1e6, 0.0).is_err(), "zero duration");
        assert!(annualized_impact(0.01, 1e6, -1.0).is_err(), "neg duration");
    }
}
