//! Bandit policy trait and multi-algorithm dispatch enum.

use crate::linucb::LinUcbPolicy;
use crate::thompson::ThompsonSamplingPolicy;
use crate::ArmSelection;
use std::collections::HashMap;

/// Trait implemented by all bandit algorithms.
pub trait Policy: Send {
    /// Select an arm given optional context features.
    fn select_arm(&self, context: Option<&HashMap<String, f64>>) -> ArmSelection;
    /// Update policy with an observed reward.
    fn update(&mut self, arm_id: &str, reward: f64, context: Option<&HashMap<String, f64>>);
    /// Serialize policy state for RocksDB snapshot.
    fn serialize(&self) -> Vec<u8>;
    /// Deserialize policy state from RocksDB snapshot.
    fn deserialize(data: &[u8]) -> Self
    where
        Self: Sized;
    /// Number of rewards processed.
    fn total_rewards(&self) -> u64;
}

/// Type-erased policy enum for multi-algorithm dispatch.
///
/// The `Policy` trait is not object-safe (`deserialize` has `Self: Sized` bound),
/// so we use this enum instead of `dyn Policy`.
#[derive(Debug, Clone)]
pub enum AnyPolicy {
    Thompson(ThompsonSamplingPolicy),
    LinUcb(LinUcbPolicy),
}

impl AnyPolicy {
    /// Returns a string identifier for the algorithm type (used in snapshot envelopes).
    pub fn policy_type(&self) -> &str {
        match self {
            AnyPolicy::Thompson(_) => "thompson_sampling",
            AnyPolicy::LinUcb(_) => "linucb",
        }
    }

    /// Deserialize a policy from snapshot bytes, dispatching on the policy type string.
    ///
    /// # Panics
    /// Panics on unknown policy type.
    pub fn deserialize(policy_type: &str, data: &[u8]) -> Self {
        match policy_type {
            "thompson_sampling" => {
                AnyPolicy::Thompson(ThompsonSamplingPolicy::deserialize(data))
            }
            "linucb" => AnyPolicy::LinUcb(LinUcbPolicy::deserialize(data)),
            other => panic!("unknown policy type: {other}"),
        }
    }

    /// Select an arm, delegating to the concrete policy.
    pub fn select_arm(&self, context: Option<&HashMap<String, f64>>) -> ArmSelection {
        match self {
            AnyPolicy::Thompson(p) => p.select_arm(context),
            AnyPolicy::LinUcb(p) => p.select_arm(context),
        }
    }

    /// Update with a reward, delegating to the concrete policy.
    pub fn update(&mut self, arm_id: &str, reward: f64, context: Option<&HashMap<String, f64>>) {
        match self {
            AnyPolicy::Thompson(p) => p.update(arm_id, reward, context),
            AnyPolicy::LinUcb(p) => p.update(arm_id, reward, context),
        }
    }

    /// Serialize policy state for snapshots.
    pub fn serialize(&self) -> Vec<u8> {
        match self {
            AnyPolicy::Thompson(p) => p.serialize(),
            AnyPolicy::LinUcb(p) => p.serialize(),
        }
    }

    /// Total rewards processed.
    pub fn total_rewards(&self) -> u64 {
        match self {
            AnyPolicy::Thompson(p) => p.total_rewards(),
            AnyPolicy::LinUcb(p) => p.total_rewards(),
        }
    }
}
