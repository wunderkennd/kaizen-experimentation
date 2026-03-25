//! E-value computation for online testing and FDR control (ADR-018 Phase 1).
//!
//! E-values are nonnegative random variables satisfying E[E] ≤ 1 under H0.
//! Unlike p-values they support optional stopping, arbitrary peeking, and
//! online FDR control via the e-BH procedure.
//!
//! # Methods
//!
//! - [`e_value_grow`]: Sequential GROW martingale for iid Gaussian observations.
//!   Uses the causal plug-in betting strategy λ_t = μ̂_{t-1} / σ², which is the
//!   asymptotically optimal (GROW) strategy that maximises expected log-wealth.
//!
//! - [`e_value_avlm`]: Regression-adjusted mixture e-value (AVLM / CUPED).
//!   Adjusts outcomes using the CUPED estimator θ = Cov(Y,X)/Var(X) to reduce
//!   variance, then applies the Gaussian mixture e-value formula to the adjusted
//!   treatment effect.  Satisfies E_{H0}[E] = 1.
//!
//! # References
//!
//! Ramdas & Wang (2024) "Hypothesis Testing with E-values" (monograph).
//! Waudby-Smith & Ramdas (2023) "Estimating means of bounded random variables
//!   by betting", JRSS-B.
//! Vovk & Wang (2021) "E-values: Calibration, combination, and applications",
//!   Annals of Statistics.

use experimentation_core::error::{assert_finite, Error, Result};

// ---------------------------------------------------------------------------
// Public result type
// ---------------------------------------------------------------------------

/// Result of an e-value computation.
#[derive(Debug, Clone)]
pub struct EValueResult {
    /// The e-value E_n (nonneg; E[E_n] ≤ 1 under H0).
    pub e_value: f64,
    /// Natural log of the e-value: log(E_n).
    /// Numerically stable representation for very large or small e-values.
    pub log_e_value: f64,
    /// Whether to reject H0 at the given α: E_n > 1/α.
    pub reject: bool,
    /// Log-wealth at each time step.  Non-empty only for the sequential GROW
    /// martingale; empty for batch methods.
    pub log_wealth_trajectory: Vec<f64>,
}

// ---------------------------------------------------------------------------
// GROW martingale (sequential, one-sample Gaussian)
// ---------------------------------------------------------------------------

/// Compute the GROW martingale e-value from a sequence of observations.
///
/// Tests H0: μ = 0 against composite H1: μ ≠ 0 with known variance σ².
///
/// The *causal plug-in* (sequential predictive) betting strategy is used:
/// at each time t the bet is
///
/// ```text
/// λ_t = μ̂_{t−1} / σ²
/// ```
///
/// where μ̂_{t−1} is the running sample mean of the preceding t − 1
/// observations (μ̂_0 = 0, so λ_1 = 0 — a safe start).
///
/// The log-wealth process is a nonneg martingale under H0:
///
/// ```text
/// log W_n = Σ_{t=1}^n [ λ_t · X_t  −  λ_t² · σ² / 2 ]
/// E_n = exp(log W_n)
/// ```
///
/// The key identity guaranteeing validity is
/// E_{H0}[exp(λ · X − λ²σ²/2)] = 1 for X ~ N(0, σ²), any fixed λ.
///
/// The plug-in strategy is asymptotically GROW-optimal: it achieves the
/// maximum expected log-wealth growth rate (Kelly criterion) relative to any
/// fixed alternative μ = δ as n → ∞ (Ramdas & Wang §4.2).
///
/// # Arguments
/// * `observations` — Sequential observations X_1, …, X_n from N(μ, σ²).
///   Must have ≥ 1 element.
/// * `sigma_sq` — Known (or pre-estimated) variance σ² > 0.
/// * `alpha` — Rejection threshold: reject when E_n > 1/α.  Must be in (0, 1).
///
/// # Errors
/// Returns `Error::Validation` for invalid inputs.
///
/// # Panics
/// Panics (fail-fast) if any intermediate value is NaN or Infinity.
pub fn e_value_grow(observations: &[f64], sigma_sq: f64, alpha: f64) -> Result<EValueResult> {
    if observations.is_empty() {
        return Err(Error::Validation(
            "observations must have at least one element".into(),
        ));
    }
    if sigma_sq <= 0.0 {
        return Err(Error::Validation("sigma_sq must be positive".into()));
    }
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }

    for (i, &x) in observations.iter().enumerate() {
        assert_finite(x, &format!("observations[{i}]"));
    }

    let n = observations.len();
    let mut log_wealth = 0.0_f64;
    let mut running_sum = 0.0_f64;
    let mut log_wealth_trajectory = Vec::with_capacity(n);

    for (t, &x_t) in observations.iter().enumerate() {
        // Causal bet: λ_t = μ̂_{t-1} / σ²
        // At t=0 (first observation): no prior data → μ̂_0 = 0 → safe start λ_1 = 0
        let mu_hat_prev = if t == 0 {
            0.0
        } else {
            running_sum / (t as f64)
        };
        let lambda_t = mu_hat_prev / sigma_sq;
        assert_finite(lambda_t, "lambda_t");

        // Log-increment: log K_t = λ_t · X_t − λ_t² · σ² / 2
        let log_increment = lambda_t * x_t - 0.5 * lambda_t * lambda_t * sigma_sq;
        assert_finite(log_increment, "log_increment");

        log_wealth += log_increment;
        assert_finite(log_wealth, "log_wealth");

        log_wealth_trajectory.push(log_wealth);
        running_sum += x_t;
    }

    let e_value = log_wealth.exp();
    assert_finite(e_value, "e_value");

    Ok(EValueResult {
        e_value,
        log_e_value: log_wealth,
        reject: e_value > 1.0 / alpha,
        log_wealth_trajectory,
    })
}

// ---------------------------------------------------------------------------
// AVLM mixture e-value (batch, two-sample, regression-adjusted)
// ---------------------------------------------------------------------------

/// Compute the CUPED-adjusted (AVLM) mixture e-value for a two-sample test.
///
/// Adjusts both groups using the CUPED estimator θ = Cov(Y,X)/Var(X) from
/// the pooled sample, reducing the variance of the effect estimate by a
/// factor (1 − ρ²) where ρ is the pooled Y–X correlation.
///
/// The adjusted effect and reduced SE are then plugged into the **Gaussian
/// mixture e-value** formula (Ramdas & Wang §3.1 / Vovk & Wang 2021 §2):
///
/// ```text
/// E = N(Δ_adj; 0, se²_adj + τ²) / N(Δ_adj; 0, se²_adj)
///
/// log E = ½ · log(se²_adj / (se²_adj + τ²))
///         + Δ_adj² · τ² / (2 · se²_adj · (se²_adj + τ²))
/// ```
///
/// This satisfies E_{H0}[E] = 1 exactly (it is the ratio of two normal
/// densities integrated against the null distribution).
///
/// The τ² hyperparameter is the mixing variance (Gaussian prior on effect
/// size).  Larger τ² increases sensitivity to large effects at the cost of
/// power against small ones.  A reasonable default is τ² = (MDE)², where
/// MDE is the minimum detectable effect.
///
/// # Arguments
/// * `control_y` — Control group outcomes (≥ 2 observations).
/// * `treatment_y` — Treatment group outcomes (≥ 2 observations).
/// * `control_x` — Control group covariates (same length as `control_y`).
/// * `treatment_x` — Treatment group covariates (same length as `treatment_y`).
/// * `tau_sq` — Mixing variance τ² > 0 (prior scale on effect size).
/// * `alpha` — Rejection threshold: reject when E > 1/α.
///
/// # Errors
/// Returns `Error::Validation` for invalid inputs.
/// Returns `Error::Numerical` if the adjusted SE is zero (degenerate data).
///
/// # Panics
/// Panics (fail-fast) if any intermediate value is NaN or Infinity.
pub fn e_value_avlm(
    control_y: &[f64],
    treatment_y: &[f64],
    control_x: &[f64],
    treatment_x: &[f64],
    tau_sq: f64,
    alpha: f64,
) -> Result<EValueResult> {
    let n_c = control_y.len();
    let n_t = treatment_y.len();

    if n_c < 2 {
        return Err(Error::Validation(
            "control group must have at least 2 observations".into(),
        ));
    }
    if n_t < 2 {
        return Err(Error::Validation(
            "treatment group must have at least 2 observations".into(),
        ));
    }
    if control_x.len() != n_c {
        return Err(Error::Validation(format!(
            "control_x length ({}) must equal control_y length ({})",
            control_x.len(),
            n_c
        )));
    }
    if treatment_x.len() != n_t {
        return Err(Error::Validation(format!(
            "treatment_x length ({}) must equal treatment_y length ({})",
            treatment_x.len(),
            n_t
        )));
    }
    if tau_sq <= 0.0 {
        return Err(Error::Validation("tau_sq must be positive".into()));
    }
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }

    // Validate all inputs.
    for (i, &v) in control_y.iter().enumerate() {
        assert_finite(v, &format!("control_y[{i}]"));
    }
    for (i, &v) in treatment_y.iter().enumerate() {
        assert_finite(v, &format!("treatment_y[{i}]"));
    }
    for (i, &v) in control_x.iter().enumerate() {
        assert_finite(v, &format!("control_x[{i}]"));
    }
    for (i, &v) in treatment_x.iter().enumerate() {
        assert_finite(v, &format!("treatment_x[{i}]"));
    }

    let n_pooled = (n_c + n_t) as f64;
    let nc = n_c as f64;
    let nt = n_t as f64;

    // -----------------------------------------------------------------------
    // Step 1: pooled means.
    // -----------------------------------------------------------------------
    let mean_y_c = control_y.iter().sum::<f64>() / nc;
    let mean_y_t = treatment_y.iter().sum::<f64>() / nt;
    let mean_x_c = control_x.iter().sum::<f64>() / nc;
    let mean_x_t = treatment_x.iter().sum::<f64>() / nt;

    let sum_y = control_y.iter().sum::<f64>() + treatment_y.iter().sum::<f64>();
    let sum_x = control_x.iter().sum::<f64>() + treatment_x.iter().sum::<f64>();
    let mean_y_pooled = sum_y / n_pooled;
    let mean_x_pooled = sum_x / n_pooled;

    assert_finite(mean_y_pooled, "mean_y_pooled");
    assert_finite(mean_x_pooled, "mean_x_pooled");

    // -----------------------------------------------------------------------
    // Step 2: pooled variance-covariance (ddof = 1).
    // -----------------------------------------------------------------------
    let mut ss_yy = 0.0_f64;
    let mut ss_xx = 0.0_f64;
    let mut ss_xy = 0.0_f64;

    for (&y, &x) in control_y.iter().zip(control_x.iter()) {
        let dy = y - mean_y_pooled;
        let dx = x - mean_x_pooled;
        ss_yy += dy * dy;
        ss_xx += dx * dx;
        ss_xy += dy * dx;
    }
    for (&y, &x) in treatment_y.iter().zip(treatment_x.iter()) {
        let dy = y - mean_y_pooled;
        let dx = x - mean_x_pooled;
        ss_yy += dy * dy;
        ss_xx += dx * dx;
        ss_xy += dy * dx;
    }

    let dof = n_pooled - 1.0;
    let var_y_pooled = ss_yy / dof;
    let var_x_pooled = ss_xx / dof;
    let cov_yx_pooled = ss_xy / dof;

    assert_finite(var_y_pooled, "var_y_pooled");
    assert_finite(var_x_pooled, "var_x_pooled");
    assert_finite(cov_yx_pooled, "cov_yx_pooled");

    // -----------------------------------------------------------------------
    // Step 3: CUPED coefficient θ = Cov(Y,X) / Var(X).
    //         If Var(X) ≈ 0 the covariate carries no information; skip.
    // -----------------------------------------------------------------------
    let theta = if var_x_pooled < f64::EPSILON {
        0.0
    } else {
        cov_yx_pooled / var_x_pooled
    };
    assert_finite(theta, "theta");

    // -----------------------------------------------------------------------
    // Step 4: CUPED-adjusted treatment effect.
    //   Δ_adj = (Ȳ_T − Ȳ_C) − θ · (X̄_T − X̄_C)
    // -----------------------------------------------------------------------
    let delta_raw = mean_y_t - mean_y_c;
    let delta_adj = delta_raw - theta * (mean_x_t - mean_x_c);
    assert_finite(delta_adj, "delta_adj");

    // -----------------------------------------------------------------------
    // Step 5: adjusted pooled variance.
    //   σ²_adj = σ²_Y − θ² · σ²_X  = σ²_Y · (1 − ρ²)
    //   Clamped to a small positive floor to avoid degenerate SE.
    // -----------------------------------------------------------------------
    let sigma_sq_adj =
        (var_y_pooled - theta * theta * var_x_pooled).max(var_y_pooled * 1e-12);
    assert_finite(sigma_sq_adj, "sigma_sq_adj");

    // se²_adj = σ²_adj · (1/n_C + 1/n_T)
    let se_sq_adj = sigma_sq_adj / nc + sigma_sq_adj / nt;
    assert_finite(se_sq_adj, "se_sq_adj");

    if se_sq_adj <= 0.0 {
        return Err(Error::Numerical(
            "adjusted standard error is zero (degenerate data)".into(),
        ));
    }

    // -----------------------------------------------------------------------
    // Step 6: Gaussian mixture e-value (Ramdas & Wang §3.1).
    //
    //   log E = ½ · log(se²_adj / (se²_adj + τ²))
    //           + Δ_adj² · τ² / (2 · se²_adj · (se²_adj + τ²))
    // -----------------------------------------------------------------------
    let se_sq_plus_tau = se_sq_adj + tau_sq;
    let log_e = 0.5 * (se_sq_adj / se_sq_plus_tau).ln()
        + delta_adj * delta_adj * tau_sq / (2.0 * se_sq_adj * se_sq_plus_tau);
    assert_finite(log_e, "log_e_value");

    let e_value = log_e.exp();
    assert_finite(e_value, "e_value");

    Ok(EValueResult {
        e_value,
        log_e_value: log_e,
        reject: e_value > 1.0 / alpha,
        log_wealth_trajectory: vec![],
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(test)]
    use proptest::prelude::*;

    // --- e_value_grow -------------------------------------------------------

    #[test]
    fn test_grow_null_effect_stays_at_one() {
        // Under H0 (μ=0), the safe-start martingale does not move.
        let obs = vec![0.0, 0.0, 0.0, 0.0];
        let r = e_value_grow(&obs, 1.0, 0.05).unwrap();
        assert!((r.e_value - 1.0).abs() < 1e-12, "e_value={}", r.e_value);
        assert!((r.log_e_value).abs() < 1e-12);
        assert!(!r.reject);
        assert_eq!(r.log_wealth_trajectory.len(), 4);
        for &w in &r.log_wealth_trajectory {
            assert!(w.abs() < 1e-12, "trajectory={w}");
        }
    }

    #[test]
    fn test_grow_single_observation_safe_start() {
        // First observation never changes the e-value (λ_1 = 0).
        let r = e_value_grow(&[99.0], 1.0, 0.05).unwrap();
        assert!((r.e_value - 1.0).abs() < 1e-12);
        assert_eq!(r.log_wealth_trajectory, vec![0.0]);
    }

    #[test]
    fn test_grow_constant_positive_observations_analytic() {
        // observations = [1.0, 1.0, 1.0], σ² = 1.0
        //   t=1: λ=0   → log K = 0         log W = 0.0
        //   t=2: λ=1.0 → log K = 0.5       log W = 0.5
        //   t=3: λ=1.0 → log K = 0.5       log W = 1.0
        // E_3 = exp(1.0) = e
        let r = e_value_grow(&[1.0, 1.0, 1.0], 1.0, 0.05).unwrap();
        let traj = &r.log_wealth_trajectory;
        assert!((traj[0] - 0.0).abs() < 1e-12, "traj[0]={}", traj[0]);
        assert!((traj[1] - 0.5).abs() < 1e-12, "traj[1]={}", traj[1]);
        assert!((traj[2] - 1.0).abs() < 1e-12, "traj[2]={}", traj[2]);
        assert!((r.log_e_value - 1.0).abs() < 1e-12);
        assert!((r.e_value - std::f64::consts::E).abs() < 1e-10);
    }

    #[test]
    fn test_grow_negative_signal_reduces_wealth() {
        // observations = [1.0, -2.0], σ² = 1.0
        //   t=1: λ=0   → log K = 0
        //   t=2: λ=1.0 → log K = 1.0*(-2.0) - 0.5 = -2.5   log W = -2.5
        let r = e_value_grow(&[1.0, -2.0], 1.0, 0.05).unwrap();
        assert!((r.log_e_value - (-2.5)).abs() < 1e-12, "log_e={}", r.log_e_value);
        assert!((r.e_value - (-2.5_f64).exp()).abs() < 1e-12);
        assert!(!r.reject);
    }

    #[test]
    fn test_grow_strong_effect_rejects() {
        // Strong, consistent positive signal should cross 1/α = 20.
        let obs: Vec<f64> = vec![2.0; 10];
        let r = e_value_grow(&obs, 1.0, 0.05).unwrap();
        assert!(r.reject, "e_value={}", r.e_value);
        assert!(r.e_value > 20.0);
    }

    #[test]
    fn test_grow_validation_errors() {
        assert!(e_value_grow(&[], 1.0, 0.05).is_err(), "empty obs");
        assert!(e_value_grow(&[1.0], 0.0, 0.05).is_err(), "zero sigma_sq");
        assert!(e_value_grow(&[1.0], -1.0, 0.05).is_err(), "neg sigma_sq");
        assert!(e_value_grow(&[1.0], 1.0, 0.0).is_err(), "zero alpha");
        assert!(e_value_grow(&[1.0], 1.0, 1.0).is_err(), "alpha=1");
    }

    // --- e_value_avlm -------------------------------------------------------

    #[test]
    fn test_avlm_no_effect_below_one() {
        // H0 should produce E ≤ 1/α (not necessarily ≤ 1 in a single run).
        let ctrl_y = vec![0.0, 1.0, -1.0, 0.5, -0.5];
        let trt_y = vec![0.1, 1.1, -0.9, 0.6, -0.4]; // tiny effect
        let ctrl_x = vec![0.0; 5];
        let trt_x = vec![0.0; 5];
        let r = e_value_avlm(&ctrl_y, &trt_y, &ctrl_x, &trt_x, 1.0, 0.05).unwrap();
        // Very small effect: e-value should be modest.
        assert!(r.e_value > 0.0);
        assert!(!r.reject, "e_value={}", r.e_value);
    }

    #[test]
    fn test_avlm_large_effect_rejects() {
        // Large, clear treatment effect should produce E > 1/α = 20.
        let ctrl_y: Vec<f64> = vec![0.0; 50];
        let trt_y: Vec<f64> = vec![5.0; 50]; // large effect
        let ctrl_x = vec![0.0; 50];
        let trt_x = vec![0.0; 50];
        let r = e_value_avlm(&ctrl_y, &trt_y, &ctrl_x, &trt_x, 1.0, 0.05).unwrap();
        assert!(r.reject, "e_value={}", r.e_value);
        assert!(r.e_value > 20.0);
    }

    #[test]
    fn test_avlm_covariate_reduces_se() {
        // With a highly correlated covariate the CUPED adjustment should
        // produce a larger e-value than without it (same effect, less noise).
        let ctrl_y: Vec<f64> = vec![-1.0, 0.0, 1.0, -1.0, 0.0, 1.0];
        let trt_y: Vec<f64> = vec![1.0, 2.0, 3.0, 1.0, 2.0, 3.0]; // effect=2.0
        // Covariate strongly correlated with outcome.
        let ctrl_x: Vec<f64> = vec![-1.0, 0.0, 1.0, -1.0, 0.0, 1.0];
        let trt_x: Vec<f64> = vec![-1.0, 0.0, 1.0, -1.0, 0.0, 1.0];
        let zero_x = vec![0.0_f64; 6];

        let r_with_cov =
            e_value_avlm(&ctrl_y, &trt_y, &ctrl_x, &trt_x, 0.25, 0.05).unwrap();
        let r_without_cov =
            e_value_avlm(&ctrl_y, &trt_y, &zero_x, &zero_x, 0.25, 0.05).unwrap();

        assert!(
            r_with_cov.e_value >= r_without_cov.e_value,
            "with_cov={} without_cov={}",
            r_with_cov.e_value,
            r_without_cov.e_value
        );
    }

    #[test]
    fn test_avlm_trajectory_empty() {
        // Batch method: no trajectory.
        let ctrl_y = vec![0.0, 1.0, -1.0];
        let trt_y = vec![2.0, 3.0, 1.0];
        let x = vec![0.0; 3];
        let r = e_value_avlm(&ctrl_y, &trt_y, &x, &x, 1.0, 0.05).unwrap();
        assert!(r.log_wealth_trajectory.is_empty());
    }

    #[test]
    fn test_avlm_analytic_no_covariate() {
        // control_y=[0,1,-1], treatment_y=[2,3,1], no covariate.
        //
        // Δ_raw = 2.0, σ²_Y = 2.0, se² = 2.0/3 + 2.0/3 = 4/3, τ² = 1.0
        //
        // log E = ½ · log(4/3 / (4/3 + 1)) + 4 · 1 / (2 · 4/3 · 7/3)
        //       = ½ · log(4/7) + 36/56
        //       = log(2) − log(7)/2  +  9/14   (approximately 0.363049)
        let ctrl_y = vec![0.0, 1.0, -1.0];
        let trt_y = vec![2.0, 3.0, 1.0];
        let x = vec![0.0; 3];
        let r = e_value_avlm(&ctrl_y, &trt_y, &x, &x, 1.0, 0.05).unwrap();

        // Analytic log-e: ln(4/7)/2 + 9/14
        let expected_log_e = 0.5 * (4.0_f64 / 7.0).ln() + 9.0 / 14.0;
        assert!(
            (r.log_e_value - expected_log_e).abs() < 1e-10,
            "log_e_value: got {} expected {}",
            r.log_e_value,
            expected_log_e
        );
    }

    #[test]
    fn test_avlm_validation_errors() {
        let y3 = vec![0.0, 1.0, 2.0];
        let x3 = vec![0.0; 3];
        let y1 = vec![0.0];

        assert!(e_value_avlm(&y1, &y3, &y1, &x3, 1.0, 0.05).is_err(), "ctrl<2");
        assert!(e_value_avlm(&y3, &y1, &x3, &y1, 1.0, 0.05).is_err(), "trt<2");
        assert!(e_value_avlm(&y3, &y3, &x3, &x3, 0.0, 0.05).is_err(), "tau_sq=0");
        assert!(e_value_avlm(&y3, &y3, &x3, &x3, 1.0, 0.0).is_err(), "alpha=0");
        assert!(e_value_avlm(&y3, &y3, &x3, &x3, 1.0, 1.0).is_err(), "alpha=1");

        // Mismatched lengths.
        let x2 = vec![0.0; 2];
        assert!(e_value_avlm(&y3, &y3, &x2, &x3, 1.0, 0.05).is_err(), "ctrl_x mismatch");
        assert!(e_value_avlm(&y3, &y3, &x3, &x2, 1.0, 0.05).is_err(), "trt_x mismatch");
    }

    // --- proptest invariants -------------------------------------------------

    proptest! {
        /// e_value_grow always returns a finite, nonnegative e-value and a
        /// trajectory whose length matches the observation count.
        #[test]
        fn grow_outputs_always_finite(
            obs in proptest::collection::vec(-5.0f64..5.0, 1..20),
            sigma_sq in 0.1f64..10.0,
        ) {
            let result = e_value_grow(&obs, sigma_sq, 0.05).unwrap();
            prop_assert!(result.e_value.is_finite(), "e_value not finite: {}", result.e_value);
            prop_assert!(result.e_value >= 0.0, "e_value negative: {}", result.e_value);
            prop_assert!(result.log_e_value.is_finite(), "log_e_value not finite");
            prop_assert_eq!(result.log_wealth_trajectory.len(), obs.len());
            for &w in &result.log_wealth_trajectory {
                prop_assert!(w.is_finite(), "trajectory entry not finite: {w}");
            }
        }

        /// reject is consistent: reject iff e_value > 1/alpha.
        #[test]
        fn grow_reject_consistent_with_threshold(
            obs in proptest::collection::vec(-3.0f64..3.0, 1..15),
            sigma_sq in 0.5f64..5.0,
            alpha in 0.01f64..0.5,
        ) {
            let result = e_value_grow(&obs, sigma_sq, alpha).unwrap();
            let threshold = 1.0 / alpha;
            if result.reject {
                prop_assert!(result.e_value > threshold,
                    "reject=true but e_value={} <= threshold={}", result.e_value, threshold);
            } else {
                prop_assert!(result.e_value <= threshold,
                    "reject=false but e_value={} > threshold={}", result.e_value, threshold);
            }
        }

        /// e_value_avlm always returns a finite, positive e-value and an empty
        /// trajectory (batch method).
        #[test]
        fn avlm_outputs_always_finite(
            ctrl in proptest::collection::vec(-5.0f64..5.0, 2..10),
            trt in proptest::collection::vec(-5.0f64..5.0, 2..10),
            tau_sq in 0.01f64..5.0,
        ) {
            let n_c = ctrl.len();
            let n_t = trt.len();
            let ctrl_x = vec![0.0f64; n_c];
            let trt_x = vec![0.0f64; n_t];
            match e_value_avlm(&ctrl, &trt, &ctrl_x, &trt_x, tau_sq, 0.05) {
                Ok(result) => {
                    prop_assert!(result.e_value.is_finite(), "e_value not finite");
                    prop_assert!(result.e_value >= 0.0, "e_value negative: {}", result.e_value);
                    prop_assert!(result.log_e_value.is_finite(), "log_e_value not finite");
                    prop_assert!(result.log_wealth_trajectory.is_empty(), "batch: trajectory must be empty");
                }
                Err(_) => {
                    // Degenerate (zero-variance) data may yield an error; that is valid.
                }
            }
        }

        /// AVLM reject is consistent: reject iff e_value > 1/alpha.
        #[test]
        fn avlm_reject_consistent_with_threshold(
            ctrl in proptest::collection::vec(-3.0f64..3.0, 2..8),
            trt in proptest::collection::vec(-3.0f64..3.0, 2..8),
            tau_sq in 0.1f64..3.0,
            alpha in 0.01f64..0.5,
        ) {
            let n_c = ctrl.len();
            let n_t = trt.len();
            let cx = vec![0.0f64; n_c];
            let tx = vec![0.0f64; n_t];
            if let Ok(result) = e_value_avlm(&ctrl, &trt, &cx, &tx, tau_sq, alpha) {
                let threshold = 1.0 / alpha;
                if result.reject {
                    prop_assert!(result.e_value > threshold,
                        "reject=true but e_value={} <= threshold={}", result.e_value, threshold);
                } else {
                    prop_assert!(result.e_value <= threshold,
                        "reject=false but e_value={} > threshold={}", result.e_value, threshold);
                }
            }
        }
    }
}
