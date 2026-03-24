//! Experimentation Management Service (M5) — Rust port (ADR-025).
//!
//! Phase 1: Core scaffold.
//!   - RBAC interceptor (ported from Go connect-go interceptor).
//!   - Lifecycle state machine (DRAFT→STARTING→RUNNING→CONCLUDING→CONCLUDED→ARCHIVED).
//!   - TOCTOU-safe PostgreSQL state transitions via `UPDATE … WHERE state = $expected`.
//!   - Stubs for all 20 RPCs from ExperimentManagementService proto.
//!
//! Phase 2 (next): Full CRUD, Kafka guardrail consumer, bucket reuse.
//! Phase 3 (next): Direct experimentation-stats imports for OnlineFdrController,
//!                 portfolio optimizer, and adaptive sample size trigger.

pub mod config;
pub mod grpc;
pub mod rbac;
pub mod state;
pub mod store;
