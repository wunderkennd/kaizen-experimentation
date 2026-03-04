//! Timestamp utilities.

use chrono::{DateTime, Utc};

/// Wrapper around chrono DateTime<Utc> for consistent timestamp handling.
pub type Timestamp = DateTime<Utc>;

/// Returns the current UTC timestamp.
pub fn now() -> Timestamp {
    Utc::now()
}
