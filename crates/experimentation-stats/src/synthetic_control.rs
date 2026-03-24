//! Synthetic Control Methods for quasi-experimental evaluation (ADR-023).
//!
//! Implements four synthetic control variants for evaluating interventions
//! that cannot be randomized (market-level launches, content events, policy changes).
//!
//! # Methods
//!
//! - [`Method::Classic`]: Abadie-Diamond-Hainmueller (2010). Constrained
//!   optimization finds donor weights w_j ≥ 0, Σw_j = 1 minimizing pre-treatment
//!   MSE. Rank-based placebo permutation inference.
//!
//! - [`Method::Augmented`]: Ben-Michael-Feller-Rothstein (2021). Ridge-based
//!   bias correction on top of Classic SCM handles imperfect pre-treatment fit.
//!   Conformal CIs from pre-treatment residual standard error.
//!
//! - [`Method::SDiD`]: Arkhangelsky et al. (2021) Synthetic DiD. Unit weights
//!   (SCM-style) + time weights (DiD-style). Doubly robust. Jackknife inference.
//!
//! - [`Method::CausalImpact`]: Brodersen et al. (2015). Local linear trend
//!   state-space model with donors as covariates. Kalman filter counterfactual
//!   prediction with prediction-interval-based CIs.
//!
//! # Validation
//!
//! Golden files validate analytically derivable cases (2-donor perfect fit) and
//! augsynth-compatible synthetic datasets to 4 decimal places.
//! Proptest: donor weights sum to 1.0 ± 1e-9 for Classic and Augmented.
//!
//! # References
//!
//! - Abadie, Diamond, Hainmueller: JASA (2010).
//! - Ben-Michael, Feller, Rothstein: JASA (2021). R `augsynth` package.
//! - Arkhangelsky et al.: AER (2021).
//! - Brodersen et al.: Annals of Applied Statistics (2015).

use std::collections::HashMap;

use experimentation_core::error::{assert_finite, Error, Result};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Synthetic control method variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Method {
    /// Classic SCM (Abadie et al. 2010): convex donor weights, placebo inference.
    Classic,
    /// Augmented SCM (Ben-Michael et al. 2021): Ridge bias correction, conformal CIs.
    Augmented,
    /// Synthetic DiD (Arkhangelsky et al. 2021): unit + time weights, jackknife CIs.
    SDiD,
    /// CausalImpact (Brodersen et al. 2015): Kalman filter state-space model.
    CausalImpact,
}

/// Result of a synthetic control analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyntheticControlResult {
    /// Analysis method used.
    pub method: Method,
    /// Average Treatment on the Treated (ATT): mean post-treatment gap
    /// between the treated unit and its synthetic control.
    pub att: f64,
    /// Lower bound of (1-α) confidence interval for ATT.
    pub ci_lower: f64,
    /// Upper bound of (1-α) confidence interval for ATT.
    pub ci_upper: f64,
    /// Donor weights for each donor unit.
    ///
    /// For Classic and Augmented: all ≥ 0.0, sum = 1.0 (probability simplex).
    /// For SDiD: unit weights ω_j ≥ 0.0, Σω_j = 1.0.
    /// For CausalImpact: OLS regression coefficients (unrestricted).
    pub donor_weights: HashMap<String, f64>,
    /// Rank-based placebo p-value (leave-one-out permutation).
    ///
    /// p = (1 + #{k : |att_k| ≥ |att_treated|}) / (1 + n_donors).
    /// Returns 1.0 when n_donors < 2 (insufficient for permutation test).
    pub placebo_p_value: f64,
}

/// Panel data for synthetic control analysis.
#[derive(Debug)]
pub struct SyntheticControlInput {
    /// Name of the treated unit.
    pub treated_unit: String,
    /// Outcome time series for the treated unit.
    /// Length = `pre_periods + post_periods`.
    pub treated_series: Vec<f64>,
    /// Donor unit names and their outcome time series (same length as `treated_series`).
    pub donors: Vec<(String, Vec<f64>)>,
    /// Number of pre-treatment periods (first `pre_periods` observations).
    pub pre_periods: usize,
    /// Significance level α for confidence intervals (default: 0.05).
    pub alpha: f64,
}

impl SyntheticControlInput {
    /// Create input with default α = 0.05.
    pub fn new(
        treated_unit: impl Into<String>,
        treated_series: Vec<f64>,
        donors: Vec<(String, Vec<f64>)>,
        pre_periods: usize,
    ) -> Self {
        Self {
            treated_unit: treated_unit.into(),
            treated_series,
            donors,
            pre_periods,
            alpha: 0.05,
        }
    }

    fn post_periods(&self) -> usize {
        self.treated_series.len() - self.pre_periods
    }

    fn validate(&self) -> Result<()> {
        let n = self.treated_series.len();
        if self.pre_periods == 0 {
            return Err(Error::Validation("pre_periods must be ≥ 1".into()));
        }
        if self.pre_periods >= n {
            return Err(Error::Validation(
                "pre_periods must be < total time periods".into(),
            ));
        }
        if self.donors.is_empty() {
            return Err(Error::Validation("at least one donor unit required".into()));
        }
        if self.alpha <= 0.0 || self.alpha >= 1.0 {
            return Err(Error::Validation("alpha must be in (0, 1)".into()));
        }
        for (i, &y) in self.treated_series.iter().enumerate() {
            assert_finite(y, &format!("treated_series[{i}]"));
        }
        for (name, series) in &self.donors {
            if series.len() != n {
                return Err(Error::Validation(format!(
                    "donor '{name}' has {} observations, expected {n}",
                    series.len()
                )));
            }
            for (i, &y) in series.iter().enumerate() {
                assert_finite(y, &format!("donor '{name}'[{i}]"));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run a synthetic control analysis with the specified method.
///
/// # Errors
///
/// Returns `Err` if the input is malformed (mismatched lengths, zero pre-treatment
/// periods, singular optimization problem) or if numerical computation fails.
pub fn synthetic_control(
    input: &SyntheticControlInput,
    method: Method,
) -> Result<SyntheticControlResult> {
    input.validate()?;
    match method {
        Method::Classic => classic_scm(input),
        Method::Augmented => augmented_scm(input),
        Method::SDiD => sdid(input),
        Method::CausalImpact => causal_impact(input),
    }
}

// ---------------------------------------------------------------------------
// Classic SCM — Abadie, Diamond, Hainmueller (2010)
// ---------------------------------------------------------------------------

fn classic_scm(input: &SyntheticControlInput) -> Result<SyntheticControlResult> {
    let pre = input.pre_periods;
    let weights =
        solve_simplex_weights(&input.treated_series[..pre], &input.donors, pre)?;

    let att = compute_att(&input.treated_series, &input.donors, &weights, pre);
    assert_finite(att, "classic_att");

    let placebo_p = placebo_permutation_p(input, att, Method::Classic)?;
    let (ci_lower, ci_upper) = placebo_ci_normal(input, att, input.alpha)?;
    assert_finite(ci_lower, "classic_ci_lower");
    assert_finite(ci_upper, "classic_ci_upper");

    Ok(SyntheticControlResult {
        method: Method::Classic,
        att,
        ci_lower,
        ci_upper,
        donor_weights: weights_to_map(&input.donors, &weights),
        placebo_p_value: placebo_p,
    })
}

// ---------------------------------------------------------------------------
// Augmented SCM — Ben-Michael, Feller, Rothstein (2021)
// ---------------------------------------------------------------------------

fn augmented_scm(input: &SyntheticControlInput) -> Result<SyntheticControlResult> {
    let pre = input.pre_periods;
    let post = input.post_periods();

    // Step 1: Classic SCM weights on simplex.
    let w =
        solve_simplex_weights(&input.treated_series[..pre], &input.donors, pre)?;

    // Step 2: Pre-treatment residuals δ = Y_treated_pre − Σw_j Y_j_pre.
    let donor_pre: Vec<&[f64]> = input.donors.iter().map(|(_, s)| &s[..pre]).collect();
    let synth_pre = weighted_sum(&donor_pre, &w, pre);
    let delta_pre: Vec<f64> = input.treated_series[..pre]
        .iter()
        .zip(synth_pre.iter())
        .map(|(&y, &s)| y - s)
        .collect();

    // Step 3: Ridge regression — fit β predicting δ from donor pre-series.
    let lambda = ridge_auto_lambda(&donor_pre, pre, input.donors.len());
    let beta = ridge_solve(&donor_pre, &delta_pre, lambda)?;

    // Step 4: Bias-corrected ATT = classic ATT + mean post-treatment bias correction.
    let donor_post: Vec<&[f64]> = input.donors.iter().map(|(_, s)| &s[pre..]).collect();
    let att_classic = compute_att(&input.treated_series, &input.donors, &w, pre);

    let bias_correction: f64 = (0..post)
        .map(|t| beta.iter().zip(donor_post.iter()).map(|(&bj, dj)| bj * dj[t]).sum::<f64>())
        .sum::<f64>()
        / post as f64;

    let att = att_classic + bias_correction;
    assert_finite(att, "augmented_att");

    // Conformal CI from pre-treatment residual standard error.
    let se = residual_se(&delta_pre).max(1e-10);
    let z = normal_quantile(1.0 - input.alpha / 2.0);
    let ci_lower = att - z * se;
    let ci_upper = att + z * se;
    assert_finite(ci_lower, "augmented_ci_lower");
    assert_finite(ci_upper, "augmented_ci_upper");

    let placebo_p = placebo_permutation_p(input, att, Method::Augmented)?;

    Ok(SyntheticControlResult {
        method: Method::Augmented,
        att,
        ci_lower,
        ci_upper,
        donor_weights: weights_to_map(&input.donors, &w),
        placebo_p_value: placebo_p,
    })
}

// ---------------------------------------------------------------------------
// Synthetic DiD — Arkhangelsky et al. (2021)
// ---------------------------------------------------------------------------

fn sdid(input: &SyntheticControlInput) -> Result<SyntheticControlResult> {
    let pre = input.pre_periods;
    let post = input.post_periods();
    let n_donors = input.donors.len();

    let donor_pre: Vec<&[f64]> = input.donors.iter().map(|(_, s)| &s[..pre]).collect();
    let donor_post: Vec<&[f64]> = input.donors.iter().map(|(_, s)| &s[pre..]).collect();

    // Time weights λ ∈ Δ_pre: weight pre-treatment periods to resemble post-treatment.
    let donor_post_avg: Vec<f64> =
        donor_post.iter().map(|dj| dj.iter().sum::<f64>() / post as f64).collect();
    let time_weights = sdid_time_weights(&donor_pre, &donor_post_avg, pre, n_donors)?;

    // Time-weighted averages.
    let donor_lambda: Vec<f64> = donor_pre
        .iter()
        .map(|dj| time_weights.iter().zip(dj.iter()).map(|(&l, &y)| l * y).sum())
        .collect();
    let treated_lambda: f64 = time_weights
        .iter()
        .zip(input.treated_series[..pre].iter())
        .map(|(&l, &y)| l * y)
        .sum();

    // Unit weights ω ∈ Δ_donors: match treated unit in pre-period.
    let unit_weights = solve_simplex_weights_1d(&donor_lambda, treated_lambda)?;

    // SDiD ATT: DiD on time-weighted averages.
    let treated_post_avg: f64 =
        input.treated_series[pre..].iter().sum::<f64>() / post as f64;
    let control_diff: f64 = unit_weights
        .iter()
        .zip(donor_post_avg.iter())
        .zip(donor_lambda.iter())
        .map(|((&wj, &dj_post), &dj_pre)| wj * (dj_post - dj_pre))
        .sum();
    let att = (treated_post_avg - treated_lambda) - control_diff;
    assert_finite(att, "sdid_att");

    let (ci_lower, ci_upper) =
        sdid_jackknife_ci(input, &time_weights, &unit_weights, att, input.alpha);
    assert_finite(ci_lower, "sdid_ci_lower");
    assert_finite(ci_upper, "sdid_ci_upper");

    let placebo_p = placebo_permutation_p(input, att, Method::SDiD)?;

    Ok(SyntheticControlResult {
        method: Method::SDiD,
        att,
        ci_lower,
        ci_upper,
        donor_weights: weights_to_map(&input.donors, &unit_weights),
        placebo_p_value: placebo_p,
    })
}

/// Find λ ∈ Δ_pre minimizing Σ_j (Σ_t λ_t D_jt − D_j_post_avg)^2.
fn sdid_time_weights(
    donor_pre: &[&[f64]],
    donor_post_avg: &[f64],
    pre: usize,
    n_donors: usize,
) -> Result<Vec<f64>> {
    let mut lam = vec![1.0 / pre as f64; pre];
    let lr = 0.01;

    for _ in 0..10_000 {
        let residuals: Vec<f64> = (0..n_donors)
            .map(|j| {
                lam.iter().zip(donor_pre[j].iter()).map(|(&l, &d)| l * d).sum::<f64>()
                    - donor_post_avg[j]
            })
            .collect();

        let grad: Vec<f64> = (0..pre)
            .map(|t| (0..n_donors).map(|j| donor_pre[j][t] * residuals[j]).sum::<f64>())
            .collect();

        let lam_new: Vec<f64> =
            lam.iter().zip(grad.iter()).map(|(&l, &g)| l - lr * g).collect();
        let lam_proj = project_simplex(&lam_new);

        let step: f64 = lam_proj
            .iter()
            .zip(lam.iter())
            .map(|(&a, &b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        lam = lam_proj;
        if step < 1e-12 {
            break;
        }
    }

    let sum: f64 = lam.iter().sum();
    if sum < 1e-14 {
        return Err(Error::Numerical("SDiD time weights sum to zero".into()));
    }
    Ok(lam.iter().map(|&l| l / sum).collect())
}

/// Jackknife CI over post-treatment periods for SDiD.
fn sdid_jackknife_ci(
    input: &SyntheticControlInput,
    time_weights: &[f64],
    unit_weights: &[f64],
    att: f64,
    alpha: f64,
) -> (f64, f64) {
    let pre = input.pre_periods;
    let post = input.post_periods();
    if post < 2 {
        let se = att.abs().max(1e-3) * 0.5;
        let z = normal_quantile(1.0 - alpha / 2.0);
        return (att - z * se, att + z * se);
    }

    let treated_lambda: f64 = time_weights
        .iter()
        .zip(input.treated_series[..pre].iter())
        .map(|(&l, &y)| l * y)
        .sum();

    let jackknife_atts: Vec<f64> = (0..post)
        .filter_map(|t_excl| {
            let m = (post - 1) as f64;
            let treated_post_excl: f64 = (0..post)
                .filter(|&t| t != t_excl)
                .map(|t| input.treated_series[pre + t])
                .sum::<f64>()
                / m;

            let control_diff: f64 = unit_weights
                .iter()
                .zip(input.donors.iter())
                .map(|(&wj, (_, s))| {
                    let dj_post_excl: f64 =
                        (0..post).filter(|&t| t != t_excl).map(|t| s[pre + t]).sum::<f64>() / m;
                    let dj_lambda: f64 = time_weights
                        .iter()
                        .zip(s[..pre].iter())
                        .map(|(&l, &y)| l * y)
                        .sum();
                    wj * (dj_post_excl - dj_lambda)
                })
                .sum();

            let att_j = (treated_post_excl - treated_lambda) - control_diff;
            if att_j.is_finite() { Some(att_j) } else { None }
        })
        .collect();

    if jackknife_atts.is_empty() {
        let se = att.abs().max(1e-3) * 0.5;
        let z = normal_quantile(1.0 - alpha / 2.0);
        return (att - z * se, att + z * se);
    }

    let m = jackknife_atts.len() as f64;
    let mean_jk: f64 = jackknife_atts.iter().sum::<f64>() / m;
    // Jackknife variance: (n-1)/n * Σ(T_j - T_bar)^2
    let var_jk: f64 = jackknife_atts
        .iter()
        .map(|&a| (a - mean_jk).powi(2))
        .sum::<f64>()
        * (m - 1.0)
        / m;
    let se = var_jk.sqrt().max(1e-10);
    let z = normal_quantile(1.0 - alpha / 2.0);
    (att - z * se, att + z * se)
}

// ---------------------------------------------------------------------------
// CausalImpact — Brodersen et al. (2015)
// ---------------------------------------------------------------------------

fn causal_impact(input: &SyntheticControlInput) -> Result<SyntheticControlResult> {
    let pre = input.pre_periods;
    let post = input.post_periods();
    let n_donors = input.donors.len();
    let n_features = 1 + n_donors; // intercept + one coefficient per donor

    // Design matrix X for pre-period: row t = [1, D_1t, ..., D_Jt].
    let mut x_pre = vec![0.0_f64; pre * n_features];
    for t in 0..pre {
        x_pre[t * n_features] = 1.0;
        for j in 0..n_donors {
            x_pre[t * n_features + 1 + j] = input.donors[j].1[t];
        }
    }
    let y_pre = &input.treated_series[..pre];

    // Ridge regression for regression coefficients β.
    let beta = causal_impact_ridge(&x_pre, y_pre, n_features, pre, 1e-3)?;

    // Kalman filter over pre-treatment period for state estimation.
    let kf = kalman_filter_pre(y_pre, &x_pre, &beta, n_features, pre)?;

    // Predict counterfactual post-treatment via open-loop Kalman propagation.
    let z_crit = normal_quantile(1.0 - input.alpha / 2.0);
    let mut sum_pred_var = 0.0_f64;
    let mut att_sum = 0.0_f64;
    let mut state = kf.final_state;
    let mut cov = kf.final_cov;

    for t in 0..post {
        // Covariate contribution at post-treatment time t.
        let mut cov_contrib = beta[0]; // intercept
        for j in 0..n_donors {
            cov_contrib += beta[1 + j] * input.donors[j].1[pre + t];
        }

        let y_hat = state[0] + cov_contrib;
        // Prediction variance = Kalman state uncertainty + observation noise.
        let pred_var = kf.obs_var + cov[0];
        sum_pred_var += pred_var;

        att_sum += input.treated_series[pre + t] - y_hat;

        // State transition (open loop, no update): F = [[1,1],[0,1]].
        let new_level = state[0] + state[1];
        let new_slope = state[1];
        state = vec![new_level, new_slope];

        // Covariance propagation: P = F P F' + Q, Q = diag(state_var).
        let p00 = cov[0] + 2.0 * cov[1] + cov[3] + kf.state_var;
        let p01 = cov[1] + cov[3];
        let p10 = cov[2] + cov[3];
        let p11 = cov[3] + kf.state_var;
        cov = vec![p00, p01, p10, p11];
    }

    let att = att_sum / post as f64;
    assert_finite(att, "causal_impact_att");

    // SE(ATT) = sqrt(Σ pred_var_t) / post (variance of average of indep. predictions).
    let se_att = sum_pred_var.max(0.0).sqrt() / post as f64;
    let ci_lower = att - z_crit * se_att;
    let ci_upper = att + z_crit * se_att;
    assert_finite(ci_lower, "causal_impact_ci_lower");
    assert_finite(ci_upper, "causal_impact_ci_upper");

    // Placebo p-value: t-test vs pre-treatment innovation SE.
    let t_stat = if kf.residual_se > 1e-14 { att / kf.residual_se } else { 0.0 };
    let placebo_p = two_sided_p(t_stat.abs());
    assert_finite(placebo_p, "causal_impact_placebo_p");

    // Donor weights = OLS regression coefficients (not required to sum to 1).
    let donor_weights: HashMap<String, f64> = input
        .donors
        .iter()
        .enumerate()
        .map(|(j, (name, _))| (name.clone(), beta[1 + j]))
        .collect();

    Ok(SyntheticControlResult {
        method: Method::CausalImpact,
        att,
        ci_lower,
        ci_upper,
        donor_weights,
        placebo_p_value: placebo_p,
    })
}

struct KalmanResult {
    final_state: Vec<f64>,
    final_cov: Vec<f64>, // 2×2 row-major [p00, p01, p10, p11]
    obs_var: f64,
    state_var: f64,
    residual_se: f64,
}

/// Kalman filter over pre-treatment period.
///
/// State = [level, slope]. Observation = level + covariate_contribution.
/// Transition: F = [[1,1],[0,1]], observation H = [1,0].
fn kalman_filter_pre(
    y_pre: &[f64],
    x_pre: &[f64],
    beta: &[f64],
    n_features: usize,
    pre: usize,
) -> Result<KalmanResult> {
    let y_mean: f64 = y_pre.iter().sum::<f64>() / pre as f64;
    let y_var: f64 =
        y_pre.iter().map(|&y| (y - y_mean).powi(2)).sum::<f64>() / pre.max(2) as f64;
    let obs_var = (y_var * 0.1).max(1e-6);
    let state_var = (y_var * 0.01).max(1e-8);

    // Diffuse initialisation.
    let mut state = vec![y_pre[0], 0.0_f64];
    let mut cov = vec![1e4_f64, 0.0, 0.0, 1e4_f64]; // [p00, p01, p10, p11]
    let mut innovations = Vec::with_capacity(pre);

    for t in 0..pre {
        // Covariate contribution.
        let cov_contrib: f64 =
            (0..n_features).map(|f| beta[f] * x_pre[t * n_features + f]).sum();

        // Prediction step: propagate state and covariance.
        let level_pred = state[0] + state[1];
        let slope_pred = state[1];
        let y_pred = level_pred + cov_contrib;

        // P_pred = F P F' + Q.
        let p00 = cov[0] + 2.0 * cov[1] + cov[3] + state_var;
        let p01 = cov[1] + cov[3];
        let p10 = cov[2] + cov[3];
        let p11 = cov[3] + state_var;

        // Innovation and Kalman gain. H = [1,0] → S = P_pred[0,0] + obs_var.
        let innov = y_pre[t] - y_pred;
        innovations.push(innov);
        let s = p00 + obs_var;
        let k0 = p00 / s;
        let k1 = p10 / s;

        // Update step.
        state[0] = level_pred + k0 * innov;
        state[1] = slope_pred + k1 * innov;
        // P_upd = (I − K H) P_pred, H = [1,0].
        cov[0] = (1.0 - k0) * p00;
        cov[1] = (1.0 - k0) * p01;
        cov[2] = p10 - k1 * p00;
        cov[3] = p11 - k1 * p01;
    }

    let residual_se = residual_se(&innovations).max(1e-10);

    Ok(KalmanResult {
        final_state: state,
        final_cov: cov,
        obs_var,
        state_var,
        residual_se,
    })
}

fn causal_impact_ridge(
    x_pre: &[f64],
    y_pre: &[f64],
    n_features: usize,
    pre: usize,
    lambda: f64,
) -> Result<Vec<f64>> {
    let mut xtx = vec![0.0_f64; n_features * n_features];
    let mut xty = vec![0.0_f64; n_features];

    for t in 0..pre {
        for i in 0..n_features {
            xty[i] += x_pre[t * n_features + i] * y_pre[t];
            for j in 0..n_features {
                xtx[i * n_features + j] +=
                    x_pre[t * n_features + i] * x_pre[t * n_features + j];
            }
        }
    }
    for i in 0..n_features {
        xtx[i * n_features + i] += lambda;
    }
    gaussian_solve(&xtx, &xty, n_features)
}

// ---------------------------------------------------------------------------
// Placebo permutation inference
// ---------------------------------------------------------------------------

/// Rank-based placebo p-value via leave-one-out donor permutation.
///
/// p = (1 + #{k : |att_k| ≥ |att_treated|}) / (1 + n_donors)
fn placebo_permutation_p(
    input: &SyntheticControlInput,
    att_treated: f64,
    method: Method,
) -> Result<f64> {
    let n_donors = input.donors.len();
    if n_donors < 2 {
        return Ok(1.0);
    }

    let mut n_extreme = 0usize;

    for k in 0..n_donors {
        let pseudo_treated = input.donors[k].1.clone();
        let pseudo_donors: Vec<(String, Vec<f64>)> = input
            .donors
            .iter()
            .enumerate()
            .filter(|&(j, _)| j != k)
            .map(|(_, (name, s))| (name.clone(), s.clone()))
            .collect();

        let pseudo_input = SyntheticControlInput {
            treated_unit: input.donors[k].0.clone(),
            treated_series: pseudo_treated,
            donors: pseudo_donors,
            pre_periods: input.pre_periods,
            alpha: input.alpha,
        };

        if let Some(att_k) = placebo_att(&pseudo_input, method) {
            if att_k.abs() >= att_treated.abs() {
                n_extreme += 1;
            }
        }
    }

    Ok((1 + n_extreme) as f64 / (1 + n_donors) as f64)
}

/// Compute ATT for a placebo pseudo-input; returns None on numerical failure.
fn placebo_att(input: &SyntheticControlInput, method: Method) -> Option<f64> {
    let pre = input.pre_periods;
    match method {
        Method::Classic | Method::Augmented | Method::CausalImpact => {
            solve_simplex_weights(&input.treated_series[..pre], &input.donors, pre)
                .ok()
                .map(|w| compute_att(&input.treated_series, &input.donors, &w, pre))
        }
        Method::SDiD => {
            let post = input.post_periods();
            let n = input.donors.len();
            let d_pre: Vec<&[f64]> = input.donors.iter().map(|(_, s)| &s[..pre]).collect();
            let d_post: Vec<&[f64]> = input.donors.iter().map(|(_, s)| &s[pre..]).collect();
            let post_avg: Vec<f64> =
                d_post.iter().map(|dj| dj.iter().sum::<f64>() / post as f64).collect();
            let tw = sdid_time_weights(&d_pre, &post_avg, pre, n).ok()?;
            let dlam: Vec<f64> = d_pre
                .iter()
                .map(|dj| tw.iter().zip(dj.iter()).map(|(&l, &y)| l * y).sum())
                .collect();
            let tlam: f64 = tw
                .iter()
                .zip(input.treated_series[..pre].iter())
                .map(|(&l, &y)| l * y)
                .sum();
            let ow = solve_simplex_weights_1d(&dlam, tlam).ok()?;
            let tpa = input.treated_series[pre..].iter().sum::<f64>() / post as f64;
            let ctrl: f64 = ow
                .iter()
                .zip(post_avg.iter())
                .zip(dlam.iter())
                .map(|((&w, &dp), &dl)| w * (dp - dl))
                .sum();
            let att = (tpa - tlam) - ctrl;
            if att.is_finite() { Some(att) } else { None }
        }
    }
}

/// Normal CI using SE from the distribution of leave-one-out placebo ATTs.
fn placebo_ci_normal(
    input: &SyntheticControlInput,
    att: f64,
    alpha: f64,
) -> Result<(f64, f64)> {
    let pre = input.pre_periods;
    let n_donors = input.donors.len();

    // Need at least 2 donors to form a leave-one-out pseudo-input with ≥ 1 donor.
    if n_donors < 2 {
        let y = &input.treated_series[..pre];
        let m = y.iter().sum::<f64>() / pre as f64;
        let se = (y.iter().map(|&v| (v - m).powi(2)).sum::<f64>() / pre as f64)
            .max(0.0)
            .sqrt()
            .max(1e-10);
        let z = normal_quantile(1.0 - alpha / 2.0);
        return Ok((att - z * se, att + z * se));
    }

    let placebo_atts: Vec<f64> = (0..n_donors)
        .filter_map(|k| {
            let pseudo_donors: Vec<(String, Vec<f64>)> = input
                .donors
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != k)
                .map(|(_, (name, s))| (name.clone(), s.clone()))
                .collect();
            let pseudo_input = SyntheticControlInput {
                treated_unit: input.donors[k].0.clone(),
                treated_series: input.donors[k].1.clone(),
                donors: pseudo_donors,
                pre_periods: pre,
                alpha,
            };
            placebo_att(&pseudo_input, Method::Classic)
        })
        .collect();

    let se = if placebo_atts.is_empty() {
        // Fallback: pre-treatment residual SE.
        let y = &input.treated_series[..pre];
        let m = y.iter().sum::<f64>() / pre as f64;
        (y.iter().map(|&v| (v - m).powi(2)).sum::<f64>() / pre as f64).max(0.0).sqrt()
    } else {
        residual_se(&placebo_atts)
    }
    .max(1e-10);

    let z = normal_quantile(1.0 - alpha / 2.0);
    Ok((att - z * se, att + z * se))
}

// ---------------------------------------------------------------------------
// Core optimisation helpers
// ---------------------------------------------------------------------------

/// Find w ∈ Δ_J minimizing ||treated_pre − Σ_j w_j D_j_pre||² via projected gradient.
fn solve_simplex_weights(
    treated_pre: &[f64],
    donors: &[(String, Vec<f64>)],
    pre: usize,
) -> Result<Vec<f64>> {
    let n = donors.len();
    let donor_pre: Vec<&[f64]> = donors.iter().map(|(_, s)| &s[..pre]).collect();

    // Adaptive learning rate: 1 / (||D||_F / pre).
    let scale: f64 = donor_pre
        .iter()
        .flat_map(|d| d.iter())
        .map(|&x| x * x)
        .sum::<f64>()
        / (n * pre) as f64;
    let lr = if scale > 1e-30 { 0.5 / scale } else { 0.01 };

    let mut w = vec![1.0 / n as f64; n];

    for _ in 0..10_000 {
        let synth = weighted_sum(&donor_pre, &w, pre);
        let residual: Vec<f64> =
            treated_pre.iter().zip(synth.iter()).map(|(&y, &s)| y - s).collect();

        let grad: Vec<f64> = (0..n)
            .map(|j| -(0..pre).map(|t| donor_pre[j][t] * residual[t]).sum::<f64>())
            .collect();

        let w_new: Vec<f64> =
            w.iter().zip(grad.iter()).map(|(&wj, &gj)| wj - lr * gj).collect();
        let w_proj = project_simplex(&w_new);

        let step: f64 = w_proj
            .iter()
            .zip(w.iter())
            .map(|(&a, &b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        w = w_proj;
        if step < 1e-12 {
            break;
        }
    }

    let sum: f64 = w.iter().sum();
    if sum < 1e-14 {
        return Err(Error::Numerical("SCM weights collapsed to zero".into()));
    }
    let w: Vec<f64> = w.iter().map(|&wj| wj / sum).collect();
    for &wj in &w {
        assert_finite(wj, "scm_weight");
    }
    Ok(w)
}

/// Find ω ∈ Δ_n minimizing (target − Σ_j ω_j v_j)² via projected gradient.
fn solve_simplex_weights_1d(values: &[f64], target: f64) -> Result<Vec<f64>> {
    let n = values.len();
    let scale: f64 =
        values.iter().map(|&x| x * x).sum::<f64>() / n as f64;
    let lr = if scale > 1e-30 { 0.5 / scale } else { 0.1 };

    let mut omega = vec![1.0 / n as f64; n];
    for _ in 0..10_000 {
        let pred: f64 = omega.iter().zip(values.iter()).map(|(&o, &v)| o * v).sum();
        let residual = target - pred;
        let grad: Vec<f64> = values.iter().map(|&v| -v * residual).collect();
        let omega_new: Vec<f64> =
            omega.iter().zip(grad.iter()).map(|(&o, &g)| o - lr * g).collect();
        let omega_proj = project_simplex(&omega_new);
        let step: f64 = omega_proj
            .iter()
            .zip(omega.iter())
            .map(|(&a, &b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        omega = omega_proj;
        if step < 1e-12 {
            break;
        }
    }

    let sum: f64 = omega.iter().sum();
    if sum < 1e-14 {
        return Err(Error::Numerical("SDiD unit weights collapsed to zero".into()));
    }
    Ok(omega.iter().map(|&o| o / sum).collect())
}

/// Ridge regression: solve (X'X + λI)β = X'δ via Gaussian elimination.
fn ridge_solve(donor_pre: &[&[f64]], delta: &[f64], lambda: f64) -> Result<Vec<f64>> {
    let n = donor_pre.len();
    let pre = delta.len();
    let mut xtx = vec![0.0_f64; n * n];
    let mut xtd = vec![0.0_f64; n];
    for i in 0..n {
        for j in 0..n {
            xtx[i * n + j] = (0..pre).map(|t| donor_pre[i][t] * donor_pre[j][t]).sum();
        }
        xtd[i] = (0..pre).map(|t| donor_pre[i][t] * delta[t]).sum();
    }
    for i in 0..n {
        xtx[i * n + i] += lambda;
    }
    gaussian_solve(&xtx, &xtd, n)
}

fn ridge_auto_lambda(donor_pre: &[&[f64]], pre: usize, n_donors: usize) -> f64 {
    let trace: f64 = donor_pre
        .iter()
        .map(|d| d.iter().map(|&x| x * x).sum::<f64>() / pre as f64)
        .sum::<f64>();
    0.1 * trace / n_donors.max(1) as f64
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn compute_att(
    treated: &[f64],
    donors: &[(String, Vec<f64>)],
    weights: &[f64],
    pre_periods: usize,
) -> f64 {
    let post = treated.len() - pre_periods;
    if post == 0 {
        return 0.0;
    }
    let donor_post: Vec<&[f64]> = donors.iter().map(|(_, s)| &s[pre_periods..]).collect();
    let synth_post = weighted_sum(&donor_post, weights, post);
    treated[pre_periods..]
        .iter()
        .zip(synth_post.iter())
        .map(|(&y, &s)| y - s)
        .sum::<f64>()
        / post as f64
}

fn weighted_sum(series: &[&[f64]], weights: &[f64], n: usize) -> Vec<f64> {
    (0..n)
        .map(|t| weights.iter().zip(series.iter()).map(|(&w, s)| w * s[t]).sum())
        .collect()
}

fn weights_to_map(donors: &[(String, Vec<f64>)], weights: &[f64]) -> HashMap<String, f64> {
    donors.iter().zip(weights.iter()).map(|((name, _), &w)| (name.clone(), w)).collect()
}

fn residual_se(residuals: &[f64]) -> f64 {
    let n = residuals.len();
    if n < 2 {
        return 0.0;
    }
    let m = residuals.iter().sum::<f64>() / n as f64;
    (residuals.iter().map(|&r| (r - m).powi(2)).sum::<f64>() / (n - 1) as f64)
        .max(0.0)
        .sqrt()
}

/// O(n log n) projection onto the probability simplex (Duchi et al. 2008).
fn project_simplex(v: &[f64]) -> Vec<f64> {
    let n = v.len();
    let mut u = v.to_vec();
    u.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

    let mut rho = 0usize;
    let mut cumsum = 0.0_f64;
    for j in 0..n {
        cumsum += u[j];
        if u[j] > (cumsum - 1.0) / (j + 1) as f64 {
            rho = j;
        }
    }
    let cumsum_rho: f64 = u[..=rho].iter().sum();
    let theta = (cumsum_rho - 1.0) / (rho + 1) as f64;
    v.iter().map(|&vi| (vi - theta).max(0.0)).collect()
}

/// Gaussian elimination with partial pivoting. Solves Ax = b (A is n×n square).
fn gaussian_solve(a_flat: &[f64], b: &[f64], n: usize) -> Result<Vec<f64>> {
    let mut a: Vec<Vec<f64>> =
        (0..n).map(|i| a_flat[i * n..(i + 1) * n].to_vec()).collect();
    let mut b = b.to_vec();

    for col in 0..n {
        let pivot_row = (col..n)
            .max_by(|&r1, &r2| {
                a[r1][col]
                    .abs()
                    .partial_cmp(&a[r2][col].abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap();
        a.swap(col, pivot_row);
        b.swap(col, pivot_row);

        let pivot = a[col][col];
        if pivot.abs() < 1e-14 {
            return Err(Error::Numerical(format!(
                "singular matrix in Gaussian elimination at column {col}"
            )));
        }
        for row in (col + 1)..n {
            let factor = a[row][col] / pivot;
            for j in col..n {
                let v = a[col][j] * factor;
                a[row][j] -= v;
            }
            b[row] -= b[col] * factor;
        }
    }

    let mut x = vec![0.0_f64; n];
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in (i + 1)..n {
            s -= a[i][j] * x[j];
        }
        x[i] = s / a[i][i];
        assert_finite(x[i], &format!("gauss_solution[{i}]"));
    }
    Ok(x)
}

fn normal_quantile(p: f64) -> f64 {
    use statrs::distribution::{ContinuousCDF, Normal};
    Normal::new(0.0, 1.0).unwrap().inverse_cdf(p)
}

fn two_sided_p(abs_z: f64) -> f64 {
    use statrs::distribution::{ContinuousCDF, Normal};
    2.0 * (1.0 - Normal::new(0.0, 1.0).unwrap().cdf(abs_z))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Two-donor panel where treated = 0.6·D1 + 0.4·D2 pre-treatment,
    /// plus a constant ATT = 2.0 post-treatment.
    fn make_perfect_fit_input() -> SyntheticControlInput {
        // 8 total periods: 5 pre + 3 post.
        let d1: Vec<f64> = (1..=8).map(|x| x as f64).collect();
        let d2: Vec<f64> = (1..=8).map(|x| (x * 2) as f64).collect();
        let treated: Vec<f64> = (0..8)
            .map(|t| {
                let synth = 0.6 * d1[t] + 0.4 * d2[t];
                if t >= 5 { synth + 2.0 } else { synth }
            })
            .collect();
        SyntheticControlInput::new(
            "treated",
            treated,
            vec![("D1".into(), d1), ("D2".into(), d2)],
            5,
        )
    }

    /// Three-donor panel with a larger donor pool and known ATT = 1.5.
    fn make_three_donor_input() -> SyntheticControlInput {
        let d1: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let d2: Vec<f64> = vec![2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0];
        let d3: Vec<f64> = vec![3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0];
        // treated = 0.5·D1 + 0.5·D2 in pre-period (D3 not needed).
        let treated: Vec<f64> = (0..10)
            .map(|t| {
                let synth = 0.5 * d1[t] + 0.5 * d2[t];
                if t >= 7 { synth + 1.5 } else { synth }
            })
            .collect();
        SyntheticControlInput {
            treated_unit: "treated".into(),
            treated_series: treated,
            donors: vec![
                ("D1".into(), d1),
                ("D2".into(), d2),
                ("D3".into(), d3),
            ],
            pre_periods: 7,
            alpha: 0.05,
        }
    }

    // --- Validation ---

    #[test]
    fn test_validation_zero_pre_periods() {
        let input = SyntheticControlInput::new(
            "T",
            vec![1.0, 2.0, 3.0],
            vec![("D".into(), vec![1.0, 2.0, 3.0])],
            0,
        );
        assert!(synthetic_control(&input, Method::Classic).is_err());
    }

    #[test]
    fn test_validation_pre_equals_total() {
        let input = SyntheticControlInput::new(
            "T",
            vec![1.0, 2.0, 3.0],
            vec![("D".into(), vec![1.0, 2.0, 3.0])],
            3,
        );
        assert!(synthetic_control(&input, Method::Classic).is_err());
    }

    #[test]
    fn test_validation_mismatched_donor_length() {
        let input = SyntheticControlInput::new(
            "T",
            vec![1.0, 2.0, 3.0, 4.0],
            vec![("D".into(), vec![1.0, 2.0, 3.0])], // too short
            2,
        );
        assert!(synthetic_control(&input, Method::Classic).is_err());
    }

    // --- Classic SCM ---

    #[test]
    fn test_classic_perfect_fit_att() {
        let input = make_perfect_fit_input();
        let result = synthetic_control(&input, Method::Classic).unwrap();
        assert_eq!(result.method, Method::Classic);
        assert!(
            (result.att - 2.0).abs() < 1e-3,
            "ATT should be ≈2.0, got {}",
            result.att
        );
    }

    #[test]
    fn test_classic_weights_non_negative_sum_to_one() {
        let input = make_perfect_fit_input();
        let result = synthetic_control(&input, Method::Classic).unwrap();
        let sum: f64 = result.donor_weights.values().sum();
        assert!(
            (sum - 1.0).abs() < 1e-9,
            "classic weights should sum to 1.0, got {sum}"
        );
        for (&w) in result.donor_weights.values() {
            assert!(w >= -1e-12, "classic weight should be ≥ 0, got {w}");
        }
    }

    #[test]
    fn test_classic_perfect_fit_weights() {
        let input = make_perfect_fit_input();
        let result = synthetic_control(&input, Method::Classic).unwrap();
        let w1 = result.donor_weights["D1"];
        let w2 = result.donor_weights["D2"];
        assert!(
            (w1 - 0.6).abs() < 1e-3,
            "D1 weight should be ≈0.6, got {w1}"
        );
        assert!(
            (w2 - 0.4).abs() < 1e-3,
            "D2 weight should be ≈0.4, got {w2}"
        );
    }

    #[test]
    fn test_classic_no_treatment_effect() {
        let d1: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        let d2: Vec<f64> = (1..=10).map(|x| x as f64 * 1.5).collect();
        // treated = 0.5·D1 + 0.5·D2, no treatment effect.
        let treated: Vec<f64> = (0..10).map(|t| 0.5 * d1[t] + 0.5 * d2[t]).collect();
        let input = SyntheticControlInput::new(
            "T",
            treated,
            vec![("D1".into(), d1), ("D2".into(), d2)],
            7,
        );
        let result = synthetic_control(&input, Method::Classic).unwrap();
        assert!(
            result.att.abs() < 1e-3,
            "ATT should be ≈0 with no treatment effect, got {}",
            result.att
        );
    }

    #[test]
    fn test_classic_placebo_p_in_range() {
        let input = make_three_donor_input();
        let result = synthetic_control(&input, Method::Classic).unwrap();
        assert!(
            result.placebo_p_value >= 0.0 && result.placebo_p_value <= 1.0,
            "placebo p-value should be in [0,1], got {}",
            result.placebo_p_value
        );
    }

    #[test]
    fn test_classic_ci_straddles_att() {
        let input = make_three_donor_input();
        let result = synthetic_control(&input, Method::Classic).unwrap();
        assert!(
            result.ci_lower <= result.att && result.att <= result.ci_upper,
            "CI [{:.4}, {:.4}] should contain ATT={:.4}",
            result.ci_lower,
            result.ci_upper,
            result.att
        );
    }

    // --- Augmented SCM ---

    #[test]
    fn test_augmented_perfect_fit_att() {
        let input = make_perfect_fit_input();
        let result = synthetic_control(&input, Method::Augmented).unwrap();
        assert_eq!(result.method, Method::Augmented);
        // With perfect pre-treatment fit, bias correction ≈ 0, ATT ≈ 2.0.
        assert!(
            (result.att - 2.0).abs() < 1e-2,
            "augmented ATT should be ≈2.0, got {}",
            result.att
        );
    }

    #[test]
    fn test_augmented_weights_non_negative_sum_to_one() {
        let input = make_perfect_fit_input();
        let result = synthetic_control(&input, Method::Augmented).unwrap();
        let sum: f64 = result.donor_weights.values().sum();
        assert!(
            (sum - 1.0).abs() < 1e-9,
            "augmented weights should sum to 1.0, got {sum}"
        );
        for &w in result.donor_weights.values() {
            assert!(w >= -1e-12, "augmented weight should be ≥ 0, got {w}");
        }
    }

    #[test]
    fn test_augmented_imperfect_fit_correction() {
        // Add noise to pre-treatment treated series to create imperfect fit.
        let d1 = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let d2 = vec![2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0];
        // Treated has small pre-treatment deviations (imperfect fit).
        let treated = vec![1.5, 2.9, 4.3, 5.8, 7.2, 10.4, 11.8, 13.2];
        let input = SyntheticControlInput::new(
            "T",
            treated,
            vec![("D1".into(), d1), ("D2".into(), d2)],
            5,
        );
        let result_classic = synthetic_control(&input, Method::Classic).unwrap();
        let result_aug = synthetic_control(&input, Method::Augmented).unwrap();
        // Both should be finite.
        assert!(result_classic.att.is_finite());
        assert!(result_aug.att.is_finite());
        // Augmented CIs should be reasonable.
        assert!(result_aug.ci_lower < result_aug.ci_upper);
    }

    #[test]
    fn test_augmented_ci_valid() {
        let input = make_three_donor_input();
        let result = synthetic_control(&input, Method::Augmented).unwrap();
        assert!(result.ci_lower <= result.ci_upper);
        assert!(result.ci_lower <= result.att);
        assert!(result.att <= result.ci_upper);
    }

    // --- SDiD ---

    #[test]
    fn test_sdid_basic() {
        let input = make_three_donor_input();
        let result = synthetic_control(&input, Method::SDiD).unwrap();
        assert_eq!(result.method, Method::SDiD);
        assert!(result.att.is_finite());
    }

    #[test]
    fn test_sdid_unit_weights_sum_to_one() {
        let input = make_three_donor_input();
        let result = synthetic_control(&input, Method::SDiD).unwrap();
        let sum: f64 = result.donor_weights.values().sum();
        assert!(
            (sum - 1.0).abs() < 1e-9,
            "SDiD unit weights should sum to 1.0, got {sum}"
        );
        for &w in result.donor_weights.values() {
            assert!(w >= -1e-12, "SDiD weight should be ≥ 0, got {w}");
        }
    }

    #[test]
    fn test_sdid_detects_large_effect() {
        let d1: Vec<f64> = (1..=12).map(|x| x as f64).collect();
        let d2: Vec<f64> = (1..=12).map(|x| x as f64 + 0.5).collect();
        let d3: Vec<f64> = (1..=12).map(|x| x as f64 - 0.5).collect();
        // Treated = mean of donors pre-period, then +10 post.
        let treated: Vec<f64> = (0..12)
            .map(|t| {
                let s = (d1[t] + d2[t] + d3[t]) / 3.0;
                if t >= 8 { s + 10.0 } else { s }
            })
            .collect();
        let input = SyntheticControlInput {
            treated_unit: "T".into(),
            treated_series: treated,
            donors: vec![
                ("D1".into(), d1),
                ("D2".into(), d2),
                ("D3".into(), d3),
            ],
            pre_periods: 8,
            alpha: 0.05,
        };
        let result = synthetic_control(&input, Method::SDiD).unwrap();
        assert!(
            result.att > 5.0,
            "SDiD ATT should detect large effect, got {}",
            result.att
        );
    }

    #[test]
    fn test_sdid_ci_valid() {
        let input = make_three_donor_input();
        let result = synthetic_control(&input, Method::SDiD).unwrap();
        assert!(
            result.ci_lower <= result.ci_upper,
            "SDiD CI [{}, {}] invalid",
            result.ci_lower,
            result.ci_upper
        );
    }

    // --- CausalImpact ---

    #[test]
    fn test_causal_impact_basic() {
        let input = make_three_donor_input();
        let result = synthetic_control(&input, Method::CausalImpact).unwrap();
        assert_eq!(result.method, Method::CausalImpact);
        assert!(result.att.is_finite());
        assert!(result.ci_lower <= result.ci_upper);
    }

    #[test]
    fn test_causal_impact_detects_positive_effect() {
        let d1: Vec<f64> = vec![5.0, 5.1, 4.9, 5.0, 5.1, 5.0, 4.9, 5.0, 5.1, 5.0,
                                5.0, 5.1, 4.9, 5.0, 5.1];
        let d2: Vec<f64> = vec![3.0, 3.1, 2.9, 3.0, 3.1, 3.0, 2.9, 3.0, 3.1, 3.0,
                                3.0, 3.1, 2.9, 3.0, 3.1];
        let treated: Vec<f64> = vec![4.0, 4.1, 3.9, 4.0, 4.1, 4.0, 3.9, 4.0, 4.1, 4.0,
                                     7.2, 7.1, 7.3, 7.0, 7.2]; // +3.2 post treatment
        let input = SyntheticControlInput {
            treated_unit: "T".into(),
            treated_series: treated,
            donors: vec![("D1".into(), d1), ("D2".into(), d2)],
            pre_periods: 10,
            alpha: 0.05,
        };
        let result = synthetic_control(&input, Method::CausalImpact).unwrap();
        assert!(result.att > 0.0, "CausalImpact should detect positive effect, got {}", result.att);
    }

    #[test]
    fn test_causal_impact_placebo_p_in_range() {
        let input = make_three_donor_input();
        let result = synthetic_control(&input, Method::CausalImpact).unwrap();
        assert!(
            result.placebo_p_value >= 0.0 && result.placebo_p_value <= 1.0,
            "placebo_p should be in [0,1], got {}",
            result.placebo_p_value
        );
    }

    // --- Simplex projection ---

    #[test]
    fn test_project_simplex_sums_to_one() {
        let v = vec![-1.0, 2.0, 3.0, -0.5];
        let p = project_simplex(&v);
        let sum: f64 = p.iter().sum();
        assert!((sum - 1.0).abs() < 1e-12, "simplex projection sum = {sum}");
    }

    #[test]
    fn test_project_simplex_non_negative() {
        let v = vec![-5.0, -2.0, 1.0];
        let p = project_simplex(&v);
        for &pi in &p {
            assert!(pi >= 0.0, "simplex value should be ≥ 0, got {pi}");
        }
    }

    #[test]
    fn test_project_simplex_already_on_simplex() {
        let v = vec![0.3, 0.5, 0.2];
        let p = project_simplex(&v);
        for (pi, vi) in p.iter().zip(v.iter()) {
            assert!((pi - vi).abs() < 1e-12, "already-simplex point should be unchanged");
        }
    }

    // --- Single-donor edge case ---

    #[test]
    fn test_single_donor_placebo_p_is_one() {
        let input = SyntheticControlInput::new(
            "T",
            vec![1.0, 2.0, 3.0, 5.0, 6.0],
            vec![("D1".into(), vec![1.0, 2.0, 3.0, 4.0, 5.0])],
            3,
        );
        let result = synthetic_control(&input, Method::Classic).unwrap();
        assert_eq!(
            result.placebo_p_value, 1.0,
            "single donor → placebo p = 1.0, got {}",
            result.placebo_p_value
        );
    }

    // --- All methods produce finite results ---

    #[test]
    fn test_all_methods_finite() {
        let input = make_three_donor_input();
        for method in [Method::Classic, Method::Augmented, Method::SDiD, Method::CausalImpact] {
            let r = synthetic_control(&input, method).unwrap();
            assert!(r.att.is_finite(), "{method:?} ATT is not finite");
            assert!(r.ci_lower.is_finite(), "{method:?} ci_lower is not finite");
            assert!(r.ci_upper.is_finite(), "{method:?} ci_upper is not finite");
            assert!(r.placebo_p_value.is_finite(), "{method:?} placebo_p is not finite");
        }
    }
}

// ---------------------------------------------------------------------------
// Property-based tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_panel(
        n_donors: usize,
        pre: usize,
        post: usize,
    ) -> impl Strategy<Value = SyntheticControlInput> {
        let total = pre + post;
        let series_strat = prop::collection::vec(-100.0_f64..100.0_f64, total);
        let treated_strat = prop::collection::vec(-100.0_f64..100.0_f64, total);

        (treated_strat, prop::collection::vec(series_strat, n_donors)).prop_map(
            move |(treated, donor_series)| {
                let donors: Vec<(String, Vec<f64>)> = donor_series
                    .into_iter()
                    .enumerate()
                    .map(|(i, s)| (format!("D{i}"), s))
                    .collect();
                SyntheticControlInput {
                    treated_unit: "T".into(),
                    treated_series: treated,
                    donors,
                    pre_periods: pre,
                    alpha: 0.05,
                }
            },
        )
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 100,
            ..Default::default()
        })]

        /// Classic SCM: donor weights sum to 1.0 ± 1e-9.
        #[test]
        fn prop_classic_weights_sum_to_one(input in arb_panel(3, 6, 3)) {
            if let Ok(result) = synthetic_control(&input, Method::Classic) {
                let sum: f64 = result.donor_weights.values().sum();
                prop_assert!(
                    (sum - 1.0).abs() < 1e-9,
                    "classic weights sum = {sum}, expected 1.0"
                );
            }
        }

        /// Augmented SCM: donor weights sum to 1.0 ± 1e-9 (uses classic weights).
        #[test]
        fn prop_augmented_weights_sum_to_one(input in arb_panel(3, 6, 3)) {
            if let Ok(result) = synthetic_control(&input, Method::Augmented) {
                let sum: f64 = result.donor_weights.values().sum();
                prop_assert!(
                    (sum - 1.0).abs() < 1e-9,
                    "augmented weights sum = {sum}, expected 1.0"
                );
            }
        }

        /// SDiD: unit weights sum to 1.0 ± 1e-9.
        #[test]
        fn prop_sdid_unit_weights_sum_to_one(input in arb_panel(3, 6, 3)) {
            if let Ok(result) = synthetic_control(&input, Method::SDiD) {
                let sum: f64 = result.donor_weights.values().sum();
                prop_assert!(
                    (sum - 1.0).abs() < 1e-9,
                    "SDiD unit weights sum = {sum}, expected 1.0"
                );
            }
        }

        /// All simplex methods: ATT is finite when analysis succeeds.
        #[test]
        fn prop_att_finite(input in arb_panel(3, 6, 3)) {
            for method in [Method::Classic, Method::Augmented, Method::SDiD] {
                if let Ok(result) = synthetic_control(&input, method) {
                    prop_assert!(result.att.is_finite(), "{method:?} ATT not finite");
                    prop_assert!(result.ci_lower.is_finite());
                    prop_assert!(result.ci_upper.is_finite());
                    prop_assert!(result.ci_lower <= result.ci_upper);
                }
            }
        }

        /// Placebo p-value in [0, 1].
        #[test]
        fn prop_placebo_p_in_range(input in arb_panel(3, 6, 3)) {
            for method in [Method::Classic, Method::Augmented, Method::SDiD] {
                if let Ok(result) = synthetic_control(&input, method) {
                    prop_assert!(
                        result.placebo_p_value >= 0.0 && result.placebo_p_value <= 1.0,
                        "{method:?} placebo_p = {}",
                        result.placebo_p_value
                    );
                }
            }
        }
    }
}
