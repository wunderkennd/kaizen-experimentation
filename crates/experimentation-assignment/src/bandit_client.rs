//! Bandit arm selection client.
//!
//! Until M4b delivers the BanditPolicyService, this module provides a mock
//! implementation that selects arms uniformly at random with equal probability.
//! The real implementation will call M4b's `SelectArm` gRPC RPC.

use std::collections::HashMap;

use rand::Rng;

use crate::config::BanditConfig;

/// Result of selecting an arm from a bandit experiment.
#[derive(Debug, Clone)]
pub struct ArmSelection {
    /// The selected arm ID.
    pub arm_id: String,
    /// Assignment probability for this arm at selection time (for IPW logging).
    pub assignment_probability: f64,
    /// Payload JSON for the selected arm.
    pub payload_json: String,
    /// All arm probabilities at selection time.
    pub all_arm_probabilities: HashMap<String, f64>,
}

/// Select an arm using uniform random (mock for M4b).
///
/// Each arm gets equal probability `1/n`. The deterministic `rng` ensures
/// the same user + experiment always gets the same arm within a session.
pub fn select_arm_uniform<R: Rng>(
    bandit_config: &BanditConfig,
    rng: &mut R,
) -> Option<ArmSelection> {
    let n = bandit_config.arms.len();
    if n == 0 {
        return None;
    }

    let prob = 1.0 / n as f64;
    let idx = rng.gen_range(0..n);
    let arm = &bandit_config.arms[idx];

    let all_probs: HashMap<String, f64> = bandit_config
        .arms
        .iter()
        .map(|a| (a.arm_id.clone(), prob))
        .collect();

    Some(ArmSelection {
        arm_id: arm.arm_id.clone(),
        assignment_probability: prob,
        payload_json: arm.payload_json.clone(),
        all_arm_probabilities: all_probs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BanditArmConfig, BanditConfig};
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn make_bandit_config(n_arms: usize) -> BanditConfig {
        let arms: Vec<BanditArmConfig> = (0..n_arms)
            .map(|i| BanditArmConfig {
                arm_id: format!("arm_{i}"),
                name: format!("Arm {i}"),
                payload_json: format!(r#"{{"arm":{i}}}"#),
            })
            .collect();

        BanditConfig {
            algorithm: "THOMPSON_SAMPLING".to_string(),
            arms,
            reward_metric_id: "clicks".to_string(),
            context_feature_keys: vec![],
            min_exploration_fraction: 0.1,
            warmup_observations: 1000,
        }
    }

    #[test]
    fn test_uniform_selection_deterministic() {
        let config = make_bandit_config(3);
        let mut rng1 = StdRng::seed_from_u64(42);
        let mut rng2 = StdRng::seed_from_u64(42);

        let sel1 = select_arm_uniform(&config, &mut rng1).unwrap();
        let sel2 = select_arm_uniform(&config, &mut rng2).unwrap();

        assert_eq!(sel1.arm_id, sel2.arm_id);
        assert!((sel1.assignment_probability - sel2.assignment_probability).abs() < f64::EPSILON);
    }

    #[test]
    fn test_uniform_probability() {
        let config = make_bandit_config(4);
        let mut rng = StdRng::seed_from_u64(0);

        let sel = select_arm_uniform(&config, &mut rng).unwrap();
        assert!((sel.assignment_probability - 0.25).abs() < f64::EPSILON);
        assert_eq!(sel.all_arm_probabilities.len(), 4);
        for prob in sel.all_arm_probabilities.values() {
            assert!((*prob - 0.25).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_uniform_balance() {
        let config = make_bandit_config(3);
        let mut counts = HashMap::new();

        for seed in 0..3000u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let sel = select_arm_uniform(&config, &mut rng).unwrap();
            *counts.entry(sel.arm_id).or_insert(0u64) += 1;
        }

        // Each arm should get ~1000 ± 100 out of 3000 trials.
        for (arm, count) in &counts {
            let frac = *count as f64 / 3000.0;
            assert!(
                (0.28..=0.39).contains(&frac),
                "arm {arm} fraction {frac:.3} outside [0.28, 0.39]"
            );
        }
    }

    #[test]
    fn test_empty_arms_returns_none() {
        let config = make_bandit_config(0);
        let mut rng = StdRng::seed_from_u64(0);
        assert!(select_arm_uniform(&config, &mut rng).is_none());
    }

    #[test]
    fn test_single_arm() {
        let config = make_bandit_config(1);
        let mut rng = StdRng::seed_from_u64(0);
        let sel = select_arm_uniform(&config, &mut rng).unwrap();
        assert_eq!(sel.arm_id, "arm_0");
        assert!((sel.assignment_probability - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_payload_propagated() {
        let config = make_bandit_config(2);
        let mut rng = StdRng::seed_from_u64(0);
        let sel = select_arm_uniform(&config, &mut rng).unwrap();
        assert!(!sel.payload_json.is_empty());
    }
}
