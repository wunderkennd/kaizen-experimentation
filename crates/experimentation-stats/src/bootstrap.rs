//! Bootstrap confidence intervals (percentile and BCa).
//!
//! Implements two bootstrap CI methods for the difference in means:
//!
//! - **Percentile**: Simple quantile-based CI from bootstrap distribution.
//! - **BCa** (Bias-Corrected and Accelerated): Adjusts for bias and skewness
//!   using jackknife influence values. Preferred for skewed data.
//!
//! Both methods use seeded RNG for exact reproducibility.
//! Validated against scipy.stats.bootstrap() with golden-file tests.

use experimentation_core::error::{assert_finite, Error, Result};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use statrs::distribution::{ContinuousCDF, Normal};

/// Which bootstrap method was used.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BootstrapMethod {
    Percentile,
    BCa,
}

/// Result of a bootstrap confidence interval computation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BootstrapResult {
    /// Point estimate: mean(treatment) - mean(control).
    pub effect: f64,
    /// Lower bound of the confidence interval.
    pub ci_lower: f64,
    /// Upper bound of the confidence interval.
    pub ci_upper: f64,
    /// Bootstrap bias estimate: mean(replicates) - observed.
    pub bias: f64,
    /// Which method produced this result.
    pub method: BootstrapMethod,
}

/// Compute a percentile bootstrap CI for the difference in means.
///
/// Resamples each group independently with replacement, computes the
/// difference in means for each replicate, then takes quantiles.
///
/// # Arguments
/// * `control` — Control group observations (≥ 2).
/// * `treatment` — Treatment group observations (≥ 2).
/// * `alpha` — Significance level (e.g. 0.05 for 95% CI).
/// * `n_resamples` — Number of bootstrap replicates (≥ 100).
/// * `seed` — RNG seed for reproducibility.
pub fn bootstrap_ci(
    control: &[f64],
    treatment: &[f64],
    alpha: f64,
    n_resamples: usize,
    seed: u64,
) -> Result<BootstrapResult> {
    validate_inputs(control, treatment, alpha, n_resamples)?;

    let observed = mean(treatment) - mean(control);
    assert_finite(observed, "observed_effect");

    let mut rng = StdRng::seed_from_u64(seed);
    let mut replicates = generate_replicates(control, treatment, n_resamples, &mut rng);

    let bias = mean(&replicates) - observed;
    assert_finite(bias, "bias");

    replicates.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let lo_idx = quantile_index(alpha / 2.0, replicates.len());
    let hi_idx = quantile_index(1.0 - alpha / 2.0, replicates.len());

    Ok(BootstrapResult {
        effect: observed,
        ci_lower: replicates[lo_idx],
        ci_upper: replicates[hi_idx],
        bias,
        method: BootstrapMethod::Percentile,
    })
}

/// Compute a BCa (Bias-Corrected and Accelerated) bootstrap CI.
///
/// Adjusts percentile boundaries for bias and skewness using:
/// - z0: bias-correction factor from proportion of replicates below observed
/// - a: acceleration factor from jackknife influence values
///
/// Preferred over simple percentile when data is skewed.
///
/// # Arguments
/// Same as [`bootstrap_ci`].
pub fn bootstrap_bca(
    control: &[f64],
    treatment: &[f64],
    alpha: f64,
    n_resamples: usize,
    seed: u64,
) -> Result<BootstrapResult> {
    validate_inputs(control, treatment, alpha, n_resamples)?;

    let observed = mean(treatment) - mean(control);
    assert_finite(observed, "observed_effect");

    let mut rng = StdRng::seed_from_u64(seed);
    let mut replicates = generate_replicates(control, treatment, n_resamples, &mut rng);

    let bias = mean(&replicates) - observed;
    assert_finite(bias, "bias");

    let norm = Normal::new(0.0, 1.0)
        .map_err(|e| Error::Numerical(format!("Normal distribution: {e}")))?;

    // z0: bias-correction factor
    let prop_below =
        replicates.iter().filter(|&&r| r < observed).count() as f64 / replicates.len() as f64;
    // Clamp to avoid ±∞ from inverse CDF
    let prop_clamped = prop_below.clamp(1e-10, 1.0 - 1e-10);
    let z0 = norm.inverse_cdf(prop_clamped);
    assert_finite(z0, "z0_bias_correction");

    // Acceleration factor from jackknife
    let a = jackknife_acceleration(control, treatment);
    assert_finite(a, "acceleration");

    // Adjusted percentiles
    let z_lo = norm.inverse_cdf(alpha / 2.0);
    let z_hi = norm.inverse_cdf(1.0 - alpha / 2.0);
    assert_finite(z_lo, "z_lo");
    assert_finite(z_hi, "z_hi");

    let adj_lo = bca_adjusted_quantile(&norm, z0, a, z_lo);
    let adj_hi = bca_adjusted_quantile(&norm, z0, a, z_hi);
    assert_finite(adj_lo, "adj_lo");
    assert_finite(adj_hi, "adj_hi");

    replicates.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let lo_idx = quantile_index(adj_lo, replicates.len());
    let hi_idx = quantile_index(adj_hi, replicates.len());

    Ok(BootstrapResult {
        effect: observed,
        ci_lower: replicates[lo_idx],
        ci_upper: replicates[hi_idx],
        bias,
        method: BootstrapMethod::BCa,
    })
}

/// BCa quantile adjustment: Phi(z0 + (z0 + z) / (1 - a*(z0 + z)))
fn bca_adjusted_quantile(norm: &Normal, z0: f64, a: f64, z: f64) -> f64 {
    let numerator = z0 + z;
    let denominator = 1.0 - a * numerator;

    if denominator.abs() < f64::EPSILON {
        return norm.cdf(z);
    }

    let adjusted = z0 + numerator / denominator;
    assert_finite(adjusted, "bca_adjusted_z");

    let q = norm.cdf(adjusted);
    assert_finite(q, "bca_quantile");
    q
}

/// Compute acceleration factor via delete-one jackknife.
fn jackknife_acceleration(control: &[f64], treatment: &[f64]) -> f64 {
    let n_c = control.len();
    let n_t = treatment.len();

    let control_sum: f64 = control.iter().sum();
    let treatment_sum: f64 = treatment.iter().sum();
    let control_mean = control_sum / n_c as f64;
    let treatment_mean = treatment_sum / n_t as f64;

    let mut jackknife_values = Vec::with_capacity(n_c + n_t);

    for &x in control {
        let new_control_mean = (control_sum - x) / (n_c - 1) as f64;
        jackknife_values.push(treatment_mean - new_control_mean);
    }

    for &x in treatment {
        let new_treatment_mean = (treatment_sum - x) / (n_t - 1) as f64;
        jackknife_values.push(new_treatment_mean - control_mean);
    }

    let theta_dot = mean(&jackknife_values);
    assert_finite(theta_dot, "jackknife_theta_dot");

    let sum_sq: f64 = jackknife_values
        .iter()
        .map(|&v| (theta_dot - v).powi(2))
        .sum();
    let sum_cube: f64 = jackknife_values
        .iter()
        .map(|&v| (theta_dot - v).powi(3))
        .sum();

    if sum_sq < f64::EPSILON {
        return 0.0;
    }

    sum_cube / (6.0 * sum_sq.powf(1.5))
}

fn generate_replicates(
    control: &[f64],
    treatment: &[f64],
    n: usize,
    rng: &mut StdRng,
) -> Vec<f64> {
    let n_c = control.len();
    let n_t = treatment.len();

    (0..n)
        .map(|_| {
            let resampled_control_sum: f64 =
                (0..n_c).map(|_| control[rng.gen_range(0..n_c)]).sum();
            let resampled_treatment_sum: f64 =
                (0..n_t).map(|_| treatment[rng.gen_range(0..n_t)]).sum();

            let effect =
                resampled_treatment_sum / n_t as f64 - resampled_control_sum / n_c as f64;
            assert_finite(effect, "bootstrap_replicate");
            effect
        })
        .collect()
}

fn quantile_index(q: f64, len: usize) -> usize {
    let idx = (q * len as f64).floor() as usize;
    idx.min(len - 1)
}

fn mean(data: &[f64]) -> f64 {
    data.iter().sum::<f64>() / data.len() as f64
}

fn validate_inputs(
    control: &[f64],
    treatment: &[f64],
    alpha: f64,
    n_resamples: usize,
) -> Result<()> {
    if control.len() < 2 {
        return Err(Error::Validation(
            "control group must have >= 2 observations".into(),
        ));
    }
    if treatment.len() < 2 {
        return Err(Error::Validation(
            "treatment group must have >= 2 observations".into(),
        ));
    }
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }
    if n_resamples < 100 {
        return Err(Error::Validation(
            "n_resamples must be >= 100 for reliable estimates".into(),
        ));
    }

    for (i, &v) in control.iter().enumerate() {
        assert_finite(v, &format!("control[{i}]"));
    }
    for (i, &v) in treatment.iter().enumerate() {
        assert_finite(v, &format!("treatment[{i}]"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_percentile_ci() {
        let control = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let treatment = vec![2.0, 3.0, 4.0, 5.0, 6.0];
        let result = bootstrap_ci(&control, &treatment, 0.05, 1000, 42).unwrap();
        assert!((result.effect - 1.0).abs() < 1e-10);
        assert!(result.ci_lower <= result.effect);
        assert!(result.ci_upper >= result.effect);
        assert_eq!(result.method, BootstrapMethod::Percentile);
    }

    #[test]
    fn test_basic_bca_ci() {
        let control = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let treatment = vec![2.0, 3.0, 4.0, 5.0, 6.0];
        let result = bootstrap_bca(&control, &treatment, 0.05, 1000, 42).unwrap();
        assert!((result.effect - 1.0).abs() < 1e-10);
        assert!(result.ci_lower < result.ci_upper);
        assert_eq!(result.method, BootstrapMethod::BCa);
    }

    #[test]
    fn test_no_effect_ci_contains_zero() {
        let control = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let treatment = vec![1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 8.5, 9.5, 0.5];
        let result = bootstrap_ci(&control, &treatment, 0.05, 5000, 42).unwrap();
        assert!(
            result.ci_lower <= 0.0 && result.ci_upper >= 0.0,
            "CI [{}, {}] should contain 0",
            result.ci_lower,
            result.ci_upper
        );
    }

    #[test]
    fn test_reproducibility() {
        let c = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let t = vec![3.0, 4.0, 5.0, 6.0, 7.0];
        let r1 = bootstrap_ci(&c, &t, 0.05, 1000, 123).unwrap();
        let r2 = bootstrap_ci(&c, &t, 0.05, 1000, 123).unwrap();
        assert_eq!(r1.ci_lower, r2.ci_lower);
        assert_eq!(r1.ci_upper, r2.ci_upper);
    }

    #[test]
    fn test_validation_errors() {
        let c = vec![1.0, 2.0, 3.0];
        let t = vec![4.0, 5.0, 6.0];
        assert!(bootstrap_ci(&[1.0], &t, 0.05, 1000, 42).is_err());
        assert!(bootstrap_ci(&c, &[1.0], 0.05, 1000, 42).is_err());
        assert!(bootstrap_ci(&c, &t, 0.0, 1000, 42).is_err());
        assert!(bootstrap_ci(&c, &t, 1.0, 1000, 42).is_err());
        assert!(bootstrap_ci(&c, &t, 0.05, 50, 42).is_err());
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn test_nan_input_panics() {
        let c = vec![1.0, f64::NAN, 3.0];
        let t = vec![4.0, 5.0, 6.0];
        let _ = bootstrap_ci(&c, &t, 0.05, 1000, 42);
    }

    #[test]
    fn test_jackknife_acceleration_symmetric() {
        let control = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let treatment = vec![6.0, 7.0, 8.0, 9.0, 10.0];
        let a = jackknife_acceleration(&control, &treatment);
        assert!(a.abs() < 0.1, "acceleration for symmetric data should be near zero, got {a}");
    }

    #[test]
    fn test_quantile_index_bounds() {
        assert_eq!(quantile_index(0.0, 100), 0);
        assert_eq!(quantile_index(0.5, 100), 50);
        assert_eq!(quantile_index(1.0, 100), 99);
    }

    mod proptest_bootstrap {
        use super::*;
        use proptest::prelude::*;

        fn finite_f64() -> impl Strategy<Value = f64> {
            -1e6f64..1e6f64
        }

        fn valid_sample(min: usize, max: usize) -> impl Strategy<Value = Vec<f64>> {
            prop::collection::vec(finite_f64(), min..=max)
        }

        proptest! {
            #[test]
            fn percentile_ci_lower_le_upper(
                control in valid_sample(3, 20),
                treatment in valid_sample(3, 20),
            ) {
                let result = bootstrap_ci(&control, &treatment, 0.05, 500, 42).unwrap();
                prop_assert!(result.ci_lower <= result.ci_upper);
            }

            #[test]
            fn bca_ci_lower_le_upper(
                control in valid_sample(3, 20),
                treatment in valid_sample(3, 20),
            ) {
                let result = bootstrap_bca(&control, &treatment, 0.05, 500, 42).unwrap();
                prop_assert!(result.ci_lower <= result.ci_upper);
            }

            #[test]
            fn effect_is_exact_mean_difference(
                control in valid_sample(2, 15),
                treatment in valid_sample(2, 15),
            ) {
                let result = bootstrap_ci(&control, &treatment, 0.05, 100, 42).unwrap();
                let expected = mean(&treatment) - mean(&control);
                prop_assert!((result.effect - expected).abs() < 1e-10);
            }

            #[test]
            fn all_outputs_finite(
                control in valid_sample(3, 15),
                treatment in valid_sample(3, 15),
            ) {
                let result = bootstrap_ci(&control, &treatment, 0.05, 200, 42).unwrap();
                prop_assert!(result.effect.is_finite());
                prop_assert!(result.ci_lower.is_finite());
                prop_assert!(result.ci_upper.is_finite());
                prop_assert!(result.bias.is_finite());
            }
        }
    }
}
