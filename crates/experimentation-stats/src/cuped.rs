//! CUPED (Controlled-experiment Using Pre-Experiment Data) variance reduction.
//!
//! Reduces variance by adjusting the metric of interest using a pre-experiment
//! covariate that is correlated with the outcome.
//!
//! Formula: Y_adj = Y - θ(X - X̄)
//! where θ = Cov(Y, X) / Var(X), computed on the pooled sample.
//!
//! Validated against numpy/scipy with ddof=1 (equivalent to R's var()/cov()).

use experimentation_core::error::{assert_finite, Error, Result};
use statrs::distribution::{ContinuousCDF, Normal};

/// Result of CUPED variance reduction analysis.
#[derive(Debug, Clone)]
pub struct CupedResult {
    /// Raw treatment effect (treatment_mean - control_mean) before adjustment.
    pub raw_effect: f64,
    /// CUPED-adjusted treatment effect.
    pub adjusted_effect: f64,
    /// Regression coefficient θ = Cov(Y,X) / Var(X).
    pub theta: f64,
    /// Standard error of the raw effect estimate.
    pub raw_se: f64,
    /// Standard error of the adjusted effect estimate.
    pub adjusted_se: f64,
    /// Fraction of variance reduced: 1 - (adjusted_var / raw_var).
    pub variance_reduction: f64,
    /// Lower bound of the confidence interval for the adjusted effect.
    pub ci_lower: f64,
    /// Upper bound of the confidence interval for the adjusted effect.
    pub ci_upper: f64,
    /// CUPED-adjusted control group mean.
    pub control_adjusted_mean: f64,
    /// CUPED-adjusted treatment group mean.
    pub treatment_adjusted_mean: f64,
}

/// Perform CUPED variance reduction on two independent samples.
pub fn cuped_adjust(
    control_y: &[f64],
    treatment_y: &[f64],
    control_x: &[f64],
    treatment_x: &[f64],
    alpha: f64,
) -> Result<CupedResult> {
    if control_y.len() != control_x.len() {
        return Err(Error::Validation(format!(
            "control_y length ({}) != control_x length ({})",
            control_y.len(),
            control_x.len()
        )));
    }
    if treatment_y.len() != treatment_x.len() {
        return Err(Error::Validation(format!(
            "treatment_y length ({}) != treatment_x length ({})",
            treatment_y.len(),
            treatment_x.len()
        )));
    }
    if control_y.len() < 2 {
        return Err(Error::Validation(
            "control group must have at least 2 observations".into(),
        ));
    }
    if treatment_y.len() < 2 {
        return Err(Error::Validation(
            "treatment group must have at least 2 observations".into(),
        ));
    }

    let n_c = control_y.len() as f64;
    let n_t = treatment_y.len() as f64;

    let mean_cy = mean(control_y);
    assert_finite(mean_cy, "mean_control_y");
    let mean_ty = mean(treatment_y);
    assert_finite(mean_ty, "mean_treatment_y");
    let mean_cx = mean(control_x);
    assert_finite(mean_cx, "mean_control_x");
    let mean_tx = mean(treatment_x);
    assert_finite(mean_tx, "mean_treatment_x");

    let all_y: Vec<f64> = control_y.iter().chain(treatment_y.iter()).copied().collect();
    let all_x: Vec<f64> = control_x.iter().chain(treatment_x.iter()).copied().collect();

    let mean_y = mean(&all_y);
    assert_finite(mean_y, "mean_pooled_y");
    let mean_x = mean(&all_x);
    assert_finite(mean_x, "mean_pooled_x");

    let var_x = variance(&all_x, mean_x);
    assert_finite(var_x, "var_x");
    if var_x == 0.0 {
        return Err(Error::Numerical(
            "covariate variance is zero — cannot compute CUPED adjustment".into(),
        ));
    }

    let cov_xy = covariance(&all_y, &all_x, mean_y, mean_x);
    assert_finite(cov_xy, "cov_xy");

    let theta = cov_xy / var_x;
    assert_finite(theta, "theta");

    let control_adj: Vec<f64> = control_y
        .iter()
        .zip(control_x.iter())
        .map(|(&y, &x)| {
            let adj = y - theta * (x - mean_x);
            assert_finite(adj, "control_adjusted_obs");
            adj
        })
        .collect();

    let treatment_adj: Vec<f64> = treatment_y
        .iter()
        .zip(treatment_x.iter())
        .map(|(&y, &x)| {
            let adj = y - theta * (x - mean_x);
            assert_finite(adj, "treatment_adjusted_obs");
            adj
        })
        .collect();

    let control_adj_mean = mean(&control_adj);
    assert_finite(control_adj_mean, "control_adjusted_mean");
    let treatment_adj_mean = mean(&treatment_adj);
    assert_finite(treatment_adj_mean, "treatment_adjusted_mean");

    let raw_effect = mean_ty - mean_cy;
    assert_finite(raw_effect, "raw_effect");

    let raw_var_c = variance(control_y, mean_cy);
    let raw_var_t = variance(treatment_y, mean_ty);
    let raw_se = (raw_var_c / n_c + raw_var_t / n_t).sqrt();
    assert_finite(raw_se, "raw_se");

    let adjusted_effect = treatment_adj_mean - control_adj_mean;
    assert_finite(adjusted_effect, "adjusted_effect");

    let adj_var_c = variance(&control_adj, control_adj_mean);
    let adj_var_t = variance(&treatment_adj, treatment_adj_mean);
    let adjusted_se = (adj_var_c / n_c + adj_var_t / n_t).sqrt();
    assert_finite(adjusted_se, "adjusted_se");

    let raw_var_diff = raw_var_c / n_c + raw_var_t / n_t;
    let adj_var_diff = adj_var_c / n_c + adj_var_t / n_t;
    let variance_reduction = if raw_var_diff > 0.0 {
        1.0 - adj_var_diff / raw_var_diff
    } else {
        0.0
    };
    assert_finite(variance_reduction, "variance_reduction");

    let z = Normal::new(0.0, 1.0)
        .map_err(|e| Error::Numerical(format!("failed to create Normal distribution: {e}")))?;
    let z_alpha = z.inverse_cdf(1.0 - alpha / 2.0);
    assert_finite(z_alpha, "z_alpha");

    let ci_lower = adjusted_effect - z_alpha * adjusted_se;
    assert_finite(ci_lower, "ci_lower");
    let ci_upper = adjusted_effect + z_alpha * adjusted_se;
    assert_finite(ci_upper, "ci_upper");

    Ok(CupedResult {
        raw_effect,
        adjusted_effect,
        theta,
        raw_se,
        adjusted_se,
        variance_reduction,
        ci_lower,
        ci_upper,
        control_adjusted_mean: control_adj_mean,
        treatment_adjusted_mean: treatment_adj_mean,
    })
}

fn mean(data: &[f64]) -> f64 {
    data.iter().sum::<f64>() / data.len() as f64
}

fn variance(data: &[f64], mean: f64) -> f64 {
    let n = data.len() as f64;
    let ss: f64 = data.iter().map(|&x| (x - mean).powi(2)).sum();
    ss / (n - 1.0)
}

fn covariance(y: &[f64], x: &[f64], mean_y: f64, mean_x: f64) -> f64 {
    let n = y.len() as f64;
    let ss: f64 = y
        .iter()
        .zip(x.iter())
        .map(|(&yi, &xi)| (yi - mean_y) * (xi - mean_x))
        .sum();
    ss / (n - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mean_basic() {
        assert!((mean(&[1.0, 2.0, 3.0]) - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_variance_basic() {
        let data = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let m = mean(&data);
        let v = variance(&data, m);
        assert!((v - 4.571429).abs() < 1e-4);
    }

    #[test]
    fn test_cuped_reduces_variance_with_correlated_covariate() {
        let control_x: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let control_y: Vec<f64> = control_x.iter().map(|&x| x * 2.0 + 1.0).collect();
        let treatment_x: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let treatment_y: Vec<f64> = treatment_x.iter().map(|&x| x * 2.0 + 3.0).collect();

        let result =
            cuped_adjust(&control_y, &treatment_y, &control_x, &treatment_x, 0.05).unwrap();
        assert!(
            result.variance_reduction > 0.90,
            "Expected high variance reduction, got {}",
            result.variance_reduction
        );
        assert!(result.adjusted_se < result.raw_se);
    }

    #[test]
    fn test_cuped_validation_errors() {
        assert!(cuped_adjust(&[1.0], &[2.0, 3.0], &[1.0], &[2.0, 3.0]).is_err());
        assert!(cuped_adjust(&[1.0, 2.0], &[3.0], &[1.0, 2.0], &[3.0]).is_err());
        assert!(cuped_adjust(&[1.0, 2.0], &[3.0, 4.0], &[1.0], &[3.0, 4.0]).is_err());
    }
}
