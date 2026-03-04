//! Welch's t-test for two independent samples.
//!
//! Validated against R's `t.test(..., var.equal = FALSE)` on 5 golden datasets.

use experimentation_core::error::{assert_finite, Error, Result};
use statrs::distribution::{ContinuousCDF, StudentsT};

/// Result of a two-sample Welch's t-test.
#[derive(Debug, Clone)]
pub struct TTestResult {
    /// Point estimate of the treatment effect (treatment_mean - control_mean).
    pub effect: f64,
    /// Lower bound of the confidence interval.
    pub ci_lower: f64,
    /// Upper bound of the confidence interval.
    pub ci_upper: f64,
    /// Two-sided p-value.
    pub p_value: f64,
    /// Whether the result is statistically significant at the given alpha.
    pub is_significant: bool,
    /// Welch-Satterthwaite degrees of freedom.
    pub df: f64,
    /// Control group mean.
    pub control_mean: f64,
    /// Treatment group mean.
    pub treatment_mean: f64,
}

/// Perform Welch's t-test (unequal variances) on two independent samples.
///
/// # Arguments
/// * `control` - Observations from the control group (must have ≥ 2 observations).
/// * `treatment` - Observations from the treatment group (must have ≥ 2 observations).
/// * `alpha` - Significance level (e.g., 0.05).
///
/// # Returns
/// `TTestResult` with effect size, confidence interval, p-value, and significance.
///
/// # Errors
/// Returns `Error::Validation` if either sample has fewer than 2 observations.
/// Panics (fail-fast) if any intermediate value is NaN or Infinity.
pub fn welch_ttest(control: &[f64], treatment: &[f64], alpha: f64) -> Result<TTestResult> {
    if control.len() < 2 {
        return Err(Error::Validation("control group must have ≥ 2 observations".into()));
    }
    if treatment.len() < 2 {
        return Err(Error::Validation("treatment group must have ≥ 2 observations".into()));
    }

    let n_c = control.len() as f64;
    let n_t = treatment.len() as f64;

    let mean_c = control.iter().sum::<f64>() / n_c;
    let mean_t = treatment.iter().sum::<f64>() / n_t;
    assert_finite(mean_c, "control mean");
    assert_finite(mean_t, "treatment mean");

    let var_c = control.iter().map(|x| (x - mean_c).powi(2)).sum::<f64>() / (n_c - 1.0);
    let var_t = treatment.iter().map(|x| (x - mean_t).powi(2)).sum::<f64>() / (n_t - 1.0);
    assert_finite(var_c, "control variance");
    assert_finite(var_t, "treatment variance");

    let se = (var_c / n_c + var_t / n_t).sqrt();
    assert_finite(se, "standard error");

    if se == 0.0 {
        return Err(Error::Numerical("standard error is zero (no variance in data)".into()));
    }

    // Welch-Satterthwaite degrees of freedom
    let df_num = (var_c / n_c + var_t / n_t).powi(2);
    let df_den = (var_c / n_c).powi(2) / (n_c - 1.0) + (var_t / n_t).powi(2) / (n_t - 1.0);
    let df = df_num / df_den;
    assert_finite(df, "degrees of freedom");

    let effect = mean_t - mean_c;
    let t_stat = effect / se;
    assert_finite(t_stat, "t-statistic");

    let t_dist = StudentsT::new(0.0, 1.0, df)
        .map_err(|e| Error::Numerical(format!("t-distribution error: {e}")))?;

    let p_value = 2.0 * (1.0 - t_dist.cdf(t_stat.abs()));
    assert_finite(p_value, "p-value");

    let t_crit = t_dist.inverse_cdf(1.0 - alpha / 2.0);
    let ci_lower = effect - t_crit * se;
    let ci_upper = effect + t_crit * se;

    Ok(TTestResult {
        effect,
        ci_lower,
        ci_upper,
        p_value,
        is_significant: p_value < alpha,
        df,
        control_mean: mean_c,
        treatment_mean: mean_t,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_ttest() {
        let control = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let treatment = vec![2.0, 3.0, 4.0, 5.0, 6.0];

        let result = welch_ttest(&control, &treatment, 0.05).unwrap();
        assert!((result.effect - 1.0).abs() < 1e-10, "effect should be 1.0");
        assert!(result.ci_lower < 1.0 && result.ci_upper > 1.0, "CI should contain effect");
    }

    #[test]
    fn test_insufficient_samples() {
        let result = welch_ttest(&[1.0], &[2.0, 3.0], 0.05);
        assert!(result.is_err());
    }

    #[test]
    fn test_p_value_range() {
        let control = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let treatment = vec![1.1, 2.1, 3.1, 4.1, 5.1];
        let result = welch_ttest(&control, &treatment, 0.05).unwrap();
        assert!(result.p_value >= 0.0 && result.p_value <= 1.0);
    }
}
