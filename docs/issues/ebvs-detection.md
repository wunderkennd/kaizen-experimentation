## Summary

The SFD requirements call for detection of EBVS (Exit Before Video Start) as distinct from standard churn or exit events. Kaizen's `PlaybackMetrics` proto captures the relevant signals (`time_to_first_frame_ms`, `playback_duration_ms`, `startup_failure_rate`) but does not codify EBVS as a first-class classification. This means EBVS analysis requires ad-hoc SQL rather than a queryable field, and guardrails cannot trigger on EBVS rate directly.

## Problem

EBVS is one of the most critical QoE failure modes for streaming platforms — a user attempted to watch content but left before video playback began. It signals a fundamentally broken experience. Currently:

1. **No dedicated field**: EBVS is latent in the data (sessions where `playback_duration_ms == 0` or `time_to_first_frame_ms > threshold`) but not explicitly classified.
2. **Guardrails can't target it**: You can set a guardrail on `startup_failure_rate` but not on EBVS specifically. A session where the user voluntarily exits during a slow start (EBVS) is different from a session where the player crashes (startup failure).
3. **Metric definitions are ambiguous**: A "Qualified Play" requires distinguishing EBVS from short plays (user started content, watched 2 seconds, and left). Without classification, minimum watch-time thresholds are the only proxy.

## Proposed Solution

Three changes, all additive:

### 1. Proto Extension

Add `ebvs_detected` to `PlaybackMetrics`:

```protobuf
// In proto/experimentation/common/v1/qoe.proto

message PlaybackMetrics {
  // ... existing 8 fields ...

  // True if the session ended before video playback began.
  // Classification: time_to_first_frame_ms > 0 AND playback_duration_ms == 0,
  // OR time_to_first_frame_ms exceeds the configured EBVS threshold.
  bool ebvs_detected = 9;
}
```

### 2. Classification Logic (M2 or Sessionizer)

If using the existing client-aggregated `QoEEvent` flow, the client sets `ebvs_detected` based on its local state (knows whether playback started).

If using the heartbeat sessionization pipeline (see heartbeat sessionization Issue), the `HeartbeatSessionizer` classifies EBVS during aggregation:

```rust
let ebvs = session.first_frame_timestamp.is_none()  // never reached first frame
    || (session.ttff_ms > config.ebvs_threshold_ms   // TTFF exceeded threshold
        && session.playback_duration_ms == 0);         // no actual playback
```

Configurable `ebvs_threshold_ms` (default: 10,000ms / 10 seconds). If TTFF exceeds this and the user left, it's EBVS.

### 3. Metric Definition

Add a platform-level metric definition for EBVS rate:

```sql
-- EBVS Rate: fraction of sessions that are EBVS
-- Metric type: PROPORTION
-- Aggregation: per-user (number of EBVS sessions / total sessions)
SELECT
  user_id,
  variant_id,
  CAST(SUM(CASE WHEN ebvs_detected THEN 1 ELSE 0 END) AS DOUBLE) / COUNT(*) AS ebvs_rate
FROM qoe_metrics
WHERE experiment_id = ?
GROUP BY user_id, variant_id
```

This metric can then be used as:
- A guardrail metric (auto-pause if EBVS rate increases by > X%)
- A primary metric for playback infrastructure experiments
- A segmentation dimension (EBVS users vs. non-EBVS users)

### 4. M6 Integration

- QoE dashboard: add EBVS rate as a time-series panel alongside rebuffer ratio and TTFF
- Guardrail configuration: EBVS rate available as a guardrail metric in the experiment creation form

## Acceptance Criteria

- [ ] `ebvs_detected` field added to `PlaybackMetrics` proto (field number 9)
- [ ] `buf lint` and `buf breaking` pass (additive change)
- [ ] If heartbeat sessionization is implemented: EBVS classification in `HeartbeatSessionizer`
- [ ] If client-aggregated flow: SDK documentation updated with EBVS classification guidance
- [ ] EBVS rate SQL template added to M3: `ebvs_rate.sql.tmpl`
- [ ] EBVS rate registered as a platform-level metric definition (type: PROPORTION)
- [ ] EBVS rate available as a guardrail metric option in M5
- [ ] M6 QoE dashboard includes EBVS rate time-series panel
- [ ] Integration test: send QoE events with `ebvs_detected = true/false`, verify EBVS rate metric computation
- [ ] `cargo test --workspace` and `go test ./...` pass

## Agent

- Agent-4 (proto change, since proto changes are centralized in Phase 5)
- Agent-2 (classification logic in M2/sessionizer)
- Agent-3 (SQL template + metric definition)
- Agent-6 (QoE dashboard update)

## Sprint

Suggested: Sprint 5.1 (alongside QoE and telemetry work)

## Dependencies

- If heartbeat sessionization is implemented, EBVS classification should be built into the sessionizer. Otherwise, clients set the field.
- No dependency on other Phase 5 ADRs.

## Labels

`agent-2`, `agent-3`, `agent-4`, `agent-6`, `P1`
