//! Two One-Sided Tests (TOST) equivalence testing — ADR-027.
//!
//! TOST inverts the standard hypothesis structure. Instead of testing H₀: μ₁ = μ₂
//! against H₁: μ₁ ≠ μ₂ (which only rejects "no difference found"), TOST tests:
//!
//! - H₀₁: μ_T − μ_C ≤ −δ   (reject if the effect is clearly greater than −δ)
//! - H₀₂: μ_T − μ_C ≥ +δ   (reject if the effect is clearly less than +δ)
//!
//! If both one-sided tests reject at level α, the treatment is declared
//! *equivalent* to control within the margin ±δ. The TOST p-value is the
//! maximum of the two one-sided p-values. Equivalence is algebraically
//! identical to the (1 − 2α) confidence interval lying entirely within (−δ, +δ).
//!
//! Internals use Welch's t-test (unequal variances, Satterthwaite df) — the
//! same formulation R's TOSTER package uses for `TOSTER::t_TOST(..., var.equal = FALSE)`.
//!
//! # Composition with CUPED (ADR-027 §2)
//!
//! `tost_cuped_equivalence_test` composes TOST with CUPED variance reduction
//! using the same pooled-θ formulation as `cuped::cuped_adjust`. This enables
//! equivalence conclusions at ~½ the sample size on high-variance metrics
//! that have a correlated pre-experiment covariate.
//!
//! # Power analysis (ADR-027 §3)
//!
//! `tost_sample_size` returns the required per-group sample size using the
//! Chow-Shao-Wang / Phillips normal approximation, which is the standard
//! design-stage formula used by R TOSTER and PASS. TOST requires roughly
//! 2× the sample size of a same-δ superiority test.
//!
//! # References
//! - Schuirmann, D.J. (1987). *J. Pharmacokinet. Biopharm.* 15(6), 657–680.
//! - Lakens, D. (2017). *Soc. Psychol. Pers. Sci.* 8(4), 355–362.
//! - Phillips, K.F. (1990). *J. Pharmacokinet. Biopharm.* 18(2), 137–144.
//! - Chow, Shao, Wang (2008). *Sample Size Calculations in Clinical Research*, §4.3.

use experimentation_core::error::{assert_finite, Error, Result};
use statrs::distribution::{ContinuousCDF, Normal, StudentsT};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Configuration for a TOST equivalence test.
#[derive(Debug, Clone)]
pub struct TostConfig {
    /// Equivalence margin in the metric's natural units. Must be strictly positive.
    /// Treatment is declared "equivalent" if the effect lies within (−δ, +δ).
    pub delta: f64,
    /// Significance level for each one-sided test. Typical value 0.05.
    pub alpha: f64,
}

impl TostConfig {
    /// New TOST config with the default α = 0.05.
    pub fn new(delta: f64) -> Self {
        Self { delta, alpha: 0.05 }
    }
}

/// Result of a TOST equivalence test.
#[derive(Debug, Clone)]
pub struct TostResult {
    /// Point estimate of the difference treatment − control.
    pub point_estimate: f64,
    /// Standard error of the difference (Welch form: √(s²_T/n_T + s²_C/n_C)).
    pub std_error: f64,
    /// Welch-Satterthwaite degrees of freedom.
    pub df: f64,
    /// p-value for H₀₁: diff ≤ −δ (upper-tail test).
    pub p_lower: f64,
    /// p-value for H₀₂: diff ≥ +δ (lower-tail test).
    pub p_upper: f64,
    /// TOST p-value — max(p_lower, p_upper). Equivalence iff p_tost < α.
    pub p_tost: f64,
    /// Lower bound of the (1 − 2α) confidence interval for the difference.
    /// Equivalence ⟺ CI ⊂ (−δ, +δ).
    pub ci_lower: f64,
    /// Upper bound of the (1 − 2α) confidence interval for the difference.
    pub ci_upper: f64,
    /// True iff both one-sided tests reject at α, i.e. CI ⊂ (−δ, +δ).
    pub equivalent: bool,
    /// The equivalence margin used for this test (echoed from config).
    pub delta: f64,
    /// Control group mean.
    pub control_mean: f64,
    /// Treatment group mean.
    pub treatment_mean: f64,
}

// ---------------------------------------------------------------------------
// Core TOST (Welch internals)
// ---------------------------------------------------------------------------

/// Run a TOST equivalence test on two independent samples using Welch's
/// t-statistic (unequal variance, Satterthwaite df).
///
/// # Arguments
/// * `control`  — observations from the control group (≥ 2).
/// * `treatment` — observations from the treatment group (≥ 2).
/// * `config`   — equivalence margin `delta` and per-side significance `alpha`.
///
/// # Errors
/// Returns `Error::Validation` for insufficient sample size or invalid config
/// (δ ≤ 0, α ∉ (0, 0.5)). Returns `Error::Numerical` when the pooled standard
/// error is exactly zero (no variance in the data).
///
/// Panics (fail-fast) if any intermediate value is NaN or non-finite.
pub fn tost_equivalence_test(
    control: &[f64],
    treatment: &[f64],
    config: &TostConfig,
) -> Result<TostResult> {
    validate_config(config)?;
    if control.len() < 2 {
        return Err(Error::Validation(
            "control group must have ≥ 2 observations".into(),
        ));
    }
    if treatment.len() < 2 {
        return Err(Error::Validation(
            "treatment group must have ≥ 2 observations".into(),
        ));
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

    let effect = mean_t - mean_c;
    assert_finite(effect, "effect (treatment − control)");

    let welch = welch_standard_error(n_c, n_t, var_c, var_t)?;
    tost_from_moments(
        effect,
        welch.se,
        welch.df,
        mean_c,
        mean_t,
        config,
    )
}

// ---------------------------------------------------------------------------
// CUPED-adjusted TOST
// ---------------------------------------------------------------------------

/// Run a TOST equivalence test on CUPED-adjusted outcomes.
///
/// Applies the pooled-θ CUPED formula `Y_adj = Y − θ (X − X̄)` to both arms
/// (identical to `cuped::cuped_adjust`), then runs the standard TOST procedure
/// on the adjusted observations.
///
/// CUPED reduces the within-arm variance, shrinking the confidence interval
/// and increasing the probability that CI ⊂ (−δ, +δ) at the same sample size —
/// so equivalence can be established with ~½ the observations when the
/// covariate explains ≥ 50 % of the outcome variance.
///
/// # Errors
/// Propagates the validation errors from `tost_equivalence_test` plus:
///   * Y / X length mismatch (`Error::Validation`).
///   * Pooled covariate variance is exactly zero (`Error::Numerical`).
pub fn tost_cuped_equivalence_test(
    control_y: &[f64],
    treatment_y: &[f64],
    control_x: &[f64],
    treatment_x: &[f64],
    config: &TostConfig,
) -> Result<TostResult> {
    validate_config(config)?;
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
            "control group must have ≥ 2 observations".into(),
        ));
    }
    if treatment_y.len() < 2 {
        return Err(Error::Validation(
            "treatment group must have ≥ 2 observations".into(),
        ));
    }

    let (adj_control, adj_treatment) =
        cuped_adjusted_samples(control_y, treatment_y, control_x, treatment_x)?;

    tost_equivalence_test(&adj_control, &adj_treatment, config)
}

// ---------------------------------------------------------------------------
// Sample size (power analysis)
// ---------------------------------------------------------------------------

/// Configuration for TOST sample-size calculation.
#[derive(Debug, Clone)]
pub struct TostPowerConfig {
    /// Equivalence margin. Must be strictly positive.
    pub delta: f64,
    /// Expected true difference under the design assumption. For infrastructure
    /// migrations this is typically 0.0. Must satisfy `|true_difference| < delta`.
    pub true_difference: f64,
    /// Estimated per-group outcome variance (assumes equal variance for design).
    pub variance: f64,
    /// Per-side significance level. Typical value 0.05.
    pub alpha: f64,
    /// Target statistical power. Typical value 0.80.
    pub power: f64,
}

impl TostPowerConfig {
    /// New config with the canonical defaults (α = 0.05, power = 0.80, Δ = 0).
    pub fn new(delta: f64, variance: f64) -> Self {
        Self {
            delta,
            true_difference: 0.0,
            variance,
            alpha: 0.05,
            power: 0.80,
        }
    }
}

/// Required per-group sample size for a TOST equivalence design.
///
/// Uses the Chow-Shao-Wang normal approximation (also adopted by R TOSTER
/// and PASS for design-stage calculations):
///
/// ```text
///   n ≈ 2 σ² (z_{1−α} + z_{1−β/2})² / (δ − |Δ|)²   when Δ = 0,
///   n ≈ 2 σ² (z_{1−α} + z_{1−β})²   / (δ − |Δ|)²   when Δ ≠ 0.
/// ```
///
/// Returns `⌈n⌉` (rounded up). This is a design-stage approximation; for
/// exact power under the noncentral t distribution, iterate `tost_equivalence_test`
/// across candidate sample sizes.
///
/// # Errors
/// Returns `Error::Validation` if the config is not well-posed:
///   * `delta ≤ 0`
///   * `variance ≤ 0`
///   * `alpha ∉ (0, 0.5)`
///   * `power ∉ (0, 1)`
///   * `|true_difference| ≥ delta` (no effect size is achievable).
pub fn tost_sample_size(config: &TostPowerConfig) -> Result<u64> {
    if config.delta <= 0.0 {
        return Err(Error::Validation(
            "equivalence margin delta must be > 0".into(),
        ));
    }
    if config.variance <= 0.0 {
        return Err(Error::Validation("variance must be > 0".into()));
    }
    if !(0.0 < config.alpha && config.alpha < 0.5) {
        return Err(Error::Validation(
            "alpha must lie in (0, 0.5)".into(),
        ));
    }
    if !(0.0 < config.power && config.power < 1.0) {
        return Err(Error::Validation("power must lie in (0, 1)".into()));
    }

    let abs_true = config.true_difference.abs();
    if abs_true >= config.delta {
        return Err(Error::Validation(format!(
            "|true_difference| ({abs_true}) must be strictly less than delta ({}) for equivalence to be achievable",
            config.delta
        )));
    }

    let z = Normal::new(0.0, 1.0)
        .map_err(|e| Error::Numerical(format!("failed to create Normal distribution: {e}")))?;
    let z_alpha = z.inverse_cdf(1.0 - config.alpha);
    assert_finite(z_alpha, "z_alpha");

    // When Δ = 0 the two one-sided tests are symmetric and each must achieve
    // power √(1 − β) under the joint to deliver overall power (1 − β). This
    // yields the z_{1−β/2} term. When Δ ≠ 0 only one side binds, so z_{1−β}.
    let beta = 1.0 - config.power;
    let z_beta = if abs_true == 0.0 {
        z.inverse_cdf(1.0 - beta / 2.0)
    } else {
        z.inverse_cdf(1.0 - beta)
    };
    assert_finite(z_beta, "z_beta");

    let margin = config.delta - abs_true;
    let n_approx = 2.0 * config.variance * (z_alpha + z_beta).powi(2) / margin.powi(2);
    assert_finite(n_approx, "n_approx");

    let n_ceil = n_approx.ceil();
    // Protect against degenerate tiny values (design must allow df ≥ 1).
    let n = if n_ceil < 2.0 { 2.0 } else { n_ceil };
    Ok(n as u64)
}

// ---------------------------------------------------------------------------
// Internals (shared with the CUPED path)
// ---------------------------------------------------------------------------

fn validate_config(config: &TostConfig) -> Result<()> {
    if !(config.delta > 0.0 && config.delta.is_finite()) {
        return Err(Error::Validation(
            "equivalence margin delta must be > 0 and finite".into(),
        ));
    }
    if !(0.0 < config.alpha && config.alpha < 0.5) {
        return Err(Error::Validation(
            "alpha must lie in (0, 0.5)".into(),
        ));
    }
    Ok(())
}

struct WelchSe {
    se: f64,
    df: f64,
}

fn welch_standard_error(n_c: f64, n_t: f64, var_c: f64, var_t: f64) -> Result<WelchSe> {
    let se = (var_c / n_c + var_t / n_t).sqrt();
    assert_finite(se, "standard error");
    if se == 0.0 {
        return Err(Error::Numerical(
            "standard error is zero (no variance in data)".into(),
        ));
    }

    let df_num = (var_c / n_c + var_t / n_t).powi(2);
    let df_den = (var_c / n_c).powi(2) / (n_c - 1.0)
        + (var_t / n_t).powi(2) / (n_t - 1.0);
    let df = df_num / df_den;
    assert_finite(df, "degrees of freedom");
    Ok(WelchSe { se, df })
}

fn tost_from_moments(
    effect: f64,
    se: f64,
    df: f64,
    mean_c: f64,
    mean_t: f64,
    config: &TostConfig,
) -> Result<TostResult> {
    let t_dist = StudentsT::new(0.0, 1.0, df)
        .map_err(|e| Error::Numerical(format!("t-distribution error: {e}")))?;

    // H₀₁: effect ≤ −δ. Upper-tail test of t_1 = (effect + δ) / se.
    let t_lower = (effect + config.delta) / se;
    assert_finite(t_lower, "t_lower");
    let p_lower = 1.0 - t_dist.cdf(t_lower);
    assert_finite(p_lower, "p_lower");

    // H₀₂: effect ≥ +δ. Lower-tail test of t_2 = (effect − δ) / se.
    let t_upper = (effect - config.delta) / se;
    assert_finite(t_upper, "t_upper");
    let p_upper = t_dist.cdf(t_upper);
    assert_finite(p_upper, "p_upper");

    let p_tost = p_lower.max(p_upper);

    let t_crit = t_dist.inverse_cdf(1.0 - config.alpha);
    assert_finite(t_crit, "t_crit");
    let half_width = t_crit * se;
    let ci_lower = effect - half_width;
    let ci_upper = effect + half_width;
    assert_finite(ci_lower, "ci_lower");
    assert_finite(ci_upper, "ci_upper");

    // Equivalence ⟺ CI ⊂ (−δ, +δ). Using strict inequalities matches the
    // strict-reject-at-α semantics of the two one-sided tests.
    let equivalent = ci_lower > -config.delta && ci_upper < config.delta;

    Ok(TostResult {
        point_estimate: effect,
        std_error: se,
        df,
        p_lower,
        p_upper,
        p_tost,
        ci_lower,
        ci_upper,
        equivalent,
        delta: config.delta,
        control_mean: mean_c,
        treatment_mean: mean_t,
    })
}

fn cuped_adjusted_samples(
    control_y: &[f64],
    treatment_y: &[f64],
    control_x: &[f64],
    treatment_x: &[f64],
) -> Result<(Vec<f64>, Vec<f64>)> {
    let all_y: Vec<f64> = control_y
        .iter()
        .chain(treatment_y.iter())
        .copied()
        .collect();
    let all_x: Vec<f64> = control_x
        .iter()
        .chain(treatment_x.iter())
        .copied()
        .collect();

    let mean_y = mean(&all_y);
    let mean_x = mean(&all_x);
    assert_finite(mean_y, "pooled mean_y");
    assert_finite(mean_x, "pooled mean_x");

    let var_x = sample_variance(&all_x, mean_x);
    assert_finite(var_x, "pooled var_x");
    if var_x == 0.0 {
        return Err(Error::Numerical(
            "covariate variance is zero — cannot compute CUPED adjustment".into(),
        ));
    }

    let cov_xy = sample_covariance(&all_y, &all_x, mean_y, mean_x);
    assert_finite(cov_xy, "pooled cov_xy");

    let theta = cov_xy / var_x;
    assert_finite(theta, "theta");

    let adjust = |y: &[f64], x: &[f64]| -> Vec<f64> {
        y.iter()
            .zip(x.iter())
            .map(|(&yi, &xi)| {
                let v = yi - theta * (xi - mean_x);
                assert_finite(v, "cuped-adjusted obs");
                v
            })
            .collect()
    };

    Ok((adjust(control_y, control_x), adjust(treatment_y, treatment_x)))
}

fn mean(data: &[f64]) -> f64 {
    data.iter().sum::<f64>() / data.len() as f64
}

fn sample_variance(data: &[f64], mean: f64) -> f64 {
    let n = data.len() as f64;
    let ss: f64 = data.iter().map(|&x| (x - mean).powi(2)).sum();
    ss / (n - 1.0)
}

fn sample_covariance(y: &[f64], x: &[f64], mean_y: f64, mean_x: f64) -> f64 {
    let n = y.len() as f64;
    let ss: f64 = y
        .iter()
        .zip(x.iter())
        .map(|(&yi, &xi)| (yi - mean_y) * (xi - mean_x))
        .sum();
    ss / (n - 1.0)
}

// ---------------------------------------------------------------------------
// Unit + property tests (cheap — heavier golden tests live in tests/)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // --- basic behaviour ---------------------------------------------------

    #[test]
    fn identical_samples_equivalent_for_positive_delta() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let cfg = TostConfig::new(2.0);
        let r = tost_equivalence_test(&x, &x, &cfg).unwrap();
        assert_eq!(r.point_estimate, 0.0);
        assert!(r.equivalent, "identical samples must be equivalent at δ=2");
        assert!(r.p_tost < cfg.alpha);
        // CI strictly inside (−δ, +δ).
        assert!(r.ci_lower > -cfg.delta && r.ci_upper < cfg.delta);
    }

    #[test]
    fn huge_effect_not_equivalent() {
        let control = vec![0.0; 40];
        let treatment = vec![10.0; 40];
        // With zero variance, the implementation errors out (see below).
        // Add tiny noise so SE is finite and the effect dominates δ.
        let mut control = control;
        let mut treatment = treatment;
        control[0] += 1e-6;
        treatment[0] += 1e-6;
        let cfg = TostConfig::new(1.0);
        let r = tost_equivalence_test(&control, &treatment, &cfg).unwrap();
        assert!(!r.equivalent, "|effect| ≫ δ must not be equivalent");
        assert!(r.p_tost > cfg.alpha);
    }

    #[test]
    fn zero_variance_returns_numerical_error() {
        let x = vec![1.0, 1.0, 1.0, 1.0];
        let cfg = TostConfig::new(0.5);
        let err = tost_equivalence_test(&x, &x, &cfg).unwrap_err();
        matches!(err, Error::Numerical(_));
    }

    #[test]
    fn invalid_config_rejected() {
        let x = vec![1.0, 2.0];
        let y = vec![1.5, 2.5];
        assert!(tost_equivalence_test(&x, &y, &TostConfig { delta: 0.0, alpha: 0.05 }).is_err());
        assert!(tost_equivalence_test(&x, &y, &TostConfig { delta: -1.0, alpha: 0.05 }).is_err());
        assert!(tost_equivalence_test(&x, &y, &TostConfig { delta: 1.0, alpha: 0.0 }).is_err());
        assert!(tost_equivalence_test(&x, &y, &TostConfig { delta: 1.0, alpha: 0.5 }).is_err());
    }

    #[test]
    fn ci_equivalence_decision_agrees_with_p_value() {
        let control: Vec<f64> = (0..50).map(|i| i as f64 * 0.1).collect();
        let treatment: Vec<f64> = (0..50).map(|i| i as f64 * 0.1 + 0.02).collect();
        let cfg = TostConfig::new(0.5);
        let r = tost_equivalence_test(&control, &treatment, &cfg).unwrap();
        let ci_inside = r.ci_lower > -cfg.delta && r.ci_upper < cfg.delta;
        assert_eq!(r.equivalent, ci_inside, "CI-based and p-value decisions must agree");
        assert_eq!(r.equivalent, r.p_tost < cfg.alpha);
    }

    // --- CUPED composition -------------------------------------------------

    #[test]
    fn cuped_tost_reduces_se_with_correlated_covariate() {
        let control_x: Vec<f64> = (0..200).map(|i| i as f64).collect();
        let control_y: Vec<f64> = control_x.iter().map(|&x| 2.0 * x + 1.0).collect();
        let treatment_x: Vec<f64> = (0..200).map(|i| i as f64).collect();
        let treatment_y: Vec<f64> = treatment_x.iter().map(|&x| 2.0 * x + 1.05).collect();

        let cfg = TostConfig::new(1.0);
        let raw = tost_equivalence_test(&control_y, &treatment_y, &cfg).unwrap();
        let cuped =
            tost_cuped_equivalence_test(&control_y, &treatment_y, &control_x, &treatment_x, &cfg)
                .unwrap();

        assert!(
            cuped.std_error < raw.std_error,
            "CUPED SE {} should be < raw SE {}",
            cuped.std_error,
            raw.std_error
        );
        assert!(cuped.equivalent, "CUPED-adjusted experiment should establish equivalence");
    }

    #[test]
    fn cuped_tost_validates_lengths() {
        let cfg = TostConfig::new(1.0);
        assert!(tost_cuped_equivalence_test(
            &[1.0, 2.0],
            &[3.0, 4.0],
            &[1.0],
            &[3.0, 4.0],
            &cfg,
        )
        .is_err());
        assert!(tost_cuped_equivalence_test(
            &[1.0, 2.0],
            &[3.0, 4.0],
            &[1.0, 2.0],
            &[3.0],
            &cfg,
        )
        .is_err());
    }

    // --- sample-size calculation ------------------------------------------

    #[test]
    fn sample_size_matches_canonical_example() {
        // Phillips (1990) / Chow-Shao-Wang §4.3 canonical reference point:
        // σ² = 1, δ = 1, Δ = 0, α = 0.05, power = 0.80
        //   z_{0.95} ≈ 1.6449, z_{0.90} ≈ 1.2816
        //   n ≈ 2 · 1 · (1.6449 + 1.2816)² / 1² ≈ 17.13 → 18
        let n = tost_sample_size(&TostPowerConfig::new(1.0, 1.0)).unwrap();
        assert_eq!(n, 18);
    }

    #[test]
    fn sample_size_scales_inverse_square_delta() {
        let n_small = tost_sample_size(&TostPowerConfig::new(0.5, 1.0)).unwrap();
        let n_large = tost_sample_size(&TostPowerConfig::new(1.0, 1.0)).unwrap();
        // Halving δ should roughly 4× the sample size. Ceiling on both ends
        // loosens the exact 4× ratio — allow ±5% slack on the expected value.
        let expected = 4.0 * n_large as f64;
        let actual = n_small as f64;
        let ratio = actual / expected;
        assert!(
            (0.95..=1.05).contains(&ratio),
            "n(δ=0.5)={n_small} should be ~4×n(δ=1)={n_large}, ratio={ratio:.3}"
        );
    }

    #[test]
    fn sample_size_rejects_unreachable_configs() {
        let bad = TostPowerConfig {
            delta: 0.1,
            true_difference: 0.2, // |Δ| > δ → no achievable effect
            variance: 1.0,
            alpha: 0.05,
            power: 0.80,
        };
        assert!(tost_sample_size(&bad).is_err());
        let zero_delta = TostPowerConfig { delta: 0.0, ..TostPowerConfig::new(1.0, 1.0) };
        assert!(tost_sample_size(&zero_delta).is_err());
    }

    // --- proptest invariants ----------------------------------------------

    proptest! {
        /// ADR-027 §7 invariant: p_tost ≥ max(p_lower, p_upper) by definition.
        #[test]
        fn prop_p_tost_is_max_of_one_sided(
            effect in -2.0f64..2.0f64,
            sigma in 0.1f64..3.0f64,
            n in 5usize..80usize,
            delta in 0.1f64..3.0f64,
        ) {
            let control: Vec<f64> = (0..n).map(|i| (i as f64) * 0.01).collect();
            let treatment: Vec<f64> = (0..n).map(|i| (i as f64) * 0.01 + effect + sigma * 1e-6 * (i as f64)).collect();
            let cfg = TostConfig::new(delta);
            if let Ok(r) = tost_equivalence_test(&control, &treatment, &cfg) {
                prop_assert!(r.p_tost >= r.p_lower - 1e-12);
                prop_assert!(r.p_tost >= r.p_upper - 1e-12);
            }
        }

        /// ADR-027 §7 invariant: equivalent==true ⟺ (1−2α) CI ⊂ (−δ, +δ).
        #[test]
        fn prop_ci_matches_equivalence(
            n in 10usize..60usize,
            shift in -1.5f64..1.5f64,
            delta in 0.2f64..2.0f64,
        ) {
            let control: Vec<f64> = (0..n).map(|i| 0.1 * i as f64).collect();
            let treatment: Vec<f64> = (0..n).map(|i| 0.1 * i as f64 + shift).collect();
            let cfg = TostConfig::new(delta);
            if let Ok(r) = tost_equivalence_test(&control, &treatment, &cfg) {
                let ci_inside = r.ci_lower > -cfg.delta && r.ci_upper < cfg.delta;
                prop_assert_eq!(r.equivalent, ci_inside);
            }
        }

        /// ADR-027 §7 invariant: as δ → ∞ equivalence is always declared
        /// (the margin swallows any finite CI).
        #[test]
        fn prop_large_delta_always_equivalent(
            n in 5usize..40usize,
            shift in -3.0f64..3.0f64,
        ) {
            let control: Vec<f64> = (0..n).map(|i| 0.1 * i as f64).collect();
            let treatment: Vec<f64> = (0..n).map(|i| 0.1 * i as f64 + shift).collect();
            let cfg = TostConfig::new(1e9);
            if let Ok(r) = tost_equivalence_test(&control, &treatment, &cfg) {
                prop_assert!(r.equivalent, "δ=1e9 must swallow any finite effect");
            }
        }

        /// All one-sided p-values live in [0, 1].
        #[test]
        fn prop_p_values_in_unit_interval(
            n in 5usize..50usize,
            shift in -2.0f64..2.0f64,
            delta in 0.05f64..4.0f64,
        ) {
            let control: Vec<f64> = (0..n).map(|i| 0.1 * i as f64).collect();
            let treatment: Vec<f64> = (0..n).map(|i| 0.1 * i as f64 + shift).collect();
            let cfg = TostConfig::new(delta);
            if let Ok(r) = tost_equivalence_test(&control, &treatment, &cfg) {
                prop_assert!((0.0..=1.0).contains(&r.p_lower));
                prop_assert!((0.0..=1.0).contains(&r.p_upper));
                prop_assert!((0.0..=1.0).contains(&r.p_tost));
            }
        }

        /// ADR-027 §7 invariant: sample size is monotone nonincreasing in δ.
        #[test]
        fn prop_sample_size_monotone_in_delta(
            delta_small in 0.1f64..1.0f64,
            delta_large in 1.0f64..5.0f64,
            sigma_sq in 0.1f64..4.0f64,
        ) {
            let cfg_small = TostPowerConfig::new(delta_small, sigma_sq);
            let cfg_large = TostPowerConfig::new(delta_large, sigma_sq);
            let n_small = tost_sample_size(&cfg_small).unwrap();
            let n_large = tost_sample_size(&cfg_large).unwrap();
            prop_assert!(n_small >= n_large, "n({delta_small}) < n({delta_large})");
        }
    }
}
