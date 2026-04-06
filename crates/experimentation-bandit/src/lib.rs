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
pub mod lp_constraints;
pub mod mad;
#[cfg(feature = "gpu")]
pub mod neural;
pub mod policy;
pub mod reward_composer;
pub mod slate;
pub mod thompson;

use std::collections::HashMap;

/// Arm selection result returned to the caller.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArmSelection {
    pub arm_id: String,
    pub assignment_probability: f64,
    pub all_arm_probabilities: HashMap<String, f64>,
    /// Whether this observation was drawn from the uniform random component
    /// of MAD mixing (ADR-018 Phase 3). When true, this observation is valid
    /// for e-process computation in M4a.
    pub is_uniform_random: bool,
}
