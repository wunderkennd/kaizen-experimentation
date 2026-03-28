//! Guardrail beta-correction for multi-metric monitoring (ADR-014).
//!
//! When K guardrail metrics are monitored simultaneously, Bonferroni correction
//! runs each at significance `alpha / K` to control the family-wise error rate
//! (FWER) across all guardrails.
//!
//! # Structs
//! - [`GuardrailCorrector`] — Bonferroni corrector for K simultaneous guardrails.
//!
//! # Enums
//! - [`MetricStakeholder`] — identifies who the metric benefits (USER/PROVIDER/PLATFORM).
//! - [`MetricAggregationLevel`] — unit of observation for analysis.
//!
//! # Validation
//! - [`validate_bandit_reward_aggregation`] — bandit rewards must use USER aggregation.
//! - [`validate_guardrail_aggregation`] — guardrails accept USER or EXPERIMENT aggregation.

use experimentation_core::error::{assert_finite, Error, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Enums (mirrors proto MetricStakeholder / MetricAggregationLevel — ADR-014)
// ─────────────────────────────────────────────────────────────────────────────

/// Identifies who a metric measures on behalf of (ADR-014).
///
/// Mirrors `MetricStakeholder` in `common/v1/metric.proto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MetricStakeholder {
    /// Unspecified — treated as invalid in validation.
    Unspecified,
    /// Subscriber-experience metric (watch time, churn, conversion).
    User,
    /// Content-provider metric (catalog coverage, Gini, longtail share).
    Provider,
    /// Platform-health metric (revenue, infrastructure cost).
    Platform,
}

/// Unit of observation used for M3 computation and M4a analysis (ADR-014).
///
/// Mirrors `MetricAggregationLevel` in `common/v1/metric.proto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MetricAggregationLevel {
    /// Unspecified — treated as invalid in validation.
    Unspecified,
    /// One row per user per experiment arm. Standard M4a analysis unit.
    User,
    /// One row per (experiment, variant, time-window). Used for catalog-level
    /// metrics where per-user disaggregation is meaningless (e.g., Gini).
    Experiment,
    /// One row per content provider per arm. Requires `content_catalog` join.
    Provider,
}

// ─────────────────────────────────────────────────────────────────────────────
// GuardrailCorrector
// ─────────────────────────────────────────────────────────────────────────────

/// Bonferroni correction for K simultaneous guardrail metrics (ADR-014).
///
/// Each of the K guardrails is tested at significance `alpha / K` so that the
/// family-wise error rate (FWER) — the probability of firing at least one false
/// alarm when all null hypotheses are true — is bounded by `alpha`.
///
/// # Example
/// ```rust
/// use experimentation_stats::guardrail::GuardrailCorrector;
///
/// let corrector = GuardrailCorrector::new(0.05, 3);
/// assert!((corrector.corrected_alpha() - 0.05 / 3.0).abs() < 1e-12);
/// assert!(corrector.is_violated(0.01));   // 0.01 ≤ 0.05/3 ≈ 0.0167 → fires
/// assert!(!corrector.is_violated(0.04)); // 0.04 > threshold → no fire
/// ```
#[derive(Debug, Clone)]
pub struct GuardrailCorrector {
    alpha: f64,
    k: usize,
}

impl GuardrailCorrector {
    /// Construct a corrector for `k` guardrail metrics at family-wise level `alpha`.
    ///
    /// # Panics
    /// Panics if `alpha` is not in `(0, 1)` or `k == 0`.
    pub fn new(alpha: f64, k: usize) -> Self {
        assert!(
            alpha > 0.0 && alpha < 1.0,
            "FAIL-FAST: alpha must be in (0, 1), got {alpha}"
        );
        assert!(k > 0, "FAIL-FAST: k must be at least 1, got {k}");
        Self { alpha, k }
    }

    /// Per-guardrail significance threshold: `alpha / K`.
    ///
    /// Each guardrail is tested against this corrected level. The FWER across
    /// all K guardrails is bounded by `alpha` via Boole's inequality.
    pub fn corrected_alpha(&self) -> f64 {
        let ca = self.alpha / self.k as f64;
        assert_finite(ca, "GuardrailCorrector::corrected_alpha");
        ca
    }

    /// Returns `true` when the guardrail fires — i.e., `p_value ≤ alpha / K`.
    ///
    /// # Panics
    /// Panics (fail-fast) if `p_value` is NaN or infinite.
    pub fn is_violated(&self, p_value: f64) -> bool {
        assert_finite(p_value, "GuardrailCorrector::is_violated p_value");
        p_value <= self.corrected_alpha()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Aggregation-level validation
// ─────────────────────────────────────────────────────────────────────────────

/// Validate that a bandit reward metric uses `User` aggregation.
///
/// Bandit arm posterior updates are computed on independent user-level
/// observations. Experiment- or provider-level aggregations break the i.i.d.
/// assumption and must be rejected.
///
/// # Errors
/// Returns `Error::Validation` if `level` is anything other than `User`.
pub fn validate_bandit_reward_aggregation(level: MetricAggregationLevel) -> Result<()> {
    if level == MetricAggregationLevel::User {
        Ok(())
    } else {
        Err(Error::Validation(
            "bandit reward metrics must use USER aggregation level; \
             EXPERIMENT and PROVIDER aggregation are not supported for rewards"
                .into(),
        ))
    }
}

/// Validate that a guardrail metric uses `User` or `Experiment` aggregation.
///
/// Guardrails may monitor per-user means (standard) or experiment-level
/// catalog aggregates (coverage, Gini). Provider-level aggregation is not
/// supported for guardrails because cross-provider comparisons require a
/// separate multi-stakeholder analysis path.
///
/// # Errors
/// Returns `Error::Validation` for `Unspecified` or `Provider` aggregation.
pub fn validate_guardrail_aggregation(level: MetricAggregationLevel) -> Result<()> {
    match level {
        MetricAggregationLevel::User | MetricAggregationLevel::Experiment => Ok(()),
        MetricAggregationLevel::Provider => Err(Error::Validation(
            "guardrail metrics do not support PROVIDER aggregation level; \
             use USER or EXPERIMENT"
                .into(),
        )),
        MetricAggregationLevel::Unspecified => Err(Error::Validation(
            "guardrail metric aggregation level must be specified (USER or EXPERIMENT)"
                .into(),
        )),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── GuardrailCorrector unit tests ─────────────────────────────────────

    #[test]
    fn corrected_alpha_is_alpha_over_k() {
        let c = GuardrailCorrector::new(0.05, 3);
        let expected = 0.05 / 3.0;
        assert!((c.corrected_alpha() - expected).abs() < 1e-14);
    }

    #[test]
    fn k1_corrected_alpha_equals_alpha() {
        let c = GuardrailCorrector::new(0.05, 1);
        assert!((c.corrected_alpha() - 0.05).abs() < 1e-14);
    }

    #[test]
    fn is_violated_fires_below_threshold() {
        let c = GuardrailCorrector::new(0.05, 3);
        let threshold = 0.05 / 3.0;
        assert!(c.is_violated(threshold - 1e-9));
        assert!(c.is_violated(threshold)); // boundary: ≤ fires
    }

    #[test]
    fn is_violated_does_not_fire_above_threshold() {
        let c = GuardrailCorrector::new(0.05, 3);
        let threshold = 0.05 / 3.0;
        assert!(!c.is_violated(threshold + 1e-9));
        assert!(!c.is_violated(1.0));
    }

    #[test]
    fn is_violated_all_below_fires_each() {
        let c = GuardrailCorrector::new(0.05, 5);
        // threshold = 0.01
        for &p in &[0.001, 0.005, 0.0099] {
            assert!(c.is_violated(p), "expected violation for p={p}");
        }
    }

    #[test]
    fn is_violated_all_above_no_fire() {
        let c = GuardrailCorrector::new(0.05, 5);
        for &p in &[0.02, 0.05, 1.0] {
            assert!(!c.is_violated(p), "unexpected violation for p={p}");
        }
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn new_panics_on_zero_alpha() {
        GuardrailCorrector::new(0.0, 3);
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn new_panics_on_alpha_one() {
        GuardrailCorrector::new(1.0, 3);
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn new_panics_on_zero_k() {
        GuardrailCorrector::new(0.05, 0);
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn is_violated_panics_on_nan() {
        let c = GuardrailCorrector::new(0.05, 2);
        c.is_violated(f64::NAN);
    }

    // ── MetricAggregationLevel validation ─────────────────────────────────

    #[test]
    fn bandit_reward_requires_user() {
        assert!(validate_bandit_reward_aggregation(MetricAggregationLevel::User).is_ok());
        assert!(validate_bandit_reward_aggregation(MetricAggregationLevel::Experiment).is_err());
        assert!(validate_bandit_reward_aggregation(MetricAggregationLevel::Provider).is_err());
        assert!(validate_bandit_reward_aggregation(MetricAggregationLevel::Unspecified).is_err());
    }

    #[test]
    fn guardrail_accepts_user_and_experiment() {
        assert!(validate_guardrail_aggregation(MetricAggregationLevel::User).is_ok());
        assert!(validate_guardrail_aggregation(MetricAggregationLevel::Experiment).is_ok());
    }

    #[test]
    fn guardrail_rejects_provider_and_unspecified() {
        assert!(validate_guardrail_aggregation(MetricAggregationLevel::Provider).is_err());
        assert!(validate_guardrail_aggregation(MetricAggregationLevel::Unspecified).is_err());
    }

    // ── FWER Monte-Carlo (10 K simulations, seeded) ───────────────────────

    /// Under the null, empirical FWER ≤ alpha for K = 1..20.
    ///
    /// Each guardrail's p-value is drawn from Uniform(0, 1) (the null
    /// distribution of a valid p-value).  By Boole's inequality, the
    /// Bonferroni-corrected FWER is bounded by alpha.  We verify this
    /// empirically with 10 000 replications.  A 5σ tolerance is added to
    /// avoid false failures from sampling variability.
    #[test]
    fn fwer_under_null_bounded_by_alpha() {
        use rand::Rng as _;
        use rand::SeedableRng as _;

        const N_SIMS: usize = 10_000;
        const ALPHA: f64 = 0.05;

        let mut rng = rand::rngs::StdRng::seed_from_u64(0xdead_beef_cafe_babe);

        for k in [1usize, 2, 3, 5, 10, 20] {
            let corrector = GuardrailCorrector::new(ALPHA, k);
            let violations = (0..N_SIMS)
                .filter(|_| {
                    (0..k).any(|_| corrector.is_violated(rng.gen::<f64>()))
                })
                .count();

            let empirical_fwer = violations as f64 / N_SIMS as f64;
            // 5σ tolerance for sampling noise
            let tolerance =
                5.0 * (ALPHA * (1.0 - ALPHA) / N_SIMS as f64).sqrt();

            assert!(
                empirical_fwer <= ALPHA + tolerance,
                "k={k}: empirical FWER {empirical_fwer:.4} exceeded alpha {ALPHA:.4} + 5σ {tolerance:.4}"
            );
        }
    }

    // ── Proptest invariants ───────────────────────────────────────────────

    mod proptest_guardrail {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// corrected_alpha is always alpha/K.
            #[test]
            fn corrected_alpha_equals_alpha_over_k(
                alpha in 0.001f64..0.499f64,
                k in 1usize..=50,
            ) {
                let c = GuardrailCorrector::new(alpha, k);
                let expected = alpha / k as f64;
                prop_assert!((c.corrected_alpha() - expected).abs() < 1e-13,
                    "expected {expected}, got {}", c.corrected_alpha());
            }

            /// is_violated is consistent with corrected_alpha.
            #[test]
            fn is_violated_consistent_with_corrected_alpha(
                alpha in 0.001f64..0.499f64,
                k in 1usize..=50,
                p in 0.0f64..=1.0f64,
            ) {
                let c = GuardrailCorrector::new(alpha, k);
                prop_assert_eq!(c.is_violated(p), p <= c.corrected_alpha());
            }

            /// Larger K gives a stricter (smaller) threshold.
            #[test]
            fn more_guardrails_means_stricter_threshold(
                alpha in 0.001f64..0.499f64,
                k1 in 1usize..=49,
            ) {
                let k2 = k1 + 1;
                let c1 = GuardrailCorrector::new(alpha, k1);
                let c2 = GuardrailCorrector::new(alpha, k2);
                prop_assert!(c1.corrected_alpha() >= c2.corrected_alpha());
            }

            /// validate_bandit_reward_aggregation only accepts User.
            #[test]
            fn bandit_reward_only_user(
                // 0=Unspecified, 1=User, 2=Experiment, 3=Provider
                level_idx in 0usize..=3,
            ) {
                let level = match level_idx {
                    0 => MetricAggregationLevel::Unspecified,
                    1 => MetricAggregationLevel::User,
                    2 => MetricAggregationLevel::Experiment,
                    _ => MetricAggregationLevel::Provider,
                };
                let result = validate_bandit_reward_aggregation(level);
                if level == MetricAggregationLevel::User {
                    prop_assert!(result.is_ok());
                } else {
                    prop_assert!(result.is_err());
                }
            }

            /// validate_guardrail_aggregation accepts User and Experiment only.
            #[test]
            fn guardrail_user_or_experiment_ok(
                level_idx in 0usize..=3,
            ) {
                let level = match level_idx {
                    0 => MetricAggregationLevel::Unspecified,
                    1 => MetricAggregationLevel::User,
                    2 => MetricAggregationLevel::Experiment,
                    _ => MetricAggregationLevel::Provider,
                };
                let result = validate_guardrail_aggregation(level);
                match level {
                    MetricAggregationLevel::User | MetricAggregationLevel::Experiment => {
                        prop_assert!(result.is_ok());
                    }
                    _ => {
                        prop_assert!(result.is_err());
                    }
                }
            }
        }
    }
}
