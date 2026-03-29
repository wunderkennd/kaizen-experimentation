//! Experimentation Management Service (M5) — Rust port (ADR-025).
//!
//! Phase 2 implements:
//!   - TOCTOU-safe lifecycle state machine (DRAFT→STARTING→RUNNING→PAUSED→CONCLUDING→CONCLUDED→ARCHIVED)
//!   - STARTING validators for META, SWITCHBACK, QUASI experiment types (ADR-013, 022, 023)
//!   - Guardrail Kafka consumer: auto-pause on breach (ADR-008)
//!   - Bucket reuse allocator with overlap detection (ADR-009)
//!   - StreamConfigUpdates tonic server-streaming RPC (ADR-025)
//!
//! Phase 4 adds:
//!   - Wire-format contract tests (M5-M6 and M1-M5)
//!   - Shadow traffic harness for Go/Rust parity validation

pub mod bucket_reuse;
pub mod config;
pub mod contract_test_support;
pub mod grpc;
pub mod kafka;
pub mod state_machine;
pub mod store;
pub mod validators;
