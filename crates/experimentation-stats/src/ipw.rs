//! Inverse Propensity Weighting (IPW) adjusted analysis.
//!
//! Hájek estimator with probability clipping for bandit experiments
//! where assignment probabilities vary across observations.
//!
//! Validated against R's `survey::svymean()`. Golden files in `tests/golden/`.

use experimentation_core::error::{assert_finite, Error, Result};

/// A single observation with its outcome, arm assignment, and assignment probability.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IpwObservation {
    pub outcome: f64,
    /// True if this observation was assigned to the treatment arm.
    pub is_treatment: bool,
    /// P(assigned to the actual arm this observation received).
    pub assignment_probability: f64,
}

/// Result of IPW-adjusted analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IpwResult {
    /// Estimated treatment effect (treatment mean - control mean).
    pub effect: f64,
    /// Standard error of the effect estimate (sandwich).
    pub se: f64,
    /// Lower bound of confidence interval.
    pub ci_lower: f64,
    /// Upper bound of confidence interval.
    pub ci_upper: f64,
    /// Two-sided p-value.
    pub p_value: f64,
    /// Total number of observations.
    pub n_observations: usize,
    /// Number of observations with clipped probabilities.
    pub n_clipped: usize,
    /// Kish's effective sample size.
    pub effective_sample_size: f64,
}

/// Compute IPW-adjusted treatment effect using the Hájek estimator.
///
/// # Arguments
/// - `observations`: Slice of observations with outcomes and assignment probabilities.
/// - `alpha`: Significance level for confidence intervals (e.g., 0.05).
/// - `min_probability`: Lower bound for probability clipping (default 0.01).
///
/// # Algorithm
/// 1. Clip probabilities to `[min_probability, 1 - min_probability]`.
/// 2. Compute Hájek weighted means for treatment and control.
/// 3. Sandwich variance estimator.
/// 4. Normal CI and two-sided p-value.
/// 5. Kish's ESS = (sum(w))^2 / sum(w^2).
pub fn ipw_estimate(
    observations: &[IpwObservation],
    alpha: f64,
    min_probability: f64,
) -> Result<IpwResult> {
    if observations.is_empty() {
        return Err(Error::Validation("observations must not be empty".into()));
    }
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation(
            "alpha must be in (0, 1) exclusive".into(),
        ));
    }
    if min_probability <= 0.0 || min_probability >= 0.5 {
        return Err(Error::Validation(
            "min_probability must be in (0, 0.5)".into(),
        ));
    }

    let max_probability = 1.0 - min_probability;

    // Separate treatment and control, clip probabilities.
    let mut treatment_obs = Vec::new();
    let mut control_obs = Vec::new();
    let mut n_clipped = 0usize;

    for obs in observations {
        assert_finite(obs.outcome, "ipw observation outcome");
        assert_finite(obs.assignment_probability, "ipw assignment_probability");

        if obs.assignment_probability <= 0.0 || obs.assignment_probability > 1.0 {
            return Err(Error::Validation(format!(
                "assignment_probability must be in (0, 1], got {}",
                obs.assignment_probability
            )));
        }

        let clipped = obs
            .assignment_probability
            .clamp(min_probability, max_probability);
        if (clipped - obs.assignment_probability).abs() > 1e-15 {
            n_clipped += 1;
        }

        if obs.is_treatment {
            treatment_obs.push((obs.outcome, clipped));
        } else {
            control_obs.push((obs.outcome, clipped));
        }
    }

    if treatment_obs.is_empty() {
        return Err(Error::Validation(
            "no treatment observations provided".into(),
        ));
    }
    if control_obs.is_empty() {
        return Err(Error::Validation("no control observations provided".into()));
    }

    // Hájek estimator: mu = sum(Y_i / p_i) / sum(1 / p_i).
    let (mean_t, weights_t) = hajek_mean(&treatment_obs)?;
    let (mean_c, weights_c) = hajek_mean(&control_obs)?;

    let effect = mean_t - mean_c;
    assert_finite(effect, "ipw effect");

    // Sandwich variance: Var(effect) = Var(mu_t) + Var(mu_c).
    let var_t = hajek_variance(&treatment_obs, mean_t, &weights_t)?;
    let var_c = hajek_variance(&control_obs, mean_c, &weights_c)?;
    let var_effect = var_t + var_c;
    assert_finite(var_effect, "ipw var_effect");

    let se = var_effect.sqrt();
    assert_finite(se, "ipw se");

    // Normal CI.
    let z = {
        use statrs::distribution::{ContinuousCDF, Normal};
        let dist = Normal::new(0.0, 1.0).map_err(|e| Error::Numerical(format!("{e}")))?;
        dist.inverse_cdf(1.0 - alpha / 2.0)
    };
    assert_finite(z, "ipw z");

    let ci_lower = effect - z * se;
    let ci_upper = effect + z * se;
    assert_finite(ci_lower, "ipw ci_lower");
    assert_finite(ci_upper, "ipw ci_upper");

    // Two-sided p-value.
    let p_value = if se < 1e-15 {
        if effect.abs() < 1e-15 {
            1.0
        } else {
            0.0
        }
    } else {
        use statrs::distribution::{ContinuousCDF, Normal};
        let dist = Normal::new(0.0, 1.0).map_err(|e| Error::Numerical(format!("{e}")))?;
        2.0 * (1.0 - dist.cdf((effect / se).abs()))
    };
    assert_finite(p_value, "ipw p_value");

    // Kish's ESS = (sum(w))^2 / sum(w^2) over all observations.
    let all_weights: Vec<f64> = weights_t.iter().chain(weights_c.iter()).copied().collect();
    let sum_w: f64 = all_weights.iter().sum();
    let sum_w2: f64 = all_weights.iter().map(|w| w * w).sum();
    let effective_sample_size = if sum_w2 > 0.0 {
        (sum_w * sum_w) / sum_w2
    } else {
        0.0
    };
    assert_finite(effective_sample_size, "ipw effective_sample_size");

    Ok(IpwResult {
        effect,
        se,
        ci_lower,
        ci_upper,
        p_value,
        n_observations: observations.len(),
        n_clipped,
        effective_sample_size,
    })
}

/// Compute Hájek weighted mean and return the weights (1/p_i).
fn hajek_mean(obs: &[(f64, f64)]) -> Result<(f64, Vec<f64>)> {
    let weights: Vec<f64> = obs.iter().map(|&(_, p)| 1.0 / p).collect();
    let sum_w: f64 = weights.iter().sum();
    assert_finite(sum_w, "hajek sum_w");

    if sum_w.abs() < 1e-15 {
        return Err(Error::Numerical(
            "sum of weights is zero in Hájek estimator".into(),
        ));
    }

    let weighted_sum: f64 = obs
        .iter()
        .zip(weights.iter())
        .map(|(&(y, _), &w)| y * w)
        .sum();
    assert_finite(weighted_sum, "hajek weighted_sum");

    let mean = weighted_sum / sum_w;
    assert_finite(mean, "hajek mean");

    Ok((mean, weights))
}

/// Hájek sandwich variance estimator.
///
/// V(mu_hat) = (1 / (sum_w)^2) * sum(w_i^2 * (Y_i - mu_hat)^2)
fn hajek_variance(obs: &[(f64, f64)], mean: f64, weights: &[f64]) -> Result<f64> {
    let sum_w: f64 = weights.iter().sum();
    assert_finite(sum_w, "hajek_variance sum_w");

    let meat: f64 = obs
        .iter()
        .zip(weights.iter())
        .map(|(&(y, _), &w)| {
            let resid = y - mean;
            w * w * resid * resid
        })
        .sum();
    assert_finite(meat, "hajek_variance meat");

    let var = meat / (sum_w * sum_w);
    assert_finite(var, "hajek_variance result");

    Ok(var)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_uniform_obs(control: &[f64], treatment: &[f64]) -> Vec<IpwObservation> {
        let mut obs = Vec::new();
        for &y in control {
            obs.push(IpwObservation {
                outcome: y,
                is_treatment: false,
                assignment_probability: 0.5,
            });
        }
        for &y in treatment {
            obs.push(IpwObservation {
                outcome: y,
                is_treatment: true,
                assignment_probability: 0.5,
            });
        }
        obs
    }

    #[test]
    fn test_uniform_assignment_matches_simple_diff() {
        let control = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let treatment = vec![6.0, 7.0, 8.0, 9.0, 10.0];
        let obs = make_uniform_obs(&control, &treatment);
        let result = ipw_estimate(&obs, 0.05, 0.01).unwrap();

        // With uniform probabilities, IPW mean = sample mean.
        let expected_effect = 5.0;
        assert!(
            (result.effect - expected_effect).abs() < 1e-10,
            "effect: expected {expected_effect}, got {}",
            result.effect
        );
        assert!(result.p_value < 0.01);
    }

    #[test]
    fn test_clipping_counts() {
        let obs = vec![
            IpwObservation {
                outcome: 1.0,
                is_treatment: false,
                assignment_probability: 0.005, // will be clipped to 0.01
            },
            IpwObservation {
                outcome: 2.0,
                is_treatment: true,
                assignment_probability: 0.5,
            },
        ];
        let result = ipw_estimate(&obs, 0.05, 0.01).unwrap();
        assert_eq!(result.n_clipped, 1);
    }

    #[test]
    fn test_effective_sample_size() {
        let obs = make_uniform_obs(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]);
        let result = ipw_estimate(&obs, 0.05, 0.01).unwrap();
        // With uniform weights, ESS = N.
        assert!(
            (result.effective_sample_size - 6.0).abs() < 1e-10,
            "ESS: expected 6, got {}",
            result.effective_sample_size
        );
    }

    #[test]
    fn test_validation_errors() {
        assert!(ipw_estimate(&[], 0.05, 0.01).is_err());
        assert!(ipw_estimate(
            &[IpwObservation {
                outcome: 1.0,
                is_treatment: true,
                assignment_probability: 0.5
            }],
            0.05,
            0.01
        )
        .is_err()); // no control

        assert!(ipw_estimate(
            &[IpwObservation {
                outcome: 1.0,
                is_treatment: false,
                assignment_probability: 0.5
            }],
            0.05,
            0.01
        )
        .is_err()); // no treatment

        // Boundary: alpha=0.0 and 1.0 are rejected (would cause inverse_cdf(1.0) = inf).
        let obs = make_uniform_obs(&[1.0, 2.0], &[3.0, 4.0]);
        assert!(ipw_estimate(&obs, 0.0, 0.01).is_err());
        assert!(ipw_estimate(&obs, 1.0, 0.01).is_err());
    }

    #[test]
    fn test_varying_probabilities() {
        let obs = vec![
            IpwObservation {
                outcome: 1.0,
                is_treatment: false,
                assignment_probability: 0.7,
            },
            IpwObservation {
                outcome: 2.0,
                is_treatment: false,
                assignment_probability: 0.3,
            },
            IpwObservation {
                outcome: 5.0,
                is_treatment: true,
                assignment_probability: 0.3,
            },
            IpwObservation {
                outcome: 6.0,
                is_treatment: true,
                assignment_probability: 0.7,
            },
        ];
        let result = ipw_estimate(&obs, 0.05, 0.01).unwrap();
        assert!(result.effect > 0.0);
        assert_eq!(result.n_clipped, 0);
        assert!(result.p_value >= 0.0 && result.p_value <= 1.0);
    }
}
