//! EBVS (Exit Before Video Start) classification.
//!
//! Issue #425: EBVS is a first-class QoE failure mode distinct from startup
//! failure (player crash). EBVS signals that the user attempted to watch
//! content but exited before playback began — e.g., TTFF was too slow.
//!
//! The classifier is a pure function so it can be invoked:
//! - Client-side by SDKs that aggregate `QoEEvent` locally.
//! - Server-side by the `HeartbeatSessionizer` (#424) during aggregation.
//! - By M2 as a backfill for events missing the field.

use experimentation_proto::common::PlaybackMetrics;

/// Default EBVS threshold (10 seconds) per Issue #425.
///
/// TTFF exceeding this with no playback is treated as EBVS.
pub const EBVS_DEFAULT_THRESHOLD_MS: i64 = 10_000;

/// Classify whether a playback session exited before video started.
///
/// Returns `true` iff no playback occurred (`playback_duration_ms == 0`) and
/// first-frame delivery did not complete in time — either never reached
/// (`ttff_ms == 0`) or exceeded the threshold.
///
/// If `playback_duration_ms > 0`, playback actually started, so the session
/// is never classified as EBVS regardless of TTFF.
///
/// Negative inputs are coerced to `0` (range validation lives in
/// [`validation::validate_playback_metrics`](crate::validation::validate_playback_metrics)).
pub fn classify_ebvs(ttff_ms: i64, playback_duration_ms: i64, threshold_ms: i64) -> bool {
    if playback_duration_ms > 0 {
        return false;
    }
    let ttff = ttff_ms.max(0);
    let threshold = threshold_ms.max(0);
    ttff == 0 || ttff > threshold
}

/// Populate `ebvs_detected` on a `PlaybackMetrics` from its TTFF and duration.
///
/// Idempotent: overwrites the existing `ebvs_detected` value. Use this in
/// server-side pipelines when the client did not set the field.
pub fn set_ebvs_detected(metrics: &mut PlaybackMetrics, threshold_ms: i64) {
    metrics.ebvs_detected = classify_ebvs(
        metrics.time_to_first_frame_ms,
        metrics.playback_duration_ms,
        threshold_ms,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify_ebvs ────────────────────────────────────────────────────

    #[test]
    fn playback_started_is_not_ebvs_even_with_high_ttff() {
        // User waited through slow start and eventually watched — not EBVS.
        assert!(!classify_ebvs(15_000, 60_000, EBVS_DEFAULT_THRESHOLD_MS));
    }

    #[test]
    fn playback_started_with_any_duration_is_not_ebvs() {
        assert!(!classify_ebvs(250, 1, EBVS_DEFAULT_THRESHOLD_MS));
    }

    #[test]
    fn zero_ttff_zero_duration_is_ebvs() {
        // Never reached first frame, never played — classic EBVS.
        assert!(classify_ebvs(0, 0, EBVS_DEFAULT_THRESHOLD_MS));
    }

    #[test]
    fn ttff_above_threshold_zero_duration_is_ebvs() {
        assert!(classify_ebvs(10_001, 0, EBVS_DEFAULT_THRESHOLD_MS));
    }

    #[test]
    fn ttff_exactly_threshold_zero_duration_is_not_ebvs() {
        // Boundary: threshold is non-strict. User got first frame just in time
        // but didn't proceed — treated as a short-play / abandoned start, not EBVS.
        assert!(!classify_ebvs(
            EBVS_DEFAULT_THRESHOLD_MS,
            0,
            EBVS_DEFAULT_THRESHOLD_MS
        ));
    }

    #[test]
    fn ttff_under_threshold_zero_duration_is_not_ebvs() {
        // First frame arrived quickly, but user exited before duration accrued.
        // This is a "short play" or instant-bounce, not EBVS.
        assert!(!classify_ebvs(500, 0, EBVS_DEFAULT_THRESHOLD_MS));
    }

    #[test]
    fn negative_ttff_treated_as_zero() {
        // Defensive: negative TTFF (malformed) is coerced — still counts as
        // "never reached first frame" when duration is 0.
        assert!(classify_ebvs(-100, 0, EBVS_DEFAULT_THRESHOLD_MS));
    }

    #[test]
    fn custom_threshold_honored() {
        // With a 5s threshold, a 7s TTFF with no playback is EBVS.
        assert!(classify_ebvs(7_000, 0, 5_000));
        // With the default 10s threshold, the same event is NOT EBVS.
        assert!(!classify_ebvs(7_000, 0, EBVS_DEFAULT_THRESHOLD_MS));
    }

    // ── set_ebvs_detected ────────────────────────────────────────────────

    fn base_metrics() -> PlaybackMetrics {
        PlaybackMetrics {
            time_to_first_frame_ms: 0,
            rebuffer_count: 0,
            rebuffer_ratio: 0.0,
            avg_bitrate_kbps: 0,
            resolution_switches: 0,
            peak_resolution_height: 0,
            startup_failure_rate: 0.0,
            playback_duration_ms: 0,
            ebvs_detected: false,
        }
    }

    #[test]
    fn set_ebvs_detected_marks_session_that_never_played() {
        let mut m = base_metrics();
        set_ebvs_detected(&mut m, EBVS_DEFAULT_THRESHOLD_MS);
        assert!(m.ebvs_detected);
    }

    #[test]
    fn set_ebvs_detected_does_not_flag_session_with_playback() {
        let mut m = base_metrics();
        m.time_to_first_frame_ms = 500;
        m.playback_duration_ms = 60_000;
        set_ebvs_detected(&mut m, EBVS_DEFAULT_THRESHOLD_MS);
        assert!(!m.ebvs_detected);
    }

    #[test]
    fn set_ebvs_detected_overrides_prior_value() {
        let mut m = base_metrics();
        // Client erroneously marked a full playback as EBVS.
        m.time_to_first_frame_ms = 250;
        m.playback_duration_ms = 60_000;
        m.ebvs_detected = true;
        set_ebvs_detected(&mut m, EBVS_DEFAULT_THRESHOLD_MS);
        assert!(!m.ebvs_detected, "server-side recompute must override client");
    }
}
