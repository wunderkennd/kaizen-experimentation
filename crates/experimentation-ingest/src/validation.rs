//! Event schema validation: required fields, timestamp bounds, enum values.

use chrono::{DateTime, Duration, Utc};
use experimentation_core::error::{assert_finite, Error, Result};
use experimentation_proto::common::{
    ExposureEvent, HeartbeatEvent, MetricEvent, ModelRetrainingEvent, PlaybackMetrics, QoEEvent,
    RewardEvent,
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

    if ts.nanos < 0 {
        return Err(Error::Validation(format!(
            "{name} has negative nanos: {}",
            ts.nanos
        )));
    }

    let dt = DateTime::<Utc>::from_timestamp(ts.seconds, ts.nanos as u32).ok_or_else(|| {
        Error::Validation(format!(
            "{name} has invalid value: seconds={}, nanos={}",
            ts.seconds, ts.nanos
        ))
    })?;

    validate_timestamp(dt)?;
    Ok(dt)
}

/// Validate an ExposureEvent: required fields + timestamp + finite floats + provenance.
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

    validate_interleaving_provenance(&event.interleaving_provenance)?;

    Ok(())
}

/// Validate interleaving provenance map: no empty keys or values.
///
/// An empty map is valid (non-interleaving experiment). When present,
/// each entry must have a non-empty item_id (key) and algorithm_id (value).
pub fn validate_interleaving_provenance(
    provenance: &std::collections::HashMap<String, String>,
) -> Result<()> {
    for (item_id, algorithm_id) in provenance {
        if item_id.is_empty() {
            return Err(Error::Validation(
                "interleaving_provenance key (item_id) must not be empty".to_string(),
            ));
        }
        if algorithm_id.is_empty() {
            return Err(Error::Validation(format!(
                "interleaving_provenance value (algorithm_id) must not be empty for item_id '{item_id}'"
            )));
        }
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

/// Validate a ModelRetrainingEvent (ADR-021): required fields + training window timestamps.
///
/// Required: event_id, model_id, training_data_start, training_data_end.
/// The training window timestamps are validated for format only (not ±24h, since
/// training data windows can reference historical time ranges).
pub fn validate_model_retraining_event(event: &ModelRetrainingEvent) -> Result<()> {
    validate_required(&event.event_id, "event_id")?;
    validate_required(&event.model_id, "model_id")?;

    // training_data_start and training_data_end are required but not ±24h validated —
    // training data windows reference historical ranges.
    let start = require_retraining_timestamp(&event.training_data_start, "training_data_start")?;
    let end = require_retraining_timestamp(&event.training_data_end, "training_data_end")?;

    if end <= start {
        return Err(Error::Validation(
            "training_data_end must be after training_data_start".to_string(),
        ));
    }

    Ok(())
}

/// Build the Bloom filter dedup key for a ModelRetrainingEvent (ADR-021).
///
/// Key format: `"{model_id}:{training_data_start_seconds}"`
///
/// Uses a composite key rather than `event_id` because retraining events are
/// semantically duplicate when the same model is retrained on the same data
/// window, regardless of the caller-supplied event identifier.
///
/// Returns `None` if `training_data_start` is absent. Validation enforces this
/// field as required and runs before the dedup check, so `None` is only
/// reachable in tests or if callers bypass validation.
pub fn model_retraining_dedup_key(event: &ModelRetrainingEvent) -> Option<String> {
    let start_secs = event.training_data_start.as_ref()?.seconds;
    Some(format!("{}:{}", event.model_id, start_secs))
}

/// Unwrap and parse a prost Timestamp for retraining event fields.
/// Unlike require_timestamp, does NOT apply the ±24h window check because
/// training data windows reference historical periods.
fn require_retraining_timestamp(
    ts: &Option<prost_types::Timestamp>,
    name: &str,
) -> Result<DateTime<Utc>> {
    let ts = ts
        .as_ref()
        .ok_or_else(|| Error::Validation(format!("{name} is required")))?;

    if ts.nanos < 0 {
        return Err(Error::Validation(format!(
            "{name} has negative nanos: {}",
            ts.nanos
        )));
    }

    DateTime::<Utc>::from_timestamp(ts.seconds, ts.nanos as u32).ok_or_else(|| {
        Error::Validation(format!(
            "{name} has invalid value: seconds={}, nanos={}",
            ts.seconds, ts.nanos
        ))
    })
}

/// Validate a HeartbeatEvent: required fields + timestamp + finite buffer health + numeric bounds.
///
/// Heartbeats feed the server-side `HeartbeatSessionizer` which aggregates them
/// into `QoEEvent`s. Upper bounds mirror `PlaybackMetrics` for per-heartbeat
/// observations (bitrate ≤ 200_000 kbps, resolution ≤ 8640) so a single noisy
/// client heartbeat cannot produce an aggregate that later fails QoE validation.
pub fn validate_heartbeat_event(event: &HeartbeatEvent) -> Result<()> {
    validate_required(&event.user_id, "user_id")?;
    validate_required(&event.device_id, "device_id")?;
    validate_required(&event.content_id, "content_id")?;
    require_timestamp(&event.timestamp, "timestamp")?;

    assert_finite(event.buffer_health_seconds, "HeartbeatEvent.buffer_health_seconds");
    if event.buffer_health_seconds < 0.0 {
        return Err(Error::Validation(format!(
            "buffer_health_seconds must be >= 0, got {}",
            event.buffer_health_seconds
        )));
    }
    if event.current_bitrate_kbps < 0 || event.current_bitrate_kbps > 200_000 {
        return Err(Error::Validation(format!(
            "current_bitrate_kbps must be in [0, 200000], got {}",
            event.current_bitrate_kbps
        )));
    }
    if event.current_resolution_height < 0 || event.current_resolution_height > 8640 {
        return Err(Error::Validation(format!(
            "current_resolution_height must be in [0, 8640], got {}",
            event.current_resolution_height
        )));
    }

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

    if m.time_to_first_frame_ms < 0 || m.time_to_first_frame_ms > 120_000 {
        return Err(Error::Validation(format!(
            "time_to_first_frame_ms must be in [0, 120000], got {}",
            m.time_to_first_frame_ms
        )));
    }
    if m.rebuffer_count < 0 || m.rebuffer_count > 10_000 {
        return Err(Error::Validation(format!(
            "rebuffer_count must be in [0, 10000], got {}",
            m.rebuffer_count
        )));
    }
    if m.avg_bitrate_kbps < 0 || m.avg_bitrate_kbps > 200_000 {
        return Err(Error::Validation(format!(
            "avg_bitrate_kbps must be in [0, 200000], got {}",
            m.avg_bitrate_kbps
        )));
    }
    if m.resolution_switches < 0 || m.resolution_switches > 10_000 {
        return Err(Error::Validation(format!(
            "resolution_switches must be in [0, 10000], got {}",
            m.resolution_switches
        )));
    }
    if m.playback_duration_ms < 0 || m.playback_duration_ms > 86_400_000 {
        return Err(Error::Validation(format!(
            "playback_duration_ms must be in [0, 86400000], got {}",
            m.playback_duration_ms
        )));
    }
    if m.peak_resolution_height < 0 || m.peak_resolution_height > 8640 {
        return Err(Error::Validation(format!(
            "peak_resolution_height must be in [0, 8640], got {}",
            m.peak_resolution_height
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
            ebvs_detected: false,
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

    // --- QoE upper-bound tests ---

    #[test]
    fn test_qoe_peak_resolution_height_zero_accepted() {
        let mut m = valid_playback_metrics();
        m.peak_resolution_height = 0; // audio-only
        assert!(validate_playback_metrics(&m).is_ok());
    }

    #[test]
    fn test_qoe_peak_resolution_height_negative_rejected() {
        let mut m = valid_playback_metrics();
        m.peak_resolution_height = -1;
        let err = validate_playback_metrics(&m).unwrap_err();
        assert!(err.to_string().contains("peak_resolution_height"));
    }

    #[test]
    fn test_qoe_peak_resolution_height_too_large_rejected() {
        let mut m = valid_playback_metrics();
        m.peak_resolution_height = 8641;
        let err = validate_playback_metrics(&m).unwrap_err();
        assert!(err.to_string().contains("peak_resolution_height"));
    }

    #[test]
    fn test_qoe_time_to_first_frame_too_large_rejected() {
        let mut m = valid_playback_metrics();
        m.time_to_first_frame_ms = 120_001;
        let err = validate_playback_metrics(&m).unwrap_err();
        assert!(err.to_string().contains("time_to_first_frame_ms"));
    }

    #[test]
    fn test_qoe_playback_duration_too_large_rejected() {
        let mut m = valid_playback_metrics();
        m.playback_duration_ms = 86_400_001;
        let err = validate_playback_metrics(&m).unwrap_err();
        assert!(err.to_string().contains("playback_duration_ms"));
    }

    #[test]
    fn test_qoe_avg_bitrate_too_large_rejected() {
        let mut m = valid_playback_metrics();
        m.avg_bitrate_kbps = 200_001;
        let err = validate_playback_metrics(&m).unwrap_err();
        assert!(err.to_string().contains("avg_bitrate_kbps"));
    }

    #[test]
    fn test_qoe_rebuffer_count_too_large_rejected() {
        let mut m = valid_playback_metrics();
        m.rebuffer_count = 10_001;
        let err = validate_playback_metrics(&m).unwrap_err();
        assert!(err.to_string().contains("rebuffer_count"));
    }

    // --- ModelRetrainingEvent tests (ADR-021) ---

    fn past_proto(offset_hours: i64) -> Option<Timestamp> {
        let t = Utc::now() - Duration::hours(offset_hours);
        Some(Timestamp {
            seconds: t.timestamp(),
            nanos: 0,
        })
    }

    fn valid_model_retraining_event() -> ModelRetrainingEvent {
        ModelRetrainingEvent {
            event_id: "mre-1".into(),
            model_id: "rec-model-v2".into(),
            training_data_start: past_proto(48),
            training_data_end: past_proto(24),
            ..Default::default()
        }
    }

    #[test]
    fn test_valid_model_retraining_event_accepted() {
        assert!(validate_model_retraining_event(&valid_model_retraining_event()).is_ok());
    }

    // --- model_retraining_dedup_key tests ---

    #[test]
    fn test_model_retraining_dedup_key_returns_composite() {
        let e = valid_model_retraining_event(); // training_data_start = now - 48h
        let start_secs = e.training_data_start.as_ref().unwrap().seconds;
        let key = model_retraining_dedup_key(&e).expect("key must be Some when start is present");
        assert_eq!(key, format!("rec-model-v2:{start_secs}"));
    }

    #[test]
    fn test_model_retraining_dedup_key_missing_start_returns_none() {
        let mut e = valid_model_retraining_event();
        e.training_data_start = None;
        assert!(model_retraining_dedup_key(&e).is_none());
    }

    #[test]
    fn test_model_retraining_dedup_key_different_windows_produce_different_keys() {
        let e1 = ModelRetrainingEvent {
            event_id: "mre-a".into(),
            model_id: "rec-model".into(),
            training_data_start: past_proto(48),
            training_data_end: past_proto(24),
            ..Default::default()
        };
        let e2 = ModelRetrainingEvent {
            event_id: "mre-b".into(),
            model_id: "rec-model".into(),
            training_data_start: past_proto(72), // different window
            training_data_end: past_proto(48),
            ..Default::default()
        };
        let k1 = model_retraining_dedup_key(&e1).unwrap();
        let k2 = model_retraining_dedup_key(&e2).unwrap();
        assert_ne!(k1, k2, "different training windows must produce different dedup keys");
    }

    #[test]
    fn test_model_retraining_dedup_key_same_window_same_key_regardless_of_event_id() {
        let start = past_proto(48);
        let end = past_proto(24);
        let e1 = ModelRetrainingEvent {
            event_id: "mre-first-attempt".into(),
            model_id: "rec-model".into(),
            training_data_start: start.clone(),
            training_data_end: end.clone(),
            ..Default::default()
        };
        let e2 = ModelRetrainingEvent {
            event_id: "mre-second-attempt".into(), // different event_id
            model_id: "rec-model".into(),
            training_data_start: start,
            training_data_end: end,
            ..Default::default()
        };
        let k1 = model_retraining_dedup_key(&e1).unwrap();
        let k2 = model_retraining_dedup_key(&e2).unwrap();
        assert_eq!(k1, k2, "same model+window must produce same dedup key regardless of event_id");
    }

    #[test]
    fn test_model_retraining_dedup_key_different_models_same_window_produce_different_keys() {
        let start = past_proto(48);
        let end = past_proto(24);
        let e1 = ModelRetrainingEvent {
            event_id: "mre-1".into(),
            model_id: "model-A".into(),
            training_data_start: start.clone(),
            training_data_end: end.clone(),
            ..Default::default()
        };
        let e2 = ModelRetrainingEvent {
            event_id: "mre-2".into(),
            model_id: "model-B".into(), // different model
            training_data_start: start,
            training_data_end: end,
            ..Default::default()
        };
        let k1 = model_retraining_dedup_key(&e1).unwrap();
        let k2 = model_retraining_dedup_key(&e2).unwrap();
        assert_ne!(k1, k2, "different models must produce different dedup keys");
    }

    #[test]
    fn test_model_retraining_missing_model_id() {
        let mut e = valid_model_retraining_event();
        e.model_id = String::new();
        let err = validate_model_retraining_event(&e).unwrap_err();
        assert!(err.to_string().contains("model_id is required"));
    }

    #[test]
    fn test_model_retraining_missing_event_id() {
        let mut e = valid_model_retraining_event();
        e.event_id = String::new();
        let err = validate_model_retraining_event(&e).unwrap_err();
        assert!(err.to_string().contains("event_id is required"));
    }

    #[test]
    fn test_model_retraining_missing_training_data_start() {
        let mut e = valid_model_retraining_event();
        e.training_data_start = None;
        let err = validate_model_retraining_event(&e).unwrap_err();
        assert!(err.to_string().contains("training_data_start is required"));
    }

    #[test]
    fn test_model_retraining_missing_training_data_end() {
        let mut e = valid_model_retraining_event();
        e.training_data_end = None;
        let err = validate_model_retraining_event(&e).unwrap_err();
        assert!(err.to_string().contains("training_data_end is required"));
    }

    #[test]
    fn test_model_retraining_end_before_start_rejected() {
        let mut e = valid_model_retraining_event();
        // swap: end is older than start
        e.training_data_start = past_proto(24);
        e.training_data_end = past_proto(48);
        let err = validate_model_retraining_event(&e).unwrap_err();
        assert!(err.to_string().contains("training_data_end must be after"));
    }

    #[test]
    fn test_model_retraining_historical_window_accepted() {
        // Training windows can reference time > 24h ago — no ±24h restriction.
        let e = ModelRetrainingEvent {
            event_id: "mre-2".into(),
            model_id: "rec-model-v1".into(),
            training_data_start: past_proto(720), // 30 days ago
            training_data_end: past_proto(360),   // 15 days ago
            ..Default::default()
        };
        assert!(validate_model_retraining_event(&e).is_ok());
    }

    #[test]
    fn test_qoe_resolution_switches_too_large_rejected() {
        let mut m = valid_playback_metrics();
        m.resolution_switches = 10_001;
        let err = validate_playback_metrics(&m).unwrap_err();
        assert!(err.to_string().contains("resolution_switches"));
    }

    #[test]
    fn test_qoe_boundary_values_accepted() {
        let m = PlaybackMetrics {
            time_to_first_frame_ms: 120_000,
            rebuffer_count: 10_000,
            rebuffer_ratio: 1.0,
            avg_bitrate_kbps: 200_000,
            resolution_switches: 10_000,
            peak_resolution_height: 8640,
            startup_failure_rate: 1.0,
            playback_duration_ms: 86_400_000,
            ebvs_detected: false,
        };
        assert!(validate_playback_metrics(&m).is_ok());
    }

    // --- Interleaving provenance tests ---

    #[test]
    fn test_exposure_no_provenance_accepted() {
        let e = valid_exposure(); // empty map by default
        assert!(validate_exposure(&e).is_ok());
    }

    #[test]
    fn test_exposure_valid_provenance_accepted() {
        let mut e = valid_exposure();
        e.interleaving_provenance
            .insert("item-1".into(), "algo-a".into());
        e.interleaving_provenance
            .insert("item-2".into(), "algo-b".into());
        assert!(validate_exposure(&e).is_ok());
    }

    #[test]
    fn test_exposure_provenance_empty_item_id_rejected() {
        let mut provenance = std::collections::HashMap::new();
        provenance.insert(String::new(), "algo-a".into());
        let err = validate_interleaving_provenance(&provenance).unwrap_err();
        assert!(err.to_string().contains("item_id) must not be empty"));
    }

    #[test]
    fn test_exposure_provenance_empty_algorithm_id_rejected() {
        let mut e = valid_exposure();
        e.interleaving_provenance
            .insert("item-1".into(), String::new());
        let err = validate_exposure(&e).unwrap_err();
        assert!(err
            .to_string()
            .contains("algorithm_id) must not be empty"));
    }

    // --- HeartbeatEvent tests ---

    fn valid_heartbeat() -> HeartbeatEvent {
        HeartbeatEvent {
            user_id: "user-1".into(),
            session_id: "sess-1".into(),
            device_id: "device-1".into(),
            timestamp: now_proto(),
            current_bitrate_kbps: 5000,
            current_resolution_height: 1080,
            buffer_health_seconds: 5.0,
            is_rebuffering: false,
            is_startup: false,
            content_id: "movie-1".into(),
            variant_id: "control".into(),
        }
    }

    #[test]
    fn test_valid_heartbeat_accepted() {
        assert!(validate_heartbeat_event(&valid_heartbeat()).is_ok());
    }

    #[test]
    fn test_heartbeat_missing_user_id() {
        let mut e = valid_heartbeat();
        e.user_id = String::new();
        let err = validate_heartbeat_event(&e).unwrap_err();
        assert!(err.to_string().contains("user_id is required"));
    }

    #[test]
    fn test_heartbeat_missing_device_id() {
        let mut e = valid_heartbeat();
        e.device_id = String::new();
        let err = validate_heartbeat_event(&e).unwrap_err();
        assert!(err.to_string().contains("device_id is required"));
    }

    #[test]
    fn test_heartbeat_missing_content_id() {
        let mut e = valid_heartbeat();
        e.content_id = String::new();
        let err = validate_heartbeat_event(&e).unwrap_err();
        assert!(err.to_string().contains("content_id is required"));
    }

    #[test]
    fn test_heartbeat_missing_timestamp() {
        let mut e = valid_heartbeat();
        e.timestamp = None;
        let err = validate_heartbeat_event(&e).unwrap_err();
        assert!(err.to_string().contains("timestamp is required"));
    }

    #[test]
    fn test_heartbeat_negative_buffer_health_rejected() {
        let mut e = valid_heartbeat();
        e.buffer_health_seconds = -1.0;
        let err = validate_heartbeat_event(&e).unwrap_err();
        assert!(err.to_string().contains("buffer_health_seconds"));
    }

    #[test]
    fn test_heartbeat_bitrate_out_of_range_rejected() {
        let mut e = valid_heartbeat();
        e.current_bitrate_kbps = 300_000;
        let err = validate_heartbeat_event(&e).unwrap_err();
        assert!(err.to_string().contains("current_bitrate_kbps"));
    }

    #[test]
    fn test_heartbeat_resolution_out_of_range_rejected() {
        let mut e = valid_heartbeat();
        e.current_resolution_height = 9000;
        let err = validate_heartbeat_event(&e).unwrap_err();
        assert!(err.to_string().contains("current_resolution_height"));
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn test_heartbeat_nan_buffer_health_panics() {
        let mut e = valid_heartbeat();
        e.buffer_health_seconds = f64::NAN;
        let _ = validate_heartbeat_event(&e);
    }
}
