//! Mixture Adaptive Design (MAD) e-processes for bandit experiments (ADR-018 Phase 3).
//!
//! # Problem
//!
//! Standard e-values assume iid data, but bandit algorithms produce
//! adaptively-collected observations where treatment assignments depend on
//! past rewards. This violates the exchangeability assumption required by
//! e-value constructions like GROW and AVLM.
//!
//! # Solution: MAD Mixing (Liang & Bojinov, HBS 2024)
//!
//! With probability ε, assign uniformly at random across all arms instead of
//! following the bandit policy. The ε-fraction observations are exchangeable
//! and form a valid basis for e-process (supermartingale) computation.
//!
//! The remaining (1 − ε) fraction follows the bandit policy normally, so
//! adaptive learning continues with minimal regret overhead.
//!
//! # E-Process Construction
//!
//! Given the uniform-component observations {X_i : is_uniform_random = true},
//! the MAD e-process is a product martingale:
//!
//! ```text
//! E_n = ∏_{t ∈ U} K_t(X_t)
//! ```
//!
//! where U = {t : is_uniform_random_t = true} and K_t is the GROW betting
//! kernel. The key property: since each K_t uses only the uniform-component
//! history, the product is a valid e-process (nonneg supermartingale with
//! E[E_n] ≤ 1 under H0) regardless of the bandit's adaptive policy.
//!
//! # Usage
//!
//! 1. Configure `BanditConfig.mad_randomization_fraction = ε` (e.g., 0.1).
//! 2. On each `SelectArm` call, [`MadMixer::select_arm`] flips a coin:
//!    - With probability ε: uniform random arm, `is_uniform_random = true`.
//!    - With probability 1 − ε: delegate to bandit policy, `is_uniform_random = false`.
//! 3. After collecting observations, call [`MadEProcess::update`] with each
//!    reward, and query [`MadEProcess::result`] for the current e-value.
//!
//! # References
//!
//! Liang & Bojinov (2024) "Mixture Adaptive Design for Experimentation
//!   with Bandits", Harvard Business School Working Paper.
//! Ramdas & Wang (2024) "Hypothesis Testing with E-values" (monograph).

use crate::policy::AnyPolicy;
use crate::ArmSelection;
use experimentation_core::error::{assert_finite, Error, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// MAD Mixer — wraps a bandit policy with ε-fraction uniform randomization
// ---------------------------------------------------------------------------

/// Configuration for MAD mixing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MadConfig {
    /// Fraction of traffic allocated to uniform randomization (ε).
    /// Must be in (0, 1). Typical values: 0.05–0.20.
    pub randomization_fraction: f64,
    /// Arm IDs available for uniform randomization.
    pub arm_ids: Vec<String>,
}

impl MadConfig {
    /// Validate the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.randomization_fraction <= 0.0 || self.randomization_fraction >= 1.0 {
            return Err(Error::Validation(
                "mad_randomization_fraction must be in (0, 1)".into(),
            ));
        }
        if self.arm_ids.is_empty() {
            return Err(Error::Validation(
                "arm_ids must have at least one arm for MAD mixing".into(),
            ));
        }
        Ok(())
    }
}

/// Wraps a bandit policy with MAD (ε-uniform) mixing.
///
/// On each `select_arm` call, with probability ε a uniform-random arm is
/// returned (with `is_uniform_random = true`); otherwise the inner policy
/// decides (with `is_uniform_random = false`).
pub struct MadMixer {
    config: MadConfig,
}

impl MadMixer {
    /// Create a new MAD mixer.
    ///
    /// # Errors
    /// Returns `Error::Validation` if the config is invalid.
    pub fn new(config: MadConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Select an arm with MAD mixing.
    ///
    /// With probability ε, returns a uniform-random arm with
    /// `is_uniform_random = true`. Otherwise delegates to the inner policy.
    pub fn select_arm<R: Rng>(
        &self,
        policy: &AnyPolicy,
        context: Option<&HashMap<String, f64>>,
        rng: &mut R,
    ) -> ArmSelection {
        let coin: f64 = rng.gen();

        if coin < self.config.randomization_fraction {
            // Uniform component: select uniformly at random.
            self.select_uniform(rng)
        } else {
            // Bandit component: delegate to policy.
            let mut selection = policy.select_arm(context);
            selection.is_uniform_random = false;
            selection
        }
    }

    /// Select an arm uniformly at random.
    fn select_uniform<R: Rng>(&self, rng: &mut R) -> ArmSelection {
        let n = self.config.arm_ids.len();
        let prob = 1.0 / n as f64;
        let idx = rng.gen_range(0..n);

        let all_arm_probabilities: HashMap<String, f64> = self
            .config
            .arm_ids
            .iter()
            .map(|id| (id.clone(), prob))
            .collect();

        ArmSelection {
            arm_id: self.config.arm_ids[idx].clone(),
            assignment_probability: prob,
            all_arm_probabilities,
            is_uniform_random: true,
        }
    }

    /// Returns the configured randomization fraction (ε).
    pub fn randomization_fraction(&self) -> f64 {
        self.config.randomization_fraction
    }

    /// Returns the arm IDs available for randomization.
    pub fn arm_ids(&self) -> &[String] {
        &self.config.arm_ids
    }
}

// ---------------------------------------------------------------------------
// MAD E-Process — sequential e-value from uniform-component observations
// ---------------------------------------------------------------------------

/// Result of a MAD e-process computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MadEProcessResult {
    /// Current e-value (product of per-observation betting kernels).
    /// E[E_n] <= 1 under H0.
    pub e_value: f64,
    /// log(e_value) for numerical stability.
    pub log_e_value: f64,
    /// Whether to reject H0 at the configured alpha: E_n > 1/alpha.
    pub reject: bool,
    /// Number of uniform-component observations processed.
    pub n_uniform: u64,
    /// Total observations seen (uniform + bandit).
    pub n_total: u64,
    /// Log-wealth at each uniform-component observation.
    pub log_wealth_trajectory: Vec<f64>,
}

/// MAD e-process: sequential e-value built from uniform-component observations.
///
/// Uses the GROW martingale betting strategy restricted to observations where
/// `is_uniform_random = true`. The bet at each step uses only the history of
/// uniform-component observations, ensuring the product is a valid e-process.
///
/// The e-process tests H0: μ_treatment − μ_control = 0 using per-arm
/// reward differences from the uniform randomization subset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MadEProcess {
    /// Known or estimated variance for the GROW betting strategy.
    sigma_sq: f64,
    /// Rejection threshold α (reject when E > 1/α).
    alpha: f64,
    /// Running sum of uniform-component reward differences.
    uniform_sum: f64,
    /// Count of uniform-component observations.
    n_uniform: u64,
    /// Total observations seen.
    n_total: u64,
    /// Current log-wealth (log of the e-value).
    log_wealth: f64,
    /// Log-wealth trajectory for the uniform component.
    log_wealth_trajectory: Vec<f64>,
}

impl MadEProcess {
    /// Create a new MAD e-process.
    ///
    /// # Arguments
    /// * `sigma_sq` — Known or pre-estimated variance σ² > 0 for the GROW
    ///   betting strategy. Can use the pooled sample variance from a pilot or
    ///   the warmup period.
    /// * `alpha` — Rejection threshold: reject when E_n > 1/α. Must be in (0, 1).
    ///
    /// # Errors
    /// Returns `Error::Validation` for invalid parameters.
    pub fn new(sigma_sq: f64, alpha: f64) -> Result<Self> {
        if sigma_sq <= 0.0 {
            return Err(Error::Validation("sigma_sq must be positive".into()));
        }
        if alpha <= 0.0 || alpha >= 1.0 {
            return Err(Error::Validation("alpha must be in (0, 1)".into()));
        }

        Ok(Self {
            sigma_sq,
            alpha,
            uniform_sum: 0.0,
            n_uniform: 0,
            n_total: 0,
            log_wealth: 0.0,
            log_wealth_trajectory: Vec::new(),
        })
    }

    /// Update the e-process with a new observation.
    ///
    /// # Arguments
    /// * `reward_diff` — Observed reward difference (treatment − control) or
    ///   per-arm centered reward for the selected arm.
    /// * `is_uniform` — Whether this observation came from the uniform component.
    ///
    /// Only uniform-component observations contribute to the e-process.
    /// Non-uniform observations are counted but do not affect the e-value.
    pub fn update(&mut self, reward_diff: f64, is_uniform: bool) {
        assert_finite(reward_diff, "reward_diff");
        self.n_total += 1;

        if !is_uniform {
            return;
        }

        // GROW causal plug-in bet: λ_t = μ̂_{t-1} / σ²
        // At first uniform observation: μ̂_0 = 0 → λ_1 = 0 (safe start).
        let mu_hat_prev = if self.n_uniform == 0 {
            0.0
        } else {
            self.uniform_sum / self.n_uniform as f64
        };
        let lambda_t = mu_hat_prev / self.sigma_sq;
        assert_finite(lambda_t, "lambda_t");

        // Log-increment: log K_t = λ_t · X_t − λ_t² · σ² / 2
        let log_increment = lambda_t * reward_diff - 0.5 * lambda_t * lambda_t * self.sigma_sq;
        assert_finite(log_increment, "log_increment");

        self.log_wealth += log_increment;
        assert_finite(self.log_wealth, "log_wealth");

        self.log_wealth_trajectory.push(self.log_wealth);
        self.uniform_sum += reward_diff;
        self.n_uniform += 1;
    }

    /// Get the current e-process result.
    pub fn result(&self) -> MadEProcessResult {
        let e_value = if self.log_wealth > 709.78 {
            f64::MAX
        } else {
            let ev = self.log_wealth.exp();
            assert_finite(ev, "e_value");
            ev
        };

        MadEProcessResult {
            e_value,
            log_e_value: self.log_wealth,
            reject: e_value > 1.0 / self.alpha,
            n_uniform: self.n_uniform,
            n_total: self.n_total,
            log_wealth_trajectory: self.log_wealth_trajectory.clone(),
        }
    }

    /// Returns the number of uniform-component observations processed.
    pub fn n_uniform(&self) -> u64 {
        self.n_uniform
    }

    /// Returns the total number of observations seen.
    pub fn n_total(&self) -> u64 {
        self.n_total
    }

    /// Returns the current log e-value.
    pub fn log_e_value(&self) -> f64 {
        self.log_wealth
    }

    /// Serialize the e-process state for persistence (RocksDB snapshot).
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("MadEProcess serialization should not fail")
    }

    /// Deserialize e-process state from a snapshot.
    pub fn from_bytes(data: &[u8]) -> Self {
        serde_json::from_slice(data).expect("MadEProcess deserialization failed")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thompson::ThompsonSamplingPolicy;
    use crate::policy::AnyPolicy;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[cfg(test)]
    use proptest::prelude::*;

    // --- MadConfig validation -------------------------------------------------

    #[test]
    fn test_config_validation_rejects_zero_epsilon() {
        let config = MadConfig {
            randomization_fraction: 0.0,
            arm_ids: vec!["a".into()],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_rejects_one_epsilon() {
        let config = MadConfig {
            randomization_fraction: 1.0,
            arm_ids: vec!["a".into()],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_rejects_empty_arms() {
        let config = MadConfig {
            randomization_fraction: 0.1,
            arm_ids: vec![],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_accepts_valid() {
        let config = MadConfig {
            randomization_fraction: 0.1,
            arm_ids: vec!["a".into(), "b".into()],
        };
        assert!(config.validate().is_ok());
    }

    // --- MadMixer -------------------------------------------------------------

    #[test]
    fn test_mixer_uniform_fraction_approximately_correct() {
        let config = MadConfig {
            randomization_fraction: 0.2,
            arm_ids: vec!["a".into(), "b".into()],
        };
        let mixer = MadMixer::new(config).unwrap();
        let policy = AnyPolicy::Thompson(ThompsonSamplingPolicy::new(
            "test".into(),
            vec!["a".into(), "b".into()],
        ));
        let mut rng = StdRng::seed_from_u64(42);

        let n = 10_000;
        let n_uniform: usize = (0..n)
            .map(|_| mixer.select_arm(&policy, None, &mut rng))
            .filter(|s| s.is_uniform_random)
            .count();

        let fraction = n_uniform as f64 / n as f64;
        // Should be approximately 0.2 (within 3σ ≈ 0.012 for n=10000)
        assert!(
            (fraction - 0.2).abs() < 0.05,
            "uniform fraction={fraction}, expected ~0.2"
        );
    }

    #[test]
    fn test_mixer_uniform_arm_probabilities_equal() {
        let config = MadConfig {
            randomization_fraction: 0.999,
            arm_ids: vec!["a".into(), "b".into(), "c".into()],
        };
        let mixer = MadMixer::new(config).unwrap();
        let policy = AnyPolicy::Thompson(ThompsonSamplingPolicy::new(
            "test".into(),
            vec!["a".into(), "b".into(), "c".into()],
        ));
        let mut rng = StdRng::seed_from_u64(7);

        // With ε ≈ 1, almost all selections should be uniform
        let selection = mixer.select_arm(&policy, None, &mut rng);
        if selection.is_uniform_random {
            // Each arm should have probability 1/3
            for prob in selection.all_arm_probabilities.values() {
                assert!(
                    (*prob - 1.0 / 3.0).abs() < 1e-10,
                    "prob={prob}, expected 1/3"
                );
            }
        }
    }

    #[test]
    fn test_mixer_bandit_component_not_uniform() {
        let config = MadConfig {
            randomization_fraction: f64::MIN_POSITIVE, // essentially never uniform
            arm_ids: vec!["a".into(), "b".into()],
        };
        let mixer = MadMixer::new(config).unwrap();
        let policy = AnyPolicy::Thompson(ThompsonSamplingPolicy::new(
            "test".into(),
            vec!["a".into(), "b".into()],
        ));
        let mut rng = StdRng::seed_from_u64(42);

        let selection = mixer.select_arm(&policy, None, &mut rng);
        assert!(!selection.is_uniform_random);
    }

    // --- MadEProcess ----------------------------------------------------------

    #[test]
    fn test_eprocess_new_validation() {
        assert!(MadEProcess::new(0.0, 0.05).is_err(), "sigma_sq=0");
        assert!(MadEProcess::new(-1.0, 0.05).is_err(), "sigma_sq<0");
        assert!(MadEProcess::new(1.0, 0.0).is_err(), "alpha=0");
        assert!(MadEProcess::new(1.0, 1.0).is_err(), "alpha=1");
        assert!(MadEProcess::new(1.0, 0.05).is_ok(), "valid");
    }

    #[test]
    fn test_eprocess_safe_start() {
        // First uniform observation should not change e-value (λ_1 = 0).
        let mut ep = MadEProcess::new(1.0, 0.05).unwrap();
        ep.update(99.0, true);
        let r = ep.result();
        assert!((r.e_value - 1.0).abs() < 1e-12, "e_value={}", r.e_value);
        assert_eq!(r.n_uniform, 1);
        assert_eq!(r.n_total, 1);
    }

    #[test]
    fn test_eprocess_non_uniform_ignored() {
        let mut ep = MadEProcess::new(1.0, 0.05).unwrap();
        // Feed 10 non-uniform observations — should not affect e-value.
        for _ in 0..10 {
            ep.update(5.0, false);
        }
        let r = ep.result();
        assert!((r.e_value - 1.0).abs() < 1e-12);
        assert_eq!(r.n_uniform, 0);
        assert_eq!(r.n_total, 10);
        assert!(r.log_wealth_trajectory.is_empty());
    }

    #[test]
    fn test_eprocess_matches_grow_on_uniform_only() {
        // If all observations are uniform, the MAD e-process should produce the
        // same result as e_value_grow from experimentation-stats.
        let obs = vec![1.0, 1.0, 1.0];
        let sigma_sq = 1.0;
        let alpha = 0.05;

        let mut ep = MadEProcess::new(sigma_sq, alpha).unwrap();
        for &x in &obs {
            ep.update(x, true);
        }

        let grow_result =
            experimentation_stats::evalue::e_value_grow(&obs, sigma_sq, alpha).unwrap();
        let mad_result = ep.result();

        assert!(
            (mad_result.log_e_value - grow_result.log_e_value).abs() < 1e-12,
            "MAD log_e={} vs GROW log_e={}",
            mad_result.log_e_value,
            grow_result.log_e_value
        );
        assert_eq!(mad_result.reject, grow_result.reject);
    }

    #[test]
    fn test_eprocess_interleaved_uniform_and_bandit() {
        // Interleave uniform and bandit observations.
        // Only uniform ones should contribute to the e-process.
        let mut ep = MadEProcess::new(1.0, 0.05).unwrap();

        // Pattern: U, B, U, B, U (U=uniform, B=bandit)
        ep.update(1.0, true);  // U: safe start, no effect
        ep.update(5.0, false); // B: ignored
        ep.update(1.0, true);  // U: λ = 1.0/1.0 = 1.0
        ep.update(5.0, false); // B: ignored
        ep.update(1.0, true);  // U: λ = 2.0/2.0/1.0 = 1.0

        let r = ep.result();
        assert_eq!(r.n_uniform, 3);
        assert_eq!(r.n_total, 5);

        // Should match GROW on [1.0, 1.0, 1.0] since bandit obs are filtered out.
        let grow =
            experimentation_stats::evalue::e_value_grow(&[1.0, 1.0, 1.0], 1.0, 0.05).unwrap();
        assert!(
            (r.log_e_value - grow.log_e_value).abs() < 1e-12,
            "interleaved MAD log_e={} vs GROW log_e={}",
            r.log_e_value,
            grow.log_e_value
        );
    }

    #[test]
    fn test_eprocess_strong_effect_rejects() {
        let mut ep = MadEProcess::new(1.0, 0.05).unwrap();
        // Strong positive signal from uniform component.
        for _ in 0..20 {
            ep.update(2.0, true);
        }
        let r = ep.result();
        assert!(r.reject, "e_value={}", r.e_value);
        assert!(r.e_value > 20.0);
    }

    #[test]
    fn test_eprocess_null_effect_no_reject() {
        let mut ep = MadEProcess::new(1.0, 0.05).unwrap();
        // Zero effect: e-value should stay at 1.
        for _ in 0..10 {
            ep.update(0.0, true);
        }
        let r = ep.result();
        assert!((r.e_value - 1.0).abs() < 1e-12);
        assert!(!r.reject);
    }

    #[test]
    fn test_eprocess_serialize_roundtrip() {
        let mut ep = MadEProcess::new(1.0, 0.05).unwrap();
        ep.update(1.0, true);
        ep.update(0.5, false);
        ep.update(2.0, true);

        let bytes = ep.to_bytes();
        let restored = MadEProcess::from_bytes(&bytes);

        assert_eq!(restored.n_uniform(), ep.n_uniform());
        assert_eq!(restored.n_total(), ep.n_total());
        assert!(
            (restored.log_e_value() - ep.log_e_value()).abs() < 1e-15,
            "restored={} original={}",
            restored.log_e_value(),
            ep.log_e_value()
        );
    }

    // --- proptest invariants -------------------------------------------------

    proptest! {
        /// MAD e-process always produces finite, nonneg e-values.
        #[test]
        fn eprocess_outputs_always_finite(
            obs in proptest::collection::vec(-5.0f64..5.0, 1..30),
            sigma_sq in 0.1f64..10.0,
            // Randomly assign each observation as uniform or bandit
            uniform_flags in proptest::collection::vec(proptest::bool::ANY, 1..30),
        ) {
            let mut ep = MadEProcess::new(sigma_sq, 0.05).unwrap();
            let n = obs.len().min(uniform_flags.len());
            for i in 0..n {
                ep.update(obs[i], uniform_flags[i]);
            }
            let r = ep.result();
            prop_assert!(r.e_value.is_finite(), "e_value not finite: {}", r.e_value);
            prop_assert!(r.e_value >= 0.0, "e_value negative: {}", r.e_value);
            prop_assert!(r.log_e_value.is_finite(), "log_e_value not finite");
            prop_assert_eq!(r.log_wealth_trajectory.len(), r.n_uniform as usize);
            prop_assert!(r.n_uniform <= r.n_total);
        }

        /// MAD e-process reject is consistent with threshold.
        #[test]
        fn eprocess_reject_consistent(
            obs in proptest::collection::vec(-3.0f64..3.0, 1..20),
            sigma_sq in 0.5f64..5.0,
            alpha in 0.01f64..0.5,
            uniform_flags in proptest::collection::vec(proptest::bool::ANY, 1..20),
        ) {
            let mut ep = MadEProcess::new(sigma_sq, alpha).unwrap();
            let n = obs.len().min(uniform_flags.len());
            for i in 0..n {
                ep.update(obs[i], uniform_flags[i]);
            }
            let r = ep.result();
            let threshold = 1.0 / alpha;
            if r.reject {
                prop_assert!(r.e_value > threshold,
                    "reject=true but e_value={} <= threshold={}", r.e_value, threshold);
            } else {
                prop_assert!(r.e_value <= threshold,
                    "reject=false but e_value={} > threshold={}", r.e_value, threshold);
            }
        }

        /// Non-uniform observations never change the e-value.
        #[test]
        fn non_uniform_does_not_affect_evalue(
            bandit_obs in proptest::collection::vec(-5.0f64..5.0, 1..20),
            sigma_sq in 0.1f64..10.0,
        ) {
            let mut ep = MadEProcess::new(sigma_sq, 0.05).unwrap();
            for &x in &bandit_obs {
                ep.update(x, false);
            }
            let r = ep.result();
            prop_assert!((r.e_value - 1.0).abs() < 1e-12,
                "non-uniform obs changed e_value to {}", r.e_value);
            prop_assert_eq!(r.n_uniform, 0);
            prop_assert_eq!(r.n_total, bandit_obs.len() as u64);
        }

        /// MAD mixer produces uniform fraction within expected range.
        #[test]
        fn mixer_fraction_within_range(
            epsilon in 0.05f64..0.5,
            seed in 0u64..10,
        ) {
            let config = MadConfig {
                randomization_fraction: epsilon,
                arm_ids: vec!["a".into(), "b".into()],
            };
            let mixer = MadMixer::new(config).unwrap();
            let policy = AnyPolicy::Thompson(ThompsonSamplingPolicy::new(
                "test".into(),
                vec!["a".into(), "b".into()],
            ));
            let mut rng = StdRng::seed_from_u64(seed);

            let n = 500;
            let n_uniform: usize = (0..n)
                .map(|_| mixer.select_arm(&policy, None, &mut rng))
                .filter(|s| s.is_uniform_random)
                .count();

            let fraction = n_uniform as f64 / n as f64;
            // Allow 5 standard deviations: σ = sqrt(ε(1-ε)/n)
            let sigma = (epsilon * (1.0 - epsilon) / n as f64).sqrt();
            let tolerance = 5.0 * sigma;
            prop_assert!(
                (fraction - epsilon).abs() < tolerance,
                "fraction={fraction} expected ~{epsilon} (tolerance={tolerance})"
            );
        }
    }
}
