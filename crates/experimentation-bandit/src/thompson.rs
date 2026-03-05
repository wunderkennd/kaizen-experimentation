//! Thompson Sampling for binary and continuous rewards.

use crate::policy::Policy;
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

/// Select an arm via Thompson Sampling from a set of Beta arms.
pub fn select_arm<R: Rng>(arms: &[BetaArm], rng: &mut R) -> ArmSelection {
    assert!(!arms.is_empty(), "must have at least one arm");

    let samples: Vec<(usize, f64)> = arms
        .iter()
        .enumerate()
        .map(|(i, arm)| (i, arm.sample(rng)))
        .collect();

    let total_sample: f64 = samples.iter().map(|(_, s)| s).sum();
    let best_idx = samples
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap()
        .0;

    let all_arm_probabilities: HashMap<String, f64> = arms
        .iter()
        .zip(samples.iter())
        .map(|(arm, (_, s))| (arm.arm_id.clone(), s / total_sample))
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
}

/// Thompson Sampling policy implementation for the LMAX policy core.
#[derive(Debug, Clone)]
pub struct ThompsonSamplingPolicy {
    experiment_id: String,
    arms: Vec<BetaArm>,
    total_rewards: u64,
}

impl ThompsonSamplingPolicy {
    /// Create a new policy with the given arm IDs and uniform priors.
    pub fn new(experiment_id: String, arm_ids: Vec<String>) -> Self {
        let arms = arm_ids.into_iter().map(BetaArm::new).collect();
        Self {
            experiment_id,
            arms,
            total_rewards: 0,
        }
    }

    /// Get experiment ID.
    pub fn experiment_id(&self) -> &str {
        &self.experiment_id
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
