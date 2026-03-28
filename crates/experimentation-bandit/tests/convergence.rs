//! Tests that Thompson Sampling converges to the optimal arm.

use experimentation_bandit::policy::Policy;
use experimentation_bandit::reward_composer::{CompositionMethod, Objective};
use experimentation_bandit::thompson::ThompsonSamplingPolicy;

#[test]
fn converges_to_optimal_arm_within_1000_rounds() {
    // Arm "a" has true reward rate 0.8, arm "b" has 0.2
    let mut policy = ThompsonSamplingPolicy::new("convergence-test".into(), vec!["a".into(), "b".into()]);

    let mut rng = rand::thread_rng();
    let mut arm_a_selections = 0u64;

    for round in 0..1000 {
        let selection = policy.select_arm(None);

        // Simulate reward based on true rates
        let reward = if selection.arm_id == "a" {
            arm_a_selections += 1;
            if rand::Rng::gen::<f64>(&mut rng) < 0.8 { 1.0 } else { 0.0 }
        } else {
            if rand::Rng::gen::<f64>(&mut rng) < 0.2 { 1.0 } else { 0.0 }
        };

        policy.update(&selection.arm_id, reward, None);

        // After 1000 rounds, arm "a" should be selected most of the time
        if round == 999 {
            let selection_rate = arm_a_selections as f64 / 1000.0;
            assert!(
                selection_rate > 0.7,
                "Expected arm 'a' to be selected >70% of the time, got {:.1}%",
                selection_rate * 100.0
            );
        }
    }
}

#[test]
fn converges_three_arms() {
    // Arm "best" = 0.9, "mid" = 0.5, "worst" = 0.1
    let mut policy = ThompsonSamplingPolicy::new(
        "three-arm-test".into(),
        vec!["best".into(), "mid".into(), "worst".into()],
    );

    let mut rng = rand::thread_rng();
    let mut best_count = 0u64;

    let rates = std::collections::HashMap::from([
        ("best", 0.9),
        ("mid", 0.5),
        ("worst", 0.1),
    ]);

    for _ in 0..1000 {
        let selection = policy.select_arm(None);
        if selection.arm_id == "best" {
            best_count += 1;
        }

        let true_rate = rates[selection.arm_id.as_str()];
        let reward = if rand::Rng::gen::<f64>(&mut rng) < true_rate {
            1.0
        } else {
            0.0
        };
        policy.update(&selection.arm_id, reward, None);
    }

    // After 1000 rounds, "best" should dominate
    assert!(
        best_count > 700,
        "Expected 'best' arm selected >700 times in 1000 rounds, got {best_count}"
    );
}

/// Integration test: multi-objective weighted-sum bandit converges to optimal arm.
///
/// Arm "high" has ground-truth metrics (engagement=0.8, quality=0.7).
/// Arm "low" has ground-truth metrics (engagement=0.3, quality=0.4).
/// Weights: engagement=0.6, quality=0.4 → Arm "high" has the higher expected reward.
/// After 1000 rounds, "high" should be selected >60% of the time.
#[test]
fn multi_objective_weighted_sum_converges_to_optimal_arm() {
    use std::collections::HashMap;

    let objectives = vec![
        Objective {
            metric_id: "engagement".into(),
            weight: 0.6,
            floor: 0.0,
            is_primary: true,
        },
        Objective {
            metric_id: "quality".into(),
            weight: 0.4,
            floor: 0.0,
            is_primary: false,
        },
    ];

    let mut policy = ThompsonSamplingPolicy::new_multi_objective(
        "multi-obj-convergence".into(),
        vec!["high".into(), "low".into()],
        objectives,
        CompositionMethod::WeightedScalarization,
    );

    let mut rng = rand::thread_rng();

    // Ground-truth metric values per arm.
    let gt_high: HashMap<String, f64> = [("engagement".into(), 0.8), ("quality".into(), 0.7)]
        .into_iter()
        .collect();
    let gt_low: HashMap<String, f64> = [("engagement".into(), 0.3), ("quality".into(), 0.4)]
        .into_iter()
        .collect();

    let mut high_selections = 0u64;
    let rounds = 1000u64;

    for _ in 0..rounds {
        let selection = policy.select_arm(None);
        let is_high = selection.arm_id == "high";

        // Add noise to ground-truth metrics to simulate a real environment.
        let mut noise = || rand::Rng::gen_range(&mut rng, -0.1f64..0.1);
        let gt = if is_high { &gt_high } else { &gt_low };
        let noisy: HashMap<String, f64> = gt
            .iter()
            .map(|(k, v)| (k.clone(), (v + noise()).clamp(0.0, 1.0)))
            .collect();

        policy.update_multi_objective(&selection.arm_id, &noisy);

        if is_high {
            high_selections += 1;
        }
    }

    let high_rate = high_selections as f64 / rounds as f64;
    assert!(
        high_rate > 0.60,
        "multi-objective bandit should converge to optimal arm 'high' >60% after {rounds} rounds (got {:.1}%)",
        high_rate * 100.0
    );
}
