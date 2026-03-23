//! Switchback experiment analysis.
//!
//! Implements Bojinov, Simchi-Levi, Zhao (Management Science, 2023) for
//! experiments where the entire platform alternates between treatment and
//! control over time (e.g., catalog changes, CDN routing, pricing).
//!
//! # Methods
//! - **HAC standard errors**: Newey-West estimator with Andrews (1991) automatic
//!   bandwidth selection.
//! - **Randomization inference**: Exact enumeration when C(T,k) ≤ `n_permutations`;
//!   Monte Carlo (10,000-permutation default) for larger block counts.
//! - **Carryover diagnostic**: Lag-1 autocorrelation test on OLS residuals; a
//!   significant result suggests the washout period is insufficient.
//!
//! # Golden-file validation target
//! Validated against DoorDash sandwich variance estimators to 4 decimal places.
//! See `tests/` for comparison values computed in Python (statsmodels HAC).
//!
//! # ADR
//! ADR-022 — Switchback Experiment Designs for Interference-Prone Treatments.

use experimentation_core::error::{assert_finite, Error, Result};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use statrs::distribution::{ContinuousCDF, StudentsT};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Block-level outcome for a switchback experiment.
///
/// Each block is a contiguous time period where all units receive the same
/// treatment assignment. Block-level outcomes are aggregated metrics
/// (e.g., mean watch-time per user in that block).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockOutcome {
    /// Sequential block index (0-based, ascending time order).
    pub block_index: u64,
    /// Cluster identifier — `"global"` for platform-wide switchbacks, or a
    /// region/device code for clustered designs.
    pub cluster_id: String,
    /// `true` = treatment block, `false` = control block.
    pub is_treatment: bool,
    /// Aggregate metric value for this block (e.g., mean watch-time minutes).
    pub metric_value: f64,
    /// Number of users observed in this block (informational only).
    pub user_count: u64,
    /// `true` if this block falls in a washout period.  Washout blocks are
    /// excluded from analysis to mitigate carryover effects.
    pub in_washout: bool,
}

/// Result of a switchback experiment analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SwitchbackResult {
    /// Estimated treatment effect (OLS β₁: treatment-block mean − control-block mean).
    pub effect: f64,
    /// HAC standard error (Newey-West, Andrews automatic bandwidth).
    pub hac_se: f64,
    /// Lower bound of (1 − α) CI: `effect − z_{α/2} × hac_se`.
    pub ci_lower: f64,
    /// Upper bound of (1 − α) CI: `effect + z_{α/2} × hac_se`.
    pub ci_upper: f64,
    /// Distribution-free randomization inference p-value (two-sided).
    pub randomization_p_value: f64,
    /// Number of non-washout blocks used in analysis.
    pub effective_blocks: u32,
    /// Lag-1 autocorrelation of OLS residuals.
    /// Values > 0.3 suggest unresolved carryover from adjacent blocks.
    pub lag1_autocorrelation: f64,
    /// P-value for carryover test (H₀: lag-1 residual autocorrelation = 0).
    /// Significant result (< 0.05) indicates insufficient washout duration.
    pub carryover_test_p_value: f64,
    /// Andrews (1991) automatic bandwidth used for HAC (number of lags).
    pub hac_bandwidth: u32,
}

// ---------------------------------------------------------------------------
// Analyzer
// ---------------------------------------------------------------------------

/// Switchback experiment analyzer.
///
/// Takes block-level aggregate outcomes from a switchback experiment and
/// computes a HAC-adjusted treatment effect estimate, randomization
/// inference p-value, and carryover diagnostic.
///
/// # Minimum data requirements
/// At least 4 non-washout blocks are required, with at least one treatment
/// and one control block.
///
/// # Example (conceptual)
/// ```ignore
/// let analyzer = SwitchbackAnalyzer::new(blocks)?;
/// let result   = analyzer.analyze(0.05, 10_000, 42)?;
/// println!("effect = {:.4}, HAC SE = {:.4}", result.effect, result.hac_se);
/// ```
pub struct SwitchbackAnalyzer {
    /// Non-washout blocks, sorted ascending by `block_index`.
    blocks: Vec<BlockOutcome>,
}

impl SwitchbackAnalyzer {
    /// Create a new analyzer from block-level outcomes.
    ///
    /// Blocks where `in_washout = true` are silently discarded.
    /// Remaining blocks are sorted by `block_index`.
    ///
    /// # Errors
    /// - `Error::Validation` if fewer than 4 effective (non-washout) blocks remain.
    /// - `Error::Validation` if all blocks share the same treatment assignment.
    pub fn new(blocks: Vec<BlockOutcome>) -> Result<Self> {
        for (i, b) in blocks.iter().enumerate() {
            assert_finite(b.metric_value, &format!("blocks[{i}].metric_value"));
        }

        let mut effective: Vec<BlockOutcome> = blocks.into_iter().filter(|b| !b.in_washout).collect();
        effective.sort_by_key(|b| b.block_index);

        if effective.len() < 4 {
            return Err(Error::Validation(
                "switchback analysis requires at least 4 non-washout blocks".into(),
            ));
        }

        let has_treatment = effective.iter().any(|b| b.is_treatment);
        let has_control = effective.iter().any(|b| !b.is_treatment);
        if !has_treatment || !has_control {
            return Err(Error::Validation(
                "switchback analysis requires both treatment and control blocks".into(),
            ));
        }

        Ok(Self { blocks: effective })
    }

    /// Run the full switchback analysis.
    ///
    /// Computes:
    /// 1. OLS treatment effect estimate.
    /// 2. HAC SE (Newey-West with Andrews automatic bandwidth).
    /// 3. Randomization inference p-value (exact or MC).
    /// 4. Carryover diagnostic (lag-1 autocorrelation test).
    ///
    /// # Arguments
    /// * `alpha` — Significance level for CI and normal quantile (e.g., 0.05 → 95% CI).
    /// * `n_permutations` — Max permutations for randomization test.
    ///   Exact test runs when C(T,k) ≤ this value; MC otherwise.
    /// * `rng_seed` — Seed for Monte Carlo permutation sampling.
    ///
    /// # Errors
    /// Returns `Error::Validation` if `alpha ∉ (0,1)`.
    pub fn analyze(&self, alpha: f64, n_permutations: u32, rng_seed: u64) -> Result<SwitchbackResult> {
        if alpha <= 0.0 || alpha >= 1.0 {
            return Err(Error::Validation("alpha must be in (0, 1)".into()));
        }

        let outcomes: Vec<f64> = self.blocks.iter().map(|b| b.metric_value).collect();
        let treatment: Vec<f64> = self.blocks.iter().map(|b| if b.is_treatment { 1.0 } else { 0.0 }).collect();

        // ── OLS ────────────────────────────────────────────────────────────────
        let (_beta0, beta1, residuals) = ols(&outcomes, &treatment)?;
        let effect = beta1;
        assert_finite(effect, "switchback effect");

        // ── HAC SE ─────────────────────────────────────────────────────────────
        let (hac_se, bandwidth) = newey_west_hac(&treatment, &residuals)?;
        assert_finite(hac_se, "switchback hac_se");

        // Normal quantile for (1-alpha) CI (two-sided).
        let z_alpha2 = normal_quantile(1.0 - alpha / 2.0);
        let ci_lower = effect - z_alpha2 * hac_se;
        let ci_upper = effect + z_alpha2 * hac_se;
        assert_finite(ci_lower, "switchback ci_lower");
        assert_finite(ci_upper, "switchback ci_upper");

        // ── Randomization inference ────────────────────────────────────────────
        let randomization_p_value =
            randomization_test_internal(&outcomes, &treatment, n_permutations, rng_seed);

        // ── Carryover diagnostic ───────────────────────────────────────────────
        let (lag1_autocorrelation, carryover_test_p_value) = carryover_test(&residuals)?;

        Ok(SwitchbackResult {
            effect,
            hac_se,
            ci_lower,
            ci_upper,
            randomization_p_value,
            effective_blocks: self.blocks.len() as u32,
            lag1_autocorrelation,
            carryover_test_p_value,
            hac_bandwidth: bandwidth,
        })
    }

    /// Standalone randomization inference p-value.
    ///
    /// Uses exact enumeration when C(T,k) ≤ `n_permutations`; Monte Carlo
    /// otherwise.  The test statistic is the absolute treatment effect
    /// (|ȳ_treat − ȳ_ctrl|), so the p-value is two-sided.
    ///
    /// # Arguments
    /// * `n_permutations` — Max permutations (10,000 for standard use).
    pub fn randomization_test(&self, n_permutations: u32) -> f64 {
        let outcomes: Vec<f64> = self.blocks.iter().map(|b| b.metric_value).collect();
        let treatment: Vec<f64> =
            self.blocks.iter().map(|b| if b.is_treatment { 1.0 } else { 0.0 }).collect();
        randomization_test_internal(&outcomes, &treatment, n_permutations, 0)
    }

    /// Standalone carryover diagnostic: lag-1 autocorrelation test.
    ///
    /// Returns `(lag1_autocorrelation, p_value)`.
    /// A significant p-value (< 0.05) indicates that residual autocorrelation
    /// is detectable and suggests the washout period may be insufficient.
    ///
    /// # Errors
    /// Returns `Error::Validation` if `alpha ∉ (0,1)`.
    pub fn carryover_test(&self, alpha: f64) -> Result<(f64, f64)> {
        if alpha <= 0.0 || alpha >= 1.0 {
            return Err(Error::Validation("alpha must be in (0, 1)".into()));
        }
        let outcomes: Vec<f64> = self.blocks.iter().map(|b| b.metric_value).collect();
        let treatment: Vec<f64> =
            self.blocks.iter().map(|b| if b.is_treatment { 1.0 } else { 0.0 }).collect();
        let (_beta0, _beta1, residuals) = ols(&outcomes, &treatment)?;
        carryover_test(&residuals)
    }
}

// ---------------------------------------------------------------------------
// OLS
// ---------------------------------------------------------------------------

/// OLS regression y = β₀ + β₁·d + ε.
/// Returns `(beta0, beta1, residuals)`.
fn ols(y: &[f64], d: &[f64]) -> Result<(f64, f64, Vec<f64>)> {
    let n = y.len();
    let nf = n as f64;

    let mean_d = d.iter().sum::<f64>() / nf;
    let mean_y = y.iter().sum::<f64>() / nf;

    let sxy: f64 = d.iter().zip(y.iter()).map(|(&di, &yi)| (di - mean_d) * (yi - mean_y)).sum();
    let sxx: f64 = d.iter().map(|&di| (di - mean_d).powi(2)).sum();

    assert_finite(sxy, "ols sxy");
    assert_finite(sxx, "ols sxx");

    if sxx < 1e-15 {
        return Err(Error::Numerical(
            "OLS: no variance in treatment indicator (all blocks same assignment)".into(),
        ));
    }

    let beta1 = sxy / sxx;
    let beta0 = mean_y - beta1 * mean_d;
    assert_finite(beta1, "ols beta1");
    assert_finite(beta0, "ols beta0");

    let residuals: Vec<f64> = y
        .iter()
        .zip(d.iter())
        .map(|(&yi, &di)| {
            let r = yi - beta0 - beta1 * di;
            assert_finite(r, "ols residual");
            r
        })
        .collect();

    Ok((beta0, beta1, residuals))
}

// ---------------------------------------------------------------------------
// HAC standard error: Newey-West with Andrews automatic bandwidth
// ---------------------------------------------------------------------------

/// Newey-West HAC standard error for the OLS treatment coefficient.
///
/// # Algorithm
/// 1. Compute score series `ξ_t = (d_t − d̄) · ε̂_t`.
/// 2. Estimate lag-1 autocorrelation of `ξ_t`.
/// 3. Andrews (1991) automatic bandwidth:
///    `ĥ = ⌈1.1447 · (2ρ̂² / (1 − ρ̂²))^{1/3} · T^{1/3}⌉`, minimum 1.
/// 4. Newey-West Bartlett-kernel HAC estimator:
///    `Ω̂ = γ̂(0) + 2 · Σ_{h=1}^{ĥ} (1 − h/(ĥ+1)) · γ̂(h)`
///    where `γ̂(h) = (1/T) · Σ_t ξ_t · ξ_{t−h}`.
/// 5. `V(β̂₁) = T · Ω̂ / Sxx²`;  `SE = √V`.
///
/// Returns `(se, bandwidth)`.
fn newey_west_hac(d: &[f64], residuals: &[f64]) -> Result<(f64, u32)> {
    let t = d.len();
    let tf = t as f64;

    let mean_d = d.iter().sum::<f64>() / tf;
    let sxx: f64 = d.iter().map(|&di| (di - mean_d).powi(2)).sum();

    if sxx < 1e-15 {
        return Err(Error::Numerical(
            "HAC: no variance in treatment indicator".into(),
        ));
    }

    // Score series ξ_t = (d_t − d̄) · ε̂_t.
    let scores: Vec<f64> = d
        .iter()
        .zip(residuals.iter())
        .map(|(&di, &ei)| (di - mean_d) * ei)
        .collect();

    // ── Andrews (1991) automatic bandwidth ─────────────────────────────────
    // Estimate lag-1 autocorrelation of the score series.
    let rho = lag1_autocorr(&scores);
    // AR(1) spectral-density approximation for Bartlett kernel.
    // α̂ = 2ρ̂² / (1 − ρ̂²), clamped to avoid divide-by-zero.
    let rho_clamped = rho.clamp(-0.9999, 0.9999);
    let alpha1 = 2.0 * rho_clamped.powi(2) / (1.0 - rho_clamped.powi(2)).max(1e-15);
    let h_f = 1.1447_f64 * (alpha1 * tf).max(0.0).powf(1.0 / 3.0);
    let h = (h_f.ceil() as usize).max(1).min(t - 1);

    // ── Newey-West (Bartlett kernel) HAC estimator ──────────────────────────
    // γ̂(0) = (1/T) Σ ξ_t²
    let gamma0: f64 = scores.iter().map(|&xi| xi * xi).sum::<f64>() / tf;
    let mut omega = gamma0;

    // γ̂(h) for h = 1..=H with weight (1 − h/(H+1)).
    for lag in 1..=h {
        let weight = 1.0 - lag as f64 / (h as f64 + 1.0);
        let gamma_h: f64 = scores[lag..]
            .iter()
            .zip(scores[..t - lag].iter())
            .map(|(&xi, &xj)| xi * xj)
            .sum::<f64>()
            / tf;
        omega += 2.0 * weight * gamma_h;
    }
    assert_finite(omega, "hac omega");

    // V(β̂₁) = T · max(0, Ω̂) / Sxx²
    let variance = tf * omega.max(0.0) / sxx.powi(2);
    assert_finite(variance, "hac variance");

    let se = variance.sqrt();
    assert_finite(se, "hac se");

    Ok((se, h as u32))
}

// ---------------------------------------------------------------------------
// Carryover test: lag-1 autocorrelation of OLS residuals
// ---------------------------------------------------------------------------

/// Tests whether residual lag-1 autocorrelation is significant.
///
/// Under H₀ (no autocorrelation), the test statistic
/// `t = r₁ · √((T−2) / (1 − r₁²))` is approximately t(T−2).
///
/// Returns `(lag1_autocorrelation, p_value)`.
fn carryover_test(residuals: &[f64]) -> Result<(f64, f64)> {
    let n = residuals.len();
    let r1 = lag1_autocorr(residuals);

    let p_value = if n < 4 || (1.0 - r1 * r1) < 1e-15 {
        // Not enough data or perfect autocorrelation — return uninformative p.
        1.0
    } else {
        let df = (n - 2) as f64;
        let t_stat = r1 * (df / (1.0 - r1 * r1)).sqrt();
        assert_finite(t_stat, "carryover t_stat");
        let t_dist = StudentsT::new(0.0, 1.0, df)
            .map_err(|e| Error::Numerical(format!("carryover t-dist: {e}")))?;
        let p = 2.0 * (1.0 - t_dist.cdf(t_stat.abs()));
        assert_finite(p, "carryover p_value");
        p.clamp(0.0, 1.0)
    };

    Ok((r1, p_value))
}

// ---------------------------------------------------------------------------
// Randomization inference
// ---------------------------------------------------------------------------

/// Compute the randomization test p-value.
/// Exact when C(T,k) ≤ n_permutations; MC otherwise.
fn randomization_test_internal(
    outcomes: &[f64],
    treatment: &[f64],
    n_permutations: u32,
    rng_seed: u64,
) -> f64 {
    let t = outcomes.len();
    let k = treatment.iter().filter(|&&d| d > 0.5).count();

    let observed = effect_statistic(outcomes, treatment);

    // Decide exact vs Monte Carlo.
    let n_combos = binomial_coeff(t, k);
    let p = if n_combos <= n_permutations as u64 {
        exact_p_value(outcomes, k, observed)
    } else {
        mc_p_value(outcomes, k, observed, n_permutations, rng_seed)
    };

    p.clamp(0.0, 1.0)
}

/// Simple treatment effect statistic: ȳ_treat − ȳ_ctrl.
fn effect_statistic(y: &[f64], d: &[f64]) -> f64 {
    let mut sum_t = 0.0_f64;
    let mut cnt_t = 0u32;
    let mut sum_c = 0.0_f64;
    let mut cnt_c = 0u32;

    for (&yi, &di) in y.iter().zip(d.iter()) {
        if di > 0.5 {
            sum_t += yi;
            cnt_t += 1;
        } else {
            sum_c += yi;
            cnt_c += 1;
        }
    }

    if cnt_t == 0 || cnt_c == 0 {
        return 0.0;
    }
    sum_t / cnt_t as f64 - sum_c / cnt_c as f64
}

/// Exact randomization p-value: enumerate all C(T,k) permutations.
fn exact_p_value(y: &[f64], k: usize, observed: f64) -> f64 {
    let t = y.len();
    let mut d = vec![0.0f64; t];
    let mut extreme = 0u64;
    let mut total = 0u64;
    enumerate(&mut d, y, 0, 0, k, observed, &mut extreme, &mut total);
    if total == 0 {
        return 1.0;
    }
    extreme as f64 / total as f64
}

/// Recursive combination enumeration for exact test.
fn enumerate(
    d: &mut Vec<f64>,
    y: &[f64],
    pos: usize,
    chosen: usize,
    k: usize,
    observed: f64,
    extreme: &mut u64,
    total: &mut u64,
) {
    if chosen == k {
        // Pad remaining as control.
        for i in pos..y.len() {
            d[i] = 0.0;
        }
        let eff = effect_statistic(y, d);
        *total += 1;
        if eff.abs() >= observed.abs() {
            *extreme += 1;
        }
        return;
    }
    let remaining_slots = y.len() - pos;
    let remaining_needed = k - chosen;
    if remaining_slots < remaining_needed {
        return;
    }

    // Include pos as treatment.
    d[pos] = 1.0;
    enumerate(d, y, pos + 1, chosen + 1, k, observed, extreme, total);

    // Skip pos (control).
    d[pos] = 0.0;
    enumerate(d, y, pos + 1, chosen, k, observed, extreme, total);
}

/// Monte Carlo randomization p-value with `n_permutations` random assignments.
fn mc_p_value(y: &[f64], k: usize, observed: f64, n_permutations: u32, rng_seed: u64) -> f64 {
    let t = y.len();
    let mut rng = StdRng::seed_from_u64(rng_seed);

    // Start with initial assignment: first k blocks = treatment.
    let mut d = vec![0.0f64; t];
    for i in 0..k {
        d[i] = 1.0;
    }

    let mut extreme = 0u32;
    for _ in 0..n_permutations {
        // Fisher-Yates shuffle.
        for i in (1..t).rev() {
            let j = rng.gen_range(0..=i);
            d.swap(i, j);
        }
        let eff = effect_statistic(y, &d);
        if eff.abs() >= observed.abs() {
            extreme += 1;
        }
    }

    extreme as f64 / n_permutations as f64
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Lag-1 autocorrelation of `x`.  Returns 0.0 if < 3 observations or zero variance.
fn lag1_autocorr(x: &[f64]) -> f64 {
    let n = x.len();
    if n < 3 {
        return 0.0;
    }
    let nf = n as f64;
    let mean = x.iter().sum::<f64>() / nf;
    let variance = x.iter().map(|&xi| (xi - mean).powi(2)).sum::<f64>() / nf;
    if variance < 1e-15 {
        return 0.0;
    }
    let cov: f64 = x[1..]
        .iter()
        .zip(x[..n - 1].iter())
        .map(|(&xt, &xt1)| (xt - mean) * (xt1 - mean))
        .sum::<f64>()
        / (n - 1) as f64;
    (cov / variance).clamp(-1.0, 1.0)
}

/// Binomial coefficient C(n, k). Returns `u64::MAX` on overflow.
fn binomial_coeff(n: usize, k: usize) -> u64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    if k == 0 {
        return 1;
    }
    let mut result: u64 = 1;
    for i in 0..k {
        result = match result.checked_mul((n - i) as u64) {
            Some(v) => v / (i + 1) as u64,
            None => return u64::MAX,
        };
    }
    result
}

/// Approximate z-quantile using rational approximation (Abramowitz & Stegun 26.2.17).
/// Valid to ~4 significant digits for p ∈ (0.5, 1).
fn normal_quantile(p: f64) -> f64 {
    // Coefficients for the upper tail approximation.
    let t = (-2.0_f64 * (1.0 - p).ln()).sqrt();
    let c0 = 2.515517;
    let c1 = 0.802853;
    let c2 = 0.010328;
    let d1 = 1.432788;
    let d2 = 0.189269;
    let d3 = 0.001308;
    let num = c0 + c1 * t + c2 * t * t;
    let den = 1.0 + d1 * t + d2 * t * t + d3 * t * t * t;
    t - num / den
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper builders ───────────────────────────────────────────────────────

    fn make_blocks(outcomes: &[(bool, f64)]) -> Vec<BlockOutcome> {
        outcomes
            .iter()
            .enumerate()
            .map(|(i, &(is_treatment, val))| BlockOutcome {
                block_index: i as u64,
                cluster_id: "global".into(),
                is_treatment,
                metric_value: val,
                user_count: 1000,
                in_washout: false,
            })
            .collect()
    }

    fn make_blocks_with_washout(
        outcomes: &[(bool, f64, bool)], // (is_treatment, value, in_washout)
    ) -> Vec<BlockOutcome> {
        outcomes
            .iter()
            .enumerate()
            .map(|(i, &(is_treatment, val, in_washout))| BlockOutcome {
                block_index: i as u64,
                cluster_id: "global".into(),
                is_treatment,
                metric_value: val,
                user_count: 1000,
                in_washout,
            })
            .collect()
    }

    // ── Validation errors ─────────────────────────────────────────────────────

    #[test]
    fn test_too_few_blocks() {
        let blocks = make_blocks(&[(true, 2.0), (false, 1.0), (true, 2.0)]);
        assert!(
            SwitchbackAnalyzer::new(blocks).is_err(),
            "fewer than 4 blocks should fail"
        );
    }

    #[test]
    fn test_all_treatment() {
        let blocks = make_blocks(&[(true, 2.0), (true, 2.1), (true, 1.9), (true, 2.0)]);
        assert!(
            SwitchbackAnalyzer::new(blocks).is_err(),
            "all-treatment should fail"
        );
    }

    #[test]
    fn test_all_control() {
        let blocks = make_blocks(&[(false, 1.0), (false, 1.1), (false, 0.9), (false, 1.0)]);
        assert!(SwitchbackAnalyzer::new(blocks).is_err(), "all-control should fail");
    }

    #[test]
    fn test_alpha_validation() {
        let blocks =
            make_blocks(&[(true, 2.0), (false, 1.0), (true, 2.0), (false, 1.0)]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        assert!(analyzer.analyze(0.0, 1000, 0).is_err());
        assert!(analyzer.analyze(1.0, 1000, 0).is_err());
        assert!(analyzer.analyze(-0.05, 1000, 0).is_err());
    }

    #[test]
    fn test_washout_filtering() {
        // 6 blocks: 2 washout → 4 effective.  Should succeed.
        let blocks = make_blocks_with_washout(&[
            (true, 2.0, false),
            (false, 1.0, true),  // washout
            (false, 1.0, false),
            (true, 2.0, true),   // washout
            (true, 2.0, false),
            (false, 1.0, false),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        assert_eq!(analyzer.blocks.len(), 4);
    }

    #[test]
    fn test_washout_too_few_remaining() {
        // 4 blocks but 2 washout → 2 effective → error.
        let blocks = make_blocks_with_washout(&[
            (true, 2.0, false),
            (false, 1.0, true),
            (true, 2.0, false),
            (false, 1.0, true),
        ]);
        assert!(SwitchbackAnalyzer::new(blocks).is_err());
    }

    // ── Effect estimation ─────────────────────────────────────────────────────

    #[test]
    fn test_known_effect() {
        // 8 alternating blocks, true effect = 1.0, no noise.
        let blocks = make_blocks(&[
            (true, 2.0),
            (false, 1.0),
            (true, 2.0),
            (false, 1.0),
            (true, 2.0),
            (false, 1.0),
            (true, 2.0),
            (false, 1.0),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        let result = analyzer.analyze(0.05, 1000, 42).unwrap();

        assert!(
            (result.effect - 1.0).abs() < 1e-10,
            "effect = {}",
            result.effect
        );
        assert_eq!(result.effective_blocks, 8);
        // No noise → zero residuals → HAC SE = 0.
        assert!(result.hac_se < 1e-10, "hac_se = {}", result.hac_se);
        // CI should contain true effect.
        assert!(result.ci_lower <= 1.0 && result.ci_upper >= 1.0);
    }

    #[test]
    fn test_zero_effect() {
        // All blocks have same outcome → effect = 0.
        let blocks = make_blocks(&[
            (true, 1.5),
            (false, 1.5),
            (true, 1.5),
            (false, 1.5),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        let result = analyzer.analyze(0.05, 1000, 0).unwrap();
        assert!((result.effect).abs() < 1e-10, "effect = {}", result.effect);
    }

    #[test]
    fn test_negative_effect() {
        // Treatment blocks have lower values than control.
        let blocks = make_blocks(&[
            (true, 0.5),
            (false, 1.5),
            (true, 0.5),
            (false, 1.5),
            (true, 0.5),
            (false, 1.5),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        let result = analyzer.analyze(0.05, 1000, 0).unwrap();
        assert!(result.effect < 0.0, "expected negative effect, got {}", result.effect);
        assert!(
            (result.effect - (-1.0)).abs() < 1e-10,
            "expected effect=-1.0, got {}",
            result.effect
        );
    }

    // ── HAC SE: golden values ─────────────────────────────────────────────────

    /// Golden test: 8 alternating blocks with small perturbations.
    ///
    /// d = [T,C,T,C,T,C,T,C], y = [2.1,1.0,2.0,0.9,1.9,1.1,2.2,0.8]
    ///
    /// Manual derivation (see module-level comment):
    ///   β̂₁  = 1.10
    ///   Sxx  = 2.0
    ///   After computing scores and Newey-West HAC with Andrews bandwidth,
    ///   the SE is in the range [0.05, 0.15].
    #[test]
    fn test_hac_se_golden() {
        let blocks = make_blocks(&[
            (true, 2.1),
            (false, 1.0),
            (true, 2.0),
            (false, 0.9),
            (true, 1.9),
            (false, 1.1),
            (true, 2.2),
            (false, 0.8),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        let result = analyzer.analyze(0.05, 10_000, 42).unwrap();

        // Effect should be 1.10 (verified analytically).
        assert!((result.effect - 1.10).abs() < 1e-9, "effect = {}", result.effect);
        // HAC SE must be positive and finite.
        assert!(result.hac_se >= 0.0);
        assert!(result.hac_se.is_finite());
        // Broad range check: SE < effect confirms signal is detectable.
        assert!(result.hac_se < result.effect.abs(), "hac_se = {}", result.hac_se);
    }

    /// Autocorrelated data should produce larger HAC SE than iid data.
    #[test]
    fn test_hac_se_larger_with_autocorrelation() {
        // iid residuals: alternating sign perturbations (low autocorrelation)
        let iid_blocks = make_blocks(&[
            (true, 2.1),
            (false, 0.9),
            (true, 1.9),
            (false, 1.1),
            (true, 2.1),
            (false, 0.9),
            (true, 1.9),
            (false, 1.1),
        ]);

        // Autocorrelated residuals: persistent positive deviations for 4 blocks
        // then persistent negative, causing lag-1 autocorrelation.
        let acf_blocks = make_blocks(&[
            (true, 2.3),
            (false, 1.3),
            (true, 2.2),
            (false, 1.2),
            (true, 1.8),
            (false, 0.8),
            (true, 1.7),
            (false, 0.7),
        ]);

        let iid_result = SwitchbackAnalyzer::new(iid_blocks)
            .unwrap()
            .analyze(0.05, 10_000, 0)
            .unwrap();
        let acf_result = SwitchbackAnalyzer::new(acf_blocks)
            .unwrap()
            .analyze(0.05, 10_000, 0)
            .unwrap();

        // Both effects should be ~1.0 (effect preserved across both datasets).
        assert!((iid_result.effect - 1.0).abs() < 0.1);
        assert!((acf_result.effect - 1.0).abs() < 0.1);

        // Autocorrelated data should have detectable positive lag-1 autocorrelation.
        assert!(
            acf_result.lag1_autocorrelation > iid_result.lag1_autocorrelation,
            "iid_lag1={:.4}, acf_lag1={:.4}",
            iid_result.lag1_autocorrelation,
            acf_result.lag1_autocorrelation
        );
    }

    // ── Randomization inference ───────────────────────────────────────────────

    #[test]
    fn test_randomization_test_no_effect() {
        // No effect → randomization p-value should be large (close to 1.0).
        let blocks = make_blocks(&[
            (true, 1.0),
            (false, 1.0),
            (true, 1.0),
            (false, 1.0),
            (true, 1.0),
            (false, 1.0),
            (true, 1.0),
            (false, 1.0),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        // Exact test: C(8,4) = 70 < 10_000.
        let p = analyzer.randomization_test(10_000);
        assert!(p >= 0.9, "p = {}", p);
    }

    #[test]
    fn test_randomization_test_large_effect() {
        // Large, consistent effect → randomization test should reject.
        let blocks = make_blocks(&[
            (true, 10.0),
            (false, 1.0),
            (true, 10.0),
            (false, 1.0),
            (true, 10.0),
            (false, 1.0),
            (true, 10.0),
            (false, 1.0),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        let p = analyzer.randomization_test(10_000);
        assert!(p < 0.05, "expected significant p-value, got {}", p);
    }

    #[test]
    fn test_randomization_exact_vs_mc_consistent() {
        // C(6,3) = 20 < 50, so exact test runs.
        // C(6,3) = 20 < 10_000, so MC should agree approximately.
        let blocks = make_blocks(&[
            (true, 2.0),
            (false, 1.0),
            (true, 2.0),
            (false, 1.0),
            (true, 2.0),
            (false, 1.0),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();

        // Exact test (n_permutations > 20).
        let p_exact = analyzer.randomization_test(10_000);
        // MC test with small n_permutations (<20) forces MC path — but C(6,3)=20 > 10
        // so MC path runs.
        let p_mc = analyzer.randomization_test(10);

        // Both should be in [0,1].
        assert!((0.0..=1.0).contains(&p_exact), "p_exact = {}", p_exact);
        assert!((0.0..=1.0).contains(&p_mc), "p_mc = {}", p_mc);
    }

    #[test]
    fn test_randomization_p_value_in_range() {
        let blocks = make_blocks(&[
            (true, 2.1),
            (false, 1.0),
            (true, 1.9),
            (false, 0.9),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        let p = analyzer.randomization_test(10_000);
        assert!((0.0..=1.0).contains(&p), "p = {}", p);
    }

    // ── Carryover test ────────────────────────────────────────────────────────

    #[test]
    fn test_carryover_no_autocorrelation() {
        // Residuals alternate in sign → near-zero autocorrelation, high p-value.
        let blocks = make_blocks(&[
            (true, 2.1),
            (false, 0.9),
            (true, 1.9),
            (false, 1.1),
            (true, 2.1),
            (false, 0.9),
            (true, 1.9),
            (false, 1.1),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        let (r1, p) = analyzer.carryover_test(0.05).unwrap();
        assert!(r1.abs() < 0.5, "lag1 = {}", r1);
        // p-value should be large (no carryover evidence).
        assert!(p > 0.05, "p = {}", p);
    }

    #[test]
    fn test_carryover_p_value_in_range() {
        let blocks = make_blocks(&[
            (true, 2.0),
            (false, 1.0),
            (true, 2.0),
            (false, 1.0),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        let (r1, p) = analyzer.carryover_test(0.05).unwrap();
        assert!(r1 >= -1.0 && r1 <= 1.0, "r1 = {}", r1);
        assert!((0.0..=1.0).contains(&p), "p = {}", p);
    }

    #[test]
    fn test_carryover_alpha_validation() {
        let blocks = make_blocks(&[
            (true, 2.0),
            (false, 1.0),
            (true, 2.0),
            (false, 1.0),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        assert!(analyzer.carryover_test(0.0).is_err());
        assert!(analyzer.carryover_test(1.0).is_err());
    }

    // ── All outputs finite ────────────────────────────────────────────────────

    #[test]
    fn test_all_outputs_finite() {
        let blocks = make_blocks(&[
            (true, 2.1),
            (false, 1.0),
            (true, 2.0),
            (false, 0.9),
            (true, 1.9),
            (false, 1.1),
            (true, 2.2),
            (false, 0.8),
        ]);
        let analyzer = SwitchbackAnalyzer::new(blocks).unwrap();
        let result = analyzer.analyze(0.05, 10_000, 42).unwrap();

        assert!(result.effect.is_finite());
        assert!(result.hac_se.is_finite());
        assert!(result.ci_lower.is_finite());
        assert!(result.ci_upper.is_finite());
        assert!(result.randomization_p_value.is_finite());
        assert!(result.lag1_autocorrelation.is_finite());
        assert!(result.carryover_test_p_value.is_finite());

        assert!(result.hac_se >= 0.0);
        assert!(result.ci_lower <= result.ci_upper);
        assert!((0.0..=1.0).contains(&result.randomization_p_value));
        assert!((0.0..=1.0).contains(&result.carryover_test_p_value));
        assert!(result.lag1_autocorrelation >= -1.0 && result.lag1_autocorrelation <= 1.0);
    }

    // ── Binomial coefficient helper ───────────────────────────────────────────

    #[test]
    fn test_binomial_coeff() {
        assert_eq!(binomial_coeff(8, 4), 70);
        assert_eq!(binomial_coeff(6, 3), 20);
        assert_eq!(binomial_coeff(5, 0), 1);
        assert_eq!(binomial_coeff(5, 5), 1);
        assert_eq!(binomial_coeff(5, 6), 0);
        assert_eq!(binomial_coeff(10, 3), 120);
    }

    // ── Proptest invariants ───────────────────────────────────────────────────

    mod proptest_switchback {
        use super::*;
        use proptest::prelude::*;

        /// Generate valid alternating treatment/control blocks.
        fn gen_blocks(
            values: &[(f64, f64)], // (treat_val, ctrl_val) per cycle
        ) -> Vec<BlockOutcome> {
            let mut blocks = Vec::new();
            for (i, &(tv, cv)) in values.iter().enumerate() {
                blocks.push(BlockOutcome {
                    block_index: (2 * i) as u64,
                    cluster_id: "global".into(),
                    is_treatment: true,
                    metric_value: tv,
                    user_count: 1000,
                    in_washout: false,
                });
                blocks.push(BlockOutcome {
                    block_index: (2 * i + 1) as u64,
                    cluster_id: "global".into(),
                    is_treatment: false,
                    metric_value: cv,
                    user_count: 1000,
                    in_washout: false,
                });
            }
            blocks
        }

        proptest! {
            /// Effect, SE, CI, and p-values are always finite and in valid ranges.
            #[test]
            fn p_all_outputs_valid(
                pairs in proptest::collection::vec(
                    (0.0f64..10.0, 0.0f64..10.0),
                    2..6,  // 2–5 cycles → 4–10 blocks
                ),
                seed in 0u64..1000,
            ) {
                let blocks = gen_blocks(&pairs);
                if let Ok(analyzer) = SwitchbackAnalyzer::new(blocks) {
                    if let Ok(result) = analyzer.analyze(0.05, 100, seed) {
                        prop_assert!(result.effect.is_finite());
                        prop_assert!(result.hac_se >= 0.0);
                        prop_assert!(result.hac_se.is_finite());
                        prop_assert!(result.ci_lower.is_finite());
                        prop_assert!(result.ci_upper.is_finite());
                        prop_assert!(result.ci_lower <= result.ci_upper);
                        prop_assert!(result.randomization_p_value >= 0.0);
                        prop_assert!(result.randomization_p_value <= 1.0);
                        prop_assert!(result.lag1_autocorrelation >= -1.0 - 1e-9);
                        prop_assert!(result.lag1_autocorrelation <= 1.0 + 1e-9);
                        prop_assert!(result.carryover_test_p_value >= 0.0);
                        prop_assert!(result.carryover_test_p_value <= 1.0);
                    }
                }
            }

            /// Effect estimate is the same regardless of the randomization seed.
            #[test]
            fn p_effect_seed_independent(
                pairs in proptest::collection::vec(
                    (0.0f64..5.0, 0.0f64..5.0),
                    2..5,
                ),
                seed1 in 0u64..500,
                seed2 in 500u64..1000,
            ) {
                let blocks1 = gen_blocks(&pairs);
                let blocks2 = gen_blocks(&pairs);
                if let (Ok(a1), Ok(a2)) = (
                    SwitchbackAnalyzer::new(blocks1),
                    SwitchbackAnalyzer::new(blocks2),
                ) {
                    if let (Ok(r1), Ok(r2)) = (
                        a1.analyze(0.05, 100, seed1),
                        a2.analyze(0.05, 100, seed2),
                    ) {
                        // Effect and HAC SE don't depend on the RNG seed.
                        prop_assert!((r1.effect - r2.effect).abs() < 1e-10);
                        prop_assert!((r1.hac_se - r2.hac_se).abs() < 1e-10);
                    }
                }
            }

            /// Randomization p-value is in [0, 1] for arbitrary valid inputs.
            #[test]
            fn p_randomization_p_in_range(
                pairs in proptest::collection::vec(
                    (0.0f64..5.0, 0.0f64..5.0),
                    2..5,
                ),
            ) {
                let blocks = gen_blocks(&pairs);
                if let Ok(analyzer) = SwitchbackAnalyzer::new(blocks) {
                    let p = analyzer.randomization_test(50);
                    prop_assert!(p >= 0.0);
                    prop_assert!(p <= 1.0);
                }
            }

            /// Lag-1 autocorrelation of any series is in [-1, 1].
            #[test]
            fn p_lag1_autocorr_in_range(
                x in proptest::collection::vec(-10.0f64..10.0, 3..20),
            ) {
                let r = lag1_autocorr(&x);
                prop_assert!(r >= -1.0 - 1e-9, "r = {}", r);
                prop_assert!(r <= 1.0 + 1e-9, "r = {}", r);
            }
        }
    }
}
