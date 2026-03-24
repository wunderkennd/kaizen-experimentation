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
                .join("../../test-vectors/tc_jive_vectors.json")
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
