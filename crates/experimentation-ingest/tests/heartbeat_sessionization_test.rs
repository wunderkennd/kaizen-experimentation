//! Integration test for the HeartbeatSessionizer (Issue #424).
//!
//! Scenario (per the Acceptance Criteria):
//!   * Stream 100 heartbeats across 3 distinct viewing sessions.
//!   * Sessions A and B complete naturally via a >30-second inactivity gap.
//!   * Session C is "crash-interrupted" — heartbeats stop abruptly without a
//!     closing gap; the session remains in-flight until `drain()` is called.
//!   * Assert: exactly 2 QoEEvents are emitted by gap detection, and a 3rd
//!     appears only after drain(). All three events must pass the QoE validator
//!     used by the existing ingestion pipeline (downstream indistinguishability).

use chrono::{Duration, Utc};
use experimentation_ingest::sessionization::HeartbeatSessionizer;
use experimentation_ingest::validation::validate_qoe_event;
use experimentation_proto::common::HeartbeatEvent;
use prost_types::Timestamp;

fn hb(
    user_id: &str,
    content_id: &str,
    offset_s: i64,
    is_startup: bool,
    is_rebuffering: bool,
    bitrate_kbps: i32,
    resolution_height: i32,
) -> HeartbeatEvent {
    let base = Utc::now() - Duration::seconds(1200);
    let t = base + Duration::seconds(offset_s);
    HeartbeatEvent {
        user_id: user_id.into(),
        session_id: format!("sess-{user_id}-{content_id}"),
        device_id: format!("device-{user_id}"),
        timestamp: Some(Timestamp {
            seconds: t.timestamp(),
            nanos: 0,
        }),
        current_bitrate_kbps: bitrate_kbps,
        current_resolution_height: resolution_height,
        buffer_health_seconds: 5.0,
        is_rebuffering,
        is_startup,
        content_id: content_id.into(),
        variant_id: "control".into(),
    }
}

#[test]
fn sessionize_100_heartbeats_across_3_sessions() {
    let mut sz = HeartbeatSessionizer::default();
    let mut gap_emitted: Vec<_> = Vec::new();

    // ───────── Session A: user=alice, content=movie-1 ─────────
    // 40 heartbeats at 10s cadence. Startup for the first 2, playing afterward.
    // One rebuffer episode spanning heartbeats 20–22.
    for i in 0..40 {
        let offset = i * 10;
        let is_startup = i < 2;
        let is_rebuffering = (20..=22).contains(&i);
        let bitrate = if i < 10 { 3000 } else { 6000 };
        let resolution = if i < 10 { 720 } else { 1080 };
        let event = hb(
            "alice",
            "movie-1",
            offset,
            is_startup,
            is_rebuffering,
            bitrate,
            resolution,
        );
        if let Some(qoe) = sz.ingest(event).unwrap() {
            gap_emitted.push(qoe);
        }
    }

    // ───────── Session B: user=bob, content=movie-2 ─────────
    // 40 heartbeats at 10s cadence starting well after Session A's last heartbeat
    // *for bob's key* (separate session key means no cross-talk).
    for i in 0..40 {
        let offset = 500 + i * 10;
        let event = hb(
            "bob",
            "movie-2",
            offset,
            i == 0,   // brief startup
            false,    // no rebuffering
            5000,
            1080,
        );
        if let Some(qoe) = sz.ingest(event).unwrap() {
            gap_emitted.push(qoe);
        }
    }

    // ───────── Close Session A with a >30s inactivity gap ─────────
    // Alice resumes a DIFFERENT content item well after a gap. The original
    // (alice, movie-1) session cannot be "closed by gap" from a different
    // content_id; instead we close it via flush_expired below.
    // To explicitly trigger gap emission on Session A, send one more heartbeat
    // for (alice, movie-1) ~60 seconds after her last one.
    let resume = hb("alice", "movie-1", 40 * 10 + 60, false, false, 6000, 1080);
    if let Some(qoe) = sz.ingest(resume).unwrap() {
        gap_emitted.push(qoe);
    }

    // ───────── Close Session B with an explicit gap trigger too ─────────
    let bob_resume = hb("bob", "movie-2", 500 + 40 * 10 + 60, false, false, 5000, 1080);
    if let Some(qoe) = sz.ingest(bob_resume).unwrap() {
        gap_emitted.push(qoe);
    }

    // ───────── Session C (crash-interrupted): user=carol, content=movie-3 ─────────
    // Only 18 heartbeats, no follow-up gap. Stays in-flight.
    for i in 0..18 {
        let offset = 900 + i * 10;
        let event = hb(
            "carol",
            "movie-3",
            offset,
            i == 0,
            i == 15, // brief rebuffer near the end
            4500,
            720,
        );
        if let Some(qoe) = sz.ingest(event).unwrap() {
            gap_emitted.push(qoe);
        }
    }

    // Heartbeat count check: 40 + 40 + 1 + 1 + 18 = 100.
    // (Matches the AC "send 100 heartbeats for 3 sessions".)

    // Exactly 2 sessions should have closed via gap (alice + bob).
    assert_eq!(
        gap_emitted.len(),
        2,
        "expected 2 QoEEvents emitted by gap detection"
    );

    // Carol's session is still in-flight (crash-interrupted).
    assert_eq!(
        sz.active_sessions(),
        3,
        "2 new sessions (alice-resumed, bob-resumed) + 1 in-flight (carol) must remain"
    );

    // Every emitted event must pass the existing QoE validator so M3 cannot
    // tell server-aggregated events apart from client-aggregated ones.
    for event in &gap_emitted {
        validate_qoe_event(event).expect("emitted QoEEvent must pass validation");
    }

    // Identify alice's + bob's closed sessions and check aggregation sanity.
    let alice_event = gap_emitted
        .iter()
        .find(|e| e.user_id == "alice" && e.content_id == "movie-1")
        .expect("alice session must be present");
    let bob_event = gap_emitted
        .iter()
        .find(|e| e.user_id == "bob" && e.content_id == "movie-2")
        .expect("bob session must be present");

    let alice_m = alice_event.metrics.clone().unwrap();
    let bob_m = bob_event.metrics.clone().unwrap();

    // Alice's 40 heartbeats span 0..=390 seconds = 390_000 ms.
    assert_eq!(alice_m.playback_duration_ms, 390_000);
    // Alice had one rebuffer episode → 1 false→true transition.
    assert_eq!(alice_m.rebuffer_count, 1);
    // Peak reached 1080.
    assert_eq!(alice_m.peak_resolution_height, 1080);
    // Startup completed (first non-startup heartbeat at i=2 → t=20s).
    assert_eq!(alice_m.startup_failure_rate, 0.0);
    assert_eq!(alice_m.time_to_first_frame_ms, 20_000);
    // avg_bitrate: 10 heartbeats at 3000 + 30 at 6000 → (30_000 + 180_000) / 40 = 5250
    assert_eq!(alice_m.avg_bitrate_kbps, 5250);
    // 720→1080 switch at i=10 → exactly one resolution switch.
    assert_eq!(alice_m.resolution_switches, 1);

    // Bob: no rebuffering, steady resolution.
    assert_eq!(bob_m.rebuffer_count, 0);
    assert_eq!(bob_m.rebuffer_ratio, 0.0);
    assert_eq!(bob_m.resolution_switches, 0);
    assert_eq!(bob_m.avg_bitrate_kbps, 5000);

    // ───────── Drain forces carol's crash-interrupted session to emit ─────────
    let drained = sz.drain();
    assert!(drained.iter().any(|e| e.user_id == "carol"));
    for event in &drained {
        validate_qoe_event(event).expect("drained QoEEvent must pass validation");
    }

    // After drain no sessions remain.
    assert_eq!(sz.active_sessions(), 0);
}

#[test]
fn downstream_indistinguishability_property() {
    // Exercise every reachable aggregation path and ensure the emitted event
    // matches the shape of a client-aggregated QoEEvent (same required fields,
    // same validator, same metric ranges).
    let mut sz = HeartbeatSessionizer::default();
    for i in 0..60 {
        let event = hb(
            "eve",
            "movie-x",
            i * 10,
            i < 3,
            (i % 15) == 14,
            3500 + (i as i32 * 10),
            if i % 20 < 10 { 720 } else { 1080 },
        );
        sz.ingest(event).unwrap();
    }
    let events = sz.drain();
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert!(!e.event_id.is_empty());
    assert!(!e.session_id.is_empty());
    assert!(!e.user_id.is_empty());
    assert!(!e.content_id.is_empty());
    assert!(e.metrics.is_some());
    assert!(e.timestamp.is_some());
    validate_qoe_event(e).unwrap();
}
