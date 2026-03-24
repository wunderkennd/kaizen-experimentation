# Agent-5 Status — Phase 5

**Module**: M5 Management
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.3
Focus: ADR-025 M5 Rust Port — Phase 1 Scaffold
Branch: work/clever-eagle
PR: (pending — see below)

## In Progress

- [x] ADR-025 Phase 1 Scaffold (this PR)
  - `crates/experimentation-management/` created in Cargo workspace
  - All 20 RPCs from `ExperimentManagementService` proto stubbed
  - RBAC interceptor ported from Go (two-phase auth/authz design)
  - Lifecycle state machine implemented (DRAFT→STARTING→RUNNING→CONCLUDING→CONCLUDED→ARCHIVED)
  - TOCTOU-safe PostgreSQL transitions via `UPDATE … WHERE state = $expected` / `rows_affected() == 1`
  - 15 unit tests passing (`cargo test -p experimentation-management`)

## Completed (Phase 5)

- [x] ADR-020 Phase 1 — Rust stats module + M5 scheduler (PR #227)
  - `crates/experimentation-stats/src/adaptive_n.rs`
  - `sql/migrations/008_adaptive_sample_size_audit.sql`
  - `services/management/internal/adaptive/`: Trigger, Processor, ConditionalPowerClient interface

- [x] ADR-025 Phase 1 — M5 Rust port scaffold (this sprint)
  - Decision trigger met: ADRs 018, 020, 021 all implemented ≥ threshold of 3
  - Crate: `crates/experimentation-management/`
  - Modules: `config`, `rbac`, `state`, `store`, `grpc`
  - RBAC: tonic interceptor (auth) + per-handler `require_role` (authz)
  - State machine: all 6 states, 7 valid transitions, `TransitionError` type
  - Store: `apply_transition()` with TOCTOU guard, `get_experiment()`, `list_experiments()`
  - Binary: `experimentation-management` gRPC server on port 50055

## Blocked

- (none currently)

## Next Up (Phase 1 → Phase 2)

- ADR-025 Phase 2: Full CRUD RPCs (CreateExperiment, UpdateExperiment, CreateLayer, CreateMetricDefinition, etc.)
  - Variant insertion, layer allocation, bucket reuse allocator
  - Kafka guardrail consumer (rdkafka, same pattern as M4b)
  - StreamConfigUpdates: tonic streaming RPC to M1
- ADR-025 Phase 3: Direct `experimentation-stats` integration
  - `OnlineFdrController` using `e_value_grow()` (ADR-018)
  - Portfolio optimizer using `power_analysis()`, `conditional_power()` (ADR-019)
  - Adaptive N trigger using `conditional_power()` (ADR-020)

## Architecture Notes (ADR-025)

- RBAC design: Go used ConnectRPC `req.Spec().Procedure` to check per-procedure role in the interceptor.
  Tonic 0.12 interceptors receive `tonic::Request<()>` with no URI access (HTTP/2 pseudo-headers not
  exposed at the gRPC layer). Resolution: interceptor handles auth (extract Identity), each handler
  calls `require_role(request.extensions(), min_role)` for authz. Identical access control matrix.
- TOCTOU: `UPDATE … WHERE state = $expected` + `rows_affected() == 1` is the standard PostgreSQL
  optimistic concurrency pattern. Matches the Go implementation semantically.
- `experimentation-stats` dependency added to Cargo.toml — not yet called in Phase 1.
  Phase 3 will add direct function calls for OnlineFdrController, portfolio optimizer, adaptive N.
