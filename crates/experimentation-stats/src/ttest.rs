//! Welch's t-test for two independent samples.
//!
//! Validated against R's `t.test(..., var.equal = FALSE)` on 5 golden datasets.

use experimentation_core::error::{assert_finite, Error, Result};
use statrs::distribution::{ContinuousCDF, StudentsT};

// ---------------------------------------------------------------------------
// Canonical Welch SE primitive — shared across ttest, tost, cate (Closes #583)
// ---------------------------------------------------------------------------

/// Standard error and Welch-Satterthwaite degrees of freedom for a two-sample
/// Welch t-statistic.
///
/// This is the **canonical** Welch SE primitive for the `experimentation-stats`
/// crate. All modules (`ttest`, `tost`, `cate`) must delegate to this function
/// rather than duplicating the formula. See issue #583.
pub(crate) struct WelchSe {
    /// Pooled standard error: `sqrt(var_c / n_c + var_t / n_t)`.
    pub(crate) se: f64,
    /// Welch-Satterthwaite degrees of freedom.
    pub(crate) df: f64,
}

/// Compute the Welch standard error and Satterthwaite degrees of freedom.
///
/// # Arguments
/// * `n_c`   — number of observations in the control group (must be > 1).
/// * `n_t`   — number of observations in the treatment group (must be > 1).
/// * `var_c` — sample variance of the control group.
/// * `var_t` — sample variance of the treatment group.
///
/// # Errors
/// Returns [`Error::Numerical`] if the pooled standard error is exactly zero
/// (both groups have zero variance). Panics (fail-fast) on NaN / non-finite
/// intermediate values via [`assert_finite`].
pub(crate) fn welch_standard_error(
    n_c: f64,
    n_t: f64,
    var_c: f64,
    var_t: f64,
) -> Result<WelchSe> {
    let se = (var_c / n_c + var_t / n_t).sqrt();
    assert_finite(se, "standard error");
    if se == 0.0 {
        return Err(Error::Numerical(
            "standard error is zero (no variance in data)".into(),
        ));
    }

    let df_num = (var_c / n_c + var_t / n_t).powi(2);
    let df_den =
        (var_c / n_c).powi(2) / (n_c - 1.0) + (var_t / n_t).powi(2) / (n_t - 1.0);
    let df = df_num / df_den;
    assert_finite(df, "degrees of freedom");
    Ok(WelchSe { se, df })
}

/// Shared Welch primitive — arithmetic mean of a sample slice.
///
/// This is the canonical mean helper for the `experimentation-stats` crate,
/// consolidating identical inline arithmetic from `ttest`, `tost`, and `cate`.
/// See issue #598.
pub(crate) fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Shared Welch primitive — Bessel-corrected sample variance given a pre-computed mean.
///
/// This is the canonical sample-variance helper for the `experimentation-stats` crate,
/// consolidating identical inline arithmetic from `ttest`, `tost`, and `cate`.
/// See issue #598.
pub(crate) fn sample_variance(xs: &[f64], mean: f64) -> f64 {
    let n = xs.len() as f64;
    let ss: f64 = xs.iter().map(|&x| (x - mean).powi(2)).sum();
    ss / (n - 1.0)
}

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

    let mean_c = mean(control);
    let mean_t = mean(treatment);
    assert_finite(mean_c, "control mean");
    assert_finite(mean_t, "treatment mean");

    let var_c = sample_variance(control, mean_c);
    let var_t = sample_variance(treatment, mean_t);
    assert_finite(var_c, "control variance");
    assert_finite(var_t, "treatment variance");

    let WelchSe { se, df } = welch_standard_error(n_c, n_t, var_c, var_t)?;

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

    /// Verify that all three call paths produce bit-identical Welch SE values.
    ///
    /// Uses the same fixture as `test_basic_ttest` ([1,2,3,4,5] vs [2,3,4,5,6]).
    /// All three paths ultimately delegate to `welch_standard_error`:
    ///
    /// 1. Direct call to the canonical primitive `welch_standard_error`.
    /// 2. `cate::compute_welch_se` (delegates to the canonical primitive).
    /// 3. `tost::tost_equivalence_test` (delegates to the canonical primitive;
    ///    exposes the SE as `TostResult::std_error`).
    ///
    /// All three `assert_eq!` comparisons are bit-identical f64 — not
    /// float-tolerance comparisons. See issue #583.
    #[test]
    fn welch_se_parity_across_call_paths() {
        let control = vec![1.0f64, 2.0, 3.0, 4.0, 5.0];
        let treatment = vec![2.0f64, 3.0, 4.0, 5.0, 6.0];

        // Compute moments from the raw slices (same arithmetic as cate.rs)
        let n_c = control.len() as f64;
        let n_t = treatment.len() as f64;
        let mean_c = control.iter().sum::<f64>() / n_c;
        let mean_t = treatment.iter().sum::<f64>() / n_t;
        let var_c =
            control.iter().map(|x| (x - mean_c).powi(2)).sum::<f64>() / (n_c - 1.0);
        let var_t =
            treatment.iter().map(|x| (x - mean_t).powi(2)).sum::<f64>() / (n_t - 1.0);

        // Path 1: canonical primitive
        let se_canonical = welch_standard_error(n_c, n_t, var_c, var_t)
            .expect("canonical welch_standard_error should not fail on valid data")
            .se;

        // Path 2: cate::compute_welch_se (delegates to Path 1)
        let se_cate = crate::cate::compute_welch_se(&control, &treatment);

        assert_eq!(
            se_canonical, se_cate,
            "welch SE must be bit-identical across all call paths (issue #583)"
        );

        // Path 3: tost::tost_equivalence_test (delegates to Path 1 via welch_standard_error)
        let tost_config = crate::tost::TostConfig { delta: 1.0, alpha: 0.05 };
        let tost_result = crate::tost::tost_equivalence_test(&control, &treatment, &tost_config)
            .expect("tost_equivalence_test should not fail on valid data");
        assert_eq!(se_canonical, tost_result.std_error, "tost path");
    }

    /// Verify that `mean` and `sample_variance` produce bit-identical results to
    /// the inline arithmetic that existed at each of the three former call sites
    /// (`ttest`, `tost`, `cate`) before the helpers were introduced.
    ///
    /// Bit-identity (not float tolerance) is required here because the refactor is
    /// only semantically valid if the helper is mathematically and floating-point
    /// equivalent to the old inline arithmetic — even a single ULP of difference
    /// would indicate that the consolidation changed numerical behaviour. See #598.
    #[test]
    fn mean_variance_parity_across_call_paths() {
        let control = vec![1.0f64, 2.0, 3.0, 4.0, 5.0];
        let treatment = vec![2.0f64, 3.0, 4.0, 5.0, 6.0];

        // ---- Inline arithmetic (verbatim from the three former call sites) ----

        // ttest.rs / cate.rs shared inline form
        let n_c = control.len() as f64;
        let n_t = treatment.len() as f64;
        let mean_c_inline = control.iter().sum::<f64>() / n_c;
        let mean_t_inline = treatment.iter().sum::<f64>() / n_t;
        let var_c_inline =
            control.iter().map(|x| (x - mean_c_inline).powi(2)).sum::<f64>() / (n_c - 1.0);
        let var_t_inline =
            treatment.iter().map(|x| (x - mean_t_inline).powi(2)).sum::<f64>() / (n_t - 1.0);

        // ---- Helper calls (introduced by #598) ----

        let mean_c_helper = mean(&control);
        let mean_t_helper = mean(&treatment);
        let var_c_helper = sample_variance(&control, mean(&control));
        let var_t_helper = sample_variance(&treatment, mean(&treatment));

        // ---- Bit-identical assertions ----

        assert_eq!(
            mean_c_inline, mean_c_helper,
            "mean(control) must be bit-identical to inline arithmetic (issue #598)"
        );
        assert_eq!(
            mean_t_inline, mean_t_helper,
            "mean(treatment) must be bit-identical to inline arithmetic (issue #598)"
        );
        assert_eq!(
            var_c_inline, var_c_helper,
            "sample_variance(control) must be bit-identical to inline arithmetic (issue #598)"
        );
        assert_eq!(
            var_t_inline, var_t_helper,
            "sample_variance(treatment) must be bit-identical to inline arithmetic (issue #598)"
        );
    }
}
