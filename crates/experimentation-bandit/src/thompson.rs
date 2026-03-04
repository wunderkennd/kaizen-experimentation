//! Thompson Sampling for binary and continuous rewards.

use experimentation_core::error::{assert_finite, Result};
use rand::Rng;
use rand_distr::{Beta, Distribution, Normal};

/// Beta-Bernoulli Thompson Sampling state for a single arm.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BetaArm {
    pub arm_id: String,
    pub alpha: f64,  // successes + 1
    pub beta: f64,   // failures + 1
}

impl BetaArm {
    pub fn new(arm_id: String) -> Self {
        Self { arm_id, alpha: 1.0, beta: 1.0 }  // Uniform prior
    }

    pub fn update(&mut self, reward: f64) {
        assert_finite(reward, "reward");
        assert!(reward >= 0.0 && reward <= 1.0, "binary reward must be in [0, 1]");
        self.alpha += reward;
        self.beta += 1.0 - reward;
    }

    pub fn sample<R: Rng>(&self, rng: &mut R) -> f64 {
        let dist = Beta::new(self.alpha, self.beta).expect("valid Beta parameters");
        dist.sample(rng)
    }
}

/// Select an arm via Thompson Sampling from a set of Beta arms.
pub fn select_arm<R: Rng>(arms: &[BetaArm], rng: &mut R) -> super::ArmSelection {
    assert!(!arms.is_empty(), "must have at least one arm");

    let samples: Vec<(usize, f64)> = arms.iter()
        .enumerate()
        .map(|(i, arm)| (i, arm.sample(rng)))
        .collect();

    let total_sample: f64 = samples.iter().map(|(_, s)| s).sum();
    let best_idx = samples.iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap()
        .0;

    let all_probs: Vec<(String, f64)> = arms.iter()
        .zip(samples.iter())
        .map(|(arm, (_, s))| (arm.arm_id.clone(), s / total_sample))
        .collect();

    super::ArmSelection {
        arm_id: arms[best_idx].arm_id.clone(),
        assignment_probability: all_probs[best_idx].1,
        all_probabilities: all_probs,
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
            BetaArm { arm_id: "a".into(), alpha: 100.0, beta: 1.0 },  // strong
            BetaArm { arm_id: "b".into(), alpha: 1.0, beta: 100.0 },  // weak
        ];
        let mut rng = rand::thread_rng();
        let result = select_arm(&arms, &mut rng);
        // Arm A should win almost always with these parameters.
        assert_eq!(result.arm_id, "a");
    }
}
