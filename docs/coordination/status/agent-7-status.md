# Agent-7 Status — Phase 5

**Module**: M7 Flags
**Last updated**: 2026-03-23

## Current Sprint

Sprint: 5.0
Focus: ADR-024 M7 Rust port
Branch: work/lively-badger

## In Progress

- [ ] Phase 2: PromoteToExperiment (M5 tonic client), audit trail write, stale flag detection (ADR-024)
  - Blocked by: none
  - ETA: next sprint

## Completed (Phase 5)

- [x] **ADR-024 Phase 1**: `crates/experimentation-flags/` scaffold
  - Cargo workspace member added
  - `tonic` + `tonic-web` gRPC server with `accept_http1(true)` for JSON HTTP mode
  - `sqlx` PostgreSQL store (runtime queries, matches project pattern from M4a)
  - Flag CRUD: CreateFlag, GetFlag, UpdateFlag, DeleteFlag, ListFlags
  - EvaluateFlag / EvaluateFlags: direct `experimentation_hash::bucket()` call — no CGo, no FFI
  - PromoteToExperiment: stub returning `UNIMPLEMENTED` (Phase 2)
  - Wire-format contract tests (13 tests, all green):
    - Proto3 binary round-trips for Flag, EvaluateFlagResponse, ListFlagsResponse
    - FlagType enum value correctness
    - Proto3 zero-value handling
    - Evaluation bucket logic parity vs Go (rollout 0%, rollout 100%, bucket determinism)
    - Pagination token base64 format
    - Optional live parity test gate (GO_M7_ADDR + RUST_M7_ADDR env vars)

## Blocked

_None._

## Next Up

- Phase 2: PromoteToExperiment (M5 tonic client), flag audit trail (PostgreSQL writes), stale flag view query
- Phase 3: rdkafka Kafka consumer for experiment conclusion events, reconciler
- Phase 4: Port 13 chaos tests, k6 20K rps load test, shadow traffic comparison, then delete `experimentation-ffi`

## Dependencies

- `experimentation-hash`: direct dep (same source, zero parity risk) — ADR-024 §Architecture
- `experimentation-proto`: flags proto compiled by `experimentation-proto` build.rs
- `experimentation-ffi`: NOT a dependency — this is the crate ADR-024 will delete after Phase 4 cutover
- M5 Management (Agent-5): needed in Phase 2 for PromoteToExperiment RPC client

## PR

- Phase 1 PR: `work/lively-badger` → `main`
