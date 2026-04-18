## Summary

M2's event ingestion pipeline currently expects structured `PlaybackMetrics` events — discrete, pre-aggregated playback session records. The SFD requirements specify support for raw 10-second heartbeat ingestion with server-side sessionization into discrete playback sessions. This gap means the platform depends on client-side sessionization logic, which varies across platforms and is harder to validate centrally.

## Problem

Clients currently must aggregate their own heartbeats into a single `QoEEvent` with a complete `PlaybackMetrics` payload before sending to M2. This creates three issues:

1. **Client complexity**: Every SDK (Web, iOS, Android, CTV) must implement session boundary detection, which leads to inconsistency across platforms.
2. **Data loss**: If the app crashes mid-session, the aggregated event is never sent. Raw heartbeats would have already been transmitted up to the last 10-second interval.
3. **Flexibility**: Server-side sessionization allows retroactive changes to session definitions (e.g., adjusting the inactivity gap threshold) without client updates.

## Proposed Solution

Add a heartbeat sessionization layer in M2's Rust ingestion pipeline, upstream of the existing `QoEEvent` validation.

### New Proto Message

```protobuf
// In proto/experimentation/common/v1/qoe.proto

message HeartbeatEvent {
  string user_id = 1;
  string session_id = 2;           // Client-generated session ID (best-effort)
  string device_id = 3;
  google.protobuf.Timestamp timestamp = 4;
  int32 current_bitrate_kbps = 5;
  int32 current_resolution_height = 6;
  double buffer_health_seconds = 7; // Seconds of video buffered ahead
  bool is_rebuffering = 8;          // True if playback is stalled
  bool is_startup = 9;              // True if TTFF has not yet occurred
  string content_id = 10;
  string variant_id = 11;           // Experiment variant (if assigned)
}
```

### New Kafka Topic

`heartbeat_events` (128 partitions, keyed by `user_id + device_id`). High partition count to handle the volume — 10-second intervals × active concurrent viewers.

### Sessionization Logic (M2 Rust)

Implement a `HeartbeatSessionizer` in `crates/experimentation-ingest/`:

1. Buffer heartbeats in memory keyed by `(user_id, device_id, content_id)`.
2. Detect session boundaries: gap > 30 seconds (configurable) between consecutive heartbeats for the same key → emit a completed `PlaybackMetrics` event.
3. Compute aggregated fields from the heartbeat stream:
   - `time_to_first_frame_ms`: timestamp of first heartbeat where `is_startup == false` minus session start
   - `rebuffer_count`: number of transitions from `is_rebuffering == false` to `true`
   - `rebuffer_ratio`: total rebuffering duration / total playback duration
   - `avg_bitrate_kbps`: mean of `current_bitrate_kbps` across all heartbeats
   - `resolution_switches`: count of changes in `current_resolution_height`
   - `peak_resolution_height`: max of `current_resolution_height`
   - `playback_duration_ms`: (last heartbeat timestamp - first heartbeat timestamp)
4. Emit the aggregated `QoEEvent` to the existing `qoe_events` Kafka topic.
5. Handle crash recovery: on restart, in-flight sessions are lost (acceptable — the gap in heartbeats naturally closes the session on the next heartbeat from the same user).

### Volume Estimate

At 100K concurrent viewers × 1 heartbeat per 10 seconds = 10K events/sec sustained. The existing M2 ingestion benchmark shows 100K events/sec capacity, so heartbeats fit within the existing throughput budget.

## Acceptance Criteria

- [ ] `HeartbeatEvent` proto message added to `qoe.proto`
- [ ] `heartbeat_events` Kafka topic configured (128 partitions)
- [ ] `HeartbeatSessionizer` in `experimentation-ingest` aggregates heartbeats into `PlaybackMetrics`
- [ ] Configurable session gap threshold (default 30s)
- [ ] Emitted `QoEEvent` is indistinguishable from a client-aggregated event (downstream M3 pipeline unchanged)
- [ ] Benchmark: 10K heartbeats/sec sustained at p99 < 20ms
- [ ] Integration test: send 100 heartbeats for 3 sessions (2 complete, 1 crash-interrupted), verify 2 `QoEEvent` outputs with correct aggregation
- [ ] `cargo test -p experimentation-ingest` passes

## Agent

Agent-2 (Event Pipeline)

## Sprint

Suggested: Sprint 5.1 (alongside other M2 work for ADR-021 ModelRetrainingEvent)

## Dependencies

- Proto schema change (additive — `HeartbeatEvent` is a new message, no breaking changes)
- No dependency on other Phase 5 ADRs

## Labels

`agent-2`, `P1`, `cluster-a`
