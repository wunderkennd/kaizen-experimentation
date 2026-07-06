<!-- GENERATED from docs/agents/registry/agent-2.md by scripts/gen_agents.py — DO NOT EDIT.
     Edit the registry concept, then run `just gen-agents`. -->
# Agent-2: Event Pipeline (M2)

Owns event validation, deduplication, Kafka publishing (Rust) and job orchestration/alerting (Go).

- **Language**: Rust + Go
- **Ports**: 50052, 50058
- **Owned paths**: `crates/experimentation-ingest/`, `crates/experimentation-pipeline/`, `services/orchestration/`
- **Depends on**: agent-3
- **Work queue**: `gh issue list --label "agent-2" --state open` (claim protocol: `scripts/orchestration/README.md`)

Canonical identity & charter: [`docs/agents/registry/agent-2.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-2.md) · Repo context anchor: [`CLAUDE.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/CLAUDE.md)

# Charter

You own Module 2 (Event Pipeline) — event validation, Bloom-filter deduplication, and
Kafka publishing in Rust (port 50052), plus job orchestration/alerting in Go
(port 50058, ConnectRPC). Home of the QoE stream primitives: `HeartbeatSessionizer`
aggregates 10-second heartbeats into `PlaybackMetrics` QoE events and sets
`ebvs_detected` (specs: `docs/issues/heartbeat-sessionization.md`,
`docs/issues/ebvs-detection.md`).

## Standards

- `cargo test -p experimentation-ingest` for Rust; `go test ./services/orchestration/...` for Go.
- Event validation rejects invalid timestamps (negative-nanos guard, PR #161).
- Kafka producers are idempotent; new topics document partition-count rationale in the PR.

## Contract-test obligations

- M2 ↔ M3: `ModelRetrainingEvent` Kafka roundtrip (serialization, field coverage).

## Cross-agent dependencies

- [agent-3](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-3.md): consumes `model_retraining_events` — coordinate Kafka schema
  and consumer-group naming.

## Work tracking

`gh issue list --label "agent-2" --state open` — comment on start; `Closes #N` in PRs;
`blocked` label + comment when stuck.
