# Agent-5 Status — Phase 5

**Module**: M5 Management
**Last updated**: 2026-03-29

## Current Sprint

Sprint: 5.3
Focus: ADR-025 Phase 4 — Validation and Cutover Preparation
Branch: work/wise-tiger

## In Progress

- [x] ADR-025 Phase 4 — Contract tests and shadow traffic harness
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

- [x] **ADR-025 Phase 4 — Contract tests + shadow traffic** (PR #273)
  - `crates/experimentation-management/src/contract_test_support.rs`: in-memory store and handler for contract tests
  - `crates/experimentation-management/tests/contract_tests.rs`: 21 contract tests + 3 shadow traffic tests
  - All 24 tests pass: `cargo test -p experimentation-management`

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

- [x] ADR-015 Phase 2 — MLRATE trigger in M5

- [x] ADR-025 Phase 2 — Rust management crate (PR #265, merged)
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

## Phase 4 Cutover Checklist (ADR-025)

### Pre-Cutover Gates

- [x] **Contract tests passing** — 21 wire-format tests green
  - 11 M5-M6 contract points (field presence, enum serialization, state transitions, error codes)
  - 10 M1-M5 contract points (experiment_id stability, hash_salt, TOCTOU safety, variant contract)
- [x] **Shadow traffic harness ready** — 3 shadow tests, graceful skip when Go M5 not running
- [ ] **Phases 1–3 complete** — full sqlx PostgreSQL backend needed before shadow traffic is meaningful
  - Phase 1: Core CRUD + RBAC interceptor + lifecycle state machine
  - Phase 2: Kafka guardrail consumer + bucket reuse + StreamConfigUpdates
  - Phase 3: OnlineFdrController (ADR-018) + portfolio optimizer (ADR-019) + adaptive N trigger (ADR-020)
- [ ] **Shadow traffic 48-hour run** — run Rust M5 alongside Go M5 at port 50056, compare all RPC responses
  - Set `GO_M5_ADDR=http://localhost:50055` to enable full shadow comparison in contract tests
  - No response diffs allowed before cutover
- [ ] **RBAC tests** — interceptor correctly enforces 4-level role hierarchy (viewer/analyst/admin/owner)
- [ ] **Lifecycle state machine tests** — proptest concurrent transition invariants
- [ ] **Guardrail consumer tests** — rdkafka consumer group migration verified (idempotent processing)

### Cutover Steps (when all gates green)

1. [ ] Deploy Rust M5 binary to staging behind separate port (e.g., 50056)
2. [ ] Redirect 1% of production traffic to Rust M5 via load balancer split
3. [ ] Monitor error rates, latency percentiles, and state machine correctness for 24 hours
4. [ ] Increase to 50% traffic split; monitor for 24 hours
5. [ ] DNS/load balancer cutover: route all M5 traffic to Rust service (port 50055)
6. [ ] Decommission Go M5 service and `services/management/` directory
7. [ ] Delete `experimentation-ffi` crate (combined with ADR-024 completion)
8. [ ] Update `CLAUDE.md` architecture table: M5 language → Rust

### Dependency Tracking

| Dependency | Owner | Status | Notes |
|------------|-------|--------|-------|
| ADR-018 E-values | Agent-4 | In progress | OnlineFdrController needs `e_value_grow()` from experimentation-stats |
| ADR-019 Portfolio | Agent-4 | Pending | `power_analysis()`, `conditional_power()` |
| ADR-020 M4a server | Agent-4 | Blocked | `ComputeConditionalPower` RPC — interface defined at `services/management/internal/adaptive/processor.go:62` |
| ADR-024 M7 cutover | Agent-7 | In progress | Delete `experimentation-ffi` only after both M5 and M7 are fully ported |

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
