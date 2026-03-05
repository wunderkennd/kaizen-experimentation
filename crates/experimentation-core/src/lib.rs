//! Foundation crate: timestamps, errors, tracing, shared types.
//!
//! Every other crate in the workspace depends on this crate.
//! Keep it minimal — only truly cross-cutting concerns belong here.

pub mod error;
pub mod telemetry;
pub mod time;

/// Re-export commonly used types.
pub use error::{Error, Result};
pub use time::Timestamp;
