//! Tests that ThompsonSamplingPolicy survives JSON serialize/deserialize roundtrip.

use experimentation_bandit::policy::Policy;
use experimentation_bandit::thompson::ThompsonSamplingPolicy;

#[test]
fn roundtrip_preserves_state() {
    let mut policy = ThompsonSamplingPolicy::new("exp-1".into(), vec!["a".into(), "b".into(), "c".into()]);

    // Apply some updates
    for _ in 0..50 {
        policy.update("a", 1.0, None);
    }
    for _ in 0..10 {
        policy.update("b", 0.0, None);
    }
    for _ in 0..30 {
        policy.update("c", 1.0, None);
    }

    let serialized = policy.serialize();
    let restored = ThompsonSamplingPolicy::deserialize(&serialized);

    assert_eq!(restored.experiment_id(), "exp-1");
    assert_eq!(restored.total_rewards(), 90);

    // Verify that arm selection still works after roundtrip
    let selection = restored.select_arm(None);
    assert!(
        selection.arm_id == "a" || selection.arm_id == "b" || selection.arm_id == "c",
        "selection should return a valid arm"
    );
}

#[test]
fn roundtrip_with_no_updates() {
    let policy = ThompsonSamplingPolicy::new("fresh".into(), vec!["x".into(), "y".into()]);
    let serialized = policy.serialize();
    let restored = ThompsonSamplingPolicy::deserialize(&serialized);

    assert_eq!(restored.experiment_id(), "fresh");
    assert_eq!(restored.total_rewards(), 0);
}
