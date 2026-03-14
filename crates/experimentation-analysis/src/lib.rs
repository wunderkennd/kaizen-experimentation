//! Experimentation Analysis Service (M4a) — library crate.
//!
//! Re-exports internal modules so that integration tests in `tests/`
//! can import the handler and configuration types.

pub mod config;
pub mod delta_reader;
pub mod grpc;
pub mod store;
