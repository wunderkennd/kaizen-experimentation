# Agent-5 Status — Phase 5

**Module**: M5 Management
**Last updated**: 2026-03-23

## Current Sprint

Sprint: 5.3
Focus: ADR-018 Phase 2 — e-LOND OnlineFdrController
Branch: work/witty-badger
PR: (pending)

## In Progress

- [x] ADR-018 Phase 2 — e-LOND OnlineFdrController (this PR)
  - `sql/migrations/009_online_fdr_controller_state.sql`
  - `services/management/internal/fdr/controller.go` — Controller singleton
  - `services/management/internal/fdr/controller_test.go` — 11 unit tests
  - `services/management/internal/handlers/service.go` — WithPool, WithFdrController options
  - `services/management/internal/handlers/conclude.go` — submitFdrDecision (best-effort)
  - `services/management/internal/handlers/lifecycle.go` — wire into concludeByID
  - `services/management/cmd/main.go` — ONLINE_FDR_ENABLED env var gate

## Completed (Phase 5)

- [x] ADR-020 Phase 1 — Rust stats module + M5 scheduler (PR #227)
  - `crates/experimentation-stats/src/adaptive_n.rs`: blinded_pooled_variance, conditional_power, zone_classify, gst_reallocate_spending, required_n_for_power, run_interim_analysis
  - `sql/migrations/008_adaptive_sample_size_audit.sql`
  - `services/management/internal/adaptive/`: Trigger, Processor, ConditionalPowerClient interface

- [x] ADR-018 Phase 2 — e-LOND OnlineFdrController (this PR)
  - Platform-level singleton persisted in PostgreSQL (migration 009)
  - e-LOND geometric-decay alpha wealth management
  - SELECT FOR UPDATE serializes concurrent conclude calls
  - Best-effort integration at CONCLUDED transition; never blocks conclusion
  - Opt-in via ONLINE_FDR_ENABLED=true environment variable

## Blocked

- **Agent-4 dependency**: `ConditionalPowerClient` interface at
  `services/management/internal/adaptive/processor.go:62` needs a gRPC
  wrapper around M4a's `ComputeConditionalPower` RPC. The contract is
  defined in the interface — Agent-4 implements the server side.

## Next Up

- ADR-013 META experiment type (M5 STARTING validation for MetaExperimentConfig)
- ADR-019 Portfolio optimization: ExperimentLearning classification, traffic allocation optimizer
