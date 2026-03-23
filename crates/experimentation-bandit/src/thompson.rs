//! Thompson Sampling for binary and continuous rewards.
//!
//! When a [`crate::reward_composer::RewardComposer`] is attached to the policy
//! (via [`ThompsonSamplingPolicy::with_reward_composer`] or by constructing with
//! [`ThompsonSamplingPolicy::new_multi_objective`]), multi-objective rewards are
//! composed into a scalar before the Beta posterior is updated. The composer's
//! EMA normalizer state is persisted alongside the arm posteriors.

use crate::policy::Policy;
use crate::reward_composer::{CompositionMethod, MetricNormalizer, Objective, RewardComposer};
use crate::ArmSelection;
use experimentation_core::error::assert_finite;
use rand::Rng;
use rand_distr::{Beta, Distribution};
use std::collections::HashMap;

/// Beta-Bernoulli Thompson Sampling state for a single arm.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BetaArm {
    pub arm_id: String,
    pub alpha: f64, // successes + 1
    pub beta: f64,  // failures + 1
}

impl BetaArm {
    pub fn new(arm_id: String) -> Self {
        Self {
            arm_id,
            alpha: 1.0,
            beta: 1.0,
        } // Uniform prior
    }

    pub fn update(&mut self, reward: f64) {
        assert_finite(reward, "reward");
        assert!(
            (0.0..=1.0).contains(&reward),
            "binary reward must be in [0, 1]"
        );
        self.alpha += reward;
        self.beta += 1.0 - reward;
    }

    pub fn sample<R: Rng>(&self, rng: &mut R) -> f64 {
        let dist = Beta::new(self.alpha, self.beta).expect("valid Beta parameters");
        dist.sample(rng)
    }
}

/// Number of Monte Carlo simulations used to estimate assignment probabilities.
const MC_SIMULATIONS: u32 = 1000;

/// Select an arm via Thompson Sampling from a set of Beta arms.
///
/// Assignment probabilities are estimated via Monte Carlo simulation: we draw
/// `MC_SIMULATIONS` independent samples from each arm's Beta posterior and
/// count how often each arm has the highest draw. This gives the marginal
/// probability that each arm would be selected, which is the correct
/// propensity score for downstream inverse propensity weighting (IPW).
pub fn select_arm<R: Rng>(arms: &[BetaArm], rng: &mut R) -> ArmSelection {
    assert!(!arms.is_empty(), "must have at least one arm");

    // Single draw to decide which arm to select *now*.
    let samples: Vec<(usize, f64)> = arms
        .iter()
        .enumerate()
        .map(|(i, arm)| (i, arm.sample(rng)))
        .collect();

    let best_idx = samples
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap()
        .0;

    // Monte Carlo estimation of assignment probabilities.
    // For each simulation, draw from every arm's posterior and record the winner.
    let mut win_counts = vec![0u32; arms.len()];
    for _ in 0..MC_SIMULATIONS {
        let mut best_val = f64::NEG_INFINITY;
        let mut best = 0usize;
        for (i, arm) in arms.iter().enumerate() {
            let draw = arm.sample(rng);
            if draw > best_val {
                best_val = draw;
                best = i;
            }
        }
        win_counts[best] += 1;
    }

    // Guarantee the actually selected arm has at least 1 win so that
    // assignment_probability > 0 (required by downstream IPW: ipw.rs:84).
    if win_counts[best_idx] == 0 {
        win_counts[best_idx] = 1;
    }

    let total_wins: u32 = win_counts.iter().sum();
    let all_arm_probabilities: HashMap<String, f64> = arms
        .iter()
        .enumerate()
        .map(|(i, arm)| {
            (arm.arm_id.clone(), win_counts[i] as f64 / total_wins as f64)
        })
        .collect();

    let assignment_probability = all_arm_probabilities[&arms[best_idx].arm_id];

    ArmSelection {
        arm_id: arms[best_idx].arm_id.clone(),
        assignment_probability,
        all_arm_probabilities,
    }
}

/// Serializable state for ThompsonSamplingPolicy.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PolicyState {
    experiment_id: String,
    arms: Vec<BetaArm>,
    total_rewards: u64,
    /// Persisted alongside arm posteriors when multi-objective reward composition
    /// is configured. `None` for single-objective policies (ADR-011).
    reward_composer: Option<RewardComposer>,
}

/// Thompson Sampling policy implementation for the LMAX policy core.
///
/// Optionally embeds a [`RewardComposer`] for multi-objective reward
/// composition (ADR-011). When present, call [`Self::update_multi_objective`]
/// instead of [`Policy::update`] to pass per-metric raw values; the composer
/// scalarizes them before the Beta posterior update.
#[derive(Debug, Clone)]
pub struct ThompsonSamplingPolicy {
    experiment_id: String,
    arms: Vec<BetaArm>,
    total_rewards: u64,
    /// Optional multi-objective reward composer (ADR-011).
    /// Serialized to RocksDB alongside arm posteriors.
    reward_composer: Option<RewardComposer>,
}

impl ThompsonSamplingPolicy {
    /// Create a new single-objective policy with uniform priors.
    pub fn new(experiment_id: String, arm_ids: Vec<String>) -> Self {
        let arms = arm_ids.into_iter().map(BetaArm::new).collect();
        Self {
            experiment_id,
            arms,
            total_rewards: 0,
            reward_composer: None,
        }
    }

    /// Create a new multi-objective policy (ADR-011).
    ///
    /// The embedded [`RewardComposer`] will normalize and compose per-metric
    /// raw reward values on each call to [`Self::update_multi_objective`].
    pub fn new_multi_objective(
        experiment_id: String,
        arm_ids: Vec<String>,
        objectives: Vec<Objective>,
        method: CompositionMethod,
    ) -> Self {
        let arms = arm_ids.into_iter().map(BetaArm::new).collect();
        Self {
            experiment_id,
            arms,
            total_rewards: 0,
            reward_composer: Some(RewardComposer::new(objectives, method)),
        }
    }

    /// Attach or replace the reward composer on an existing policy.
    pub fn with_reward_composer(mut self, composer: RewardComposer) -> Self {
        self.reward_composer = Some(composer);
        self
    }

    /// Get experiment ID.
    pub fn experiment_id(&self) -> &str {
        &self.experiment_id
    }

    /// Returns a reference to the embedded reward composer, if any.
    pub fn reward_composer(&self) -> Option<&RewardComposer> {
        self.reward_composer.as_ref()
    }

    /// Update the posterior using multiple raw metric observations.
    ///
    /// The embedded [`RewardComposer`] normalizes and scalarizes `metric_values`
    /// into a single reward in [0, 1] (via sigmoid), then updates the Beta
    /// posterior for `arm_id`.
    ///
    /// # Panics
    /// - No reward composer is configured (call `new_multi_objective` first).
    /// - `arm_id` is unknown.
    /// - Any metric in the composer's objectives is absent from `metric_values`.
    pub fn update_multi_objective(
        &mut self,
        arm_id: &str,
        metric_values: &HashMap<String, f64>,
    ) {
        let composer = self.reward_composer.as_mut().expect(
            "update_multi_objective requires a reward_composer — use new_multi_objective()",
        );
        let scalar = composer.compose(metric_values);
        // Map scalar (unbounded z-score) to binary reward in [0, 1] via sigmoid.
        // This is required by the Beta–Bernoulli model which expects reward ∈ [0, 1].
        let binary_reward = 1.0 / (1.0 + (-scalar).exp());
        assert_finite(binary_reward, "sigmoid-mapped multi-objective reward");

        let arm = self
            .arms
            .iter_mut()
            .find(|a| a.arm_id == arm_id)
            .unwrap_or_else(|| panic!("unknown arm_id: {arm_id}"));
        arm.update(binary_reward);
        self.total_rewards += 1;
    }

    /// Returns `true` if this policy has multi-objective reward composition enabled.
    pub fn is_multi_objective(&self) -> bool {
        self.reward_composer.is_some()
    }

    /// Access the EMA normalizer's metric statistics directly (for diagnostics / UI).
    pub fn metric_normalizer(&self) -> Option<&MetricNormalizer> {
        self.reward_composer.as_ref().map(|c| &c.normalizer)
    }
}

impl Policy for ThompsonSamplingPolicy {
    fn select_arm(&self, _context: Option<&HashMap<String, f64>>) -> ArmSelection {
        let mut rng = rand::thread_rng();
        select_arm(&self.arms, &mut rng)
    }

    fn update(&mut self, arm_id: &str, reward: f64, _context: Option<&HashMap<String, f64>>) {
        let arm = self
            .arms
            .iter_mut()
            .find(|a| a.arm_id == arm_id)
            .unwrap_or_else(|| panic!("unknown arm_id: {arm_id}"));
        arm.update(reward);
        self.total_rewards += 1;
    }

    fn serialize(&self) -> Vec<u8> {
        let state = PolicyState {
            experiment_id: self.experiment_id.clone(),
            arms: self.arms.clone(),
            total_rewards: self.total_rewards,
            reward_composer: self.reward_composer.clone(),
        };
        serde_json::to_vec(&state).expect("policy state serialization should not fail")
    }

    fn deserialize(data: &[u8]) -> Self {
        let state: PolicyState =
            serde_json::from_slice(data).expect("policy state deserialization failed");
        Self {
            experiment_id: state.experiment_id,
            arms: state.arms,
            total_rewards: state.total_rewards,
            reward_composer: state.reward_composer,
        }
    }

    fn total_rewards(&self) -> u64 {
        self.total_rewards
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arm_update() {
        let mut arm = BetaArm::new("a".into());
        arm.update(1.0);
        assert_eq!(arm.alpha, 2.0);
        assert_eq!(arm.beta, 1.0);
    }

    #[test]
    fn test_select_arm() {
        let arms = vec![
            BetaArm {
                arm_id: "a".into(),
                alpha: 100.0,
                beta: 1.0,
            }, // strong
            BetaArm {
                arm_id: "b".into(),
                alpha: 1.0,
                beta: 100.0,
            }, // weak
        ];
        let mut rng = rand::thread_rng();
        let result = select_arm(&arms, &mut rng);
        // Arm A should win almost always with these parameters.
        assert_eq!(result.arm_id, "a");
        assert!(result.all_arm_probabilities.contains_key("a"));
        assert!(result.all_arm_probabilities.contains_key("b"));
    }

    #[test]
    fn test_policy_select_and_update() {
        let mut policy =
            ThompsonSamplingPolicy::new("exp-1".into(), vec!["a".into(), "b".into()]);
        assert_eq!(policy.total_rewards(), 0);

        let selection = policy.select_arm(None);
        assert!(selection.arm_id == "a" || selection.arm_id == "b");

        policy.update("a", 1.0, None);
        assert_eq!(policy.total_rewards(), 1);
    }

    #[test]
    fn test_policy_serialize_roundtrip() {
        let mut policy =
            ThompsonSamplingPolicy::new("exp-1".into(), vec!["a".into(), "b".into()]);
        policy.update("a", 1.0, None);
        policy.update("b", 0.0, None);

        let data = policy.serialize();
        let restored = ThompsonSamplingPolicy::deserialize(&data);

        assert_eq!(restored.experiment_id(), "exp-1");
        assert_eq!(restored.total_rewards(), 2);
    }
}
