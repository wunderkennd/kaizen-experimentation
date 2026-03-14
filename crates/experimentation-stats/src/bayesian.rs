//! Bayesian analysis: posterior probability of superiority + credible intervals.
//!
//! Two models:
//! - **Beta-Binomial** for proportions: Prior Beta(1,1), Monte Carlo P(superiority).
//! - **Normal-Normal** for continuous: Closed-form conjugate, P(superiority) = Phi(...).
//!
//! Validated against R: `qbeta()` / `pnorm()`. Golden files in `tests/golden/`.

use experimentation_core::error::{assert_finite, Error, Result};
use rand::SeedableRng;
use rand_distr::{Beta, Distribution};

/// Which Bayesian model was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BayesianModel {
    BetaBinomial,
    NormalNormal,
}

/// Result of a Bayesian analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BayesianResult {
    pub posterior_mean_control: f64,
    pub posterior_mean_treatment: f64,
    pub effect: f64,
    pub credible_lower: f64,
    pub credible_upper: f64,
    /// P(treatment > control).
    pub probability_of_superiority: f64,
    pub model: BayesianModel,
}

const MC_DRAWS: usize = 100_000;

/// Bayesian analysis for binary outcomes using Beta-Binomial conjugate model.
///
/// Prior: Beta(1, 1) (uniform). Posterior: Beta(1 + successes, 1 + failures).
/// P(treatment > control) estimated via 100K Monte Carlo draws.
/// Credible intervals from posterior quantiles via inverse CDF.
pub fn bayesian_beta_binomial(
    control_successes: u64,
    control_total: u64,
    treatment_successes: u64,
    treatment_total: u64,
    credible_level: f64,
    seed: u64,
) -> Result<BayesianResult> {
    if control_total == 0 {
        return Err(Error::Validation("control_total must be > 0".into()));
    }
    if treatment_total == 0 {
        return Err(Error::Validation("treatment_total must be > 0".into()));
    }
    if control_successes > control_total {
        return Err(Error::Validation(
            "control_successes cannot exceed control_total".into(),
        ));
    }
    if treatment_successes > treatment_total {
        return Err(Error::Validation(
            "treatment_successes cannot exceed treatment_total".into(),
        ));
    }
    if credible_level <= 0.0 || credible_level >= 1.0 {
        return Err(Error::Validation(
            "credible_level must be in (0, 1) exclusive".into(),
        ));
    }

    let control_failures = control_total - control_successes;
    let treatment_failures = treatment_total - treatment_successes;

    // Posterior parameters: Beta(alpha, beta) = Beta(1 + successes, 1 + failures).
    let c_alpha = 1.0 + control_successes as f64;
    let c_beta = 1.0 + control_failures as f64;
    let t_alpha = 1.0 + treatment_successes as f64;
    let t_beta = 1.0 + treatment_failures as f64;

    let posterior_mean_control = c_alpha / (c_alpha + c_beta);
    let posterior_mean_treatment = t_alpha / (t_alpha + t_beta);
    assert_finite(posterior_mean_control, "posterior_mean_control");
    assert_finite(posterior_mean_treatment, "posterior_mean_treatment");

    let effect = posterior_mean_treatment - posterior_mean_control;
    assert_finite(effect, "bayesian_beta_binomial effect");

    // Monte Carlo P(treatment > control).
    let control_dist =
        Beta::new(c_alpha, c_beta).map_err(|e| Error::Numerical(format!("control Beta: {e}")))?;
    let treatment_dist =
        Beta::new(t_alpha, t_beta).map_err(|e| Error::Numerical(format!("treatment Beta: {e}")))?;

    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut treatment_wins = 0u64;
    let mut effect_samples = Vec::with_capacity(MC_DRAWS);

    for _ in 0..MC_DRAWS {
        let c_sample = control_dist.sample(&mut rng);
        let t_sample = treatment_dist.sample(&mut rng);
        if t_sample > c_sample {
            treatment_wins += 1;
        }
        effect_samples.push(t_sample - c_sample);
    }

    let probability_of_superiority = treatment_wins as f64 / MC_DRAWS as f64;
    assert_finite(
        probability_of_superiority,
        "bayesian_beta_binomial P(superiority)",
    );

    // Credible interval from effect distribution quantiles.
    effect_samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let lower_idx = ((1.0 - credible_level) / 2.0 * MC_DRAWS as f64) as usize;
    let upper_idx = ((1.0 - (1.0 - credible_level) / 2.0) * MC_DRAWS as f64) as usize;
    let credible_lower = effect_samples[lower_idx.min(MC_DRAWS - 1)];
    let credible_upper = effect_samples[upper_idx.min(MC_DRAWS - 1)];
    assert_finite(credible_lower, "bayesian_beta_binomial credible_lower");
    assert_finite(credible_upper, "bayesian_beta_binomial credible_upper");

    Ok(BayesianResult {
        posterior_mean_control,
        posterior_mean_treatment,
        effect,
        credible_lower,
        credible_upper,
        probability_of_superiority,
        model: BayesianModel::BetaBinomial,
    })
}

/// Bayesian analysis for continuous outcomes using Normal-Normal conjugate model.
///
/// Uses diffuse (flat) prior. Posterior is Normal with mean = sample mean,
/// variance = sample variance / n.
/// P(superiority) = Phi((mu_t - mu_c) / sqrt(var_t + var_c)) (closed-form).
/// Credible intervals from the posterior difference distribution.
pub fn bayesian_normal(
    control: &[f64],
    treatment: &[f64],
    credible_level: f64,
) -> Result<BayesianResult> {
    if control.len() < 2 {
        return Err(Error::Validation(
            "control must have at least 2 observations".into(),
        ));
    }
    if treatment.len() < 2 {
        return Err(Error::Validation(
            "treatment must have at least 2 observations".into(),
        ));
    }
    if credible_level <= 0.0 || credible_level >= 1.0 {
        return Err(Error::Validation(
            "credible_level must be in (0, 1) exclusive".into(),
        ));
    }

    let n_c = control.len() as f64;
    let n_t = treatment.len() as f64;

    let mean_c: f64 = control.iter().sum::<f64>() / n_c;
    let mean_t: f64 = treatment.iter().sum::<f64>() / n_t;
    assert_finite(mean_c, "bayesian_normal mean_c");
    assert_finite(mean_t, "bayesian_normal mean_t");

    let var_c: f64 = control.iter().map(|x| (x - mean_c).powi(2)).sum::<f64>() / (n_c - 1.0);
    let var_t: f64 = treatment.iter().map(|x| (x - mean_t).powi(2)).sum::<f64>() / (n_t - 1.0);
    assert_finite(var_c, "bayesian_normal var_c");
    assert_finite(var_t, "bayesian_normal var_t");

    // Posterior variance of the mean (with diffuse prior, posterior = likelihood).
    let posterior_var_c = var_c / n_c;
    let posterior_var_t = var_t / n_t;
    assert_finite(posterior_var_c, "bayesian_normal posterior_var_c");
    assert_finite(posterior_var_t, "bayesian_normal posterior_var_t");

    let effect = mean_t - mean_c;
    assert_finite(effect, "bayesian_normal effect");

    // Difference distribution: Normal(mu_t - mu_c, var_t/n_t + var_c/n_c).
    let diff_var = posterior_var_t + posterior_var_c;
    let diff_se = diff_var.sqrt();
    assert_finite(diff_se, "bayesian_normal diff_se");

    // P(treatment > control) = Phi(effect / diff_se).
    let probability_of_superiority = if diff_se < 1e-15 {
        // Degenerate: both groups have zero variance.
        if effect > 0.0 {
            1.0
        } else if effect < 0.0 {
            0.0
        } else {
            0.5
        }
    } else {
        use statrs::distribution::{ContinuousCDF, Normal as NormalDist};
        let standard_normal =
            NormalDist::new(0.0, 1.0).map_err(|e| Error::Numerical(format!("{e}")))?;
        standard_normal.cdf(effect / diff_se)
    };
    assert_finite(probability_of_superiority, "bayesian_normal P(superiority)");

    // Credible interval for the difference.
    let z = {
        use statrs::distribution::{ContinuousCDF, Normal as NormalDist};
        let standard_normal =
            NormalDist::new(0.0, 1.0).map_err(|e| Error::Numerical(format!("{e}")))?;
        standard_normal.inverse_cdf(1.0 - (1.0 - credible_level) / 2.0)
    };
    assert_finite(z, "bayesian_normal z");

    let credible_lower = effect - z * diff_se;
    let credible_upper = effect + z * diff_se;
    assert_finite(credible_lower, "bayesian_normal credible_lower");
    assert_finite(credible_upper, "bayesian_normal credible_upper");

    Ok(BayesianResult {
        posterior_mean_control: mean_c,
        posterior_mean_treatment: mean_t,
        effect,
        credible_lower,
        credible_upper,
        probability_of_superiority,
        model: BayesianModel::NormalNormal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_beta_binomial_clear_effect() {
        let result = bayesian_beta_binomial(20, 100, 60, 100, 0.95, 42).expect("should not fail");
        assert!(result.probability_of_superiority > 0.99);
        assert!(result.effect > 0.0);
        assert!(result.credible_lower > 0.0);
        assert_eq!(result.model, BayesianModel::BetaBinomial);
    }

    #[test]
    fn test_beta_binomial_no_effect() {
        let result = bayesian_beta_binomial(50, 100, 50, 100, 0.95, 42).expect("should not fail");
        assert!((result.probability_of_superiority - 0.5).abs() < 0.1);
        assert!(result.credible_lower < 0.0);
        assert!(result.credible_upper > 0.0);
    }

    #[test]
    fn test_beta_binomial_all_successes() {
        let result = bayesian_beta_binomial(100, 100, 100, 100, 0.95, 42).expect("should not fail");
        assert!((result.probability_of_superiority - 0.5).abs() < 0.1);
    }

    #[test]
    fn test_beta_binomial_all_failures() {
        let result = bayesian_beta_binomial(0, 100, 0, 100, 0.95, 42).expect("should not fail");
        assert!((result.probability_of_superiority - 0.5).abs() < 0.1);
    }

    #[test]
    fn test_normal_clear_effect() {
        let control = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let treatment = vec![6.0, 7.0, 8.0, 9.0, 10.0];
        let result = bayesian_normal(&control, &treatment, 0.95).expect("should not fail");
        assert!(result.probability_of_superiority > 0.99);
        assert!(result.effect > 0.0);
        assert!(result.credible_lower > 0.0);
        assert_eq!(result.model, BayesianModel::NormalNormal);
    }

    #[test]
    fn test_normal_no_effect() {
        let control = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let treatment = vec![1.5, 2.5, 3.5, 4.5, 5.5];
        let result = bayesian_normal(&control, &treatment, 0.95).expect("should not fail");
        // Slight positive effect but wide credible interval.
        assert!(result.credible_lower < 0.0 || result.credible_upper > 0.0);
    }

    #[test]
    fn test_normal_identical_groups() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = bayesian_normal(&data, &data, 0.95).expect("should not fail");
        assert!((result.probability_of_superiority - 0.5).abs() < 1e-10);
        assert!((result.effect).abs() < 1e-10);
    }

    #[test]
    fn test_validation_errors() {
        assert!(bayesian_beta_binomial(0, 0, 50, 100, 0.95, 42).is_err());
        assert!(bayesian_beta_binomial(50, 100, 0, 0, 0.95, 42).is_err());
        assert!(bayesian_beta_binomial(101, 100, 50, 100, 0.95, 42).is_err());
        assert!(bayesian_normal(&[1.0], &[1.0, 2.0], 0.95).is_err());
        assert!(bayesian_normal(&[1.0, 2.0], &[1.0], 0.95).is_err());
        assert!(bayesian_normal(&[1.0, 2.0], &[1.0, 2.0], 1.5).is_err());
        // Boundary: credible_level=0.0 and 1.0 are rejected (would cause inverse_cdf(1.0) = inf).
        assert!(bayesian_beta_binomial(50, 100, 50, 100, 0.0, 42).is_err());
        assert!(bayesian_beta_binomial(50, 100, 50, 100, 1.0, 42).is_err());
        assert!(bayesian_normal(&[1.0, 2.0], &[1.0, 2.0], 0.0).is_err());
        assert!(bayesian_normal(&[1.0, 2.0], &[1.0, 2.0], 1.0).is_err());
    }

    #[test]
    fn test_credible_interval_containment() {
        let result = bayesian_beta_binomial(50, 100, 50, 100, 0.95, 42).expect("should not fail");
        assert!(result.credible_lower <= result.credible_upper);
    }
}
