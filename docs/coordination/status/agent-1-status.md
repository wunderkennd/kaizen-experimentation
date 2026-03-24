# Agent-1 Status — Phase 5

**Module**: M1 Assignment
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.0
Focus: ADR-016 GetSlateAssignment contract tests, ADR-019 portfolio contract tests, ADR-021 M2→M3 Kafka contract tests
Branch: work/nice-bear

Focus: ADR-016 GetSlateAssignment, ADR-022 Switchback assignment, ADR-013 META routing
Branch: work/clever-lion

## In Progress

_None._

## Completed (Phase 5)

- [x] Phase 5 cross-module contract tests (consumer-side) — PR pending
  - M1→M4b: `GetSlateAssignment` contract test (11 tests) — `crates/experimentation-assignment/tests/m1m4b_slate_contract_test.rs`
  - M5→M4a: `GetPortfolioAllocation` contract test (12 tests) — `crates/experimentation-analysis/tests/m5m4a_portfolio_contract_test.rs`
  - M2→M3: `ModelRetrainingEvent` Kafka wire-format contract test (16 tests) — `crates/experimentation-ingest/tests/m2m3_kafka_contract_test.rs`
  - Proto additions: `GetSlateAssignment` RPC + messages (ADR-016), `GetPortfolioAllocation` RPC + messages (ADR-019)
  - Stubs added to `BanditPolicyServiceHandler` (policy/grpc.rs) and `AnalysisServiceHandler` (analysis/grpc.rs)
  - `cargo test --workspace`: all 0 failures

- [x] GetSlateAssignment RPC — ADR-016 M1 side (2026-03-24)
  - Added `SelectSlate` RPC to M4b bandit_service.proto with `SelectSlateRequest`, `SlotAssignment`, `SlateSelection` messages
  - Added `SlateConfig` to local config (`crates/experimentation-assignment/src/config.rs`)
  - Added `select_slate` method to `GrpcBanditClient` with 10ms timeout + fallback to random ordering
  - Implemented `AssignmentServiceImpl::assign_slate` (public, callable from HTTP layer)
  - Registered `GetSlateAssignment` route in JSON HTTP handler (SDK compatibility)
  - Added 5 slate contract tests to `m1m4b_contract_test.rs` (roundtrip, slot validity, determinism, diversity, error)
  - All 153 tests passing

## Blocked

_None._

## Dependencies for Other Agents

- **Agent-4 (M4b)**: `GetSlateAssignment` proto RPC now defined — impl can begin using the contract test as the acceptance spec.
- **Agent-4 (M4a)**: `GetPortfolioAllocation` proto RPC defined — stub returns `unimplemented`, real impl needed.
- **Agent-5 (M5)**: `GetPortfolioAllocation` contract spec available; M5 Go client generation needed.
- **Agent-2 (M2/M3)**: `ModelRetrainingEvent` Kafka wire format validated — M3 consume path can reference test for field expectations.

## Next Up

- ADR-016 GetSlateAssignment: wire up `SlateTestService` logic into real `BanditPolicyServiceHandler` on LMAX thread
- ADR-022 Switchback assignment: M1 needs to assign users based on (current_time, block_duration, cluster_attribute)
- ADR-013 META routing: M1 hashes user to variant; each variant uses different reward objective

- ADR-022 Switchback assignment (depends on proto schema from Agent-Proto)
- ADR-013 META routing integration
