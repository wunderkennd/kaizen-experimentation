# Agent-5 Status — Phase 5

**Module**: M5 Management
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.3
Focus: ADR-025 Phase 2 — M5 Rust port lifecycle and orchestration
Branch: work/happy-panda
PR: (pending)

## In Progress

- [x] ADR-025 Phase 2 — `crates/experimentation-management/` Rust crate (this PR)

## Completed (Phase 5)

- [x] ADR-020 Phase 1 — Rust stats module + M5 scheduler (PR #227)
  - `crates/experimentation-stats/src/adaptive_n.rs`
  - `sql/migrations/008_adaptive_sample_size_audit.sql`
  - `services/management/internal/adaptive/`: Trigger, Processor, ConditionalPowerClient interface

- [x] ADR-025 Phase 2 — Rust management crate (this PR)
  - `crates/experimentation-management/` — new crate added to workspace
  - **State machine**: DRAFT→STARTING→RUNNING→PAUSED→CONCLUDING→CONCLUDED→ARCHIVED
    - TOCTOU-safe via `UPDATE … WHERE state=$expected`, check `rows_affected()==1`
    - `src/state_machine.rs`: valid edge graph + `transition()` function
  - **STARTING validators**: META (reward weights, variant count), SWITCHBACK (cycles≥4, block≥1h), QUASI (2+ donors, temporal ordering)
    - `src/validators.rs`: `validate_starting()` dispatch
  - **Guardrail Kafka consumer**: subscribes to `guardrail_alerts`, auto-pauses via TOCTOU-safe `pause_transition()`, publishes to `experiment_lifecycle`
    - `src/kafka.rs`
  - **Bucket reuse allocator**: overlap detection (SQL: `start_bucket < $end AND $start < end_bucket`), cooldown enforcement
    - `src/bucket_reuse.rs`
  - **StreamConfigUpdates tonic streaming RPC**: broadcast channel + active-experiment backfill on reconnect
    - Added to `management_service.proto`
    - Implemented in `src/grpc.rs`
  - **Proptest**: 5 proptest properties + 5 regular tests for concurrent state transitions
    - `tests/lifecycle_proptest.rs`
  - **SQL migration**: `sql/migrations/009_management_phase5.sql` (PAUSED state, Phase 5 types, AVLM method)
  - **Proto**: `EXPERIMENT_STATE_PAUSED = 7` in experiment.proto

## Blocked

- **Agent-4 dependency (unchanged)**: `ConditionalPowerClient` interface at
  `services/management/internal/adaptive/processor.go:62` needs a gRPC wrapper
  around M4a's `ComputeConditionalPower` RPC.

## Next Up

- ADR-018 Phase 2: e-LOND OnlineFdrController singleton + PostgreSQL persistence
- ADR-019 Portfolio optimization: ExperimentLearning classification, traffic allocation optimizer
- StreamConfigUpdates M1 client update: `stream_client.rs` uses `AssignmentServiceClient`
  but should use `ManagementServiceClient` — cross-agent PR needed with Agent-1
