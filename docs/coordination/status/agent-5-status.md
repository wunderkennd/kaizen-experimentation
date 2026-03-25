# Agent-5 Status — Phase 5

**Module**: M5 Management
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.3
Focus: ADR-018 Phase 2 — e-LOND OnlineFdrController; Phase 5 docs pass
Branch: work/witty-badger, work/bold-eagle

## In Progress

- [x] ADR-018 Phase 2 — e-LOND OnlineFdrController
  - `sql/migrations/009_online_fdr_controller_state.sql`
  - `services/management/internal/fdr/controller.go` — Controller singleton
  - `services/management/internal/fdr/controller_test.go` — 11 unit tests
  - `services/management/internal/handlers/service.go` — WithPool, WithFdrController options
  - `services/management/internal/handlers/conclude.go` — submitFdrDecision (best-effort)
  - `services/management/internal/handlers/lifecycle.go` — wire into concludeByID
  - `services/management/cmd/main.go` — ONLINE_FDR_ENABLED env var gate

Focus: ADR-015 Phase 2 MLRATE Model Training Trigger
Branch: work/silly-badger
PR: (pending)

## In Progress

- [x] ADR-015 Phase 2 — MLRATE model training trigger (this PR)
  - New `services/management/internal/mlrate/` package: `ModelTrainingRequest`, `Publisher` interface, `KafkaPublisher`, `MemPublisher`, `ShouldTrigger`, `Emit`
  - `StartExperiment` in `handlers/lifecycle.go` calls `maybeEmitModelTrainingRequest` after DRAFT→STARTING transition
  - Trigger condition: `sequential_method == "AVLM"` AND `surrogate_model_id != ""`
  - Event fields: `experiment_id`, `metric_id` (primary), `covariate_metric_id` (surrogate target), `training_data_start` (now - 30d), `training_data_end` (now)
  - Kafka topic `model_training_requests` added to docker-compose.yml
  - AVLM added to `seqMethodToString`/`stringToSeqMethod` maps in `store/convert.go`
  - 9 unit tests in `mlrate/publisher_test.go`; all pass

- [x] **Phase 5 CHANGELOG and implementation plan** (work/bold-eagle)
  - Created `docs/coordination/CHANGELOG-phase5.md` — release notes for Phase 5 (all 15 ADRs, per-cluster breakdown, breaking changes, migration guide, performance characteristics)
  - Created `docs/coordination/phase5-implementation-plan.md` — canonical status table for all 19 Phase 5 work items with PR references
  - Updated `docs/coordination/status/agent-5-status.md` — this file

## Completed (Phase 5) — latest first

- [x] **ADR-020 Phase 1 — Adaptive sample size recalculation** (PR #227, merged 2026-03-24)
  - `crates/experimentation-stats/src/adaptive_n.rs`: `blinded_pooled_variance`, `conditional_power`,
    `zone_classify` (FAVORABLE/PROMISING/FUTILE/INCONCLUSIVE), `gst_reallocate_spending`,
    `required_n_for_power`, `run_interim_analysis`
  - `sql/migrations/008_adaptive_sample_size_audit.sql` — audit trail for interim analysis events
  - `services/management/internal/adaptive/`: `Trigger`, `Processor`, `ConditionalPowerClient` interface
  - M5 scheduler: triggers interim analysis at `interim_fraction x planned_duration`
  - Promising zone: extends experiment duration, adjusts GST boundaries, notifies owner
  - Futile zone: sends notification recommending early termination
  - AVLM interaction: uses `variance_effective = variance_pooled x (1 - rho^2)` when AVLM configured
  - M6 UI: AVLM boundary plot, adaptive-N zone badge + timeline (PR #223, merged)
  - All workspace tests green

- [x] **ADR-014 Phase 1 — Guardrail Bonferroni beta-correction + M5 metric validation** (PR #212, merged)
  - `guardrail_bonferroni()` in `crates/experimentation-stats/src/multiple_comparison.rs`
  - M5 validation: `ValidateCreateMetricDefinition` enforces `stakeholder` + `aggregation_level`;
    `ValidateBanditRewardMetricAggregation()`, `ValidateGuardrailMetricAggregation()` exported
  - Store layer: `MetricDefinitionRow` gains `Stakeholder`, `AggregationLevel`; bidirectional proto<->row mapping
  - DB migration: `sql/migrations/007_metric_stakeholder_aggregation.sql`
  - Handler: `validateMetricsForStart` + `validateTypeConfigForStart` enforce aggregation rules
  - Infrastructure: generated `gen/go/go.mod` + full buf codegen — unblocked all Go compilation

- [x] ADR-018 Phase 2 — e-LOND OnlineFdrController
  - Platform-level singleton persisted in PostgreSQL (migration 009)
  - e-LOND geometric-decay alpha wealth management
  - SELECT FOR UPDATE serializes concurrent conclude calls
  - Best-effort integration at CONCLUDED transition; never blocks conclusion
  - Opt-in via ONLINE_FDR_ENABLED=true environment variable

- [x] ADR-015 Phase 2 — MLRATE trigger in M5 (this PR)

## Blocked

- **Agent-4 dependency (partial)**: `ConditionalPowerClient` interface at
  `services/management/internal/adaptive/processor.go:62` requires gRPC wrapper around M4a's
  `ComputeConditionalPower` RPC. The interface contract is defined; Agent-4 implements server side.
  Phase 1 statistical module is complete and unblocked.

- **Agent-4 dependency**: `ConditionalPowerClient` interface at `services/management/internal/adaptive/processor.go:62` needs a gRPC wrapper around M4a's `ComputeConditionalPower` RPC. The contract is defined in the interface — Agent-4 implements the server side.
- **gen/go infrastructure**: `gen/go/go.mod` missing; packages that depend on `github.com/org/experimentation/gen/go` (handlers, store, sequential, validation) cannot compile in the workspace. Pre-existing issue. The `mlrate` package was structured to be testable without gen/go (same pattern as `adaptive`).

## Next Up

1. **ADR-013 META Experiment Type** (Sprint 5.4)
   - M5 STARTING validation for `MetaExperimentConfig` (variant objectives, reward composition)
   - Validate cross-variant business outcome metric alignment

2. **ADR-019 Portfolio Optimization** (Sprint 5.4)
   - `ExperimentLearning` classification at CONCLUDED transition (mandatory for archive)
   - Portfolio data endpoints (win rate, learning rate, annualized impact, traffic utilization)
   - Optimal alpha recommendation (`RecommendAlpha` function, advisory only)
   - Traffic allocation optimizer for underpowered concurrent experiments

3. **ADR-025 M5 Rust port** (conditional — awaiting go/no-go decision)

## ADR-025 Trigger Status

Port M5 to Rust when >=3 of {ADR-015 P2, ADR-018 (full), ADR-019, ADR-020, ADR-021}:

| ADR | Status |
|-----|--------|
| ADR-020 Adaptive N | Complete (PR #227) |
| ADR-021 Feedback Loops | Complete (PR #222) |
| ADR-018 Phase 1 | Complete (PR #200) |
| ADR-018 Phase 2/3 | Planned (Sprint 5.3/5.5) |
| ADR-019 Portfolio | Planned (Sprint 5.4) |
| ADR-015 Phase 2 | Planned (Sprint 5.5) |

**Current count**: 2/5 (need 3 to trigger). Evaluate at Sprint 5.5 end.
