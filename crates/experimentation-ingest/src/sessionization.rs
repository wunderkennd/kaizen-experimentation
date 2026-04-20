//! Heartbeat sessionization: aggregate 10-second heartbeat events into `QoEEvent`s.
//!
//! Clients emit a `HeartbeatEvent` roughly every 10 seconds during active playback.
//! This module buffers heartbeats keyed by `(user_id, device_id, content_id)`,
//! detects session boundaries via an inactivity gap (default 30s), and emits
//! aggregated `QoEEvent`s indistinguishable from client-aggregated events so
//! that the downstream M3 pipeline does not need to distinguish origins.
//!
//! Crash recovery: in-memory state is intentionally non-durable. On restart,
//! in-flight sessions are dropped. The next heartbeat from a recovered client
//! naturally starts a new session because the gap threshold elapsed during the
//! restart window (design doc accepts a brief dedup gap on restart).

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use experimentation_core::error::{assert_finite, Result};
use experimentation_proto::common::{HeartbeatEvent, PlaybackMetrics, QoEEvent};
use prost_types::Timestamp;
use tracing::{debug, warn};

use crate::validation::{require_timestamp, validate_heartbeat_event};

/// Default inactivity gap after which a session is considered closed.
///
/// 30 seconds at a 10-second heartbeat cadence tolerates two consecutive missed
/// heartbeats (e.g., a network blip) without splitting a genuinely continuous
/// viewing session.
pub const DEFAULT_SESSION_GAP_SECS: i64 = 30;

/// Upper bounds mirrored from `PlaybackMetrics` validation so the emitted
/// `QoEEvent` is guaranteed to pass downstream validation.
const MAX_TIME_TO_FIRST_FRAME_MS: i64 = 120_000;
const MAX_PLAYBACK_DURATION_MS: i64 = 86_400_000;
const MAX_REBUFFER_COUNT: i32 = 10_000;
const MAX_RESOLUTION_SWITCHES: i32 = 10_000;
const MAX_AVG_BITRATE_KBPS: i32 = 200_000;
const MAX_PEAK_RESOLUTION_HEIGHT: i32 = 8640;

/// Configuration for [`HeartbeatSessionizer`].
#[derive(Debug, Clone)]
pub struct SessionizerConfig {
    /// Inactivity window; if the next heartbeat's timestamp is more than
    /// `session_gap` after the session's last heartbeat, the session closes
    /// and a new one starts.
    pub session_gap: Duration,
}

impl Default for SessionizerConfig {
    fn default() -> Self {
        Self {
            session_gap: Duration::seconds(DEFAULT_SESSION_GAP_SECS),
        }
    }
}

/// Composite session key.
///
/// The tuple (user_id, device_id, content_id) uniquely identifies a viewing
/// session; choosing this over the client-supplied `session_id` alone protects
/// against SDK bugs that re-use session IDs across content switches.
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct SessionKey {
    pub user_id: String,
    pub device_id: String,
    pub content_id: String,
}

impl SessionKey {
    fn from_heartbeat(hb: &HeartbeatEvent) -> Self {
        Self {
            user_id: hb.user_id.clone(),
            device_id: hb.device_id.clone(),
            content_id: hb.content_id.clone(),
        }
    }
}

#[derive(Debug)]
struct SessionState {
    client_session_id: String,
    first_timestamp: DateTime<Utc>,
    last_timestamp: DateTime<Utc>,
    // Earliest heartbeat where `is_startup == false`; None until the player rendered a frame.
    first_frame_timestamp: Option<DateTime<Utc>>,
    // Count of `false -> true` transitions in `is_rebuffering`.
    rebuffer_transitions: i32,
    // Tracks whether the most recent heartbeat was rebuffering so we can detect transitions.
    prev_is_rebuffering: bool,
    // Summed rebuffering duration across the session, in milliseconds.
    // Approximated by attributing `(t_next - t_prev)` whenever `prev_is_rebuffering` is true.
    total_rebuffer_ms: i64,
    // For bitrate averaging.
    bitrate_sum: i64,
    heartbeat_count: i64,
    // For resolution-switch tracking (None until the first heartbeat is seen).
    prev_resolution_height: Option<i32>,
    resolution_switches: i32,
    peak_resolution_height: i32,
    // Sticky flag — once the player leaves startup, it stays out.
    startup_completed: bool,
}

impl SessionState {
    fn open(hb: &HeartbeatEvent, ts: DateTime<Utc>) -> Self {
        let startup_completed = !hb.is_startup;
        let first_frame = if startup_completed { Some(ts) } else { None };
        Self {
            client_session_id: hb.session_id.clone(),
            first_timestamp: ts,
            last_timestamp: ts,
            first_frame_timestamp: first_frame,
            rebuffer_transitions: i32::from(hb.is_rebuffering),
            prev_is_rebuffering: hb.is_rebuffering,
            total_rebuffer_ms: 0,
            bitrate_sum: hb.current_bitrate_kbps as i64,
            heartbeat_count: 1,
            prev_resolution_height: Some(hb.current_resolution_height),
            resolution_switches: 0,
            peak_resolution_height: hb.current_resolution_height.max(0),
            startup_completed,
        }
    }

    fn apply(&mut self, hb: &HeartbeatEvent, ts: DateTime<Utc>) {
        let elapsed_ms = ts.signed_duration_since(self.last_timestamp).num_milliseconds();

        // Attribute the gap to rebuffering if the previous heartbeat was rebuffering.
        // This is an approximation: the true transition happens at some point in
        // [t_prev, t_next]; we assume it persisted for the full interval.
        if self.prev_is_rebuffering && elapsed_ms > 0 {
            self.total_rebuffer_ms = self.total_rebuffer_ms.saturating_add(elapsed_ms);
        }

        if !self.prev_is_rebuffering && hb.is_rebuffering {
            self.rebuffer_transitions = self.rebuffer_transitions.saturating_add(1);
        }
        self.prev_is_rebuffering = hb.is_rebuffering;

        if !hb.is_startup && !self.startup_completed {
            self.startup_completed = true;
            self.first_frame_timestamp = Some(ts);
        }

        self.bitrate_sum = self.bitrate_sum.saturating_add(hb.current_bitrate_kbps as i64);
        self.heartbeat_count = self.heartbeat_count.saturating_add(1);

        if let Some(prev) = self.prev_resolution_height {
            if prev != hb.current_resolution_height {
                self.resolution_switches = self.resolution_switches.saturating_add(1);
            }
        }
        self.prev_resolution_height = Some(hb.current_resolution_height);

        if hb.current_resolution_height > self.peak_resolution_height {
            self.peak_resolution_height = hb.current_resolution_height;
        }

        self.last_timestamp = ts;
    }

    fn finalize(mut self, key: &SessionKey) -> QoEEvent {
        // If the session ended while still rebuffering, attribute the trailing
        // interval up to `last_timestamp` (already accounted for on apply() but
        // not for the very first heartbeat of a still-rebuffering session).
        let playback_duration_ms = self
            .last_timestamp
            .signed_duration_since(self.first_timestamp)
            .num_milliseconds()
            .max(0);

        let ttff_ms = self
            .first_frame_timestamp
            .map(|t| {
                t.signed_duration_since(self.first_timestamp)
                    .num_milliseconds()
                    .max(0)
            })
            .unwrap_or(0)
            .min(MAX_TIME_TO_FIRST_FRAME_MS);

        let rebuffer_ratio = if playback_duration_ms > 0 {
            let ratio = self.total_rebuffer_ms.min(playback_duration_ms) as f64
                / playback_duration_ms as f64;
            assert_finite(ratio, "HeartbeatSessionizer.rebuffer_ratio");
            ratio.clamp(0.0, 1.0)
        } else {
            0.0
        };

        let avg_bitrate = if self.heartbeat_count > 0 {
            (self.bitrate_sum / self.heartbeat_count) as i32
        } else {
            0
        };

        let startup_failure_rate = if self.startup_completed { 0.0 } else { 1.0 };

        // Clamp all fields into PlaybackMetrics-valid ranges to guarantee the
        // emitted QoEEvent passes downstream validation even for noisy clients.
        let metrics = PlaybackMetrics {
            time_to_first_frame_ms: ttff_ms,
            rebuffer_count: self.rebuffer_transitions.clamp(0, MAX_REBUFFER_COUNT),
            rebuffer_ratio,
            avg_bitrate_kbps: avg_bitrate.clamp(0, MAX_AVG_BITRATE_KBPS),
            resolution_switches: self.resolution_switches.clamp(0, MAX_RESOLUTION_SWITCHES),
            peak_resolution_height: self
                .peak_resolution_height
                .clamp(0, MAX_PEAK_RESOLUTION_HEIGHT),
            startup_failure_rate,
            playback_duration_ms: playback_duration_ms.min(MAX_PLAYBACK_DURATION_MS),
        };

        // `session_id` on the emitted event prefers the client-supplied value —
        // downstream joins often key off it. Fall back to a deterministic
        // synthesized ID when the client omitted one.
        let session_id = if self.client_session_id.is_empty() {
            format!(
                "srv-sess-{}-{}-{}",
                key.user_id, key.device_id, self.first_timestamp.timestamp_millis()
            )
        } else {
            std::mem::take(&mut self.client_session_id)
        };

        let last_ts_proto = Timestamp {
            seconds: self.last_timestamp.timestamp(),
            nanos: self.last_timestamp.timestamp_subsec_nanos() as i32,
        };

        QoEEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            session_id,
            content_id: key.content_id.clone(),
            user_id: key.user_id.clone(),
            metrics: Some(metrics),
            cdn_provider: String::new(),
            abr_algorithm: String::new(),
            encoding_profile: String::new(),
            timestamp: Some(last_ts_proto),
        }
    }
}

/// Aggregates heartbeats into `QoEEvent`s via gap-based sessionization.
///
/// Use [`ingest`] to push heartbeats in wall-clock order. Returned events are
/// ready for publishing to the `qoe_events` Kafka topic. Call [`flush_expired`]
/// periodically (e.g., every `session_gap / 2`) to close idle sessions without
/// waiting for another heartbeat. Call [`drain`] on graceful shutdown.
pub struct HeartbeatSessionizer {
    sessions: HashMap<SessionKey, SessionState>,
    config: SessionizerConfig,
    dropped_out_of_order: u64,
}

impl Default for HeartbeatSessionizer {
    fn default() -> Self {
        Self::with_config(SessionizerConfig::default())
    }
}

impl HeartbeatSessionizer {
    pub fn with_config(config: SessionizerConfig) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
            dropped_out_of_order: 0,
        }
    }

    /// Number of currently open (in-flight) sessions.
    pub fn active_sessions(&self) -> usize {
        self.sessions.len()
    }

    /// Count of heartbeats discarded because their timestamp was older than
    /// the most recent heartbeat for the same session key.
    pub fn dropped_out_of_order(&self) -> u64 {
        self.dropped_out_of_order
    }

    /// Ingest a single heartbeat. Returns any `QoEEvent` closed by this
    /// heartbeat's arrival (i.e., a gap was detected).
    ///
    /// Validates the heartbeat first; returns `Err` if validation fails.
    /// Out-of-order heartbeats (timestamp < last for the same key) are dropped
    /// with a warning and counted in `dropped_out_of_order`.
    pub fn ingest(&mut self, hb: HeartbeatEvent) -> Result<Option<QoEEvent>> {
        validate_heartbeat_event(&hb)?;
        let ts = require_timestamp(&hb.timestamp, "timestamp")?;
        let key = SessionKey::from_heartbeat(&hb);

        let mut emitted = None;
        if let Some(existing) = self.sessions.get(&key) {
            let gap = ts.signed_duration_since(existing.last_timestamp);
            if gap < Duration::zero() {
                // Out-of-order arrival within the same session — drop.
                self.dropped_out_of_order = self.dropped_out_of_order.saturating_add(1);
                warn!(
                    user_id = %hb.user_id,
                    device_id = %hb.device_id,
                    content_id = %hb.content_id,
                    gap_ms = gap.num_milliseconds(),
                    "dropping out-of-order heartbeat"
                );
                return Ok(None);
            }
            if gap > self.config.session_gap {
                // Gap exceeded → emit the existing session and start a fresh one.
                let state = self.sessions.remove(&key).expect("just checked present");
                emitted = Some(state.finalize(&key));
                debug!(
                    user_id = %key.user_id,
                    content_id = %key.content_id,
                    gap_ms = gap.num_milliseconds(),
                    "session gap exceeded; emitting QoEEvent"
                );
            }
        }

        // Either open a new session or append to the existing one.
        match self.sessions.get_mut(&key) {
            Some(state) => state.apply(&hb, ts),
            None => {
                self.sessions.insert(key, SessionState::open(&hb, ts));
            }
        }

        Ok(emitted)
    }

    /// Emit all sessions whose last heartbeat is older than `now - session_gap`.
    /// Returns the emitted events. Intended for periodic sweeps that close
    /// abandoned sessions without a new heartbeat to trigger them.
    pub fn flush_expired(&mut self, now: DateTime<Utc>) -> Vec<QoEEvent> {
        let cutoff = now - self.config.session_gap;
        let expired_keys: Vec<SessionKey> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.last_timestamp < cutoff)
            .map(|(k, _)| k.clone())
            .collect();

        let mut events = Vec::with_capacity(expired_keys.len());
        for key in expired_keys {
            if let Some(state) = self.sessions.remove(&key) {
                events.push(state.finalize(&key));
            }
        }
        events
    }

    /// Emit every open session. Call on graceful shutdown to preserve in-flight
    /// state. Crash exit will of course lose these events — the design doc
    /// accepts that loss per the sessionizer's non-durable contract.
    pub fn drain(&mut self) -> Vec<QoEEvent> {
        let mut events = Vec::with_capacity(self.sessions.len());
        for (key, state) in self.sessions.drain() {
            events.push(state.finalize(&key));
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(offset_s: i64) -> Option<Timestamp> {
        // Anchor near "now" so validate_timestamp's ±24h check passes.
        let base = Utc::now();
        let t = base + Duration::seconds(offset_s);
        Some(Timestamp {
            seconds: t.timestamp(),
            nanos: 0,
        })
    }

    fn base_heartbeat(offset_s: i64) -> HeartbeatEvent {
        HeartbeatEvent {
            user_id: "user-1".into(),
            session_id: "sess-client-1".into(),
            device_id: "device-1".into(),
            timestamp: ts(offset_s),
            current_bitrate_kbps: 5000,
            current_resolution_height: 1080,
            buffer_health_seconds: 6.0,
            is_rebuffering: false,
            is_startup: false,
            content_id: "movie-1".into(),
            variant_id: "control".into(),
        }
    }

    #[test]
    fn single_heartbeat_opens_session_without_emission() {
        let mut s = HeartbeatSessionizer::default();
        let emitted = s.ingest(base_heartbeat(0)).unwrap();
        assert!(emitted.is_none(), "first heartbeat should never emit");
        assert_eq!(s.active_sessions(), 1);
    }

    #[test]
    fn continuous_heartbeats_do_not_close_session() {
        let mut s = HeartbeatSessionizer::default();
        for i in 0..5 {
            let emitted = s.ingest(base_heartbeat(i * 10)).unwrap();
            assert!(emitted.is_none());
        }
        assert_eq!(s.active_sessions(), 1);
    }

    #[test]
    fn gap_exceeding_threshold_emits_session() {
        let mut s = HeartbeatSessionizer::default();
        s.ingest(base_heartbeat(0)).unwrap();
        s.ingest(base_heartbeat(10)).unwrap();
        // 50-second gap > 30-second default
        let emitted = s.ingest(base_heartbeat(60)).unwrap();
        let event = emitted.expect("session should be emitted on gap");
        assert_eq!(event.user_id, "user-1");
        assert_eq!(event.content_id, "movie-1");
        assert_eq!(event.session_id, "sess-client-1");
        let m = event.metrics.expect("metrics present");
        assert_eq!(m.playback_duration_ms, 10_000);
        // After emission, the third heartbeat started a new session
        assert_eq!(s.active_sessions(), 1);
    }

    #[test]
    fn gap_equal_to_threshold_does_not_emit() {
        // Exactly at the 30s threshold is tolerated — the rule is strictly greater than.
        let mut s = HeartbeatSessionizer::default();
        s.ingest(base_heartbeat(0)).unwrap();
        let emitted = s.ingest(base_heartbeat(30)).unwrap();
        assert!(emitted.is_none());
        assert_eq!(s.active_sessions(), 1);
    }

    #[test]
    fn different_keys_track_independently() {
        let mut s = HeartbeatSessionizer::default();
        let mut a = base_heartbeat(0);
        a.user_id = "alice".into();
        let mut b = base_heartbeat(0);
        b.user_id = "bob".into();
        s.ingest(a).unwrap();
        s.ingest(b).unwrap();
        assert_eq!(s.active_sessions(), 2);
    }

    #[test]
    fn different_content_same_user_tracked_independently() {
        let mut s = HeartbeatSessionizer::default();
        let mut a = base_heartbeat(0);
        a.content_id = "movie-a".into();
        let mut b = base_heartbeat(0);
        b.content_id = "movie-b".into();
        s.ingest(a).unwrap();
        s.ingest(b).unwrap();
        assert_eq!(s.active_sessions(), 2);
    }

    #[test]
    fn ttff_is_gap_from_first_to_first_non_startup_heartbeat() {
        let mut s = HeartbeatSessionizer::default();
        let mut startup = base_heartbeat(0);
        startup.is_startup = true;
        s.ingest(startup).unwrap();

        let mut startup2 = base_heartbeat(2);
        startup2.is_startup = true;
        s.ingest(startup2).unwrap();

        // At t=5s the first frame renders.
        let first_frame = base_heartbeat(5);
        s.ingest(first_frame).unwrap();

        let events = s.drain();
        assert_eq!(events.len(), 1);
        let m = events[0].metrics.clone().unwrap();
        assert_eq!(m.time_to_first_frame_ms, 5000);
        assert_eq!(m.startup_failure_rate, 0.0);
    }

    #[test]
    fn startup_failure_when_never_leaves_startup() {
        let mut s = HeartbeatSessionizer::default();
        for i in 0..4 {
            let mut hb = base_heartbeat(i * 10);
            hb.is_startup = true;
            s.ingest(hb).unwrap();
        }
        let events = s.drain();
        let m = events[0].metrics.clone().unwrap();
        assert_eq!(m.startup_failure_rate, 1.0);
        assert_eq!(m.time_to_first_frame_ms, 0);
    }

    #[test]
    fn rebuffer_count_is_false_to_true_transitions() {
        let mut s = HeartbeatSessionizer::default();
        // Sequence: playing, playing, rebuffering, rebuffering, playing, rebuffering, playing
        // Transitions false->true: hb[1]->hb[2] and hb[4]->hb[5] = 2
        let pattern = [false, false, true, true, false, true, false];
        for (i, is_reb) in pattern.iter().enumerate() {
            let mut hb = base_heartbeat((i as i64) * 10);
            hb.is_rebuffering = *is_reb;
            s.ingest(hb).unwrap();
        }
        let events = s.drain();
        let m = events[0].metrics.clone().unwrap();
        assert_eq!(m.rebuffer_count, 2);
    }

    #[test]
    fn rebuffer_ratio_approximates_total_rebuffer_duration() {
        let mut s = HeartbeatSessionizer::default();
        // 6 heartbeats, 10s apart: [ok, reb, reb, ok, reb, ok]
        //   ok->reb at t=10s  (10s of attributed rebuffer: 10->20)
        //   reb->reb at t=20s (10s more: 20->30)
        //   reb->ok at t=30s  (attributed because PREVIOUS was rebuffering, so 30->40... wait no)
        //
        // Attribution rule: on apply(new_hb), if prev_is_rebuffering we add (new_ts - prev_ts)
        // Sequence with apply semantics:
        //   open at t=0   prev=false
        //   apply t=10 reb  prev=false→true (no rebuffer duration added; prev was false)
        //   apply t=20 reb  prev=true→true (+10_000ms)
        //   apply t=30 ok   prev=true→false (+10_000ms — session leaves rebuffering)
        //   apply t=40 reb  prev=false→true (no rebuffer added)
        //   apply t=50 ok   prev=true→false (+10_000ms)
        // Total rebuffer_ms = 30_000; total duration = 50_000; ratio = 0.6
        let pattern = [false, true, true, false, true, false];
        for (i, is_reb) in pattern.iter().enumerate() {
            let mut hb = base_heartbeat((i as i64) * 10);
            hb.is_rebuffering = *is_reb;
            s.ingest(hb).unwrap();
        }
        let events = s.drain();
        let m = events[0].metrics.clone().unwrap();
        assert!((m.rebuffer_ratio - 0.6).abs() < 1e-9, "got {}", m.rebuffer_ratio);
        assert_eq!(m.playback_duration_ms, 50_000);
    }

    #[test]
    fn avg_bitrate_computed_across_heartbeats() {
        let mut s = HeartbeatSessionizer::default();
        let bitrates = [1000, 2000, 3000, 4000];
        for (i, br) in bitrates.iter().enumerate() {
            let mut hb = base_heartbeat((i as i64) * 10);
            hb.current_bitrate_kbps = *br;
            s.ingest(hb).unwrap();
        }
        let m = s.drain()[0].metrics.clone().unwrap();
        assert_eq!(m.avg_bitrate_kbps, 2500);
    }

    #[test]
    fn resolution_switches_count_changes() {
        let mut s = HeartbeatSessionizer::default();
        let heights = [720, 720, 1080, 1080, 480, 1080];
        for (i, h) in heights.iter().enumerate() {
            let mut hb = base_heartbeat((i as i64) * 10);
            hb.current_resolution_height = *h;
            s.ingest(hb).unwrap();
        }
        let m = s.drain()[0].metrics.clone().unwrap();
        // Switches: 720->1080, 1080->480, 480->1080 = 3
        assert_eq!(m.resolution_switches, 3);
        assert_eq!(m.peak_resolution_height, 1080);
    }

    #[test]
    fn peak_resolution_tracks_max() {
        let mut s = HeartbeatSessionizer::default();
        for (i, h) in [480, 1080, 2160, 720].iter().enumerate() {
            let mut hb = base_heartbeat((i as i64) * 10);
            hb.current_resolution_height = *h;
            s.ingest(hb).unwrap();
        }
        let m = s.drain()[0].metrics.clone().unwrap();
        assert_eq!(m.peak_resolution_height, 2160);
    }

    #[test]
    fn playback_duration_is_last_minus_first() {
        let mut s = HeartbeatSessionizer::default();
        for i in 0..4 {
            s.ingest(base_heartbeat(i * 10)).unwrap();
        }
        let m = s.drain()[0].metrics.clone().unwrap();
        assert_eq!(m.playback_duration_ms, 30_000);
    }

    #[test]
    fn drain_emits_all_open_sessions_and_clears_state() {
        let mut s = HeartbeatSessionizer::default();
        let mut a = base_heartbeat(0);
        a.user_id = "alice".into();
        let mut b = base_heartbeat(0);
        b.user_id = "bob".into();
        s.ingest(a).unwrap();
        s.ingest(b).unwrap();
        let events = s.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(s.active_sessions(), 0);
    }

    #[test]
    fn flush_expired_emits_only_stale_sessions() {
        let mut s = HeartbeatSessionizer::default();
        let mut fresh = base_heartbeat(0);
        fresh.user_id = "fresh".into();
        let mut stale = base_heartbeat(-120); // 2 minutes ago
        stale.user_id = "stale".into();
        s.ingest(stale).unwrap();
        s.ingest(fresh).unwrap();
        let now = Utc::now();
        let events = s.flush_expired(now);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].user_id, "stale");
        assert_eq!(s.active_sessions(), 1);
    }

    #[test]
    fn out_of_order_heartbeat_is_dropped() {
        let mut s = HeartbeatSessionizer::default();
        s.ingest(base_heartbeat(20)).unwrap();
        // Arrives with a timestamp before the last-seen heartbeat for this key.
        let result = s.ingest(base_heartbeat(10)).unwrap();
        assert!(result.is_none());
        assert_eq!(s.dropped_out_of_order(), 1);
        assert_eq!(s.active_sessions(), 1);
    }

    #[test]
    fn invalid_heartbeat_returns_error() {
        let mut s = HeartbeatSessionizer::default();
        let mut bad = base_heartbeat(0);
        bad.user_id = String::new();
        let err = s.ingest(bad).unwrap_err();
        assert!(matches!(err, experimentation_core::error::Error::Validation(_)));
        assert_eq!(s.active_sessions(), 0);
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn nan_buffer_health_panics_at_validation() {
        let mut s = HeartbeatSessionizer::default();
        let mut bad = base_heartbeat(0);
        bad.buffer_health_seconds = f64::NAN;
        // Panics inside validate_heartbeat_event via assert_finite.
        let _ = s.ingest(bad);
    }

    #[test]
    fn emitted_event_passes_qoe_validation() {
        let mut s = HeartbeatSessionizer::default();
        // Mix rebuffering + resolution changes to exercise aggregation paths.
        let hbs = [
            (0, false, 720),
            (10, false, 1080),
            (20, true, 1080),
            (30, false, 1080),
        ];
        for (off, reb, h) in hbs {
            let mut hb = base_heartbeat(off);
            hb.is_rebuffering = reb;
            hb.current_resolution_height = h;
            s.ingest(hb).unwrap();
        }
        let event = s.drain().into_iter().next().unwrap();
        // The emitted event must round-trip through the QoE validator.
        crate::validation::validate_qoe_event(&event).unwrap();
    }

    #[test]
    fn custom_session_gap_is_respected() {
        let mut s = HeartbeatSessionizer::with_config(SessionizerConfig {
            session_gap: Duration::seconds(5),
        });
        s.ingest(base_heartbeat(0)).unwrap();
        // 7-second gap > 5-second custom threshold → should emit.
        let emitted = s.ingest(base_heartbeat(7)).unwrap();
        assert!(emitted.is_some());
    }

    #[test]
    fn server_synthesizes_session_id_when_client_omits() {
        let mut s = HeartbeatSessionizer::default();
        let mut hb = base_heartbeat(0);
        hb.session_id = String::new();
        s.ingest(hb).unwrap();
        let event = s.drain().into_iter().next().unwrap();
        assert!(event.session_id.starts_with("srv-sess-"));
    }

    #[test]
    fn rebuffer_count_cumulative_across_many_transitions() {
        let mut s = HeartbeatSessionizer::default();
        // Alternate every heartbeat: 5 false→true transitions across 10 heartbeats.
        for i in 0..10 {
            let mut hb = base_heartbeat(i * 10);
            hb.is_rebuffering = i % 2 == 1;
            s.ingest(hb).unwrap();
        }
        let m = s.drain()[0].metrics.clone().unwrap();
        assert_eq!(m.rebuffer_count, 5);
    }

    #[test]
    fn gap_boundary_starts_fresh_session_with_new_heartbeat_values() {
        let mut s = HeartbeatSessionizer::default();
        // First session: two heartbeats at 1000 kbps
        let mut h1 = base_heartbeat(0);
        h1.current_bitrate_kbps = 1000;
        s.ingest(h1).unwrap();
        let mut h2 = base_heartbeat(10);
        h2.current_bitrate_kbps = 1000;
        s.ingest(h2).unwrap();

        // Gap → session 1 emitted. New session at 4000 kbps.
        let mut h3 = base_heartbeat(60);
        h3.current_bitrate_kbps = 4000;
        let emitted = s.ingest(h3).unwrap().unwrap();
        let m = emitted.metrics.unwrap();
        assert_eq!(m.avg_bitrate_kbps, 1000, "first session avg");

        // New session's avg is 4000 on drain.
        let drained = s.drain();
        assert_eq!(drained.len(), 1);
        let m2 = drained[0].metrics.clone().unwrap();
        assert_eq!(m2.avg_bitrate_kbps, 4000);
    }
}
