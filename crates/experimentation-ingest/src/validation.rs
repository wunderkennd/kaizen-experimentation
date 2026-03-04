//! Event schema validation: required fields, timestamp bounds, enum values.

use chrono::{DateTime, Duration, Utc};
use experimentation_core::error::{assert_finite, Error, Result};
use experimentation_proto::common::{
    ExposureEvent, MetricEvent, PlaybackMetrics, QoEEvent, RewardEvent,
};

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

/// Unwrap a prost Timestamp, convert to chrono DateTime, and validate ±24h.
pub fn require_timestamp(
    ts: &Option<prost_types::Timestamp>,
    name: &str,
) -> Result<DateTime<Utc>> {
    let ts = ts
        .as_ref()
        .ok_or_else(|| Error::Validation(format!("{name} is required")))?;

    let dt = DateTime::<Utc>::from_timestamp(ts.seconds, ts.nanos as u32).ok_or_else(|| {
        Error::Validation(format!(
            "{name} has invalid value: seconds={}, nanos={}",
            ts.seconds, ts.nanos
        ))
    })?;

    validate_timestamp(dt)?;
    Ok(dt)
}

/// Validate an ExposureEvent: required fields + timestamp + finite floats.
pub fn validate_exposure(event: &ExposureEvent) -> Result<()> {
    validate_required(&event.event_id, "event_id")?;
    validate_required(&event.experiment_id, "experiment_id")?;
    validate_required(&event.user_id, "user_id")?;
    validate_required(&event.variant_id, "variant_id")?;
    require_timestamp(&event.timestamp, "timestamp")?;

    if event.assignment_probability != 0.0 {
        assert_finite(
            event.assignment_probability,
            "ExposureEvent.assignment_probability",
        );
    }

    Ok(())
}

/// Validate a MetricEvent: required fields + timestamp + finite value.
pub fn validate_metric_event(event: &MetricEvent) -> Result<()> {
    validate_required(&event.event_id, "event_id")?;
    validate_required(&event.user_id, "user_id")?;
    validate_required(&event.event_type, "event_type")?;
    require_timestamp(&event.timestamp, "timestamp")?;

    assert_finite(event.value, "MetricEvent.value");

    Ok(())
}

/// Validate a RewardEvent: required fields + timestamp + finite reward.
pub fn validate_reward_event(event: &RewardEvent) -> Result<()> {
    validate_required(&event.event_id, "event_id")?;
    validate_required(&event.experiment_id, "experiment_id")?;
    validate_required(&event.user_id, "user_id")?;
    validate_required(&event.arm_id, "arm_id")?;
    require_timestamp(&event.timestamp, "timestamp")?;

    assert_finite(event.reward, "RewardEvent.reward");

    Ok(())
}

/// Validate a QoEEvent: required fields + timestamp + playback metrics.
pub fn validate_qoe_event(event: &QoEEvent) -> Result<()> {
    validate_required(&event.event_id, "event_id")?;
    validate_required(&event.session_id, "session_id")?;
    validate_required(&event.content_id, "content_id")?;
    validate_required(&event.user_id, "user_id")?;
    require_timestamp(&event.timestamp, "timestamp")?;

    let metrics = event
        .metrics
        .as_ref()
        .ok_or_else(|| Error::Validation("metrics is required".to_string()))?;

    validate_playback_metrics(metrics)?;

    Ok(())
}

/// Validate PlaybackMetrics: ranges, non-negative values, finite floats.
pub fn validate_playback_metrics(m: &PlaybackMetrics) -> Result<()> {
    assert_finite(m.rebuffer_ratio, "PlaybackMetrics.rebuffer_ratio");
    if m.rebuffer_ratio < 0.0 || m.rebuffer_ratio > 1.0 {
        return Err(Error::Validation(format!(
            "rebuffer_ratio must be in [0, 1], got {}",
            m.rebuffer_ratio
        )));
    }

    assert_finite(
        m.startup_failure_rate,
        "PlaybackMetrics.startup_failure_rate",
    );
    if m.startup_failure_rate != 0.0 && m.startup_failure_rate != 1.0 {
        return Err(Error::Validation(format!(
            "startup_failure_rate must be 0.0 or 1.0, got {}",
            m.startup_failure_rate
        )));
    }

    if m.time_to_first_frame_ms < 0 {
        return Err(Error::Validation(format!(
            "time_to_first_frame_ms must be non-negative, got {}",
            m.time_to_first_frame_ms
        )));
    }
    if m.rebuffer_count < 0 {
        return Err(Error::Validation(format!(
            "rebuffer_count must be non-negative, got {}",
            m.rebuffer_count
        )));
    }
    if m.avg_bitrate_kbps < 0 {
        return Err(Error::Validation(format!(
            "avg_bitrate_kbps must be non-negative, got {}",
            m.avg_bitrate_kbps
        )));
    }
    if m.resolution_switches < 0 {
        return Err(Error::Validation(format!(
            "resolution_switches must be non-negative, got {}",
            m.resolution_switches
        )));
    }
    if m.playback_duration_ms < 0 {
        return Err(Error::Validation(format!(
            "playback_duration_ms must be non-negative, got {}",
            m.playback_duration_ms
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::Timestamp;

    fn now_proto() -> Option<Timestamp> {
        let now = Utc::now();
        Some(Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        })
    }

    fn old_proto() -> Option<Timestamp> {
        let old = Utc::now() - Duration::hours(25);
        Some(Timestamp {
            seconds: old.timestamp(),
            nanos: 0,
        })
    }

    fn valid_exposure() -> ExposureEvent {
        ExposureEvent {
            event_id: "evt-1".into(),
            experiment_id: "exp-1".into(),
            user_id: "user-1".into(),
            variant_id: "control".into(),
            timestamp: now_proto(),
            assignment_probability: 0.5,
            ..Default::default()
        }
    }

    #[test]
    fn test_valid_exposure_accepted() {
        assert!(validate_exposure(&valid_exposure()).is_ok());
    }

    #[test]
    fn test_exposure_missing_experiment_id() {
        let mut e = valid_exposure();
        e.experiment_id = String::new();
        let err = validate_exposure(&e).unwrap_err();
        assert!(err.to_string().contains("experiment_id is required"));
    }

    #[test]
    fn test_exposure_missing_event_id() {
        let mut e = valid_exposure();
        e.event_id = String::new();
        let err = validate_exposure(&e).unwrap_err();
        assert!(err.to_string().contains("event_id is required"));
    }

    #[test]
    fn test_exposure_old_timestamp_rejected() {
        let mut e = valid_exposure();
        e.timestamp = old_proto();
        let err = validate_exposure(&e).unwrap_err();
        assert!(err.to_string().contains("outside ±24h"));
    }

    #[test]
    fn test_exposure_missing_timestamp() {
        let mut e = valid_exposure();
        e.timestamp = None;
        let err = validate_exposure(&e).unwrap_err();
        assert!(err.to_string().contains("timestamp is required"));
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn test_exposure_nan_probability_panics() {
        let mut e = valid_exposure();
        e.assignment_probability = f64::NAN;
        let _ = validate_exposure(&e);
    }

    fn valid_metric_event() -> MetricEvent {
        MetricEvent {
            event_id: "evt-2".into(),
            user_id: "user-1".into(),
            event_type: "play_start".into(),
            value: 42.0,
            timestamp: now_proto(),
            ..Default::default()
        }
    }

    #[test]
    fn test_valid_metric_event_accepted() {
        assert!(validate_metric_event(&valid_metric_event()).is_ok());
    }

    #[test]
    fn test_metric_event_missing_event_type() {
        let mut e = valid_metric_event();
        e.event_type = String::new();
        let err = validate_metric_event(&e).unwrap_err();
        assert!(err.to_string().contains("event_type is required"));
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn test_metric_event_infinity_value_panics() {
        let mut e = valid_metric_event();
        e.value = f64::INFINITY;
        let _ = validate_metric_event(&e);
    }

    fn valid_reward_event() -> RewardEvent {
        RewardEvent {
            event_id: "evt-3".into(),
            experiment_id: "exp-1".into(),
            user_id: "user-1".into(),
            arm_id: "arm-a".into(),
            reward: 0.85,
            timestamp: now_proto(),
            ..Default::default()
        }
    }

    #[test]
    fn test_valid_reward_event_accepted() {
        assert!(validate_reward_event(&valid_reward_event()).is_ok());
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn test_reward_nan_panics() {
        let mut e = valid_reward_event();
        e.reward = f64::NAN;
        let _ = validate_reward_event(&e);
    }

    fn valid_playback_metrics() -> PlaybackMetrics {
        PlaybackMetrics {
            time_to_first_frame_ms: 250,
            rebuffer_count: 1,
            rebuffer_ratio: 0.02,
            avg_bitrate_kbps: 5000,
            resolution_switches: 2,
            peak_resolution_height: 1080,
            startup_failure_rate: 0.0,
            playback_duration_ms: 60000,
        }
    }

    fn valid_qoe_event() -> QoEEvent {
        QoEEvent {
            event_id: "evt-4".into(),
            session_id: "sess-1".into(),
            content_id: "movie-1".into(),
            user_id: "user-1".into(),
            metrics: Some(valid_playback_metrics()),
            timestamp: now_proto(),
            ..Default::default()
        }
    }

    #[test]
    fn test_valid_qoe_event_accepted() {
        assert!(validate_qoe_event(&valid_qoe_event()).is_ok());
    }

    #[test]
    fn test_qoe_missing_metrics() {
        let mut e = valid_qoe_event();
        e.metrics = None;
        let err = validate_qoe_event(&e).unwrap_err();
        assert!(err.to_string().contains("metrics is required"));
    }

    #[test]
    fn test_qoe_rebuffer_ratio_out_of_range() {
        let mut m = valid_playback_metrics();
        m.rebuffer_ratio = 1.5;
        let err = validate_playback_metrics(&m).unwrap_err();
        assert!(err.to_string().contains("rebuffer_ratio"));
    }

    #[test]
    fn test_qoe_invalid_startup_failure_rate() {
        let mut m = valid_playback_metrics();
        m.startup_failure_rate = 0.5;
        let err = validate_playback_metrics(&m).unwrap_err();
        assert!(err.to_string().contains("startup_failure_rate"));
    }

    #[test]
    fn test_qoe_negative_duration() {
        let mut m = valid_playback_metrics();
        m.playback_duration_ms = -1;
        let err = validate_playback_metrics(&m).unwrap_err();
        assert!(err.to_string().contains("playback_duration_ms"));
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn test_qoe_nan_rebuffer_ratio_panics() {
        let mut m = valid_playback_metrics();
        m.rebuffer_ratio = f64::NAN;
        let _ = validate_playback_metrics(&m);
    }
}
