//! Experimentation Feature Flag Service (M7) — Rust port (ADR-024).
//!
//! Phase 1: Flag CRUD + EvaluateFlag/EvaluateFlags.
//! Phase 2: PromoteToExperiment, audit trail.
//! Phase 3: Kafka reconciler.

pub mod config;
pub mod grpc;
pub mod store;
