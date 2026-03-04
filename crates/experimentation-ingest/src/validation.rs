//! Event schema validation: required fields, timestamp bounds, enum values.

use experimentation_core::error::{Error, Result};
use chrono::{DateTime, Utc, Duration};

/// Validate that a timestamp is within ±24 hours of server time.
pub fn validate_timestamp(event_time: DateTime<Utc>) -> Result<()> {
    let now = Utc::now();
    let lower = now - Duration::hours(24);
    let upper = now + Duration::hours(24);

    if event_time < lower || event_time > upper {
        return Err(Error::Validation(format!(
            "event timestamp {event_time} is outside ±24h window of server time {now}"
        )));
    }
    Ok(())
}

/// Validate that a required string field is non-empty.
pub fn validate_required(field: &str, field_name: &str) -> Result<()> {
    if field.is_empty() {
        return Err(Error::Validation(format!("{field_name} is required")));
    }
    Ok(())
}
