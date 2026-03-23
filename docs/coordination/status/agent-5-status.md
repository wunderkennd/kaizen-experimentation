# Agent-5 Status — Phase 5

**Module**: M5 Management
**Last updated**: 2026-03-23

## Current Sprint

Sprint: 5.2
Focus: ADR-020 Adaptive Sample Size
Branch: work/proud-bear
PR: https://github.com/wunderkennd/kaizen-experimentation/pull/227

## In Progress

- [x] ADR-020 Adaptive Sample Size (PR #227, pending merge)
  - Blocked by: Agent-4 — `ComputeConditionalPower` M4a RPC needed for live wiring
  - Statistical math complete; M5 scheduler complete; M4a delegation interface defined

## Completed (Phase 5)

- [x] ADR-020 Phase 1 — Rust stats module + M5 scheduler (PR #227)
  - `crates/experimentation-stats/src/adaptive_n.rs`: blinded_pooled_variance, conditional_power, zone_classify, gst_reallocate_spending, required_n_for_power, run_interim_analysis
  - `sql/migrations/008_adaptive_sample_size_audit.sql`
  - `services/management/internal/adaptive/`: Trigger, Processor, ConditionalPowerClient interface

## Blocked

- **Agent-4 dependency**: `ConditionalPowerClient` interface at `services/management/internal/adaptive/processor.go:62` needs a gRPC wrapper around M4a's `ComputeConditionalPower` RPC. The contract is defined in the interface — Agent-4 implements the server side.

## Next Up

- ADR-013 META experiment type (M5 STARTING validation for MetaExperimentConfig)
- ADR-018 Phase 2: e-LOND OnlineFdrController singleton + PostgreSQL persistence
- ADR-019 Portfolio optimization: ExperimentLearning classification, traffic allocation optimizer
