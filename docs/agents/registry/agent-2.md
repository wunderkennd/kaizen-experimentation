---
type: Kaizen Module Agent
title: "Agent-2: Event Pipeline (M2)"
description: Owns event validation, deduplication, Kafka publishing (Rust) and job orchestration/alerting (Go).
resource: https://github.com/wunderkennd/kaizen-experimentation/tree/main/crates/experimentation-ingest
tags: [module-agent, rust, go, kafka, ingestion, qoe]
timestamp: 2026-07-04T12:00:00Z
id: agent-2
label: agent-2
executors: [claude-workflow, claude-web, multiclaude]
language: Rust + Go
ports: [50052, 50058]
owned_paths:
  - crates/experimentation-ingest/
  - crates/experimentation-pipeline/
  - services/orchestration/
depends_on: [agent-3]
---

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

- [agent-3](/agent-3.md): consumes `model_retraining_events` — coordinate Kafka schema
  and consumer-group naming.

## Work tracking

`gh issue list --label "agent-2" --state open` — comment on start; `Closes #N` in PRs;
`blocked` label + comment when stuck.
