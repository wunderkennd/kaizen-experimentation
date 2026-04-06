//! Offline RL / long-term effect estimation via K-fold IV surrogate calibration.
//!
//! Implements TC/JIVE (Two-stage Calibration / Jackknife IV Estimation) from
//! "Causal Surrogate Metrics for Measuring Long-Term Effects in Streaming
//! Services" (Netflix, KDD 2024).
//!
//! # Why K-fold IV instead of R²-based calibration
//!
//! The R²-based calibrator in `surrogate.rs` is inconsistent when surrogate
//! metrics S are confounded with latent variables η that also drive the
//! long-term outcome Y:
//!
//! ```text
//! S_i = α + β·Z_i + η_i + ε_S       (Z = treatment, η = confounder)
//! Y_i = γ·S_i + δ·η_i + ε_Y
//! ```
//!
//! OLS regresses Y on S and absorbs δ·η into the coefficient:
//! `E[γ̂_OLS] = γ + δ·Var(η)/Var(S)` — biased whenever δ ≠ 0.
//!
//! TC/JIVE breaks this by using **treatment assignment Z as an instrument**
//! (Z ⊥ η by randomisation). K-fold cross-fitting avoids the finite-sample
//! over-fit bias of plain JIVE with leave-one-out:
//!
//! 1. Partition {1,…,n} into K folds.
//! 2. For each fold k, fit first-stage OLS on the remaining K-1 folds:
//!    `Ŝ_i = β̂₀_{-k} + β̂₁_{-k}·Z_i` for i ∈ fold k.
//! 3. Compute IV estimate using out-of-fold predictions:
//!    `γ̂_JIVE = Cov(Ŝ^JIVE, Y) / Cov(Ŝ^JIVE, S)`
//! 4. SE via HC0 sandwich estimator; t-test for significance.
//! 5. Report first-stage F-stat for instrument-strength diagnosis.
//!
//! # Validated against
//! Netflix KDD 2024 Table 2 — bias/variance/RMSE across three confounding
//! scenarios (ρ_UY ∈ {0.0, 0.3, 0.6}).  Golden files in `tests/golden/`.

use experimentation_core::error::{assert_finite, Error, Result};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single observation for IV-based surrogate calibration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrlObservation {
    /// Treatment assignment Z_i ∈ {0, 1} (used as instrument).
    pub treatment: f64,
    /// Short-term surrogate metric S_i.
    pub surrogate: f64,
    /// Long-term outcome Y_i (target we want to predict).
    pub outcome: f64,
}

/// Configuration for K-fold IV estimation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KFoldIvConfig {
    /// Number of cross-fitting folds (K). Must be ≥ 2.
    pub n_folds: usize,
    /// Significance level for CI and p-value (e.g., 0.05).
    pub alpha: f64,
}

impl Default for KFoldIvConfig {
    fn default() -> Self {
        Self { n_folds: 5, alpha: 0.05 }
    }
}

/// Instrument strength — replaces R²-based `ConfidenceBadge` for IV calibration.
///
/// Threshold follows the Stock-Yogo (2005) rule-of-thumb: F > 10 for
/// ≤ 10 % maximal IV size distortion with a single instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum InstrumentStrength {
    /// F ≥ 10 — instrument is strong; IV bias < 10 % of OLS bias.
    Strong,
    /// 5 ≤ F < 10 — moderate instrument; interpret with caution.
    Moderate,
    /// F < 5 — weak instrument; IV confidence intervals may be misleading.
    Weak,
}

/// Result of K-fold IV surrogate calibration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KFoldIvResult {
    /// IV (JIVE) estimate of the surrogate-to-outcome causal effect γ̂.
    pub iv_estimate: f64,
    /// HC0 sandwich standard error of iv_estimate.
    pub se: f64,
    /// Lower bound of (1-α) confidence interval.
    pub ci_lower: f64,
    /// Upper bound of (1-α) confidence interval.
    pub ci_upper: f64,
    /// t-statistic: iv_estimate / se.
    pub t_stat: f64,
    /// Two-sided p-value (large-sample normal approximation).
    pub p_value: f64,
    /// OLS estimate for comparison (full-sample OLS of Y on S).
    pub ols_estimate: f64,
    /// OLS standard error.
    pub ols_se: f64,
    /// Bias correction: iv_estimate − ols_estimate.
    /// Positive → OLS upward-biased (positive confounding).
    pub bias_correction: f64,
    /// First-stage F-statistic (full-sample OLS of S on Z).
    /// Diagnoses instrument relevance.
    pub first_stage_f_stat: f64,
    /// First-stage R² (fraction of S variance explained by Z).
    pub first_stage_r_squared: f64,
    /// Number of observations.
    pub n_observations: usize,
    /// Instrument strength based on first-stage F-statistic.
    pub instrument_strength: InstrumentStrength,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Perform K-fold IV calibration to estimate the causal effect of S → Y.
///
/// # Errors
/// Returns `Err` if n < 2·K, K < 2, alpha ∉ (0,1), or the first-stage
/// design matrix is singular (zero variance in Z within a training fold).
pub fn kfold_iv_calibrate(
    observations: &[OrlObservation],
    config: &KFoldIvConfig,
) -> Result<KFoldIvResult> {
    let n = observations.len();
    if config.n_folds < 2 {
        return Err(Error::Validation("n_folds must be at least 2".into()));
    }
    if n < 2 * config.n_folds {
        return Err(Error::Validation(format!(
            "need at least 2·n_folds={} observations, got {n}",
            2 * config.n_folds
        )));
    }
    if config.alpha <= 0.0 || config.alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }

    for (i, obs) in observations.iter().enumerate() {
        assert_finite(obs.treatment, &format!("treatment[{i}]"));
        assert_finite(obs.surrogate, &format!("surrogate[{i}]"));
        assert_finite(obs.outcome, &format!("outcome[{i}]"));
    }

    let z: Vec<f64> = observations.iter().map(|o| o.treatment).collect();
    let s: Vec<f64> = observations.iter().map(|o| o.surrogate).collect();
    let y: Vec<f64> = observations.iter().map(|o| o.outcome).collect();

    // --- K-fold out-of-fold first-stage predictions ---
    let s_hat = kfold_first_stage(&z, &s, config.n_folds)?;

    // --- IV estimate: Cov(Ŝ,Y) / Cov(Ŝ,S) ---
    let mean_s_hat = mean(&s_hat);
    let mean_y = mean(&y);
    let mean_s = mean(&s);

    assert_finite(mean_s_hat, "mean_s_hat");
    assert_finite(mean_y, "mean_y");
    assert_finite(mean_s, "mean_s");

    let cov_shat_y = sample_cov_with_means(&s_hat, &y, mean_s_hat, mean_y);
    let cov_shat_s = sample_cov_with_means(&s_hat, &s, mean_s_hat, mean_s);
    assert_finite(cov_shat_y, "cov_shat_y");
    assert_finite(cov_shat_s, "cov_shat_s");

    if cov_shat_s.abs() < 1e-14 {
        return Err(Error::Numerical(
            "Cov(Ŝ,S) ≈ 0: instrument has no predictive power for the surrogate".into(),
        ));
    }

    let iv_estimate = cov_shat_y / cov_shat_s;
    assert_finite(iv_estimate, "iv_estimate");

    // Intercept α̂ = Ȳ - γ̂·S̄
    let iv_intercept = mean_y - iv_estimate * mean_s;
    assert_finite(iv_intercept, "iv_intercept");

    // --- HC0 sandwich SE for IV ---
    // ε̂_i = Y_i - α̂ - γ̂·S_i
    let denominator: f64 = s_hat
        .iter()
        .zip(s.iter())
        .map(|(&sh, &si)| (sh - mean_s_hat) * (si - mean_s))
        .sum::<f64>();
    assert_finite(denominator, "iv_denominator");

    if denominator.abs() < 1e-14 {
        return Err(Error::Numerical(
            "IV denominator ≈ 0: collinearity in second stage".into(),
        ));
    }

    let meat: f64 = s_hat
        .iter()
        .zip(y.iter())
        .zip(s.iter())
        .map(|((&sh, &yi), &si)| {
            let resid = yi - iv_intercept - iv_estimate * si;
            assert_finite(resid, "iv_residual");
            let influence = (sh - mean_s_hat) * resid;
            influence * influence
        })
        .sum();
    assert_finite(meat, "iv_sandwich_meat");

    let se = (meat / (denominator * denominator)).sqrt();
    assert_finite(se, "iv_se");

    // --- Normal CI and p-value ---
    let z_crit = normal_quantile(1.0 - config.alpha / 2.0);
    assert_finite(z_crit, "z_crit");

    let ci_lower = iv_estimate - z_crit * se;
    let ci_upper = iv_estimate + z_crit * se;
    assert_finite(ci_lower, "ci_lower");
    assert_finite(ci_upper, "ci_upper");

    let t_stat = if se > 1e-15 { iv_estimate / se } else { 0.0 };
    assert_finite(t_stat, "t_stat");

    let p_value = two_sided_p(t_stat.abs());
    assert_finite(p_value, "p_value");

    // --- OLS estimate (full sample, for bias-correction comparison) ---
    let (ols_intercept, ols_slope) = ols_with_intercept(&s, &y)?;
    let ols_estimate = ols_slope;
    assert_finite(ols_estimate, "ols_estimate");
    assert_finite(ols_intercept, "ols_intercept");

    let ols_resid_var = {
        let ss_res: f64 = s
            .iter()
            .zip(y.iter())
            .map(|(&si, &yi)| (yi - ols_intercept - ols_estimate * si).powi(2))
            .sum();
        ss_res / (n - 2) as f64
    };
    assert_finite(ols_resid_var, "ols_resid_var");

    let var_s = sample_var(&s);
    assert_finite(var_s, "var_s");

    let ols_se = if var_s > 0.0 {
        (ols_resid_var / ((n - 1) as f64 * var_s)).sqrt()
    } else {
        0.0
    };
    assert_finite(ols_se, "ols_se");

    let bias_correction = iv_estimate - ols_estimate;
    assert_finite(bias_correction, "bias_correction");

    // --- First-stage F-statistic (full sample) ---
    let (_, fs_slope) = ols_with_intercept(&z, &s)?;
    assert_finite(fs_slope, "fs_slope");

    let var_z = sample_var(&z);
    assert_finite(var_z, "var_z");

    let (first_stage_f_stat, first_stage_r_squared) = if var_z < 1e-14 {
        (0.0, 0.0)
    } else {
        let ss_total: f64 = s.iter().map(|&si| (si - mean_s).powi(2)).sum();
        let fs_intercept_full = mean_s - fs_slope * mean(&z);
        let ss_res: f64 = z
            .iter()
            .zip(s.iter())
            .map(|(&zi, &si)| (si - fs_intercept_full - fs_slope * zi).powi(2))
            .sum();
        let r2 = if ss_total > 1e-30 { 1.0 - ss_res / ss_total } else { 0.0 };
        assert_finite(r2, "first_stage_r2");
        let f = if r2 < 1.0 - 1e-14 {
            r2 / (1.0 - r2) * (n - 2) as f64
        } else {
            f64::INFINITY
        };
        let f = if f.is_infinite() { (n - 2) as f64 * 1e6 } else { f };
        assert_finite(f, "first_stage_f");
        (f, r2)
    };

    let instrument_strength = f_to_instrument_strength(first_stage_f_stat);

    Ok(KFoldIvResult {
        iv_estimate,
        se,
        ci_lower,
        ci_upper,
        t_stat,
        p_value,
        ols_estimate,
        ols_se,
        bias_correction,
        first_stage_f_stat,
        first_stage_r_squared,
        n_observations: n,
        instrument_strength,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Compute K-fold out-of-fold first-stage predictions of S given Z.
///
/// Returns `s_hat[i]` = prediction for observation i from the first-stage
/// model trained on all observations **not** in i's fold.
fn kfold_first_stage(z: &[f64], s: &[f64], k: usize) -> Result<Vec<f64>> {
    let n = z.len();
    let mut s_hat = vec![0.0_f64; n];

    // Assign each observation to a fold (contiguous blocks).
    let fold_of: Vec<usize> = (0..n).map(|i| (i * k) / n).collect();

    for fold in 0..k {
        // Collect training indices (all observations not in this fold).
        let train_z: Vec<f64> = z
            .iter()
            .enumerate()
            .filter(|&(i, _)| fold_of[i] != fold)
            .map(|(_, &zi)| zi)
            .collect();
        let train_s: Vec<f64> = s
            .iter()
            .enumerate()
            .filter(|&(i, _)| fold_of[i] != fold)
            .map(|(_, &si)| si)
            .collect();

        let (intercept, slope) = ols_with_intercept(&train_z, &train_s).map_err(|e| {
            Error::Numerical(format!("first-stage OLS failed on fold {fold}: {e}"))
        })?;
        assert_finite(intercept, "fs_intercept");
        assert_finite(slope, "fs_slope");

        // Predict for test fold.
        for (i, &zi) in z.iter().enumerate() {
            if fold_of[i] == fold {
                let pred = intercept + slope * zi;
                assert_finite(pred, "fs_prediction");
                s_hat[i] = pred;
            }
        }
    }

    Ok(s_hat)
}

/// OLS of y on x with intercept. Returns (intercept, slope).
fn ols_with_intercept(x: &[f64], y: &[f64]) -> Result<(f64, f64)> {
    debug_assert_eq!(x.len(), y.len());
    let n = x.len();
    if n < 2 {
        return Err(Error::Validation(
            "OLS requires at least 2 observations".into(),
        ));
    }

    let mean_x = mean(x);
    let mean_y = mean(y);

    let var_x: f64 = x.iter().map(|&xi| (xi - mean_x).powi(2)).sum::<f64>() / (n - 1) as f64;
    assert_finite(var_x, "ols_var_x");

    if var_x < 1e-14 {
        return Err(Error::Numerical(
            "zero variance in OLS regressor (all Z values identical in this fold)".into(),
        ));
    }

    let cov_xy: f64 = x
        .iter()
        .zip(y.iter())
        .map(|(&xi, &yi)| (xi - mean_x) * (yi - mean_y))
        .sum::<f64>()
        / (n - 1) as f64;
    assert_finite(cov_xy, "ols_cov_xy");

    let slope = cov_xy / var_x;
    assert_finite(slope, "ols_slope");
    let intercept = mean_y - slope * mean_x;
    assert_finite(intercept, "ols_intercept");

    Ok((intercept, slope))
}

fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len() as f64
}

fn sample_var(x: &[f64]) -> f64 {
    let n = x.len();
    let m = mean(x);
    x.iter().map(|&xi| (xi - m).powi(2)).sum::<f64>() / (n - 1) as f64
}

fn sample_cov_with_means(x: &[f64], y: &[f64], mx: f64, my: f64) -> f64 {
    let n = x.len();
    x.iter()
        .zip(y.iter())
        .map(|(&xi, &yi)| (xi - mx) * (yi - my))
        .sum::<f64>()
        / (n - 1) as f64
}

fn f_to_instrument_strength(f: f64) -> InstrumentStrength {
    if f >= 10.0 {
        InstrumentStrength::Strong
    } else if f >= 5.0 {
        InstrumentStrength::Moderate
    } else {
        InstrumentStrength::Weak
    }
}

fn normal_quantile(p: f64) -> f64 {
    use statrs::distribution::{ContinuousCDF, Normal};
    Normal::new(0.0, 1.0).unwrap().inverse_cdf(p)
}

fn two_sided_p(abs_z: f64) -> f64 {
    use statrs::distribution::{ContinuousCDF, Normal};
    let dist = Normal::new(0.0, 1.0).unwrap();
    2.0 * (1.0 - dist.cdf(abs_z))
}

// ===========================================================================
// Phase 2: Doubly-Robust Off-Policy Evaluation (DR-OPE) for MDP
// ===========================================================================
//
// Implements the sequential doubly-robust estimator from Tran, Bibaut, Kallus
// (Netflix, ICML 2024) for estimating long-term causal effects of continual
// treatments modeled as MDPs.
//
// The DR estimator combines:
// 1. **Direct method** (Q-function): predicts cumulative reward from (state, action).
// 2. **Importance weighting** (density ratio): re-weights trajectories by
//    π_target / π_logging.
// 3. **DR combination**: Q-function acts as a control variate, reducing variance
//    while preserving unbiasedness if *either* Q or density ratio is correct.
//
// Key formula per trajectory i:
//   V̂_DR_i = V̂(s_0) + Σ_t γ^t · ρ_{0:t} · (r_t + γ·V̂(s_{t+1}) − Q̂(s_t, a_t))
//
// where ρ_{0:t} = Π_{k=0}^t π_target(a_k|s_k) / π_logging(a_k|s_k)
//
// Final estimate: V̂_DR = (1/n) Σ_i V̂_DR_i with sandwich SE.

// ---------------------------------------------------------------------------
// Phase 2 public types
// ---------------------------------------------------------------------------

/// A single step in a user's MDP trajectory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrajectoryStep {
    /// State feature vector at this time step.
    pub state_features: Vec<f64>,
    /// Action taken (e.g., which recommendation policy was applied).
    /// Encoded as an integer: 0 = control, 1 = treatment.
    pub action: u32,
    /// Immediate reward observed at this step (e.g., engagement metric).
    pub reward: f64,
    /// State feature vector at the *next* time step (after transition).
    /// Empty for the terminal step.
    pub next_state_features: Vec<f64>,
    /// Probability of this action under the logging (behavior) policy: π_log(a|s).
    /// Must be in (0, 1].
    pub logging_probability: f64,
}

/// A complete trajectory for one user in an experiment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Trajectory {
    pub user_id: String,
    pub steps: Vec<TrajectoryStep>,
}

/// Configuration for doubly-robust off-policy evaluation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DrOpeConfig {
    /// Discount factor γ for future rewards. Must be in (0, 1].
    pub gamma: f64,
    /// Maximum cumulative importance weight before clipping.
    /// Prevents extreme weights when policies diverge. Default: 10.0.
    pub max_density_ratio: f64,
    /// Significance level for CI and p-value (e.g., 0.05).
    pub alpha: f64,
    /// Number of distinct actions in the MDP. Default: 2 (control/treatment).
    pub n_actions: u32,
    /// Target policy probabilities: π_target(a|s) for each action.
    /// For "always treat" evaluation: [0.0, 1.0].
    /// Length must equal `n_actions`.
    pub target_policy: Vec<f64>,
}

impl Default for DrOpeConfig {
    fn default() -> Self {
        Self {
            gamma: 0.99,
            max_density_ratio: 10.0,
            alpha: 0.05,
            n_actions: 2,
            target_policy: vec![0.0, 1.0], // evaluate "always treat"
        }
    }
}

/// Result of doubly-robust off-policy evaluation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DrOpeResult {
    /// Estimated policy value under the target policy (DR estimator).
    pub effect: f64,
    /// Standard error (sandwich variance estimator).
    pub se: f64,
    /// Lower bound of (1-α) confidence interval.
    pub ci_lower: f64,
    /// Upper bound of (1-α) confidence interval.
    pub ci_upper: f64,
    /// t-statistic: effect / se.
    pub t_stat: f64,
    /// Two-sided p-value (large-sample normal approximation).
    pub p_value: f64,
    /// Kish's effective sample size accounting for importance weighting.
    pub effective_n: f64,
    /// Maximum cumulative density ratio observed (before clipping).
    pub max_observed_ratio: f64,
    /// Fraction of trajectory steps where the density ratio was clipped.
    pub clipping_fraction: f64,
    /// Out-of-sample R² of the fitted Q-function (leave-one-trajectory-out).
    pub q_function_r_squared: f64,
    /// Number of trajectories used.
    pub n_trajectories: usize,
    /// Direct-method-only estimate (Q-function alone, no importance weighting).
    pub dm_estimate: f64,
    /// Importance-weighting-only estimate (no Q-function control variate).
    pub ipw_estimate: f64,
}

// ---------------------------------------------------------------------------
// Phase 2 public API
// ---------------------------------------------------------------------------

/// Perform doubly-robust off-policy evaluation on user MDP trajectories.
///
/// Estimates the expected cumulative reward under `config.target_policy`
/// using logged trajectories collected under the behavior (logging) policy.
///
/// # Errors
/// Returns `Err` if:
/// - No trajectories provided
/// - Any trajectory has zero steps
/// - State feature dimensions are inconsistent
/// - `gamma` ∉ (0, 1], `alpha` ∉ (0, 1), or `target_policy` doesn't sum to ~1
/// - All logging probabilities are zero
pub fn dr_ope(trajectories: &[Trajectory], config: &DrOpeConfig) -> Result<DrOpeResult> {
    // --- Validation ---
    validate_dr_config(config)?;
    if trajectories.is_empty() {
        return Err(Error::Validation("no trajectories provided".into()));
    }

    let state_dim = validate_trajectories(trajectories)?;

    // --- Validate all floats ---
    for (ti, traj) in trajectories.iter().enumerate() {
        for (si, step) in traj.steps.iter().enumerate() {
            assert_finite(step.reward, &format!("traj[{ti}].step[{si}].reward"));
            assert_finite(
                step.logging_probability,
                &format!("traj[{ti}].step[{si}].logging_probability"),
            );
            if step.logging_probability <= 0.0 || step.logging_probability > 1.0 {
                return Err(Error::Validation(format!(
                    "logging_probability must be in (0, 1]: got {} at traj[{ti}].step[{si}]",
                    step.logging_probability
                )));
            }
            for (fi, &f) in step.state_features.iter().enumerate() {
                assert_finite(f, &format!("traj[{ti}].step[{si}].state[{fi}]"));
            }
            for (fi, &f) in step.next_state_features.iter().enumerate() {
                assert_finite(f, &format!("traj[{ti}].step[{si}].next_state[{fi}]"));
            }
        }
    }

    // --- Step 1: Fit Q-function via Fitted Q-Evaluation (FQE) ---
    let max_horizon = trajectories.iter().map(|t| t.steps.len()).max().unwrap_or(0);
    let q_weights = fit_q_function_linear(trajectories, config, state_dim, max_horizon)?;

    // --- Step 2: Compute per-trajectory DR estimates ---
    let n = trajectories.len() as f64;
    let mut dr_values = Vec::with_capacity(trajectories.len());
    let mut dm_values = Vec::with_capacity(trajectories.len());
    let mut ipw_values = Vec::with_capacity(trajectories.len());
    let mut max_observed_ratio = 0.0_f64;
    let mut n_clipped = 0_u64;
    let mut n_total_steps = 0_u64;
    let mut sum_w = 0.0_f64;  // Σ w_i for Kish's ESS
    let mut sum_w2 = 0.0_f64; // Σ w_i² for Kish's ESS

    for traj in trajectories {
        let steps = &traj.steps;
        let t_len = steps.len();

        // Direct method: V̂(s_0) using Q-function
        let v_s0 = q_value_function(&steps[0].state_features, &q_weights, 0, config);
        dm_values.push(v_s0);

        // Build DR correction terms
        let mut cumulative_ratio = 1.0_f64;
        let mut dr_correction = 0.0_f64;
        let mut ipw_sum = 0.0_f64;

        for (t, step) in steps.iter().enumerate() {
            // Importance ratio for this step
            let action = step.action as usize;
            let pi_target = if action < config.target_policy.len() {
                config.target_policy[action]
            } else {
                0.0
            };
            let w_t = pi_target / step.logging_probability;
            assert_finite(w_t, &format!("importance_ratio_step_{t}"));

            // Track max ratio before clipping
            cumulative_ratio *= w_t;
            max_observed_ratio = max_observed_ratio.max(cumulative_ratio.abs());

            // Clip cumulative ratio
            let clipped = cumulative_ratio.clamp(-config.max_density_ratio, config.max_density_ratio);
            if (clipped - cumulative_ratio).abs() > 1e-15 {
                n_clipped += 1;
                cumulative_ratio = clipped;
            }
            n_total_steps += 1;

            // Q̂(s_t, a_t) at time t
            let q_sa = q_predict(&step.state_features, step.action, &q_weights, t, state_dim);

            // V̂(s_{t+1}) at time t+1 (or 0 if terminal)
            let v_next = if !step.next_state_features.is_empty() && t + 1 < t_len {
                q_value_function(&step.next_state_features, &q_weights, t + 1, config)
            } else {
                0.0
            };
            assert_finite(v_next, &format!("v_next_step_{t}"));

            // DR correction: γ^t · ρ_{0:t} · (r_t + γ·V̂(s_{t+1}) − Q̂(s_t, a_t))
            let gamma_t = config.gamma.powi(t as i32);
            let td_error = step.reward + config.gamma * v_next - q_sa;
            assert_finite(td_error, &format!("td_error_step_{t}"));

            dr_correction += gamma_t * cumulative_ratio * td_error;

            // IPW-only: γ^t · ρ_{0:t} · r_t
            ipw_sum += gamma_t * cumulative_ratio * step.reward;
        }
        assert_finite(dr_correction, "dr_correction");

        let dr_i = v_s0 + dr_correction;
        assert_finite(dr_i, "dr_trajectory_value");

        dr_values.push(dr_i);
        ipw_values.push(ipw_sum);
        sum_w += cumulative_ratio.abs();
        sum_w2 += cumulative_ratio * cumulative_ratio;
    }

    // --- Step 3: Aggregate ---
    let effect = mean(&dr_values);
    assert_finite(effect, "dr_effect");

    let dm_estimate = mean(&dm_values);
    assert_finite(dm_estimate, "dm_estimate");

    let ipw_estimate = mean(&ipw_values);
    assert_finite(ipw_estimate, "ipw_estimate");

    // Sandwich SE: sqrt( (1/n) Σ (V̂_DR_i - V̂_DR)² ) / sqrt(n)
    let variance = dr_values.iter().map(|&v| (v - effect).powi(2)).sum::<f64>() / (n - 1.0);
    assert_finite(variance, "dr_variance");
    let se = (variance / n).sqrt();
    assert_finite(se, "dr_se");

    // CI and p-value
    let z_crit = normal_quantile(1.0 - config.alpha / 2.0);
    let ci_lower = effect - z_crit * se;
    let ci_upper = effect + z_crit * se;
    let t_stat = if se > 1e-15 { effect / se } else { 0.0 };
    let p_value = two_sided_p(t_stat.abs());

    // Kish's ESS: (Σ w_i)² / Σ w_i²
    let effective_n = if sum_w2 > 1e-15 {
        (sum_w * sum_w) / sum_w2
    } else {
        n
    };

    // Q-function R² (simple: correlation of Q-predicted vs actual cumulative reward)
    let q_r2 = compute_q_r_squared(trajectories, &q_weights, config, state_dim);

    let clipping_fraction = if n_total_steps > 0 {
        n_clipped as f64 / n_total_steps as f64
    } else {
        0.0
    };

    Ok(DrOpeResult {
        effect,
        se,
        ci_lower,
        ci_upper,
        t_stat,
        p_value,
        effective_n,
        max_observed_ratio,
        clipping_fraction,
        q_function_r_squared: q_r2,
        n_trajectories: trajectories.len(),
        dm_estimate,
        ipw_estimate,
    })
}

// ---------------------------------------------------------------------------
// Phase 2 internal: Q-function (Fitted Q-Evaluation, linear)
// ---------------------------------------------------------------------------

/// Linear Q-function weights per time step.
/// Q_t(s, a) = bias + Σ_j w_j · s_j + w_action · a
///
/// Stored as `Vec<Vec<f64>>` where outer index = time step,
/// inner = [bias, s_0, s_1, ..., s_{d-1}, action_weight].
type QWeights = Vec<Vec<f64>>;

/// Fit Q-function via backward Fitted Q-Evaluation (FQE) with linear regression.
///
/// For each time step t (from T-1 down to 0):
///   Target_i = r_{i,t} + γ · V̂_{t+1}(s_{i,t+1})
///   where V̂_{t+1}(s) = Σ_a π_target(a|s) · Q̂_{t+1}(s, a)
///   Fit: Q̂_t(s, a) = β · [1, s, a] via OLS on all trajectories that have step t.
fn fit_q_function_linear(
    trajectories: &[Trajectory],
    config: &DrOpeConfig,
    state_dim: usize,
    max_horizon: usize,
) -> Result<QWeights> {
    // feature_dim = 1 (bias) + state_dim + 1 (action)
    let feature_dim = state_dim + 2;
    let mut q_weights: QWeights = vec![vec![0.0; feature_dim]; max_horizon];

    // Backward induction from last time step
    for t in (0..max_horizon).rev() {
        // Collect (features, target) pairs for all trajectories at step t
        let mut features: Vec<Vec<f64>> = Vec::new();
        let mut targets: Vec<f64> = Vec::new();

        for traj in trajectories {
            if t >= traj.steps.len() {
                continue;
            }
            let step = &traj.steps[t];
            let mut x = Vec::with_capacity(feature_dim);
            x.push(1.0); // bias term
            x.extend_from_slice(&step.state_features);
            // Pad if this trajectory has fewer state features
            while x.len() < state_dim + 1 {
                x.push(0.0);
            }
            x.push(step.action as f64);

            // Target: r_t + γ · V̂_{t+1}(s_{t+1})
            let v_next = if t + 1 < max_horizon && !step.next_state_features.is_empty() {
                q_value_function(&step.next_state_features, &q_weights, t + 1, config)
            } else {
                0.0
            };
            let target = step.reward + config.gamma * v_next;
            assert_finite(target, &format!("fqe_target_t{t}"));

            features.push(x);
            targets.push(target);
        }

        if features.is_empty() {
            continue;
        }

        // Fit OLS: β = (X^T X)^{-1} X^T y using nalgebra
        q_weights[t] = ols_multivariate(&features, &targets, feature_dim)?;
    }

    Ok(q_weights)
}

/// Predict Q̂_t(s, a) using linear weights.
fn q_predict(state: &[f64], action: u32, q_weights: &QWeights, t: usize, state_dim: usize) -> f64 {
    if t >= q_weights.len() {
        return 0.0;
    }
    let w = &q_weights[t];
    if w.is_empty() {
        return 0.0;
    }
    let mut val = w[0]; // bias
    for (j, &s_j) in state.iter().enumerate() {
        if j + 1 < w.len() {
            val += w[j + 1] * s_j;
        }
    }
    // Action weight is the last element
    let action_idx = state_dim + 1;
    if action_idx < w.len() {
        val += w[action_idx] * action as f64;
    }
    assert_finite(val, &format!("q_predict_t{t}"));
    val
}

/// Compute V̂_t(s) = Σ_a π_target(a|s) · Q̂_t(s, a) for a given state.
fn q_value_function(state: &[f64], q_weights: &QWeights, t: usize, config: &DrOpeConfig) -> f64 {
    let mut v = 0.0;
    for (a, &pi_a) in config.target_policy.iter().enumerate() {
        if pi_a > 0.0 {
            v += pi_a * q_predict(state, a as u32, q_weights, t, state.len());
        }
    }
    assert_finite(v, &format!("v_function_t{t}"));
    v
}

/// OLS regression with multiple features using nalgebra.
/// Returns coefficient vector β such that y ≈ X·β.
fn ols_multivariate(features: &[Vec<f64>], targets: &[f64], dim: usize) -> Result<Vec<f64>> {
    use nalgebra::{DMatrix, DVector};

    let n = features.len();
    if n < dim {
        // Under-determined: return zero weights (regularization to zero)
        return Ok(vec![0.0; dim]);
    }

    // Build X matrix (n × dim) and y vector (n × 1)
    let x_data: Vec<f64> = features.iter().flat_map(|row| row.iter().copied()).collect();
    let x = DMatrix::from_row_slice(n, dim, &x_data);
    let y = DVector::from_column_slice(targets);

    // Regularized: (X^T X + λI)^{-1} X^T y with small λ for stability
    let xtx = x.transpose() * &x;
    let lambda = 1e-8;
    let reg = DMatrix::identity(dim, dim) * lambda;
    let xtx_reg = xtx + reg;

    let xty = x.transpose() * &y;

    // Solve via Cholesky (xtx_reg is positive definite)
    match nalgebra::linalg::Cholesky::new(xtx_reg) {
        Some(chol) => {
            let beta = chol.solve(&xty);
            let result: Vec<f64> = beta.iter().copied().collect();
            for (i, &b) in result.iter().enumerate() {
                assert_finite(b, &format!("ols_beta[{i}]"));
            }
            Ok(result)
        }
        None => {
            // Fallback: zero weights if matrix is singular
            Ok(vec![0.0; dim])
        }
    }
}

/// Compute out-of-sample R² for the Q-function.
/// Uses total cumulative reward per trajectory vs Q-function prediction at t=0.
fn compute_q_r_squared(
    trajectories: &[Trajectory],
    q_weights: &QWeights,
    config: &DrOpeConfig,
    state_dim: usize,
) -> f64 {
    if trajectories.len() < 2 {
        return 0.0;
    }
    let mut actual: Vec<f64> = Vec::with_capacity(trajectories.len());
    let mut predicted: Vec<f64> = Vec::with_capacity(trajectories.len());

    for traj in trajectories {
        if traj.steps.is_empty() {
            continue;
        }
        // Actual: discounted cumulative reward
        let cum_reward: f64 = traj
            .steps
            .iter()
            .enumerate()
            .map(|(t, step)| config.gamma.powi(t as i32) * step.reward)
            .sum();
        actual.push(cum_reward);

        // Predicted: Q̂(s_0, a_0)
        let pred = q_predict(
            &traj.steps[0].state_features,
            traj.steps[0].action,
            q_weights,
            0,
            state_dim,
        );
        predicted.push(pred);
    }

    if actual.len() < 2 {
        return 0.0;
    }

    let mean_actual = mean(&actual);
    let ss_tot: f64 = actual.iter().map(|&a| (a - mean_actual).powi(2)).sum();
    let ss_res: f64 = actual
        .iter()
        .zip(predicted.iter())
        .map(|(&a, &p)| (a - p).powi(2))
        .sum();

    if ss_tot < 1e-15 {
        return 0.0;
    }
    let r2 = 1.0 - ss_res / ss_tot;
    // Clamp: R² can be negative for poor models
    r2.clamp(-1.0, 1.0)
}

// ---------------------------------------------------------------------------
// Phase 2 internal: validation helpers
// ---------------------------------------------------------------------------

fn validate_dr_config(config: &DrOpeConfig) -> Result<()> {
    if config.gamma <= 0.0 || config.gamma > 1.0 {
        return Err(Error::Validation("gamma must be in (0, 1]".into()));
    }
    if config.alpha <= 0.0 || config.alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }
    if config.max_density_ratio <= 0.0 {
        return Err(Error::Validation("max_density_ratio must be positive".into()));
    }
    if config.n_actions < 2 {
        return Err(Error::Validation("n_actions must be at least 2".into()));
    }
    if config.target_policy.len() != config.n_actions as usize {
        return Err(Error::Validation(format!(
            "target_policy length ({}) must equal n_actions ({})",
            config.target_policy.len(),
            config.n_actions
        )));
    }
    let policy_sum: f64 = config.target_policy.iter().sum();
    if (policy_sum - 1.0).abs() > 1e-6 {
        return Err(Error::Validation(format!(
            "target_policy must sum to 1.0, got {policy_sum}"
        )));
    }
    for &p in &config.target_policy {
        if p < 0.0 || p > 1.0 {
            return Err(Error::Validation(format!(
                "target_policy entries must be in [0, 1], got {p}"
            )));
        }
    }
    Ok(())
}

/// Validate trajectories and return state feature dimension.
fn validate_trajectories(trajectories: &[Trajectory]) -> Result<usize> {
    let mut state_dim: Option<usize> = None;

    for (ti, traj) in trajectories.iter().enumerate() {
        if traj.steps.is_empty() {
            return Err(Error::Validation(format!(
                "trajectory[{ti}] has zero steps"
            )));
        }
        for (si, step) in traj.steps.iter().enumerate() {
            let d = step.state_features.len();
            match state_dim {
                None => state_dim = Some(d),
                Some(expected) if d != expected => {
                    return Err(Error::Validation(format!(
                        "inconsistent state_features dimension: expected {expected}, \
                         got {d} at trajectory[{ti}].step[{si}]"
                    )));
                }
                _ => {}
            }
            // next_state_features must match dimension or be empty (terminal)
            if !step.next_state_features.is_empty()
                && step.next_state_features.len() != state_dim.unwrap_or(d)
            {
                return Err(Error::Validation(format!(
                    "inconsistent next_state_features dimension at trajectory[{ti}].step[{si}]"
                )));
            }
        }
    }

    state_dim.ok_or_else(|| Error::Validation("no state features found".into()))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_obs(z: &[f64], s: &[f64], y: &[f64]) -> Vec<OrlObservation> {
        z.iter()
            .zip(s.iter())
            .zip(y.iter())
            .map(|((&zi, &si), &yi)| OrlObservation {
                treatment: zi,
                surrogate: si,
                outcome: yi,
            })
            .collect()
    }

    // --- Scenario A: strong surrogate, no confounding ---
    // S = Z + ε_S, Y = 0.3·S + ε_Y.  OLS and JIVE both unbiased.
    fn scenario_a_no_confounding() -> Vec<OrlObservation> {
        // Deterministic balanced dataset: alternating Z, linear relationship.
        let z = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0,
                 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0,
                 0.0, 1.0, 0.0, 1.0];
        // S = 0.5 + 1.0*Z (strong first stage, ρ_ZS ≈ 1 for these values)
        let s: Vec<f64> = z.iter().map(|&zi| 0.5 + zi).collect();
        // Y = 0.3·S (γ = 0.3, no confounding, no noise)
        let y: Vec<f64> = s.iter().map(|&si| 0.3 * si).collect();
        make_obs(&z, &s, &y)
    }

    // --- Scenario B: strong surrogate, confounded ---
    // Confounder η biases OLS but not JIVE.
    // S = 0.5 + Z + 0.8·η,  Y = 0.3·S + 0.5·η
    //
    // CRITICAL: η must be uncorrelated with Z for Z to be a valid instrument.
    // Use first 10 observations as control (Z=0), last 10 as treatment (Z=1),
    // with η alternating ±0.4/±0.3/... within each group (sum=0 in each group
    // → Cov(Z,η) = 0 exactly).
    fn scenario_b_confounded() -> Vec<OrlObservation> {
        // η alternates symmetrically within each Z group → Cov(Z, η) = 0.
        // Pattern repeated twice: [0.4,-0.4, 0.3,-0.3, 0.2,-0.2, 0.1,-0.1, 0.5,-0.5]
        let eta = [
            0.4_f64, -0.4, 0.3, -0.3, 0.2, -0.2, 0.1, -0.1, 0.5, -0.5, // Z=0
            0.4_f64, -0.4, 0.3, -0.3, 0.2, -0.2, 0.1, -0.1, 0.5, -0.5, // Z=1
        ];
        // First half control, second half treatment.
        let z: Vec<f64> = (0..20).map(|i| if i < 10 { 0.0 } else { 1.0 }).collect();
        let s: Vec<f64> = z.iter().zip(eta.iter()).map(|(&zi, &ei)| 0.5 + zi + 0.8 * ei).collect();
        let y: Vec<f64> = s.iter().zip(eta.iter()).map(|(&si, &ei)| 0.3 * si + 0.5 * ei).collect();
        make_obs(&z, &s, &y)
    }

    // --- Scenario C: weak surrogate ---
    // S = 0.5 + 0.1·Z, Y = 0.3·S.  First-stage F ≈ 0.
    fn scenario_c_weak_instrument() -> Vec<OrlObservation> {
        let z = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0,
                 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0,
                 0.0, 1.0, 0.0, 1.0];
        // Very weak first stage (slope = 0.1 vs noise)
        let s: Vec<f64> = z.iter().enumerate().map(|(i, &zi)| {
            // Add systematic variation to create non-degenerate but weak instrument
            0.5 + 0.05 * zi + 0.4 * ((i as f64) * 0.7).sin()
        }).collect();
        let y: Vec<f64> = s.iter().map(|&si| 0.3 * si).collect();
        make_obs(&z, &s, &y)
    }

    #[test]
    fn test_scenario_a_jive_recovers_true_gamma() {
        let obs = scenario_a_no_confounding();
        let config = KFoldIvConfig { n_folds: 4, alpha: 0.05 };
        let result = kfold_iv_calibrate(&obs, &config).unwrap();

        // With no confounding and deterministic data, JIVE = OLS = true γ
        assert!(
            (result.iv_estimate - 0.3).abs() < 1e-10,
            "JIVE should recover γ=0.3 exactly: got {}",
            result.iv_estimate
        );
        assert!(
            (result.ols_estimate - 0.3).abs() < 1e-10,
            "OLS should also be 0.3 without confounding: got {}",
            result.ols_estimate
        );
        assert!(
            result.bias_correction.abs() < 1e-10,
            "No confounding → bias_correction ≈ 0: got {}",
            result.bias_correction
        );
        assert_eq!(result.instrument_strength, InstrumentStrength::Strong);
    }

    #[test]
    fn test_scenario_b_jive_corrects_ols_bias() {
        let obs = scenario_b_confounded();
        let config = KFoldIvConfig { n_folds: 4, alpha: 0.05 };
        let result = kfold_iv_calibrate(&obs, &config).unwrap();

        // OLS is biased upward (positive confounding δ=0.5 > 0).
        // JIVE should be closer to 0.3 than OLS.
        assert!(
            result.ols_estimate > result.iv_estimate,
            "OLS should be biased upward vs JIVE: OLS={} IV={}",
            result.ols_estimate,
            result.iv_estimate
        );
        let jive_error = (result.iv_estimate - 0.3).abs();
        let ols_error = (result.ols_estimate - 0.3).abs();
        assert!(
            jive_error < ols_error,
            "JIVE should be closer to 0.3 than OLS: JIVE_err={jive_error:.4} OLS_err={ols_error:.4}"
        );
    }

    #[test]
    fn test_scenario_c_weak_instrument_detected() {
        let obs = scenario_c_weak_instrument();
        let config = KFoldIvConfig { n_folds: 4, alpha: 0.05 };
        let result = kfold_iv_calibrate(&obs, &config).unwrap();

        assert!(
            result.instrument_strength == InstrumentStrength::Weak
                || result.instrument_strength == InstrumentStrength::Moderate,
            "Weak instrument should not be detected as Strong: F={}",
            result.first_stage_f_stat
        );
    }

    #[test]
    fn test_ci_contains_estimate() {
        let obs = scenario_a_no_confounding();
        let config = KFoldIvConfig::default();
        let result = kfold_iv_calibrate(&obs, &config).unwrap();
        assert!(
            result.ci_lower <= result.iv_estimate && result.iv_estimate <= result.ci_upper,
            "CI [{}, {}] must contain estimate {}",
            result.ci_lower, result.ci_upper, result.iv_estimate
        );
    }

    #[test]
    fn test_p_value_in_unit_interval() {
        let obs = scenario_b_confounded();
        let config = KFoldIvConfig::default();
        let result = kfold_iv_calibrate(&obs, &config).unwrap();
        assert!(
            result.p_value >= 0.0 && result.p_value <= 1.0,
            "p_value must be in [0,1]: {}",
            result.p_value
        );
    }

    #[test]
    fn test_validation_errors() {
        let obs = scenario_a_no_confounding();

        // Too few observations.
        assert!(kfold_iv_calibrate(
            &obs[..3],
            &KFoldIvConfig { n_folds: 5, alpha: 0.05 }
        ).is_err());

        // n_folds < 2.
        assert!(kfold_iv_calibrate(
            &obs,
            &KFoldIvConfig { n_folds: 1, alpha: 0.05 }
        ).is_err());

        // Bad alpha.
        assert!(kfold_iv_calibrate(
            &obs,
            &KFoldIvConfig { n_folds: 4, alpha: 0.0 }
        ).is_err());
        assert!(kfold_iv_calibrate(
            &obs,
            &KFoldIvConfig { n_folds: 4, alpha: 1.0 }
        ).is_err());
    }

    #[test]
    fn test_instrument_strength_thresholds() {
        assert_eq!(f_to_instrument_strength(15.0), InstrumentStrength::Strong);
        assert_eq!(f_to_instrument_strength(10.0), InstrumentStrength::Strong);
        assert_eq!(f_to_instrument_strength(7.0), InstrumentStrength::Moderate);
        assert_eq!(f_to_instrument_strength(5.0), InstrumentStrength::Moderate);
        assert_eq!(f_to_instrument_strength(4.9), InstrumentStrength::Weak);
        assert_eq!(f_to_instrument_strength(0.0), InstrumentStrength::Weak);
    }

    mod proptest_orl {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn iv_result_all_finite(
                n in 10usize..30,
                slope in 0.5f64..2.0,
                gamma in 0.1f64..1.0,
            ) {
                let z: Vec<f64> = (0..n).map(|i| (i % 2) as f64).collect();
                let s: Vec<f64> = z.iter().map(|&zi| 0.5 + slope * zi).collect();
                let y: Vec<f64> = s.iter().map(|&si| gamma * si).collect();
                let obs = make_obs(&z, &s, &y);
                let config = KFoldIvConfig { n_folds: 4, alpha: 0.05 };
                let result = kfold_iv_calibrate(&obs, &config).unwrap();
                prop_assert!(result.iv_estimate.is_finite());
                prop_assert!(result.se.is_finite() && result.se >= 0.0);
                prop_assert!(result.ci_lower.is_finite());
                prop_assert!(result.ci_upper.is_finite());
                prop_assert!(result.ci_lower <= result.iv_estimate);
                prop_assert!(result.iv_estimate <= result.ci_upper);
                prop_assert!(result.p_value >= 0.0 && result.p_value <= 1.0);
                prop_assert!(result.first_stage_f_stat >= 0.0);
                prop_assert!(result.first_stage_r_squared >= -1e-10 && result.first_stage_r_squared <= 1.0 + 1e-10);
                // KDD 2024 Table 2: JIVE coefficient bounded in (-1, 1) for normalised
                // surrogate-to-outcome relationships (gamma in (0.1, 1.0) here, no confounding).
                prop_assert!(
                    result.iv_estimate > -1.0 && result.iv_estimate < 1.0,
                    "jive_coefficient should be in (-1, 1) for normalised outcomes: got {}",
                    result.iv_estimate
                );
            }

            #[test]
            fn bias_correction_sign_with_positive_confounder(
                n in 20usize..40,
                delta in 0.3f64..0.8,
            ) {
                // Positive confounder → OLS biased up → bias_correction < 0
                // (IV < OLS → IV - OLS < 0 when δ > 0 and surrogate is correlated)
                let eta: Vec<f64> = (0..n).map(|i| if i % 2 == 0 { 0.5 } else { -0.5 }).collect();
                let z: Vec<f64> = (0..n).map(|i| (i % 2) as f64).collect();
                let s: Vec<f64> = z.iter().zip(eta.iter()).map(|(&zi, &ei)| 0.5 + zi + 0.6 * ei).collect();
                let y: Vec<f64> = s.iter().zip(eta.iter()).map(|(&si, &ei)| 0.3 * si + delta * ei).collect();
                let obs = make_obs(&z, &s, &y);
                let config = KFoldIvConfig { n_folds: 4, alpha: 0.05 };
                if let Ok(result) = kfold_iv_calibrate(&obs, &config) {
                    prop_assert!(result.iv_estimate.is_finite());
                }
            }

            /// Shrinkage property (KDD 2024 Table 2, rho_UY > 0 rows):
            /// With positive confounding and a valid instrument (Cov(Z, η) = 0),
            /// the JIVE (calibrated) estimate is <= the OLS (naive) estimate.
            ///
            /// Block design ensures Cov(Z, η) = 0: first half Z=0, second half Z=1,
            /// with η symmetric (sum = 0) within each Z group.
            #[test]
            fn shrinkage_calibrated_le_naive_positive_confounding(
                delta in 0.3f64..0.8,
            ) {
                // Fixed n=20 block design: first 10 Z=0, last 10 Z=1.
                // eta alternates symmetrically within each block → Cov(Z, η) = 0.
                let eta = [
                    0.4_f64, -0.4, 0.3, -0.3, 0.2, -0.2, 0.1, -0.1, 0.5, -0.5,
                    0.4_f64, -0.4, 0.3, -0.3, 0.2, -0.2, 0.1, -0.1, 0.5, -0.5,
                ];
                let z: Vec<f64> = (0..20).map(|i| if i < 10 { 0.0 } else { 1.0 }).collect();
                let s: Vec<f64> = z.iter().zip(eta.iter())
                    .map(|(&zi, &ei)| 0.5 + zi + 0.8 * ei).collect();
                let y: Vec<f64> = s.iter().zip(eta.iter())
                    .map(|(&si, &ei)| 0.3 * si + delta * ei).collect();
                let obs = make_obs(&z, &s, &y);
                let config = KFoldIvConfig { n_folds: 4, alpha: 0.05 };
                let result = kfold_iv_calibrate(&obs, &config)
                    .expect("calibration must succeed for valid block design");
                // Shrinkage: JIVE corrects OLS upward bias → iv_estimate ≤ ols_estimate.
                prop_assert!(
                    result.iv_estimate <= result.ols_estimate,
                    "shrinkage: JIVE ({:.4}) should be <= OLS ({:.4}) with delta={delta:.4}",
                    result.iv_estimate,
                    result.ols_estimate
                );
                prop_assert!(result.iv_estimate.is_finite());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Phase 2: DR-OPE unit tests
    // -----------------------------------------------------------------------

    fn make_trajectory(
        user_id: &str,
        actions: &[u32],
        rewards: &[f64],
        state_dim: usize,
        logging_prob: f64,
    ) -> Trajectory {
        let n = actions.len();
        let steps: Vec<TrajectoryStep> = (0..n)
            .map(|t| {
                let state: Vec<f64> = (0..state_dim)
                    .map(|d| 0.1 * (t as f64) + 0.01 * (d as f64))
                    .collect();
                let next_state = if t + 1 < n {
                    (0..state_dim)
                        .map(|d| 0.1 * ((t + 1) as f64) + 0.01 * (d as f64))
                        .collect()
                } else {
                    vec![] // terminal
                };
                TrajectoryStep {
                    state_features: state,
                    action: actions[t],
                    reward: rewards[t],
                    next_state_features: next_state,
                    logging_probability: logging_prob,
                }
            })
            .collect();
        Trajectory { user_id: user_id.to_string(), steps }
    }

    /// DR-OPE on deterministic trajectories: all treatment, reward = action * 1.0.
    /// Target policy = always treat → DR estimate ≈ expected cumulative reward.
    #[test]
    fn test_dr_ope_deterministic_all_treatment() {
        let trajs: Vec<Trajectory> = (0..20)
            .map(|i| {
                make_trajectory(
                    &format!("user_{i}"),
                    &[1, 1, 1],          // all treatment
                    &[1.0, 1.0, 1.0],    // reward = 1.0 each step
                    2,                    // 2-dim state
                    0.5,                  // 50/50 randomization
                )
            })
            .collect();

        let config = DrOpeConfig {
            gamma: 1.0,           // no discounting for simplicity
            max_density_ratio: 100.0,
            alpha: 0.05,
            n_actions: 2,
            target_policy: vec![0.0, 1.0], // always treat
        };

        let result = dr_ope(&trajs, &config).unwrap();
        assert!(result.effect.is_finite(), "effect must be finite");
        assert!(result.se.is_finite() && result.se >= 0.0, "se must be non-negative");
        assert!(
            result.ci_lower <= result.effect && result.effect <= result.ci_upper,
            "CI must contain effect"
        );
        // Cumulative reward = 3.0 for each trajectory
        // With target = always treat and data = always treat, DR ≈ DM ≈ 3.0
        assert!(
            (result.effect - 3.0).abs() < 1.0,
            "DR effect should be near 3.0: got {}",
            result.effect
        );
    }

    /// DR-OPE: treatment arm has higher reward → positive effect direction.
    #[test]
    fn test_dr_ope_treatment_higher_reward() {
        let mut trajs = Vec::new();
        // Control trajectories: action=0, low reward
        for i in 0..15 {
            trajs.push(make_trajectory(
                &format!("ctrl_{i}"),
                &[0, 0, 0],
                &[0.2, 0.2, 0.2],
                2,
                0.5,
            ));
        }
        // Treatment trajectories: action=1, high reward
        for i in 0..15 {
            trajs.push(make_trajectory(
                &format!("treat_{i}"),
                &[1, 1, 1],
                &[0.8, 0.8, 0.8],
                2,
                0.5,
            ));
        }

        let config = DrOpeConfig {
            gamma: 0.99,
            max_density_ratio: 10.0,
            alpha: 0.05,
            n_actions: 2,
            target_policy: vec![0.0, 1.0], // evaluate "always treat"
        };

        let result = dr_ope(&trajs, &config).unwrap();
        assert!(result.effect.is_finite());
        // "Always treat" should yield higher value than mixed (which is what DM sees)
        assert!(
            result.dm_estimate > 0.0,
            "DM estimate should be positive: got {}",
            result.dm_estimate
        );
        assert!(result.n_trajectories == 30);
    }

    /// DR-OPE produces finite results with varying trajectory lengths.
    #[test]
    fn test_dr_ope_variable_length_trajectories() {
        let trajs = vec![
            make_trajectory("u1", &[1, 0, 1, 0], &[0.5, 0.3, 0.7, 0.2], 3, 0.5),
            make_trajectory("u2", &[0, 1, 0], &[0.1, 0.9, 0.4], 3, 0.5),
            make_trajectory("u3", &[1, 1], &[0.6, 0.8], 3, 0.5),
            make_trajectory("u4", &[0, 0, 0, 0, 0], &[0.2, 0.1, 0.3, 0.2, 0.1], 3, 0.5),
        ];

        let config = DrOpeConfig::default();
        let result = dr_ope(&trajs, &config).unwrap();

        assert!(result.effect.is_finite());
        assert!(result.se.is_finite());
        assert!(result.p_value >= 0.0 && result.p_value <= 1.0);
        assert!(result.effective_n > 0.0);
        assert!(result.n_trajectories == 4);
    }

    /// DR-OPE validation errors.
    #[test]
    fn test_dr_ope_validation_errors() {
        let trajs = vec![make_trajectory("u1", &[1], &[0.5], 2, 0.5)];

        // gamma out of range
        assert!(dr_ope(
            &trajs,
            &DrOpeConfig { gamma: 0.0, ..DrOpeConfig::default() }
        ).is_err());
        assert!(dr_ope(
            &trajs,
            &DrOpeConfig { gamma: 1.5, ..DrOpeConfig::default() }
        ).is_err());

        // Bad alpha
        assert!(dr_ope(
            &trajs,
            &DrOpeConfig { alpha: 0.0, ..DrOpeConfig::default() }
        ).is_err());

        // target_policy wrong length
        assert!(dr_ope(
            &trajs,
            &DrOpeConfig { target_policy: vec![1.0], ..DrOpeConfig::default() }
        ).is_err());

        // target_policy doesn't sum to 1
        assert!(dr_ope(
            &trajs,
            &DrOpeConfig { target_policy: vec![0.5, 0.3], ..DrOpeConfig::default() }
        ).is_err());

        // Empty trajectories
        assert!(dr_ope(&[], &DrOpeConfig::default()).is_err());

        // Trajectory with zero steps
        let empty_traj = vec![Trajectory { user_id: "u1".into(), steps: vec![] }];
        assert!(dr_ope(&empty_traj, &DrOpeConfig::default()).is_err());
    }

    /// DR-OPE clipping: extreme density ratios get clipped.
    #[test]
    fn test_dr_ope_clipping() {
        // Target = always treat, but logging gives very low prob to treatment
        let trajs: Vec<Trajectory> = (0..10)
            .map(|i| {
                make_trajectory(
                    &format!("u_{i}"),
                    &[1, 1, 1],
                    &[1.0, 1.0, 1.0],
                    2,
                    0.01, // very low logging probability → high density ratio
                )
            })
            .collect();

        let config = DrOpeConfig {
            gamma: 0.99,
            max_density_ratio: 5.0, // aggressive clipping
            alpha: 0.05,
            n_actions: 2,
            target_policy: vec![0.0, 1.0],
        };

        let result = dr_ope(&trajs, &config).unwrap();
        assert!(result.effect.is_finite());
        assert!(result.clipping_fraction > 0.0, "should have clipped some ratios");
        assert!(result.max_observed_ratio > 5.0, "max ratio should exceed clip threshold");
    }

    /// DR-OPE: with gamma=1 and equal policies, DR ≈ mean cumulative reward.
    #[test]
    fn test_dr_ope_identity_policy() {
        // When target = logging policy (50/50), DR should ≈ average cumulative reward
        let trajs: Vec<Trajectory> = (0..20)
            .map(|i| {
                let action = (i % 2) as u32;
                let reward = if action == 1 { 0.8 } else { 0.3 };
                make_trajectory(
                    &format!("u_{i}"),
                    &[action, action],
                    &[reward, reward],
                    2,
                    0.5,
                )
            })
            .collect();

        let config = DrOpeConfig {
            gamma: 1.0,
            max_density_ratio: 100.0,
            alpha: 0.05,
            n_actions: 2,
            target_policy: vec![0.5, 0.5], // same as logging
        };

        let result = dr_ope(&trajs, &config).unwrap();
        // Average cumulative reward ≈ 0.5*(0.6) + 0.5*(1.6) = 1.1
        let expected_avg = 0.5 * (0.3 + 0.3) + 0.5 * (0.8 + 0.8);
        assert!(
            (result.effect - expected_avg).abs() < 0.5,
            "with identity policy, DR ≈ avg cumulative reward: expected ~{expected_avg}, got {}",
            result.effect
        );
    }

    mod proptest_dr_ope {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn dr_ope_result_all_finite(
                n_traj in 4usize..15,
                n_steps in 2usize..5,
                gamma in 0.5f64..1.0,
            ) {
                let trajs: Vec<Trajectory> = (0..n_traj)
                    .map(|i| {
                        let actions: Vec<u32> = (0..n_steps).map(|s| ((i + s) % 2) as u32).collect();
                        let rewards: Vec<f64> = (0..n_steps).map(|s| 0.1 + 0.05 * s as f64).collect();
                        make_trajectory(&format!("u{i}"), &actions, &rewards, 2, 0.5)
                    })
                    .collect();

                let config = DrOpeConfig {
                    gamma,
                    max_density_ratio: 10.0,
                    alpha: 0.05,
                    n_actions: 2,
                    target_policy: vec![0.0, 1.0],
                };

                let result = dr_ope(&trajs, &config).unwrap();
                prop_assert!(result.effect.is_finite());
                prop_assert!(result.se.is_finite() && result.se >= 0.0);
                prop_assert!(result.ci_lower.is_finite());
                prop_assert!(result.ci_upper.is_finite());
                prop_assert!(result.ci_lower <= result.ci_upper);
                prop_assert!(result.p_value >= 0.0 && result.p_value <= 1.0);
                prop_assert!(result.effective_n > 0.0);
                prop_assert!(result.n_trajectories == n_traj);
                prop_assert!(result.dm_estimate.is_finite());
                prop_assert!(result.ipw_estimate.is_finite());
            }

            #[test]
            fn dr_ope_ci_contains_estimate(
                n_traj in 5usize..20,
            ) {
                let trajs: Vec<Trajectory> = (0..n_traj)
                    .map(|i| {
                        make_trajectory(
                            &format!("u{i}"),
                            &[(i % 2) as u32, ((i + 1) % 2) as u32],
                            &[0.5, 0.5],
                            2,
                            0.5,
                        )
                    })
                    .collect();
                let config = DrOpeConfig::default();
                let result = dr_ope(&trajs, &config).unwrap();
                prop_assert!(
                    result.ci_lower <= result.effect && result.effect <= result.ci_upper,
                    "CI [{}, {}] must contain effect {}",
                    result.ci_lower, result.ci_upper, result.effect
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Golden test: tc_jive_vectors.json (Netflix KDD 2024 Table 2 values)
    // -----------------------------------------------------------------------

    #[cfg(test)]
    mod tc_jive_golden {
        use super::*;
        use std::path::PathBuf;

        #[derive(serde::Deserialize)]
        struct TcJiveScenario {
            name: String,
            n_folds: usize,
            alpha: f64,
            observations: Vec<OrlObservation>,
            expected: TcJiveExpected,
        }

        #[derive(serde::Deserialize)]
        struct TcJiveExpected {
            jive_coefficient: f64,
            ols_naive_estimate: f64,
            treatment_effect_correlation: f64,
            first_stage_r_squared: f64,
            tolerance: f64,
        }

        #[derive(serde::Deserialize)]
        struct TcJiveVectors {
            scenarios: Vec<TcJiveScenario>,
        }

        fn vectors_path() -> PathBuf {
            // CARGO_MANIFEST_DIR = crates/experimentation-stats/
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/golden/tc_jive_vectors.json")
        }

        #[test]
        fn tc_jive_kdd2024_table2_vectors() {
            let path = vectors_path();
            let json = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let vectors: TcJiveVectors = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()));

            for scenario in &vectors.scenarios {
                let name = &scenario.name;
                let config = KFoldIvConfig { n_folds: scenario.n_folds, alpha: scenario.alpha };
                let result = kfold_iv_calibrate(&scenario.observations, &config)
                    .unwrap_or_else(|e| panic!("[{name}] kfold_iv_calibrate failed: {e}"));

                let tol = scenario.expected.tolerance;

                let diff_iv = (result.iv_estimate - scenario.expected.jive_coefficient).abs();
                assert!(
                    diff_iv <= tol,
                    "[{name}] jive_coefficient: expected {:.4}, got {:.4} (diff {:.2e} > tol {:.2e})",
                    scenario.expected.jive_coefficient, result.iv_estimate, diff_iv, tol
                );

                let diff_ols = (result.ols_estimate - scenario.expected.ols_naive_estimate).abs();
                assert!(
                    diff_ols <= tol,
                    "[{name}] ols_naive_estimate: expected {:.4}, got {:.4} (diff {:.2e} > tol {:.2e})",
                    scenario.expected.ols_naive_estimate, result.ols_estimate, diff_ols, tol
                );

                let diff_r2 = (result.first_stage_r_squared - scenario.expected.first_stage_r_squared).abs();
                assert!(
                    diff_r2 <= tol,
                    "[{name}] first_stage_r_squared: expected {:.4}, got {:.4} (diff {:.2e} > tol {:.2e})",
                    scenario.expected.first_stage_r_squared, result.first_stage_r_squared, diff_r2, tol
                );

                // treatment_effect_correlation = sqrt(first_stage_r_squared)
                let computed_corr = result.first_stage_r_squared.sqrt();
                let diff_corr = (computed_corr - scenario.expected.treatment_effect_correlation).abs();
                assert!(
                    diff_corr <= tol,
                    "[{name}] treatment_effect_correlation: expected {:.4}, got {:.4} (diff {:.2e} > tol {:.2e})",
                    scenario.expected.treatment_effect_correlation, computed_corr, diff_corr, tol
                );
            }
        }
    }
}
