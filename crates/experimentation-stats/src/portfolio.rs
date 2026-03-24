//! Portfolio power analysis for experiment program optimization (ADR-019).
//!
//! Implements three program-level statistical recommendations:
//!
//! 1. **`optimal_alpha`**: Recommends the per-experiment significance threshold
//!    that minimises expected false discoveries at the portfolio level, given the
//!    program's historical win rate, target FDR, and target power.  Derived from
//!    Storey-Tibshirani (2003) FDR framework.
//!
//! 2. **`annualized_impact`**: Projects an observed in-experiment lift to an
//!    annualized business-value figure using three scaling methods (linear,
//!    compound, conservative geometric mean).
//!
//! 3. **`traffic_allocation_optimizer`**: Recommends per-experiment traffic
//!    fractions across a portfolio of concurrent experiments so that each arm
//!    achieves at least `min_power` at the requested alpha and MDE.  Uses the
//!    standard two-sample equal-allocation formula and scales proportionally
//!    within the available traffic budget.
//!
//! # References
//! - Storey & Tibshirani (2003) "Statistical significance for genome-wide studies"
//!   PNAS 100(16). — FDR / π₀ estimation framework.
//! - Netflix EC '25 "Optimizing experimentation program returns" (ADR-019).
//! - Spotify EwL framework (September 2025).

use experimentation_core::error::{assert_finite, Error, Result};
use statrs::distribution::{ContinuousCDF, Normal};

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Portfolio-level parameters for significance-threshold recommendation.
///
/// Passed to [`optimal_alpha`].
#[derive(Debug, Clone)]
pub struct PortfolioParams {
    /// Historical fraction of experiments that produced a statistically
    /// significant positive result on the primary metric.  Used as the
    /// Bayesian prior probability that a new experiment has a true positive
    /// effect.  Must be strictly in (0, 1).
    pub prior_win_rate: f64,

    /// Target false discovery rate (FDR) for the portfolio.
    /// Typical value: 0.05.  Must be in (0, 1).
    pub fdr_target: f64,

    /// Target statistical power (1 − β) per experiment.
    /// Typical value: 0.80.  Must be in (0, 1).
    pub target_power: f64,
}

/// Parameters for a single experiment in the traffic allocation problem.
///
/// One element of the `experiments` slice passed to
/// [`traffic_allocation_optimizer`].
#[derive(Debug, Clone)]
pub struct ExperimentSpec {
    /// Stable identifier for the experiment.
    pub experiment_id: String,

    /// Minimum detectable effect expressed as a *relative* lift
    /// (e.g., `0.02` = 2% relative to the baseline mean).
    /// Must be > 0.
    pub mde_relative: f64,

    /// Expected mean of the primary metric in the control arm.
    /// Used to convert `mde_relative` to an absolute effect: δ = mde × mean.
    /// Must be > 0.
    pub baseline_mean: f64,

    /// Variance of the primary metric (pooled across arms).
    /// Must be > 0.
    pub baseline_variance: f64,

    /// Total number of variants *including* control (≥ 2).
    /// Traffic per arm = `recommended_fraction / n_variants`.
    pub n_variants: usize,
}

/// Parameters for annualised impact projection.
///
/// Passed to [`annualized_impact`].
#[derive(Debug, Clone)]
pub struct AnnualizedImpactParams {
    /// Relative lift observed during the experiment (e.g., `0.02` = 2%).
    /// Must be ≥ 0.
    pub observed_lift_relative: f64,

    /// Baseline value of the primary metric *per user per year*
    /// (e.g., annual subscription revenue per subscriber in USD).
    /// Must be ≥ 0.
    pub annual_baseline_per_user: f64,

    /// Total addressable user count.  Must be > 0.
    pub total_users: u64,

    /// Duration of the experiment in days.  Used to compute the
    /// annualisation factor.  Must be > 0.
    pub experiment_duration_days: f64,

    /// Fraction of total traffic assigned to the *treatment* arm(s).
    /// E.g., 0.50 for a 50 / 50 split.  Must be in (0, 1].
    pub treatment_fraction: f64,
}

/// Parameters for the traffic allocation optimiser.
///
/// Passed to [`traffic_allocation_optimizer`].
#[derive(Debug, Clone)]
pub struct TrafficAllocationInput {
    /// One entry per experiment in the concurrent portfolio.
    pub experiments: Vec<ExperimentSpec>,

    /// Fraction of total user traffic that is available for all experiments
    /// combined (e.g., `0.60` = 60 %).  Must be in (0, 1].
    pub available_traffic_fraction: f64,

    /// Minimum statistical power required for each experiment.
    /// Typically 0.80.  Must be in (0, 1).
    pub min_power: f64,

    /// Per-experiment significance threshold.  Typically the output of
    /// [`optimal_alpha`].  Must be in (0, 1).
    pub alpha: f64,
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// Recommended traffic allocation for one experiment.
#[derive(Debug, Clone)]
pub struct TrafficAllocation {
    /// Experiment identifier (matches `ExperimentSpec::experiment_id`).
    pub experiment_id: String,

    /// Recommended fraction of *total* user traffic to assign to this
    /// experiment (treatment + control combined).  The per-arm fraction is
    /// `recommended_traffic_fraction / n_variants`.
    pub recommended_traffic_fraction: f64,

    /// Minimum number of users *per arm* to achieve `min_power`.
    pub required_n_per_arm: u64,
}

/// Top-level result returned by portfolio power analysis.
///
/// Contains the outputs of all three portfolio functions.
#[derive(Debug, Clone)]
pub struct PortfolioRecommendation {
    /// Recommended per-experiment significance threshold (≡ [`optimal_alpha`]).
    /// In (0, 1).
    pub optimal_alpha: f64,

    /// Projected annualised business impact (≡ [`annualized_impact`]).
    /// In the same unit as `annual_baseline_per_user × total_users`.
    /// Non-negative.
    pub annualized_impact: f64,

    /// Per-experiment traffic allocation (≡ [`traffic_allocation_optimizer`]).
    pub traffic_allocations: Vec<TrafficAllocation>,

    /// Expected portfolio-level FDR given `optimal_alpha` and the
    /// historical win rate.  For informational display.
    pub expected_portfolio_fdr: f64,
}

// ---------------------------------------------------------------------------
// optimal_alpha
// ---------------------------------------------------------------------------

/// Recommend the per-experiment significance threshold (α*) that controls
/// the portfolio-level false discovery rate.
///
/// # Method
///
/// From the Storey-Tibshirani FDR framework the expected FDR for a single
/// test at level α, given prior win rate π₁ and power (1 − β), is:
///
/// ```text
/// FDR = π₀ · α  /  (π₀ · α  +  π₁ · power)
/// ```
///
/// where π₀ = 1 − π₁ = 1 − `prior_win_rate`.
///
/// Solving for α given a target FDR yields:
///
/// ```text
/// α* = FDR_target · π₁ · power  /  (π₀ · (1 − FDR_target))
/// ```
///
/// The result is clamped to the interval (0.001, 0.20).  The upper bound
/// reflects platform policy; the lower bound avoids degenerate under-powered
/// designs.
///
/// # Arguments
/// * `params` — Portfolio parameters.
///
/// # Returns
/// Recommended α* in (0.001, 0.20).
///
/// # Errors
/// Returns `Error::Validation` if any parameter is outside its valid range.
pub fn optimal_alpha(params: &PortfolioParams) -> Result<f64> {
    let PortfolioParams {
        prior_win_rate,
        fdr_target,
        target_power,
    } = params;

    if !(*prior_win_rate > 0.0 && *prior_win_rate < 1.0) {
        return Err(Error::Validation(
            "prior_win_rate must be strictly in (0, 1)".into(),
        ));
    }
    if !(*fdr_target > 0.0 && *fdr_target < 1.0) {
        return Err(Error::Validation(
            "fdr_target must be strictly in (0, 1)".into(),
        ));
    }
    if !(*target_power > 0.0 && *target_power < 1.0) {
        return Err(Error::Validation(
            "target_power must be strictly in (0, 1)".into(),
        ));
    }

    let pi1 = *prior_win_rate; // prior probability of true effect
    let pi0 = 1.0 - pi1; // prior probability of null

    // α* = FDR_target × π₁ × power / (π₀ × (1 − FDR_target))
    let alpha_unclamped = (fdr_target * pi1 * target_power) / (pi0 * (1.0 - fdr_target));
    assert_finite(alpha_unclamped, "optimal_alpha_unclamped");

    // Clamp to platform policy window.
    let alpha = alpha_unclamped.clamp(0.001, 0.20);
    assert_finite(alpha, "optimal_alpha");

    Ok(alpha)
}

// ---------------------------------------------------------------------------
// annualized_impact
// ---------------------------------------------------------------------------

/// Project an in-experiment lift to an annualised business-value figure.
///
/// Three projection methods are computed and the *conservative* estimate
/// (geometric mean of linear and compound) is returned.
///
/// ## Linear method
/// Assumes the relative lift observed during the experiment applies uniformly
/// to the full user base after rollout:
/// ```text
/// impact_linear = lift × annual_baseline_per_user × total_users
/// ```
///
/// ## Compound method
/// Compounds the observed per-experiment-period lift over a full year.
/// When the experiment runs for `d` days the compounding factor is
/// `(1 + lift)^(365 / d) − 1`.
/// ```text
/// impact_compound = compound_lift × annual_baseline_per_user × total_users
/// ```
///
/// ## Conservative method (returned)
/// Geometric mean of the two projections:
/// ```text
/// impact = sqrt(impact_linear × impact_compound)
/// ```
/// Falls back to `impact_linear` if `impact_compound` is negative (i.e., when
/// `lift < 0`; the function is defined for `lift ≥ 0` but is safe for small
/// negatives due to the clamp below).
///
/// # Returns
/// Annualised business impact ≥ 0 in the same unit as
/// `annual_baseline_per_user × total_users`.
///
/// # Errors
/// Returns `Error::Validation` if parameters are out of range.
pub fn annualized_impact(params: &AnnualizedImpactParams) -> Result<f64> {
    let AnnualizedImpactParams {
        observed_lift_relative,
        annual_baseline_per_user,
        total_users,
        experiment_duration_days,
        treatment_fraction,
    } = params;

    if *observed_lift_relative < 0.0 {
        return Err(Error::Validation(
            "observed_lift_relative must be ≥ 0".into(),
        ));
    }
    if *annual_baseline_per_user < 0.0 {
        return Err(Error::Validation(
            "annual_baseline_per_user must be ≥ 0".into(),
        ));
    }
    if *total_users == 0 {
        return Err(Error::Validation("total_users must be > 0".into()));
    }
    if *experiment_duration_days <= 0.0 {
        return Err(Error::Validation(
            "experiment_duration_days must be > 0".into(),
        ));
    }
    if !(*treatment_fraction > 0.0 && *treatment_fraction <= 1.0) {
        return Err(Error::Validation(
            "treatment_fraction must be in (0, 1]".into(),
        ));
    }

    assert_finite(*observed_lift_relative, "observed_lift_relative");
    assert_finite(*annual_baseline_per_user, "annual_baseline_per_user");
    assert_finite(*experiment_duration_days, "experiment_duration_days");
    assert_finite(*treatment_fraction, "treatment_fraction");

    let total_annual_baseline = *annual_baseline_per_user * (*total_users as f64);
    assert_finite(total_annual_baseline, "total_annual_baseline");

    // Linear: full-year impact assuming 100 % rollout.
    let impact_linear = observed_lift_relative * total_annual_baseline;
    assert_finite(impact_linear, "impact_linear");

    // Compound: annualise via compounding over experiment duration.
    let periods_per_year = 365.0 / experiment_duration_days;
    assert_finite(periods_per_year, "periods_per_year");
    let compound_factor = (1.0 + observed_lift_relative).powf(periods_per_year) - 1.0;
    assert_finite(compound_factor, "compound_factor");
    let impact_compound = compound_factor.max(0.0) * total_annual_baseline;
    assert_finite(impact_compound, "impact_compound");

    // Conservative: geometric mean.
    let impact_conservative = if impact_linear > 0.0 && impact_compound > 0.0 {
        (impact_linear * impact_compound).sqrt()
    } else {
        impact_linear.max(0.0)
    };
    assert_finite(impact_conservative, "impact_conservative");

    // Scale by treatment fraction (models partial rollout equivalent to experiment exposure).
    let scaled = impact_conservative * treatment_fraction;
    assert_finite(scaled, "annualized_impact_scaled");

    Ok(scaled.max(0.0))
}

// ---------------------------------------------------------------------------
// traffic_allocation_optimizer
// ---------------------------------------------------------------------------

/// Recommend per-experiment traffic fractions for a portfolio of concurrent
/// experiments.
///
/// # Method
///
/// For each experiment the minimum sample size per arm is computed via the
/// standard equal-allocation two-sample formula:
///
/// ```text
/// n_per_arm = 2 · σ² · (z_{α/2} + z_β)² / δ²
/// ```
///
/// where δ = `mde_relative × baseline_mean` (absolute MDE) and σ² =
/// `baseline_variance`.
///
/// Required total users per experiment:
/// ```text
/// n_total = n_per_arm × n_variants
/// ```
///
/// Traffic fractions are then allocated *proportionally* to the required
/// sample sizes and scaled so that the sum is ≤
/// `available_traffic_fraction`.
///
/// # Returns
/// One [`TrafficAllocation`] per experiment, in the same order as
/// `input.experiments`.
///
/// # Errors
/// Returns `Error::Validation` if any parameter is out of range or if no
/// experiments are provided.
pub fn traffic_allocation_optimizer(
    input: &TrafficAllocationInput,
) -> Result<Vec<TrafficAllocation>> {
    let TrafficAllocationInput {
        experiments,
        available_traffic_fraction,
        min_power,
        alpha,
    } = input;

    if experiments.is_empty() {
        return Err(Error::Validation("experiments must not be empty".into()));
    }
    if !(*available_traffic_fraction > 0.0 && *available_traffic_fraction <= 1.0) {
        return Err(Error::Validation(
            "available_traffic_fraction must be in (0, 1]".into(),
        ));
    }
    if !(*min_power > 0.0 && *min_power < 1.0) {
        return Err(Error::Validation(
            "min_power must be strictly in (0, 1)".into(),
        ));
    }
    if !(*alpha > 0.0 && *alpha < 1.0) {
        return Err(Error::Validation("alpha must be strictly in (0, 1)".into()));
    }

    let normal = Normal::new(0.0, 1.0).map_err(|e| Error::Numerical(e.to_string()))?;

    // z_{α/2} for two-sided test; z_β for power.
    let z_alpha_half = normal.inverse_cdf(1.0 - alpha / 2.0);
    let z_beta = normal.inverse_cdf(*min_power);
    assert_finite(z_alpha_half, "z_alpha_half");
    assert_finite(z_beta, "z_beta");
    let z_sum_sq = (z_alpha_half + z_beta).powi(2);
    assert_finite(z_sum_sq, "z_sum_sq");

    // Compute required n_per_arm and n_total for each experiment.
    let mut required_n_per_arm: Vec<u64> = Vec::with_capacity(experiments.len());

    for (i, spec) in experiments.iter().enumerate() {
        if spec.mde_relative <= 0.0 {
            return Err(Error::Validation(format!(
                "experiments[{i}].mde_relative must be > 0"
            )));
        }
        if spec.baseline_mean <= 0.0 {
            return Err(Error::Validation(format!(
                "experiments[{i}].baseline_mean must be > 0"
            )));
        }
        if spec.baseline_variance <= 0.0 {
            return Err(Error::Validation(format!(
                "experiments[{i}].baseline_variance must be > 0"
            )));
        }
        if spec.n_variants < 2 {
            return Err(Error::Validation(format!(
                "experiments[{i}].n_variants must be ≥ 2"
            )));
        }

        let delta = spec.mde_relative * spec.baseline_mean;
        assert_finite(delta, &format!("experiments[{i}].delta"));

        // n = 2σ²(z_{α/2} + z_β)² / δ²
        let n_f = 2.0 * spec.baseline_variance * z_sum_sq / (delta * delta);
        assert_finite(n_f, &format!("experiments[{i}].n_per_arm_f"));

        let n = n_f.ceil() as u64;
        required_n_per_arm.push(n);
    }

    // Total users needed per experiment = n_per_arm × n_variants.
    let required_total: Vec<f64> = experiments
        .iter()
        .zip(required_n_per_arm.iter())
        .map(|(spec, &n)| {
            let t = n as f64 * spec.n_variants as f64;
            assert_finite(t, "required_total_per_exp");
            t
        })
        .collect();

    let grand_total: f64 = required_total.iter().sum();
    assert_finite(grand_total, "grand_total_required");

    // Allocate proportionally; cap at available_traffic_fraction.
    let allocations: Vec<TrafficAllocation> = experiments
        .iter()
        .zip(required_total.iter())
        .zip(required_n_per_arm.iter())
        .map(|((spec, &req), &n_arm)| {
            let frac = if grand_total > 0.0 {
                (req / grand_total) * available_traffic_fraction
            } else {
                available_traffic_fraction / experiments.len() as f64
            };
            assert_finite(frac, "allocation_fraction");
            TrafficAllocation {
                experiment_id: spec.experiment_id.clone(),
                recommended_traffic_fraction: frac.min(1.0),
                required_n_per_arm: n_arm,
            }
        })
        .collect();

    Ok(allocations)
}

// ---------------------------------------------------------------------------
// Combined entry-point
// ---------------------------------------------------------------------------

/// Run full portfolio power analysis and return a [`PortfolioRecommendation`].
///
/// Combines [`optimal_alpha`], [`annualized_impact`], and
/// [`traffic_allocation_optimizer`] into a single call for convenience.
///
/// The `alpha` used for traffic allocation is the output of `optimal_alpha`;
/// callers may override this by calling the constituent functions directly.
///
/// # Arguments
/// * `portfolio_params` — For alpha recommendation.
/// * `impact_params` — For annualised impact projection.
/// * `traffic_input` — For traffic allocation (note: `traffic_input.alpha`
///   is *overridden* with `optimal_alpha`'s output before optimisation).
pub fn portfolio_power_analysis(
    portfolio_params: &PortfolioParams,
    impact_params: &AnnualizedImpactParams,
    traffic_input: &TrafficAllocationInput,
) -> Result<PortfolioRecommendation> {
    let alpha = optimal_alpha(portfolio_params)?;
    let impact = annualized_impact(impact_params)?;

    // Override alpha in traffic input with the recommended value.
    let effective_traffic_input = TrafficAllocationInput {
        experiments: traffic_input.experiments.clone(),
        available_traffic_fraction: traffic_input.available_traffic_fraction,
        min_power: traffic_input.min_power,
        alpha,
    };
    let allocations = traffic_allocation_optimizer(&effective_traffic_input)?;

    // Expected portfolio FDR at the recommended alpha.
    // FDR = π₀ · α / (π₀ · α + π₁ · power)
    let pi1 = portfolio_params.prior_win_rate;
    let pi0 = 1.0 - pi1;
    let power = portfolio_params.target_power;
    let expected_fdr = (pi0 * alpha) / (pi0 * alpha + pi1 * power);
    assert_finite(expected_fdr, "expected_portfolio_fdr");

    Ok(PortfolioRecommendation {
        optimal_alpha: alpha,
        annualized_impact: impact,
        traffic_allocations: allocations,
        expected_portfolio_fdr: expected_fdr,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn default_portfolio_params() -> PortfolioParams {
        PortfolioParams {
            prior_win_rate: 0.40,
            fdr_target: 0.05,
            target_power: 0.80,
        }
    }

    fn default_impact_params() -> AnnualizedImpactParams {
        AnnualizedImpactParams {
            observed_lift_relative: 0.02,
            annual_baseline_per_user: 150.0,
            total_users: 1_000_000,
            experiment_duration_days: 14.0,
            treatment_fraction: 0.50,
        }
    }

    fn default_experiments() -> Vec<ExperimentSpec> {
        vec![
            ExperimentSpec {
                experiment_id: "exp-a".into(),
                mde_relative: 0.02,
                baseline_mean: 100.0,
                baseline_variance: 400.0,
                n_variants: 2,
            },
            ExperimentSpec {
                experiment_id: "exp-b".into(),
                mde_relative: 0.05,
                baseline_mean: 100.0,
                baseline_variance: 400.0,
                n_variants: 2,
            },
        ]
    }

    // -------------------------------------------------------------------------
    // optimal_alpha tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_optimal_alpha_high_win_rate() {
        // High win rate → larger alpha (more experiments are expected to succeed).
        let params = PortfolioParams {
            prior_win_rate: 0.70,
            fdr_target: 0.05,
            target_power: 0.80,
        };
        let alpha = optimal_alpha(&params).unwrap();
        assert!(alpha > 0.0 && alpha < 1.0, "alpha={alpha}");
        // With 70% win rate the formula gives ~0.093 → unclamped.
        assert!(alpha > 0.05, "expected alpha > 0.05 for high win rate, got {alpha}");
    }

    #[test]
    fn test_optimal_alpha_low_win_rate() {
        // Low win rate → smaller alpha (conservative to avoid false discoveries).
        let params = PortfolioParams {
            prior_win_rate: 0.15,
            fdr_target: 0.05,
            target_power: 0.80,
        };
        let alpha = optimal_alpha(&params).unwrap();
        assert!(alpha > 0.0 && alpha < 1.0, "alpha={alpha}");
        assert!(alpha < 0.05, "expected alpha < 0.05 for low win rate, got {alpha}");
    }

    #[test]
    fn test_optimal_alpha_default_params() {
        let alpha = optimal_alpha(&default_portfolio_params()).unwrap();
        assert!(alpha > 0.0 && alpha < 1.0, "alpha={alpha}");
    }

    #[test]
    fn test_optimal_alpha_clamped_at_max() {
        // Very high win rate: formula exceeds 0.20 → clamped.
        let params = PortfolioParams {
            prior_win_rate: 0.99,
            fdr_target: 0.05,
            target_power: 0.80,
        };
        let alpha = optimal_alpha(&params).unwrap();
        assert!((alpha - 0.20).abs() < 1e-10, "should clamp to 0.20, got {alpha}");
    }

    #[test]
    fn test_optimal_alpha_clamped_at_min() {
        // Very low win rate: formula below 0.001 → clamped.
        let params = PortfolioParams {
            prior_win_rate: 0.001,
            fdr_target: 0.01,
            target_power: 0.80,
        };
        let alpha = optimal_alpha(&params).unwrap();
        assert!((alpha - 0.001).abs() < 1e-10, "should clamp to 0.001, got {alpha}");
    }

    #[test]
    fn test_optimal_alpha_validation_win_rate_zero() {
        let params = PortfolioParams {
            prior_win_rate: 0.0,
            fdr_target: 0.05,
            target_power: 0.80,
        };
        assert!(optimal_alpha(&params).is_err());
    }

    #[test]
    fn test_optimal_alpha_validation_win_rate_one() {
        let params = PortfolioParams {
            prior_win_rate: 1.0,
            fdr_target: 0.05,
            target_power: 0.80,
        };
        assert!(optimal_alpha(&params).is_err());
    }

    #[test]
    fn test_optimal_alpha_validation_fdr() {
        let params = PortfolioParams {
            prior_win_rate: 0.40,
            fdr_target: 0.0,
            target_power: 0.80,
        };
        assert!(optimal_alpha(&params).is_err());

        let params2 = PortfolioParams {
            fdr_target: 1.0,
            ..params
        };
        assert!(optimal_alpha(&params2).is_err());
    }

    #[test]
    fn test_optimal_alpha_validation_power() {
        let params = PortfolioParams {
            prior_win_rate: 0.40,
            fdr_target: 0.05,
            target_power: 0.0,
        };
        assert!(optimal_alpha(&params).is_err());
    }

    #[test]
    fn test_optimal_alpha_increases_with_win_rate() {
        // Monotonicity: higher win rate → higher (or equal) alpha.
        let win_rates = [0.10, 0.25, 0.40, 0.55, 0.70, 0.85];
        let alphas: Vec<f64> = win_rates
            .iter()
            .map(|&w| {
                optimal_alpha(&PortfolioParams {
                    prior_win_rate: w,
                    fdr_target: 0.05,
                    target_power: 0.80,
                })
                .unwrap()
            })
            .collect();
        for i in 1..alphas.len() {
            assert!(
                alphas[i] >= alphas[i - 1] - 1e-12,
                "alpha should be non-decreasing with win rate: {alphas:?}"
            );
        }
    }

    // -------------------------------------------------------------------------
    // annualized_impact tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_annualized_impact_positive() {
        let impact = annualized_impact(&default_impact_params()).unwrap();
        assert!(impact >= 0.0, "impact must be non-negative, got {impact}");
    }

    #[test]
    fn test_annualized_impact_zero_lift() {
        let params = AnnualizedImpactParams {
            observed_lift_relative: 0.0,
            ..default_impact_params()
        };
        let impact = annualized_impact(&params).unwrap();
        assert_eq!(impact, 0.0);
    }

    #[test]
    fn test_annualized_impact_scales_with_users() {
        let base = default_impact_params();
        let doubled = AnnualizedImpactParams {
            total_users: base.total_users * 2,
            ..base.clone()
        };
        let i1 = annualized_impact(&base).unwrap();
        let i2 = annualized_impact(&doubled).unwrap();
        // Doubling users should roughly double the impact.
        assert!(
            (i2 / i1 - 2.0).abs() < 0.1,
            "doubling users should ~double impact: i1={i1}, i2={i2}"
        );
    }

    #[test]
    fn test_annualized_impact_scales_with_baseline() {
        let base = default_impact_params();
        let doubled = AnnualizedImpactParams {
            annual_baseline_per_user: base.annual_baseline_per_user * 2.0,
            ..base.clone()
        };
        let i1 = annualized_impact(&base).unwrap();
        let i2 = annualized_impact(&doubled).unwrap();
        assert!(
            (i2 / i1 - 2.0).abs() < 0.1,
            "doubling baseline should ~double impact: i1={i1}, i2={i2}"
        );
    }

    #[test]
    fn test_annualized_impact_compound_gt_linear_for_short_experiments() {
        // Short experiment duration → high compounding factor → compound > linear.
        let params = AnnualizedImpactParams {
            observed_lift_relative: 0.05,
            annual_baseline_per_user: 100.0,
            total_users: 1_000,
            experiment_duration_days: 7.0, // 52x/year compounding
            treatment_fraction: 1.0,
        };
        let impact = annualized_impact(&params).unwrap();
        // Conservative geometric mean must be ≥ linear part.
        let linear = 0.05 * 100.0 * 1_000.0 * 1.0;
        assert!(impact >= linear * 0.95, "conservative should be close to or above linear for short experiments");
    }

    #[test]
    fn test_annualized_impact_validation_negative_lift() {
        let params = AnnualizedImpactParams {
            observed_lift_relative: -0.01,
            ..default_impact_params()
        };
        assert!(annualized_impact(&params).is_err());
    }

    #[test]
    fn test_annualized_impact_validation_zero_users() {
        let params = AnnualizedImpactParams {
            total_users: 0,
            ..default_impact_params()
        };
        assert!(annualized_impact(&params).is_err());
    }

    #[test]
    fn test_annualized_impact_validation_zero_duration() {
        let params = AnnualizedImpactParams {
            experiment_duration_days: 0.0,
            ..default_impact_params()
        };
        assert!(annualized_impact(&params).is_err());
    }

    #[test]
    fn test_annualized_impact_validation_invalid_treatment_fraction() {
        let params = AnnualizedImpactParams {
            treatment_fraction: 0.0,
            ..default_impact_params()
        };
        assert!(annualized_impact(&params).is_err());

        let params2 = AnnualizedImpactParams {
            treatment_fraction: 1.1,
            ..default_impact_params()
        };
        assert!(annualized_impact(&params2).is_err());
    }

    // -------------------------------------------------------------------------
    // traffic_allocation_optimizer tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_traffic_allocation_fractions_sum_le_available() {
        let input = TrafficAllocationInput {
            experiments: default_experiments(),
            available_traffic_fraction: 0.60,
            min_power: 0.80,
            alpha: 0.05,
        };
        let allocs = traffic_allocation_optimizer(&input).unwrap();
        let total: f64 = allocs.iter().map(|a| a.recommended_traffic_fraction).sum();
        assert!(
            total <= 0.60 + 1e-10,
            "total fraction {total} exceeds available 0.60"
        );
    }

    #[test]
    fn test_traffic_allocation_smaller_mde_gets_more_traffic() {
        // exp-a has smaller MDE (0.02) → needs more users → gets higher fraction.
        let exps = vec![
            ExperimentSpec {
                experiment_id: "small-mde".into(),
                mde_relative: 0.02,
                baseline_mean: 100.0,
                baseline_variance: 400.0,
                n_variants: 2,
            },
            ExperimentSpec {
                experiment_id: "large-mde".into(),
                mde_relative: 0.10,
                baseline_mean: 100.0,
                baseline_variance: 400.0,
                n_variants: 2,
            },
        ];
        let input = TrafficAllocationInput {
            experiments: exps,
            available_traffic_fraction: 0.50,
            min_power: 0.80,
            alpha: 0.05,
        };
        let allocs = traffic_allocation_optimizer(&input).unwrap();
        assert!(
            allocs[0].recommended_traffic_fraction > allocs[1].recommended_traffic_fraction,
            "smaller MDE should get more traffic: {:?}", allocs
        );
    }

    #[test]
    fn test_traffic_allocation_required_n_positive() {
        let input = TrafficAllocationInput {
            experiments: default_experiments(),
            available_traffic_fraction: 0.60,
            min_power: 0.80,
            alpha: 0.05,
        };
        let allocs = traffic_allocation_optimizer(&input).unwrap();
        for a in &allocs {
            assert!(a.required_n_per_arm > 0);
        }
    }

    #[test]
    fn test_traffic_allocation_order_preserved() {
        let exps = default_experiments();
        let ids: Vec<_> = exps.iter().map(|e| e.experiment_id.clone()).collect();
        let input = TrafficAllocationInput {
            experiments: exps,
            available_traffic_fraction: 0.60,
            min_power: 0.80,
            alpha: 0.05,
        };
        let allocs = traffic_allocation_optimizer(&input).unwrap();
        for (i, a) in allocs.iter().enumerate() {
            assert_eq!(a.experiment_id, ids[i]);
        }
    }

    #[test]
    fn test_traffic_allocation_validation_empty() {
        let input = TrafficAllocationInput {
            experiments: vec![],
            available_traffic_fraction: 0.60,
            min_power: 0.80,
            alpha: 0.05,
        };
        assert!(traffic_allocation_optimizer(&input).is_err());
    }

    #[test]
    fn test_traffic_allocation_validation_invalid_alpha() {
        let input = TrafficAllocationInput {
            experiments: default_experiments(),
            available_traffic_fraction: 0.60,
            min_power: 0.80,
            alpha: 0.0,
        };
        assert!(traffic_allocation_optimizer(&input).is_err());
    }

    #[test]
    fn test_traffic_allocation_single_experiment() {
        let input = TrafficAllocationInput {
            experiments: vec![ExperimentSpec {
                experiment_id: "solo".into(),
                mde_relative: 0.05,
                baseline_mean: 50.0,
                baseline_variance: 100.0,
                n_variants: 2,
            }],
            available_traffic_fraction: 0.40,
            min_power: 0.80,
            alpha: 0.05,
        };
        let allocs = traffic_allocation_optimizer(&input).unwrap();
        assert_eq!(allocs.len(), 1);
        assert!((allocs[0].recommended_traffic_fraction - 0.40).abs() < 1e-10);
    }

    // -------------------------------------------------------------------------
    // portfolio_power_analysis integration test
    // -------------------------------------------------------------------------

    #[test]
    fn test_portfolio_power_analysis_combined() {
        let portfolio_params = default_portfolio_params();
        let impact_params = default_impact_params();
        let traffic_input = TrafficAllocationInput {
            experiments: default_experiments(),
            available_traffic_fraction: 0.50,
            min_power: 0.80,
            alpha: 0.05, // overridden by optimal_alpha inside
        };

        let rec = portfolio_power_analysis(&portfolio_params, &impact_params, &traffic_input).unwrap();

        // optimal_alpha in (0, 1)
        assert!(rec.optimal_alpha > 0.0 && rec.optimal_alpha < 1.0);
        // annualized_impact >= 0
        assert!(rec.annualized_impact >= 0.0);
        // correct number of allocations
        assert_eq!(rec.traffic_allocations.len(), 2);
        // expected FDR in (0, 1)
        assert!(rec.expected_portfolio_fdr > 0.0 && rec.expected_portfolio_fdr < 1.0);
    }

    // -------------------------------------------------------------------------
    // Proptest invariants
    // -------------------------------------------------------------------------

    #[cfg(test)]
    mod proptest_portfolio {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// optimal_alpha is always in (0, 1) for any valid input.
            #[test]
            fn optimal_alpha_in_unit_interval(
                win_rate in 0.01f64..0.99f64,
                fdr_target in 0.01f64..0.49f64,
                power in 0.51f64..0.99f64,
            ) {
                let params = PortfolioParams {
                    prior_win_rate: win_rate,
                    fdr_target,
                    target_power: power,
                };
                let alpha = optimal_alpha(&params).unwrap();
                prop_assert!(alpha > 0.0, "alpha must be > 0, got {alpha}");
                prop_assert!(alpha < 1.0, "alpha must be < 1, got {alpha}");
            }

            /// annualized_impact is always ≥ 0 for any valid non-negative lift.
            #[test]
            fn annualized_impact_non_negative(
                lift in 0.0f64..10.0f64,
                baseline in 0.0f64..10_000.0f64,
                users in 1u64..10_000_000u64,
                duration in 1.0f64..365.0f64,
                treatment_frac in 0.01f64..1.0f64,
            ) {
                let params = AnnualizedImpactParams {
                    observed_lift_relative: lift,
                    annual_baseline_per_user: baseline,
                    total_users: users,
                    experiment_duration_days: duration,
                    treatment_fraction: treatment_frac,
                };
                let impact = annualized_impact(&params).unwrap();
                prop_assert!(impact >= 0.0, "impact must be non-negative, got {impact}");
            }

            /// traffic fractions sum to ≤ available_traffic_fraction.
            #[test]
            fn traffic_fractions_budget_respected(
                n_exp in 1usize..=5usize,
                available in 0.1f64..=1.0f64,
                mde in 0.01f64..0.20f64,
                alpha in 0.01f64..0.20f64,
                power in 0.60f64..0.95f64,
            ) {
                let experiments = (0..n_exp)
                    .map(|i| ExperimentSpec {
                        experiment_id: format!("exp-{i}"),
                        mde_relative: mde,
                        baseline_mean: 100.0,
                        baseline_variance: 400.0,
                        n_variants: 2,
                    })
                    .collect();
                let input = TrafficAllocationInput {
                    experiments,
                    available_traffic_fraction: available,
                    min_power: power,
                    alpha,
                };
                let allocs = traffic_allocation_optimizer(&input).unwrap();
                let total: f64 = allocs.iter().map(|a| a.recommended_traffic_fraction).sum();
                prop_assert!(
                    total <= available + 1e-10,
                    "total {total} exceeds available {available}"
                );
            }
        }
    }
}
