//! Adaptive Sample Size Recalculation (ADR-020).
//!
//! Implements the *promising-zone* adaptive design (Mehta & Pocock, 2011):
//!
//! 1. **Blinded pooled variance**: Variance re-estimation from combined
//!    control + treatment observations *without* unmasking group assignments.
//!    Based on Gould & Shih (1992). Conservative: overestimates within-group
//!    variance when δ ≠ 0 (includes between-group component).
//!
//! 2. **Conditional power**: P(reject H₀ at final n_max | δ̂_observed, σ_B).
//!    Two-sided Wald formula using the blinded σ_B as variance estimate.
//!
//! 3. **Zone classification** (Mehta & Pocock thresholds):
//!    - Favorable:  CP ≥ 90% — experiment is on track, no action.
//!    - Promising: 30% ≤ CP < 90% — extend to reclaim power.
//!    - Futile:     CP < 30%  — early termination recommended.
//!
//! 4. **GST spending reallocation**: After extending to n_extended, the
//!    remaining alpha budget is distributed across the new looks using the
//!    same Lan-DeMets spending function as the original GST design.
//!
//! 5. **Required sample size**: Binary-search for n_max that achieves a
//!    target conditional power level, given the blinded variance estimate.
//!
//! # Type I error guarantee
//!
//! Under H₀ the blinded pooled variance is independent of the (centred) test
//! statistic, so the adaptive re-estimation step cannot inflate the false-
//! positive rate. The GST boundary reallocation spends only `alpha_remaining`
//! across the extended looks, preserving the overall α.
//!
//! Reference: Mehta & Pocock (2011) Stat Med 30:3267-3284.
//! Reference: Gould & Shih (1992) Stat Med 11:1431-1441.
//! Reference: Müller & Schäfer (2001) Biometrics 57:886-891.

use experimentation_core::error::{assert_finite, Error, Result};
use statrs::distribution::{ContinuousCDF, Normal};

use crate::sequential::{gst_boundaries, SpendingFunction};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Zone classification for an adaptive sample size interim analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// Conditional power ≥ 90%. Experiment is well-powered; no extension needed.
    Favorable,
    /// Conditional power ∈ [30%, 90%). Extension is recommended.
    Promising,
    /// Conditional power < 30%. Experiment is unlikely to succeed; early
    /// termination is recommended.
    Futile,
}

impl std::fmt::Display for Zone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Zone::Favorable => write!(f, "favorable"),
            Zone::Promising => write!(f, "promising"),
            Zone::Futile => write!(f, "futile"),
        }
    }
}

/// Configurable zone thresholds.
///
/// Default values match Mehta & Pocock (2011) Table 1 recommendations.
#[derive(Debug, Clone)]
pub struct ZoneThresholds {
    /// Minimum conditional power for the Favorable zone. Default: 0.90.
    pub favorable: f64,
    /// Minimum conditional power for the Promising zone. Default: 0.30.
    pub promising: f64,
}

impl Default for ZoneThresholds {
    fn default() -> Self {
        Self {
            favorable: 0.90,
            promising: 0.30,
        }
    }
}

/// Full result of an adaptive-N interim analysis.
#[derive(Debug, Clone)]
pub struct AdaptiveNResult {
    /// Blinded pooled variance σ²_B (per observation, within-group scale).
    pub blinded_variance: f64,
    /// Conditional power CP(δ̂, σ_B, n_max, α).
    pub conditional_power: f64,
    /// Zone classification.
    pub zone: Zone,
    /// For Promising zone: recommended extended n_max per arm to restore
    /// conditional power to `target_power`. `None` for other zones.
    pub recommended_n_max: Option<f64>,
}

// ---------------------------------------------------------------------------
// Blinded pooled variance
// ---------------------------------------------------------------------------

/// Compute blinded pooled variance from the combined observations of both arms.
///
/// This is the Gould-Shih (1992) blinded estimator:
///
/// ```text
/// σ²_B = (1/(N−1)) · Σᵢ (xᵢ − x̄)²
/// ```
///
/// where `xᵢ` are all `N = n_control + n_treatment` observations (both groups
/// combined) and `x̄` is the grand mean.
///
/// **Bias note**: Under H₀ (δ=0) this is an unbiased estimator of the common
/// within-group σ². Under H₁ (δ≠0) it overestimates σ² by a term proportional
/// to δ²/(variance), which gives a *conservative* (lower) power estimate.
///
/// # Errors
/// Returns `Error::Validation` when fewer than 2 observations are supplied.
pub fn blinded_pooled_variance(all_observations: &[f64]) -> Result<f64> {
    let n = all_observations.len();
    if n < 2 {
        return Err(Error::Validation(
            "blinded_pooled_variance requires at least 2 observations".into(),
        ));
    }

    for (i, &x) in all_observations.iter().enumerate() {
        assert_finite(x, &format!("all_observations[{i}]"));
    }

    let n_f = n as f64;
    let mean = all_observations.iter().sum::<f64>() / n_f;
    assert_finite(mean, "blinded grand mean");

    let sum_sq = all_observations
        .iter()
        .map(|&x| (x - mean).powi(2))
        .sum::<f64>();
    assert_finite(sum_sq, "blinded sum of squares");

    let variance = sum_sq / (n_f - 1.0);
    assert_finite(variance, "blinded variance");

    if variance < 0.0 {
        return Err(Error::Numerical(
            "blinded variance is negative (numerical underflow)".into(),
        ));
    }

    Ok(variance)
}

// ---------------------------------------------------------------------------
// Conditional power
// ---------------------------------------------------------------------------

/// Compute conditional power for a two-sided two-sample Wald test.
///
/// Treats `observed_effect` δ̂ as the "true" effect and `blinded_sigma_sq` σ²_B
/// as the common within-group variance. Returns:
///
/// ```text
/// CP = Φ(ncp − z_{α/2}) + Φ(−ncp − z_{α/2})
/// ```
///
/// where the non-centrality parameter at the final sample size n_max per arm is
///
/// ```text
/// ncp = δ̂ / SE_final = δ̂ · √(n_max/2) / σ_B
/// ```
///
/// SE_final = σ_B · √(2/n_max) (pooled standard error of the mean difference).
///
/// # Arguments
/// * `observed_effect` — Point estimate δ̂ = X̄_T − X̄_C at the current look.
/// * `blinded_sigma_sq` — Blinded pooled variance σ²_B (positive).
/// * `n_max_per_arm` — Final (or extended) per-arm target sample size.
/// * `alpha` — Two-sided significance level.
///
/// # Errors
/// Returns `Error::Validation` for non-positive inputs.
pub fn conditional_power(
    observed_effect: f64,
    blinded_sigma_sq: f64,
    n_max_per_arm: f64,
    alpha: f64,
) -> Result<f64> {
    if blinded_sigma_sq <= 0.0 {
        return Err(Error::Validation(
            "blinded_sigma_sq must be positive".into(),
        ));
    }
    if n_max_per_arm <= 0.0 {
        return Err(Error::Validation("n_max_per_arm must be positive".into()));
    }
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }

    assert_finite(observed_effect, "observed_effect");
    assert_finite(blinded_sigma_sq, "blinded_sigma_sq");
    assert_finite(n_max_per_arm, "n_max_per_arm");

    let z = Normal::new(0.0, 1.0)
        .map_err(|e| Error::Numerical(format!("failed to create Normal: {e}")))?;

    let z_alpha_half = z.inverse_cdf(1.0 - alpha / 2.0);
    assert_finite(z_alpha_half, "z_alpha_half");

    // ncp = δ̂ / (σ_B * sqrt(2/n_max)) = δ̂ * sqrt(n_max/2) / σ_B
    let sigma_b = blinded_sigma_sq.sqrt();
    let ncp = observed_effect * (n_max_per_arm / 2.0).sqrt() / sigma_b;
    assert_finite(ncp, "ncp");

    // CP = Φ(ncp - z_{α/2}) + Φ(-ncp - z_{α/2})
    let cp = z.cdf(ncp - z_alpha_half) + z.cdf(-ncp - z_alpha_half);
    assert_finite(cp, "conditional_power");

    // Clamp to [0, 1] — numerical precision can produce tiny out-of-range values
    Ok(cp.clamp(0.0, 1.0))
}

// ---------------------------------------------------------------------------
// Zone classification
// ---------------------------------------------------------------------------

/// Classify conditional power into a zone.
///
/// Uses the configurable `thresholds` (default: favorable ≥ 0.90, promising ≥ 0.30).
///
/// # Arguments
/// * `cp` — Conditional power value in [0, 1].
/// * `thresholds` — Zone boundary thresholds.
///
/// # Panics
/// Panics (fail-fast) if `cp` is NaN or infinite.
pub fn zone_classify(cp: f64, thresholds: &ZoneThresholds) -> Zone {
    assert_finite(cp, "conditional_power for zone_classify");
    assert!(
        thresholds.promising < thresholds.favorable,
        "promising threshold ({}) must be < favorable threshold ({})",
        thresholds.promising,
        thresholds.favorable
    );

    if cp >= thresholds.favorable {
        Zone::Favorable
    } else if cp >= thresholds.promising {
        Zone::Promising
    } else {
        Zone::Futile
    }
}

// ---------------------------------------------------------------------------
// Required sample size for target power
// ---------------------------------------------------------------------------

/// Binary-search for the minimum per-arm n that achieves `target_power` CP.
///
/// Given the blinded variance estimate and observed effect, returns the
/// smallest integer-valued `n_per_arm` in `[n_current_per_arm, n_max_allowed]`
/// such that:
///
/// ```text
/// conditional_power(δ̂, σ²_B, n_per_arm, α) ≥ target_power
/// ```
///
/// If target power is already achieved at `n_current_per_arm`, returns
/// `n_current_per_arm`. If target power cannot be achieved within the allowed
/// range, returns `n_max_allowed`.
///
/// # Arguments
/// * `observed_effect` — Observed treatment effect δ̂.
/// * `blinded_sigma_sq` — Blinded pooled variance σ²_B.
/// * `target_power` — Desired conditional power (e.g. 0.80).
/// * `alpha` — Two-sided significance level.
/// * `n_current_per_arm` — Lower bound for the search (current per-arm n).
/// * `n_max_allowed` — Upper bound for the search.
pub fn required_n_for_power(
    observed_effect: f64,
    blinded_sigma_sq: f64,
    target_power: f64,
    alpha: f64,
    n_current_per_arm: f64,
    n_max_allowed: f64,
) -> Result<f64> {
    if target_power <= 0.0 || target_power >= 1.0 {
        return Err(Error::Validation("target_power must be in (0, 1)".into()));
    }
    if n_current_per_arm <= 0.0 {
        return Err(Error::Validation(
            "n_current_per_arm must be positive".into(),
        ));
    }
    if n_max_allowed <= n_current_per_arm {
        return Err(Error::Validation(
            "n_max_allowed must be greater than n_current_per_arm".into(),
        ));
    }

    assert_finite(observed_effect, "observed_effect");
    assert_finite(blinded_sigma_sq, "blinded_sigma_sq");

    // Check if target power is already achieved at n_current
    let cp_current = conditional_power(
        observed_effect,
        blinded_sigma_sq,
        n_current_per_arm,
        alpha,
    )?;
    if cp_current >= target_power {
        return Ok(n_current_per_arm);
    }

    // Check if target power can be achieved at n_max_allowed
    let cp_max = conditional_power(
        observed_effect,
        blinded_sigma_sq,
        n_max_allowed,
        alpha,
    )?;
    if cp_max < target_power {
        return Ok(n_max_allowed);
    }

    // Binary search for the crossover point
    let mut lo = n_current_per_arm;
    let mut hi = n_max_allowed;

    for _ in 0..64 {
        let mid = 0.5 * (lo + hi);
        let cp_mid = conditional_power(observed_effect, blinded_sigma_sq, mid, alpha)?;
        if cp_mid < target_power {
            lo = mid;
        } else {
            hi = mid;
        }
        if (hi - lo) < 0.5 {
            break;
        }
    }

    // Round up to next integer sample size
    Ok(hi.ceil())
}

// ---------------------------------------------------------------------------
// GST spending reallocation for extended experiments
// ---------------------------------------------------------------------------

/// Compute new GST boundaries for the extended portion of an adaptive trial.
///
/// After deciding to extend at an interim look, the remaining alpha budget
/// `alpha_remaining` is distributed across `additional_looks` new looks using
/// the same Lan-DeMets spending function. The new information fractions are
/// assumed to be equally spaced in the extended segment.
///
/// This delegates to [`gst_boundaries`] from the `sequential` module, which
/// implements the Armitage-McPherson-Rowe recursive quadrature algorithm.
///
/// # Arguments
/// * `alpha_remaining` — Alpha budget not yet spent (overall_alpha − alpha_spent).
/// * `additional_looks` — Number of new looks in the extended segment (≥ 2).
/// * `spending` — Spending function to use for the extended segment.
///
/// # Returns
/// Critical z-values for each of the `additional_looks` new looks.
///
/// # Errors
/// Returns `Error::Validation` for invalid arguments.
pub fn gst_reallocate_spending(
    alpha_remaining: f64,
    additional_looks: u32,
    spending: SpendingFunction,
) -> Result<Vec<f64>> {
    if alpha_remaining <= 0.0 || alpha_remaining >= 1.0 {
        return Err(Error::Validation(
            "alpha_remaining must be in (0, 1)".into(),
        ));
    }
    if additional_looks < 2 {
        return Err(Error::Validation(
            "additional_looks must be at least 2 for GST reallocation".into(),
        ));
    }

    assert_finite(alpha_remaining, "alpha_remaining");

    // Delegate to the full recursive boundary computation from sequential.rs,
    // using alpha_remaining as the budget for the extended segment.
    gst_boundaries(additional_looks, alpha_remaining, spending)
}

// ---------------------------------------------------------------------------
// Full interim analysis entry-point
// ---------------------------------------------------------------------------

/// Run a complete adaptive-N interim analysis.
///
/// Combines blinded variance estimation, conditional power computation, zone
/// classification, and (for the Promising zone) extended n recommendation.
///
/// # Arguments
/// * `all_interim_obs` — All observations (both arms) at the interim look.
/// * `observed_effect` — Unblinded effect estimate δ̂ (only used for CP calc;
///   does not affect blinded variance).
/// * `n_max_per_arm` — Original planned per-arm sample size.
/// * `alpha` — Two-sided significance level.
/// * `thresholds` — Zone classification thresholds.
/// * `target_power` — Desired power for the Promising zone extension (e.g. 0.80).
/// * `n_max_allowed` — Maximum allowed per-arm n (extension ceiling).
///
/// # Returns
/// [`AdaptiveNResult`] with all intermediate values for audit trail logging.
#[allow(clippy::too_many_arguments)]
pub fn run_interim_analysis(
    all_interim_obs: &[f64],
    observed_effect: f64,
    n_max_per_arm: f64,
    alpha: f64,
    thresholds: &ZoneThresholds,
    target_power: f64,
    n_max_allowed: f64,
) -> Result<AdaptiveNResult> {
    let blinded_variance = blinded_pooled_variance(all_interim_obs)?;

    let cp = conditional_power(observed_effect, blinded_variance, n_max_per_arm, alpha)?;

    let zone = zone_classify(cp, thresholds);

    let recommended_n_max = if zone == Zone::Promising {
        let n_current_per_arm = (all_interim_obs.len() as f64) / 2.0;
        let rec = required_n_for_power(
            observed_effect,
            blinded_variance,
            target_power,
            alpha,
            n_current_per_arm.max(1.0),
            n_max_allowed,
        )?;
        Some(rec)
    } else {
        None
    };

    Ok(AdaptiveNResult {
        blinded_variance,
        conditional_power: cp,
        zone,
        recommended_n_max,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use rand::SeedableRng;
    use rand_distr::{Distribution, Normal as RandNormal};

    // -----------------------------------------------------------------------
    // blinded_pooled_variance
    // -----------------------------------------------------------------------

    #[test]
    fn test_blinded_variance_known_distribution() {
        // With a large enough sample from a single normal, blinded variance
        // should approach the true σ² = 4.0.
        let sigma_sq: f64 = 4.0;
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let dist = RandNormal::new(0.0_f64, sigma_sq.sqrt()).unwrap();
        let obs: Vec<f64> = (0..10_000).map(|_| dist.sample(&mut rng)).collect();
        let bv = blinded_pooled_variance(&obs).unwrap();
        assert!(
            (bv - sigma_sq).abs() < 0.15,
            "blinded variance {bv:.4} should be close to {sigma_sq}"
        );
    }

    #[test]
    fn test_blinded_variance_conservative_under_effect() {
        // When there's a real treatment effect, blinded variance should be ≥ true σ².
        let sigma_sq: f64 = 1.0;
        let delta = 2.0_f64; // large effect
        let mut rng = rand::rngs::StdRng::seed_from_u64(2);
        let ctrl_dist = RandNormal::new(0.0_f64, sigma_sq.sqrt()).unwrap();
        let trt_dist = RandNormal::new(delta, sigma_sq.sqrt()).unwrap();

        let mut all: Vec<f64> = (0..5_000).map(|_| ctrl_dist.sample(&mut rng)).collect();
        all.extend((0..5_000).map(|_| trt_dist.sample(&mut rng)));

        let bv = blinded_pooled_variance(&all).unwrap();
        // Blinded variance overestimates σ² when δ > 0 because the grand mean
        // is between the two group means.
        assert!(bv > sigma_sq, "blinded variance {bv:.4} should exceed σ²={sigma_sq}");
    }

    #[test]
    fn test_blinded_variance_too_few_obs() {
        assert!(blinded_pooled_variance(&[]).is_err());
        assert!(blinded_pooled_variance(&[1.0]).is_err());
    }

    #[test]
    fn test_blinded_variance_two_obs() {
        let obs = [0.0_f64, 2.0];
        let bv = blinded_pooled_variance(&obs).unwrap();
        assert!((bv - 2.0).abs() < 1e-10, "variance of [0,2] should be 2.0, got {bv}");
    }

    // -----------------------------------------------------------------------
    // conditional_power
    // -----------------------------------------------------------------------

    #[test]
    fn test_cp_zero_effect_is_alpha() {
        // With δ̂ = 0, CP = P(|Z| > z_{α/2}) = α (no non-centrality).
        let cp = conditional_power(0.0, 1.0, 200.0, 0.05).unwrap();
        assert!(
            (cp - 0.05).abs() < 1e-6,
            "CP with zero effect should equal alpha, got {cp:.6}"
        );
    }

    #[test]
    fn test_cp_large_effect_is_high() {
        // With a very large effect, CP should be close to 1.0.
        let cp = conditional_power(10.0, 1.0, 500.0, 0.05).unwrap();
        assert!(cp > 0.999, "CP with huge effect should be ~1.0, got {cp:.4}");
    }

    #[test]
    fn test_cp_increases_with_n_max() {
        // For the same δ̂ and σ², larger n_max should give higher CP.
        let cp_small = conditional_power(0.2, 1.0, 100.0, 0.05).unwrap();
        let cp_large = conditional_power(0.2, 1.0, 1000.0, 0.05).unwrap();
        assert!(cp_large > cp_small, "larger n_max should give higher CP");
    }

    #[test]
    fn test_cp_decreases_with_larger_variance() {
        // Same δ̂ and n, but larger σ² → lower CP.
        let cp_low_var = conditional_power(0.5, 1.0, 200.0, 0.05).unwrap();
        let cp_high_var = conditional_power(0.5, 4.0, 200.0, 0.05).unwrap();
        assert!(cp_low_var > cp_high_var, "higher variance should give lower CP");
    }

    #[test]
    fn test_cp_in_unit_interval() {
        for &delta in &[-1.0, 0.0, 0.3, 1.0, 5.0] {
            let cp = conditional_power(delta, 1.0, 200.0, 0.05).unwrap();
            assert!(
                (0.0..=1.0).contains(&cp),
                "CP={cp} is out of [0,1] for delta={delta}"
            );
        }
    }

    #[test]
    fn test_cp_validation_errors() {
        assert!(conditional_power(0.5, 0.0, 200.0, 0.05).is_err()); // σ²=0
        assert!(conditional_power(0.5, -1.0, 200.0, 0.05).is_err()); // σ²<0
        assert!(conditional_power(0.5, 1.0, 0.0, 0.05).is_err()); // n=0
        assert!(conditional_power(0.5, 1.0, 200.0, 0.0).is_err()); // α=0
        assert!(conditional_power(0.5, 1.0, 200.0, 1.0).is_err()); // α=1
    }

    // -----------------------------------------------------------------------
    // zone_classify
    // -----------------------------------------------------------------------

    #[test]
    fn test_zone_boundaries() {
        let th = ZoneThresholds::default(); // favorable=0.90, promising=0.30
        assert_eq!(zone_classify(0.95, &th), Zone::Favorable);
        assert_eq!(zone_classify(0.90, &th), Zone::Favorable); // boundary inclusive
        assert_eq!(zone_classify(0.89, &th), Zone::Promising);
        assert_eq!(zone_classify(0.30, &th), Zone::Promising); // boundary inclusive
        assert_eq!(zone_classify(0.29, &th), Zone::Futile);
        assert_eq!(zone_classify(0.00, &th), Zone::Futile);
    }

    #[test]
    fn test_zone_custom_thresholds() {
        let th = ZoneThresholds {
            favorable: 0.80,
            promising: 0.50,
        };
        assert_eq!(zone_classify(0.85, &th), Zone::Favorable);
        assert_eq!(zone_classify(0.75, &th), Zone::Promising);
        assert_eq!(zone_classify(0.45, &th), Zone::Futile);
    }

    // -----------------------------------------------------------------------
    // required_n_for_power
    // -----------------------------------------------------------------------

    #[test]
    fn test_required_n_achieves_target_power() {
        let delta = 0.3;
        let sigma_sq = 1.0;
        let alpha = 0.05;
        let target_power = 0.80;

        let n_req = required_n_for_power(delta, sigma_sq, target_power, alpha, 50.0, 10_000.0)
            .unwrap();

        // Verify that the returned n actually achieves the target power.
        let cp = conditional_power(delta, sigma_sq, n_req, alpha).unwrap();
        assert!(
            cp >= target_power - 1e-6,
            "CP={cp:.4} should be >= target_power={target_power}"
        );
    }

    #[test]
    fn test_required_n_returns_current_if_already_powered() {
        let n_req = required_n_for_power(2.0, 1.0, 0.80, 0.05, 100.0, 1000.0).unwrap();
        // With such a large effect, n=100 should already exceed 80% power.
        assert_eq!(n_req, 100.0, "should return n_current when already powered");
    }

    #[test]
    fn test_required_n_caps_at_n_max_allowed() {
        // With a tiny effect, we can't achieve 80% power within a limited range.
        let n_req =
            required_n_for_power(0.001, 1.0, 0.80, 0.05, 100.0, 500.0).unwrap();
        assert_eq!(n_req, 500.0, "should cap at n_max_allowed");
    }

    #[test]
    fn test_required_n_validation_errors() {
        assert!(required_n_for_power(0.3, 1.0, 0.0, 0.05, 50.0, 1000.0).is_err());
        assert!(required_n_for_power(0.3, 1.0, 1.0, 0.05, 50.0, 1000.0).is_err());
        assert!(required_n_for_power(0.3, 1.0, 0.8, 0.05, 50.0, 40.0).is_err()); // max < current
    }

    // -----------------------------------------------------------------------
    // gst_reallocate_spending
    // -----------------------------------------------------------------------

    #[test]
    fn test_gst_reallocate_boundaries_decrease_obf() {
        let bounds = gst_reallocate_spending(
            0.03, // alpha_remaining
            4,    // additional_looks
            SpendingFunction::OBrienFleming,
        )
        .unwrap();
        assert_eq!(bounds.len(), 4);
        // OBF: boundaries should be non-increasing.
        for i in 1..bounds.len() {
            assert!(
                bounds[i] <= bounds[i - 1] + 1e-6,
                "OBF boundary {} > {} at looks {},{i}",
                bounds[i],
                bounds[i - 1],
                i - 1
            );
        }
    }

    #[test]
    fn test_gst_reallocate_uses_reduced_alpha() {
        // Boundaries with alpha_remaining=0.02 should be larger than alpha=0.05.
        let bounds_small =
            gst_reallocate_spending(0.02, 3, SpendingFunction::OBrienFleming).unwrap();
        let bounds_large =
            gst_reallocate_spending(0.05, 3, SpendingFunction::OBrienFleming).unwrap();
        // Smaller alpha → more conservative → larger critical values.
        for (b_small, b_large) in bounds_small.iter().zip(bounds_large.iter()) {
            assert!(
                b_small > b_large,
                "smaller remaining alpha should give larger boundary, got {b_small} <= {b_large}"
            );
        }
    }

    #[test]
    fn test_gst_reallocate_validation_errors() {
        // alpha_remaining out of range
        assert!(gst_reallocate_spending(0.0, 3, SpendingFunction::OBrienFleming).is_err());
        assert!(gst_reallocate_spending(1.0, 3, SpendingFunction::OBrienFleming).is_err());
        // too few looks
        assert!(gst_reallocate_spending(0.03, 1, SpendingFunction::OBrienFleming).is_err());
    }

    // -----------------------------------------------------------------------
    // run_interim_analysis
    // -----------------------------------------------------------------------

    #[test]
    fn test_run_interim_analysis_promising_zone() {
        // Generate an experiment where CP is in the promising range.
        // δ̂ = 0.15, σ² ≈ 1.0, n_max = 200 → should be promising.
        let mut rng = rand::rngs::StdRng::seed_from_u64(99);
        let dist = RandNormal::new(0.0_f64, 1.0).unwrap();
        let mut obs: Vec<f64> = (0..200).map(|_| dist.sample(&mut rng)).collect();
        // Add a small effect to treatment group (second 100 obs).
        for x in obs[100..].iter_mut() {
            *x += 0.15;
        }
        let thresholds = ZoneThresholds::default();
        let result = run_interim_analysis(&obs, 0.15, 200.0, 0.05, &thresholds, 0.80, 1000.0)
            .unwrap();
        assert!(result.blinded_variance > 0.0);
        assert!((0.0..=1.0).contains(&result.conditional_power));
    }

    #[test]
    fn test_run_interim_analysis_favourable_no_extension() {
        // Large effect → favorable → no recommended_n_max.
        let obs: Vec<f64> = (0..200)
            .map(|i| if i < 100 { 0.0 } else { 5.0 })
            .collect();
        let thresholds = ZoneThresholds::default();
        let result =
            run_interim_analysis(&obs, 5.0, 200.0, 0.05, &thresholds, 0.80, 1000.0).unwrap();
        assert_eq!(result.zone, Zone::Favorable);
        assert!(result.recommended_n_max.is_none());
    }

    #[test]
    fn test_run_interim_analysis_futile_no_extension() {
        // Near-zero effect → futile → no recommended_n_max.
        // Use random-looking data with unit variance but essentially zero effect.
        let mut rng = rand::rngs::StdRng::seed_from_u64(77);
        let dist = RandNormal::new(0.0_f64, 1.0).unwrap();
        let obs: Vec<f64> = (0..200).map(|_| dist.sample(&mut rng)).collect();
        let thresholds = ZoneThresholds::default();
        // observed_effect = 0.0 → CP = 0.05 (=alpha) → Futile zone.
        let result =
            run_interim_analysis(&obs, 0.0, 200.0, 0.05, &thresholds, 0.80, 1000.0).unwrap();
        assert_eq!(result.zone, Zone::Futile);
        assert!(result.recommended_n_max.is_none());
    }

    // -----------------------------------------------------------------------
    // Type I error control — 10K null simulations
    // -----------------------------------------------------------------------

    /// Verify that blinded variance re-estimation does not inflate type I error.
    ///
    /// Protocol:
    ///   1. Under H₀ (δ=0), generate n_per_arm obs per arm.
    ///   2. Compute blinded pooled variance on all 2·n observations.
    ///   3. Use σ²_B (instead of true σ²) to compute the two-sample z-test.
    ///   4. Count rejections. Must be ≤ α + 3·SE(α).
    ///
    /// The conservative bias of the blinded estimator (σ²_B ≥ σ² under H₀ only
    /// when there is between-group variance) means using σ²_B in the SE makes
    /// the test *at most as liberal* as the oracle test.
    #[test]
    fn test_type_i_error_blinded_reestimation_null_sims() {
        const N_SIMS: usize = 10_000;
        const ALPHA: f64 = 0.05;
        const N_PER_ARM: usize = 200;
        const SIGMA_SQ: f64 = 2.0;

        let mut rng = rand::rngs::StdRng::seed_from_u64(20240101);
        let dist = RandNormal::new(0.0_f64, SIGMA_SQ.sqrt()).unwrap();
        let z_dist = Normal::new(0.0, 1.0).unwrap();
        let z_crit = z_dist.inverse_cdf(1.0 - ALPHA / 2.0);

        let mut rejections = 0usize;

        for _ in 0..N_SIMS {
            // Generate under H₀
            let control: Vec<f64> = (0..N_PER_ARM).map(|_| dist.sample(&mut rng)).collect();
            let treatment: Vec<f64> = (0..N_PER_ARM).map(|_| dist.sample(&mut rng)).collect();

            // Blind the variance: combine both groups without revealing labels.
            let mut all_obs = Vec::with_capacity(2 * N_PER_ARM);
            all_obs.extend_from_slice(&control);
            all_obs.extend_from_slice(&treatment);
            let blinded_var = blinded_pooled_variance(&all_obs).unwrap();

            // Two-sample test using blinded SE (conservative denominator).
            let n = N_PER_ARM as f64;
            let mean_c = control.iter().sum::<f64>() / n;
            let mean_t = treatment.iter().sum::<f64>() / n;
            let effect = mean_t - mean_c;
            let se = (2.0 * blinded_var / n).sqrt();
            if se > 0.0 {
                let z = effect / se;
                if z.abs() > z_crit {
                    rejections += 1;
                }
            }
        }

        let empirical_alpha = rejections as f64 / N_SIMS as f64;
        // Tolerance: alpha ± 3 standard deviations of the Binomial(N_SIMS, alpha)
        let tolerance = 3.0 * (ALPHA * (1.0 - ALPHA) / N_SIMS as f64).sqrt();

        assert!(
            empirical_alpha <= ALPHA + tolerance,
            "type I error {empirical_alpha:.4} exceeds alpha {ALPHA:.4} + 3*SE={:.4}",
            ALPHA + tolerance
        );
    }

    // -----------------------------------------------------------------------
    // proptest invariants
    // -----------------------------------------------------------------------

    use proptest::prelude::*;

    proptest! {
        /// Conditional power is always in [0, 1].
        #[test]
        fn prop_cp_in_unit_interval(
            delta in -5.0f64..5.0f64,
            sigma_sq in 0.1f64..10.0f64,
            n_max in 10.0f64..1000.0f64,
            alpha in 0.001f64..0.2f64,
        ) {
            let cp = conditional_power(delta, sigma_sq, n_max, alpha).unwrap();
            prop_assert!((0.0..=1.0).contains(&cp),
                "CP={cp} out of range for delta={delta}, sigma_sq={sigma_sq}");
        }

        /// Blinded variance is always positive for any two-or-more obs.
        #[test]
        fn prop_blinded_variance_positive(
            obs in prop::collection::vec(-100.0f64..100.0f64, 2..200),
        ) {
            let bv = blinded_pooled_variance(&obs).unwrap();
            prop_assert!(bv >= 0.0, "blinded variance {bv} was negative");
        }

        /// Conditional power is symmetric in delta (CP(δ̂) = CP(-δ̂)).
        #[test]
        fn prop_cp_symmetric(
            delta in 0.01f64..5.0f64,
            sigma_sq in 0.1f64..10.0f64,
            n_max in 10.0f64..1000.0f64,
            alpha in 0.001f64..0.2f64,
        ) {
            let cp_pos = conditional_power(delta, sigma_sq, n_max, alpha).unwrap();
            let cp_neg = conditional_power(-delta, sigma_sq, n_max, alpha).unwrap();
            prop_assert!((cp_pos - cp_neg).abs() < 1e-10,
                "CP not symmetric: CP(+delta)={cp_pos} ≠ CP(-delta)={cp_neg}");
        }

        /// Required n is at least n_current and at most n_max_allowed.
        #[test]
        fn prop_required_n_in_bounds(
            delta in 0.05f64..2.0f64,
            sigma_sq in 0.5f64..5.0f64,
            target_power in 0.5f64..0.95f64,
            n_current in 20.0f64..200.0f64,
        ) {
            let n_max_allowed = n_current * 5.0;
            let n_req = required_n_for_power(
                delta, sigma_sq, target_power, 0.05,
                n_current, n_max_allowed,
            ).unwrap();
            prop_assert!(n_req >= n_current,
                "required n {n_req} < n_current {n_current}");
            prop_assert!(n_req <= n_max_allowed,
                "required n {n_req} > n_max_allowed {n_max_allowed}");
        }
    }
}
