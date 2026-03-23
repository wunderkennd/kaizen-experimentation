//! Multi-armed bandit algorithms.
//!
//! Implements Thompson Sampling, LinUCB, and Neural Contextual Bandits.
//! M4b (Policy Service) wraps this crate in the LMAX single-threaded core.
//!
//! # Architecture (from ADR-002)
//! All policy state mutations happen on a single dedicated thread.
//! This crate provides pure functions — no threads, no channels.
//! The threading model is the responsibility of experimentation-policy.

pub mod cold_start;
pub mod linucb;
#[cfg(feature = "gpu")]
pub mod neural;
pub mod policy;
pub mod reward_composer;
pub mod thompson;

use std::collections::HashMap;

/// Arm selection result returned to the caller.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArmSelection {
    pub arm_id: String,
    pub assignment_probability: f64,
    pub all_arm_probabilities: HashMap<String, f64>,
}
