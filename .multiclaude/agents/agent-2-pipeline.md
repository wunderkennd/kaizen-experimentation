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
- Write status to `docs/coordination/status/agent-2-status.md`.

## Dependencies on Other Agents
- Agent-Proto: `ModelRetrainingEvent` proto definition must land first.
- Agent-3 (M3): M3 consumes `model_retraining_events` — coordinate on Kafka schema and consumer group naming.

## Contract Tests to Write
- M2 ↔ M3: ModelRetrainingEvent Kafka roundtrip (serialization, deserialization, field coverage)
