# Agent-2: Event Pipeline

You own Module 2 (Event Pipeline) — event validation, deduplication, Kafka publishing (Rust), and job orchestration/alerting (Go).

Languages: Rust (ingestion) + Go (orchestration)
Crates: `crates/experimentation-ingest/`, `crates/experimentation-pipeline/`
Go service: `services/pipeline-orch/`
Service ports: 50052 (Rust gRPC), 50058 (Go ConnectRPC)

## Phase 5 ADR Responsibilities

### Primary Owner
- **ADR-021 (Feedback Loop Interference)**: Implement `ModelRetrainingEvent` ingestion. New Kafka topic `model_retraining_events` (8 partitions). Validate incoming events: `model_id`, `training_data_start/end`, `active_experiment_ids`. Publish to Kafka with idempotent producer. Add to Bloom filter dedup pipeline.

### Supporting Role
- None. Agent-2 has the lightest Phase 5 workload.

## Coding Standards
- Run `cargo test -p experimentation-ingest` for Rust changes.
- Run `go test ./services/pipeline-orch/...` for Go changes.
- Event validation must reject invalid timestamps (negative nanos guard from PR #161).
- Kafka topic creation: document partition count rationale in PR description.
## Work Tracking
Find your assigned work via GitHub Issues:
```bash
gh issue list --label "agent-2" --state open
gh issue view <number>
```
When starting work, comment on the Issue. When creating a PR, include `Closes #<number>`.
If blocked, add the `blocked` label and comment explaining the blocker.

## Dependencies on Other Agents
- Agent-Proto: `ModelRetrainingEvent` proto definition must land first.
- Agent-3 (M3): M3 consumes `model_retraining_events` — coordinate on Kafka schema and consumer group naming.

## Contract Tests to Write
- M2 ↔ M3: ModelRetrainingEvent Kafka roundtrip (serialization, deserialization, field coverage)

## Sprint 5.1 Add-On Work (post-Phase-5)

The SFD gap analysis added two new Sprint 5.1 items to your queue. Both extend the M2 ingestion pipeline.

### Sprint 5.1: Measurement Foundations
- **[#424](https://github.com/wunderkennd/kaizen-experimentation/issues/424) Heartbeat Sessionization** — sole owner. Add `HeartbeatEvent` proto, `heartbeat_events` Kafka topic (128 partitions), and `HeartbeatSessionizer` in `crates/experimentation-ingest/`. Aggregate 10-second heartbeats into the existing `PlaybackMetrics` `QoEEvent` shape. Spec: [`docs/issues/heartbeat-sessionization.md`](../../docs/issues/heartbeat-sessionization.md). Cluster A.
- **[#425](https://github.com/wunderkennd/kaizen-experimentation/issues/425) EBVS Detection** — classification logic in the `HeartbeatSessionizer` (you OR the client sets `ebvs_detected: bool`). Spec: [`docs/issues/ebvs-detection.md`](../../docs/issues/ebvs-detection.md). Multi-agent (Agent-3, -4, -6 own the rest).

Find them with `gh issue list --label "agent-2,sprint-5.1" --state open`.
