//! Feedback loop interference detection for recommendation model experiments.
//!
//! When a recommendation model is retrained on data that includes treatment
//! exposures, feedback loops can contaminate effect estimates: the model
//! "learns" from the treatment, causing post-retrain effects to differ from
//! pre-retrain effects in ways that reflect contamination, not genuine causal
//! treatment effects.
//!
//! This module implements ADR-021:
//! - **Paired t-test**: Tests whether treatment effects shift systematically
//!   before vs after each retraining event. Significant shifts suggest the
//!   retrained model is amplifying or attenuating the measured effect.
//! - **Contamination-effect correlation**: Pearson r between the fraction of
//!   training data containing treatment exposures and the post-retrain effect
//!   estimate. High positive correlation indicates feedback loop distortion.
//! - **Bias-corrected extrapolation**: OLS regression of post-retrain effect
//!   on contamination fraction; extrapolation to zero contamination yields the
//!   estimated uncontaminated treatment effect.
//!
//! See design doc §17.5 and arXiv 2310.17496v4 (Feedback loop interference in
//! A/B tests, 2024) for theoretical background.

use experimentation_core::error::{assert_finite, Error, Result};
use statrs::distribution::{ContinuousCDF, StudentsT};

/// A single retraining event paired with pre/post treatment effect measurements.
///
/// `pre_retrain_effect` is the treatment effect estimate from the 7-day window
/// immediately *before* the retrained model was deployed.  `post_retrain_effect`
/// is the estimate from the 7-day window immediately *after*.  Both are
/// point estimates (treatment_mean − control_mean) computed by M3.
///
/// `contamination_fraction` is the fraction of training examples that were
/// drawn from the treatment arm, as computed by M3's contamination pipeline.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RetrainingEffectObservation {
    /// Fraction of training data containing treatment exposures [0.0, 1.0].
    pub contamination_fraction: f64,
    /// Treatment effect estimate in the 7-day window before this retraining.
    pub pre_retrain_effect: f64,
    /// Treatment effect estimate in the 7-day window after this retraining.
    pub post_retrain_effect: f64,
}

/// Result of feedback loop detection analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FeedbackLoopResult {
    /// True if feedback loop contamination is detected (p_value < alpha and
    /// |contamination_effect_correlation| > 0.5, or p_value < alpha/2 alone).
    pub feedback_loop_detected: bool,
    /// Two-sided p-value from paired t-test on (post − pre) differences.
    /// Small values indicate systematic shifts tied to retraining events.
    pub paired_ttest_p_value: f64,
    /// Mean treatment effect across the pre-retrain windows.
    pub mean_pre_retrain_effect: f64,
    /// Mean treatment effect across the post-retrain windows.
    pub mean_post_retrain_effect: f64,
    /// Mean signed shift: mean(post − pre). Positive means post-retrain effects
    /// are larger; negative means the model suppressed measured treatment gains.
    pub mean_effect_shift: f64,
    /// Pearson correlation between contamination_fraction and post_retrain_effect.
    /// |r| > 0.5 indicates contamination is driving effect magnitude.
    pub contamination_effect_correlation: f64,
    /// Estimated bias: how much of the mean post-retrain effect is attributable
    /// to contamination (β₁ × mean_contamination_fraction from OLS).
    pub bias_estimate: f64,
    /// Bias-corrected treatment effect: OLS intercept β₀ when extrapolating
    /// contamination_fraction → 0.  Best estimate of the "true" effect without
    /// feedback loop influence.
    pub bias_corrected_effect: f64,
    /// Number of retraining events analysed.
    pub n_retraining_events: usize,
}

/// Simplified interference result for downstream consumers that need only
/// the key detection outputs without the full `FeedbackLoopResult` payload.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InterferenceResult {
    /// True if feedback loop contamination was detected.
    pub detected: bool,
    /// Two-sided p-value from the paired t-test on (post − pre) differences.
    pub p_value: f64,
    /// Multiplicative factor by which the detected feedback loop inflates
    /// treatment effect estimates: `mean_post / mean_pre` when `mean_pre ≠ 0`,
    /// otherwise 1.0. Apply to raw effects via `bias_corrected_effect()`.
    pub bias_correction_factor: f64,
    /// Bias-corrected treatment effect from OLS extrapolation to zero
    /// contamination fraction.
    pub corrected_effect: f64,
}

impl From<FeedbackLoopResult> for InterferenceResult {
    fn from(r: FeedbackLoopResult) -> Self {
        let bias_correction_factor = if r.mean_pre_retrain_effect.abs() > 1e-15 {
            r.mean_post_retrain_effect / r.mean_pre_retrain_effect
        } else {
            1.0
        };
        InterferenceResult {
            detected: r.feedback_loop_detected,
            p_value: r.paired_ttest_p_value,
            bias_correction_factor,
            corrected_effect: r.bias_corrected_effect,
        }
    }
}

/// Returns `true` when `p_value` is below `threshold`, indicating statistically
/// significant contamination at the given significance level.
///
/// # Panics
/// Panics in debug mode if `threshold` is not in (0, 1).
pub fn contamination_flag(p_value: f64, threshold: f64) -> bool {
    debug_assert!(threshold > 0.0 && threshold < 1.0, "threshold must be in (0, 1)");
    p_value < threshold
}

/// Detects feedback loop contamination across a sequence of retraining events.
///
/// # Minimum data requirements
/// At least 3 retraining event observations are required (df ≥ 2 for t-test;
/// at least 2 distinct contamination values for OLS to be non-degenerate).
pub struct FeedbackLoopDetector {
    observations: Vec<RetrainingEffectObservation>,
}

impl FeedbackLoopDetector {
    /// Create a detector from a set of paired pre/post retraining observations.
    ///
    /// # Errors
    /// Returns `Error::Validation` if:
    /// - Fewer than 3 observations are provided.
    /// - Any `contamination_fraction` is outside [0, 1].
    pub fn new(observations: Vec<RetrainingEffectObservation>) -> Result<Self> {
        if observations.len() < 3 {
            return Err(Error::Validation(
                "feedback loop detection requires at least 3 retraining event observations".into(),
            ));
        }
        for (i, obs) in observations.iter().enumerate() {
            if !(0.0..=1.0).contains(&obs.contamination_fraction) {
                return Err(Error::Validation(format!(
                    "observation[{i}].contamination_fraction = {} is outside [0, 1]",
                    obs.contamination_fraction
                )));
            }
            assert_finite(obs.pre_retrain_effect, &format!("observation[{i}].pre_retrain_effect"));
            assert_finite(obs.post_retrain_effect, &format!("observation[{i}].post_retrain_effect"));
        }
        Ok(Self { observations })
    }

    /// Apply bias correction to a raw treatment effect using the pre/post ratio.
    ///
    /// Uses the ratio `mean_pre / mean_post` across all observations to scale
    /// `raw` back toward the uncontaminated estimate. When `mean_post ≈ 0` the
    /// ratio is undefined and `raw` is returned unchanged.
    ///
    /// # Note
    /// For the full OLS-based bias correction (recommended), use
    /// `detect().bias_corrected_effect` instead, which extrapolates to zero
    /// contamination fraction via regression.
    pub fn bias_corrected_effect(&self, raw: f64) -> f64 {
        let n = self.observations.len() as f64;
        let mean_pre = self.observations.iter().map(|o| o.pre_retrain_effect).sum::<f64>() / n;
        let mean_post = self.observations.iter().map(|o| o.post_retrain_effect).sum::<f64>() / n;
        if mean_post.abs() < 1e-15 {
            return raw;
        }
        raw * (mean_pre / mean_post)
    }

    /// Run full feedback loop detection analysis.
    ///
    /// # Arguments
    /// * `alpha` — Significance level for paired t-test (e.g. 0.05).
    ///
    /// # Detection criterion
    /// `feedback_loop_detected = true` when:
    /// - `paired_ttest_p_value < alpha` **and** `|contamination_effect_correlation| > 0.5`, OR
    /// - `paired_ttest_p_value < alpha / 2` (very strong paired-test signal alone).
    ///
    /// The dual criterion reduces false positives from random week-to-week
    /// effect variation (paired test alone) while still flagging cases where
    /// a strong contamination-effect relationship exists.
    pub fn detect(&self, alpha: f64) -> Result<FeedbackLoopResult> {
        if alpha <= 0.0 || alpha >= 1.0 {
            return Err(Error::Validation("alpha must be in (0, 1)".into()));
        }

        let n = self.observations.len();
        let nf = n as f64;

        // ── Paired t-test on (post − pre) differences ──────────────────────────
        let diffs: Vec<f64> = self
            .observations
            .iter()
            .map(|o| o.post_retrain_effect - o.pre_retrain_effect)
            .collect();

        let mean_d = diffs.iter().sum::<f64>() / nf;
        assert_finite(mean_d, "mean_effect_shift");

        let var_d = diffs.iter().map(|&d| (d - mean_d).powi(2)).sum::<f64>() / (nf - 1.0);
        assert_finite(var_d, "var_d");

        let paired_ttest_p_value = if var_d <= 0.0 {
            // All differences identical → no variance to test.
            1.0
        } else {
            let se_d = (var_d / nf).sqrt();
            assert_finite(se_d, "se_d");
            let t_stat = mean_d / se_d;
            assert_finite(t_stat, "t_stat_paired");
            let df = nf - 1.0;
            let t_dist = StudentsT::new(0.0, 1.0, df)
                .map_err(|e| Error::Numerical(format!("t-distribution error: {e}")))?;
            let p = 2.0 * (1.0 - t_dist.cdf(t_stat.abs()));
            assert_finite(p, "paired_ttest_p_value");
            p.clamp(0.0, 1.0)
        };

        // ── Summary effect statistics ───────────────────────────────────────────
        let mean_pre = self.observations.iter().map(|o| o.pre_retrain_effect).sum::<f64>() / nf;
        let mean_post = self.observations.iter().map(|o| o.post_retrain_effect).sum::<f64>() / nf;
        assert_finite(mean_pre, "mean_pre_retrain_effect");
        assert_finite(mean_post, "mean_post_retrain_effect");

        // ── Pearson correlation: contamination_fraction vs post_retrain_effect ──
        let mean_c = self
            .observations
            .iter()
            .map(|o| o.contamination_fraction)
            .sum::<f64>()
            / nf;
        assert_finite(mean_c, "mean_contamination");

        let cov_cy = self
            .observations
            .iter()
            .map(|o| (o.contamination_fraction - mean_c) * (o.post_retrain_effect - mean_post))
            .sum::<f64>()
            / (nf - 1.0);
        let var_c = self
            .observations
            .iter()
            .map(|o| (o.contamination_fraction - mean_c).powi(2))
            .sum::<f64>()
            / (nf - 1.0);
        let var_y = self
            .observations
            .iter()
            .map(|o| (o.post_retrain_effect - mean_post).powi(2))
            .sum::<f64>()
            / (nf - 1.0);

        assert_finite(cov_cy, "cov_cy");
        assert_finite(var_c, "var_c");
        assert_finite(var_y, "var_y");

        let contamination_effect_correlation = if var_c <= 0.0 || var_y <= 0.0 {
            // Degenerate: no spread in one or both variables.
            0.0
        } else {
            let r = cov_cy / (var_c.sqrt() * var_y.sqrt());
            assert_finite(r, "contamination_effect_correlation");
            r.clamp(-1.0, 1.0)
        };

        // ── OLS bias correction ─────────────────────────────────────────────────
        // Model: post_retrain_effect = β₀ + β₁ × contamination_fraction
        // β₁ = cov(c,y) / var(c)
        // β₀ = mean(y) - β₁ × mean(c)  ← uncontaminated estimate
        let (bias_corrected_effect, bias_estimate) = if var_c <= 0.0 {
            // All contamination fractions identical → OLS undefined; use mean.
            (mean_post, 0.0)
        } else {
            let beta1 = cov_cy / var_c;
            let beta0 = mean_post - beta1 * mean_c;
            assert_finite(beta1, "ols_beta1");
            assert_finite(beta0, "ols_beta0");
            let bias = mean_post - beta0; // = β₁ × mean(c)
            assert_finite(bias, "bias_estimate");
            (beta0, bias)
        };

        // ── Detection criterion ─────────────────────────────────────────────────
        let strong_correlation = contamination_effect_correlation.abs() > 0.5;
        let feedback_loop_detected = (paired_ttest_p_value < alpha && strong_correlation)
            || paired_ttest_p_value < alpha / 2.0;

        Ok(FeedbackLoopResult {
            feedback_loop_detected,
            paired_ttest_p_value,
            mean_pre_retrain_effect: mean_pre,
            mean_post_retrain_effect: mean_post,
            mean_effect_shift: mean_d,
            contamination_effect_correlation,
            bias_estimate,
            bias_corrected_effect,
            n_retraining_events: n,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(contamination: f64, pre: f64, post: f64) -> RetrainingEffectObservation {
        RetrainingEffectObservation {
            contamination_fraction: contamination,
            pre_retrain_effect: pre,
            post_retrain_effect: post,
        }
    }

    // ── Validation ────────────────────────────────────────────────────────────

    #[test]
    fn test_too_few_observations() {
        let err = FeedbackLoopDetector::new(vec![obs(0.1, 0.1, 0.1), obs(0.2, 0.2, 0.2)]);
        assert!(err.is_err());
    }

    #[test]
    fn test_contamination_out_of_range() {
        let err = FeedbackLoopDetector::new(vec![
            obs(1.5, 0.1, 0.1),
            obs(0.2, 0.2, 0.2),
            obs(0.3, 0.3, 0.3),
        ]);
        assert!(err.is_err());
    }

    #[test]
    fn test_alpha_validation() {
        let det = FeedbackLoopDetector::new(vec![
            obs(0.1, 0.1, 0.1),
            obs(0.2, 0.2, 0.2),
            obs(0.3, 0.3, 0.3),
        ])
        .unwrap();
        assert!(det.detect(0.0).is_err());
        assert!(det.detect(1.0).is_err());
    }

    // ── No feedback loop: identical pre/post effects ──────────────────────────

    #[test]
    fn test_no_feedback_loop_identical() {
        let observations = vec![
            obs(0.10, 0.05, 0.05),
            obs(0.15, 0.05, 0.05),
            obs(0.20, 0.05, 0.05),
            obs(0.25, 0.05, 0.05),
            obs(0.30, 0.05, 0.05),
        ];
        let det = FeedbackLoopDetector::new(observations).unwrap();
        let result = det.detect(0.05).unwrap();
        assert!(!result.feedback_loop_detected, "no shift → no detection");
        assert!((result.mean_effect_shift - 0.0).abs() < 1e-10);
        assert!((result.paired_ttest_p_value - 1.0).abs() < 1e-9);
    }

    // ── Strong feedback loop: post > pre, contamination correlates ────────────

    #[test]
    fn test_strong_feedback_loop_detected() {
        // Designed so that:
        // - post_retrain_effect = pre + 2*contamination (strong positive feedback)
        // - paired test will clearly reject H0
        let observations = vec![
            obs(0.10, 0.05, 0.25),
            obs(0.20, 0.05, 0.45),
            obs(0.30, 0.05, 0.65),
            obs(0.40, 0.05, 0.85),
            obs(0.50, 0.05, 1.05),
        ];
        let det = FeedbackLoopDetector::new(observations).unwrap();
        let result = det.detect(0.05).unwrap();
        assert!(result.feedback_loop_detected, "strong feedback loop should be detected");
        assert!(result.paired_ttest_p_value < 0.05, "p={}", result.paired_ttest_p_value);
        assert!(
            result.contamination_effect_correlation > 0.9,
            "r={}",
            result.contamination_effect_correlation
        );
    }

    // ── Bias correction: OLS extrapolation to zero contamination ──────────────

    #[test]
    fn test_bias_correction_golden() {
        // post_effect = 0.05 + 0.5 * contamination
        // → β₀ = 0.05, β₁ = 0.5
        // mean_contamination = 0.30, so bias = 0.5 * 0.30 = 0.15
        // bias_corrected = 0.05
        let observations = vec![
            obs(0.10, 0.03, 0.10),  // post = 0.05 + 0.5*0.10
            obs(0.20, 0.04, 0.15),  // post = 0.05 + 0.5*0.20
            obs(0.30, 0.05, 0.20),  // post = 0.05 + 0.5*0.30
            obs(0.40, 0.06, 0.25),  // post = 0.05 + 0.5*0.40
            obs(0.50, 0.07, 0.30),  // post = 0.05 + 0.5*0.50
        ];
        let det = FeedbackLoopDetector::new(observations).unwrap();
        let result = det.detect(0.05).unwrap();

        // β₀ = 0.05 (uncontaminated effect)
        assert!(
            (result.bias_corrected_effect - 0.05).abs() < 1e-8,
            "bias_corrected_effect = {}",
            result.bias_corrected_effect
        );
        // bias = β₁ × mean(c) = 0.5 × 0.30 = 0.15
        assert!(
            (result.bias_estimate - 0.15).abs() < 1e-8,
            "bias_estimate = {}",
            result.bias_estimate
        );
    }

    // ── Mean effect statistics ────────────────────────────────────────────────

    #[test]
    fn test_mean_effects_correct() {
        let observations = vec![
            obs(0.1, 0.10, 0.20),
            obs(0.2, 0.20, 0.30),
            obs(0.3, 0.30, 0.40),
        ];
        let det = FeedbackLoopDetector::new(observations).unwrap();
        let result = det.detect(0.05).unwrap();
        assert!((result.mean_pre_retrain_effect - 0.20).abs() < 1e-10);
        assert!((result.mean_post_retrain_effect - 0.30).abs() < 1e-10);
        assert!((result.mean_effect_shift - 0.10).abs() < 1e-10);
        assert_eq!(result.n_retraining_events, 3);
    }

    // ── Correlation: uncorrelated contamination/effect ────────────────────────

    #[test]
    fn test_uncorrelated_contamination() {
        // Contamination varies but post-effect stays constant → r ≈ 0
        let observations = vec![
            obs(0.10, 0.05, 0.05),
            obs(0.20, 0.05, 0.05),
            obs(0.30, 0.05, 0.05),
            obs(0.40, 0.05, 0.05),
            obs(0.50, 0.05, 0.05),
        ];
        let det = FeedbackLoopDetector::new(observations).unwrap();
        let result = det.detect(0.05).unwrap();
        assert!(
            result.contamination_effect_correlation.abs() < 1e-8,
            "r={}",
            result.contamination_effect_correlation
        );
    }

    // ── contamination_flag ────────────────────────────────────────────────────

    #[test]
    fn test_contamination_flag_below_threshold() {
        assert!(contamination_flag(0.01, 0.05));
        assert!(contamination_flag(0.049, 0.05));
    }

    #[test]
    fn test_contamination_flag_at_or_above_threshold() {
        assert!(!contamination_flag(0.05, 0.05));
        assert!(!contamination_flag(0.99, 0.05));
    }

    // ── InterferenceResult conversion ─────────────────────────────────────────

    #[test]
    fn test_interference_result_from_feedback_loop_result() {
        let observations = vec![
            obs(0.10, 0.05, 0.25),
            obs(0.20, 0.05, 0.45),
            obs(0.30, 0.05, 0.65),
            obs(0.40, 0.05, 0.85),
            obs(0.50, 0.05, 1.05),
        ];
        let det = FeedbackLoopDetector::new(observations).unwrap();
        let full = det.detect(0.05).unwrap();
        let ir: InterferenceResult = full.clone().into();
        assert_eq!(ir.detected, full.feedback_loop_detected);
        assert_eq!(ir.p_value, full.paired_ttest_p_value);
        assert_eq!(ir.corrected_effect, full.bias_corrected_effect);
    }

    // ── bias_corrected_effect method ──────────────────────────────────────────

    #[test]
    fn test_bias_corrected_effect_method_ratio() {
        // mean_pre = 0.05, mean_post = 0.65 → ratio = 0.05/0.65
        let observations = vec![
            obs(0.10, 0.05, 0.25),
            obs(0.20, 0.05, 0.45),
            obs(0.30, 0.05, 0.65),
            obs(0.40, 0.05, 0.85),
            obs(0.50, 0.05, 1.05),
        ];
        let det = FeedbackLoopDetector::new(observations).unwrap();
        let mean_pre = 0.05f64;
        let mean_post = (0.25 + 0.45 + 0.65 + 0.85 + 1.05) / 5.0;
        let raw = 0.5;
        let expected = raw * (mean_pre / mean_post);
        let got = det.bias_corrected_effect(raw);
        assert!((got - expected).abs() < 1e-10, "got {got}, expected {expected}");
    }

    #[test]
    fn test_bias_corrected_effect_zero_mean_post() {
        // When mean_post ≈ 0, return raw unchanged.
        let observations = vec![
            obs(0.10, 0.5, 0.0),
            obs(0.20, 0.5, 0.0),
            obs(0.30, 0.5, 0.0),
        ];
        let det = FeedbackLoopDetector::new(observations).unwrap();
        let raw = 0.3;
        let got = det.bias_corrected_effect(raw);
        assert!((got - raw).abs() < 1e-10, "expected passthrough, got {got}");
    }

    // ── Proptest invariants ───────────────────────────────────────────────────

    mod proptest_feedback {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Under the null hypothesis (pre == post for all observations),
            /// the paired differences are identically zero. The detector must
            /// never flag feedback loop contamination regardless of alpha,
            /// because variance is zero and the test returns p = 1.0.
            #[test]
            fn null_no_detection(
                effects in proptest::collection::vec(-1.0f64..1.0, 3..10),
                contams in proptest::collection::vec(0.01f64..0.99, 3..10),
            ) {
                let n = effects.len().min(contams.len());
                if n < 3 { return Ok(()); }
                // pre == post for every observation → null is exactly true.
                let observations: Vec<RetrainingEffectObservation> = (0..n)
                    .map(|i| RetrainingEffectObservation {
                        contamination_fraction: contams[i],
                        pre_retrain_effect: effects[i],
                        post_retrain_effect: effects[i],
                    })
                    .collect();
                if let Ok(det) = FeedbackLoopDetector::new(observations) {
                    let result = det.detect(0.05).unwrap();
                    prop_assert!(
                        !result.feedback_loop_detected,
                        "under null (pre==post), detection must not fire; p={}",
                        result.paired_ttest_p_value
                    );
                    // p-value must be 1.0 when all diffs are zero.
                    prop_assert!(
                        (result.paired_ttest_p_value - 1.0).abs() < 1e-9,
                        "expected p=1.0 under null, got {}",
                        result.paired_ttest_p_value
                    );
                }
            }

            #[test]
            fn p_value_in_range(
                effects in proptest::collection::vec((-1.0f64..1.0, -1.0f64..1.0), 3..10),
            ) {
                let observations: Vec<RetrainingEffectObservation> = effects
                    .iter()
                    .enumerate()
                    .map(|(i, &(pre, post))| RetrainingEffectObservation {
                        contamination_fraction: (i as f64 + 1.0) / (effects.len() as f64 + 1.0),
                        pre_retrain_effect: pre,
                        post_retrain_effect: post,
                    })
                    .collect();
                let det = FeedbackLoopDetector::new(observations).unwrap();
                let result = det.detect(0.05).unwrap();
                prop_assert!(result.paired_ttest_p_value >= 0.0);
                prop_assert!(result.paired_ttest_p_value <= 1.0);
            }

            #[test]
            fn correlation_in_range(
                effects in proptest::collection::vec((-1.0f64..1.0, -1.0f64..1.0), 3..10),
            ) {
                let observations: Vec<RetrainingEffectObservation> = effects
                    .iter()
                    .enumerate()
                    .map(|(i, &(pre, post))| RetrainingEffectObservation {
                        contamination_fraction: (i as f64 + 1.0) / (effects.len() as f64 + 1.0),
                        pre_retrain_effect: pre,
                        post_retrain_effect: post,
                    })
                    .collect();
                let det = FeedbackLoopDetector::new(observations).unwrap();
                let result = det.detect(0.05).unwrap();
                prop_assert!(result.contamination_effect_correlation >= -1.0 - 1e-9);
                prop_assert!(result.contamination_effect_correlation <= 1.0 + 1e-9);
            }

            #[test]
            fn all_outputs_finite(
                effects in proptest::collection::vec((-1.0f64..1.0, -1.0f64..1.0), 3..10),
                contaminations in proptest::collection::vec(0.0f64..1.0, 3..10),
            ) {
                let n = effects.len().min(contaminations.len());
                if n < 3 { return Ok(()); }
                let observations: Vec<RetrainingEffectObservation> = (0..n)
                    .map(|i| RetrainingEffectObservation {
                        contamination_fraction: contaminations[i],
                        pre_retrain_effect: effects[i].0,
                        post_retrain_effect: effects[i].1,
                    })
                    .collect();
                if let Ok(det) = FeedbackLoopDetector::new(observations) {
                    if let Ok(result) = det.detect(0.05) {
                        prop_assert!(result.paired_ttest_p_value.is_finite());
                        prop_assert!(result.contamination_effect_correlation.is_finite());
                        prop_assert!(result.bias_estimate.is_finite());
                        prop_assert!(result.bias_corrected_effect.is_finite());
                        prop_assert!(result.mean_pre_retrain_effect.is_finite());
                        prop_assert!(result.mean_post_retrain_effect.is_finite());
                        prop_assert!(result.mean_effect_shift.is_finite());
                    }
                }
            }
        }
    }
}
