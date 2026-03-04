//! Platform-wide error types.
//!
//! All crates use these error types for consistency.
//! Fail-fast principle: NaN, Infinity, and overflow are unrecoverable errors.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("numerical error: {0}")]
    Numerical(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("state error: {0}")]
    InvalidState(String),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Fail-fast check for NaN or Infinity in floating-point values.
/// Panics immediately with context — NaN propagation is a correctness bug.
#[inline]
pub fn assert_finite(value: f64, context: &str) {
    assert!(
        value.is_finite(),
        "FAIL-FAST: non-finite value ({value}) in {context}"
    );
}
