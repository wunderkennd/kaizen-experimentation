//! Tests that Thompson Sampling converges to the optimal arm.

use experimentation_bandit::policy::Policy;
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

        // After 500 rounds, arm "a" should be selected most of the time
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
