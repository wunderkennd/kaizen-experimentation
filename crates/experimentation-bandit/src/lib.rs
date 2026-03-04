//! Multi-armed bandit algorithms.
//!
//! Implements Thompson Sampling, LinUCB, and Neural Contextual Bandits.
//! M4b (Policy Service) wraps this crate in the LMAX single-threaded core.
//!
//! # Architecture (from ADR-002)
//! All policy state mutations happen on a single dedicated thread.
//! This crate provides pure functions — no threads, no channels.
//! The threading model is the responsibility of experimentation-policy.

pub mod thompson;
// Phase 2: pub mod linucb;
// Phase 3: pub mod neural;

/// Arm selection result returned to the caller.
#[derive(Debug, Clone)]
pub struct ArmSelection {
    pub arm_id: String,
    pub assignment_probability: f64,
    pub all_probabilities: Vec<(String, f64)>,
}
