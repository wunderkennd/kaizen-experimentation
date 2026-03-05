//! Bandit policy trait for all algorithm implementations.

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
