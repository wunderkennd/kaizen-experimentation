# Agent-5: Experiment Management Service

You own Module 5 (Experiment Management Service) — CRUD, lifecycle state machine, RBAC, guardrails, bucket reuse, and all new experiment type management.

Language: Go (conditionally migrating to Rust per ADR-025)
Service: `services/management/`
Service port: 50055 (ConnectRPC)

## Phase 5 ADR Responsibilities

### Primary Owner
- **ADR-013 (Meta-Experiments)**: Add `EXPERIMENT_TYPE_META` to lifecycle. STARTING validation: `MetaExperimentConfig.base_algorithm` set, all variant `payload_json` valid, metric_ids resolve, reward weights sum to 1.0, warn if primary metric overlaps reward objectives. Isolated policy state per variant.
- **ADR-018 Phase 2 (OnlineFdrController)**: Implement platform-level singleton in M5. Persist state in PostgreSQL (`online_fdr_controller_state` table). On each CONCLUDED transition, submit primary metric e-value to controller. Store reject/don't-reject decision on experiment record. Alpha wealth, rejection count, test history checkpointed after every decision.
- **ADR-019 (Portfolio)**: Implement portfolio data endpoints served to M6. `ExperimentLearning` classification required at CONCLUDED→ARCHIVED transition. Win rate, learning rate (EwL), annualized impact, traffic utilization, experiment throughput, power distribution. Optimal alpha recommendation during experiment creation (advisory). Traffic allocation optimizer. Decision rule evaluation (monthly batch job).
- **ADR-020 (Adaptive Sample Size)**: Schedule interim analysis at `interim_fraction × planned_duration`. Request conditional power from M4a. Zone classification: favorable (no action), promising (extend duration, adjust GST boundaries), futile (notify owner). Audit trail for all recalculations. Update experiment planned_duration on extension.
- **ADR-022 (Switchback)**: Add `EXPERIMENT_TYPE_SWITCHBACK`. STARTING validation: `planned_cycles >= 4`, `block_duration >= 1h`, cluster_attribute resolves. Adaptive block length trigger after 2 cycles. No bucket allocation (time-based, not hash-based).
- **ADR-023 (Quasi-Experiment)**: Add `EXPERIMENT_TYPE_QUASI`. Simplified lifecycle — no assignment serving, no traffic allocation, no guardrails. STARTING validation: panel data exists for treated unit and all donors, pre-treatment period sufficient.

### Supporting Role
- **ADR-014 (Provider Metrics)**: Validate `MetricStakeholder` and `MetricAggregationLevel` on metric definitions. Enforce: bandit rewards require USER aggregation; guardrails accept USER or EXPERIMENT. Configure guardrail beta-correction (Bonferroni on power side).
- **ADR-021 (Feedback Loops)**: Consume `model_retraining_events` correlation results from M4a. Surface mitigation recommendations to M6 via experiment record.
- **ADR-025 (Conditional Rust Port)**: Evaluate trigger at end of Sprint 5.5. If ≥ 3 of {015 P2, 018, 019, 020, 021} are complete, plan M5 Rust port.

## Coding Standards
- Run `go test ./services/management/...` before creating PR.
- State transitions use `UPDATE ... WHERE state = $expected` with `RowsAffected() == 1` for TOCTOU safety.
- RBAC: all new RPCs must be wired into the auth interceptor with appropriate role levels.
- Audit trail: all lifecycle transitions, configuration changes, and classification actions logged.
- New PostgreSQL migrations go in `sql/migrations/` with sequential numbering.
## Work Tracking
Find your assigned work via GitHub Issues:
```bash
gh issue list --label "agent-5" --state open
gh issue view <number>
```
When starting work, comment on the Issue. When creating a PR, include `Closes #<number>`.
If blocked, add the `blocked` label and comment explaining the blocker.

## Dependencies on Other Agents
- Agent-Proto: New experiment types, `ExperimentLearning` enum, `AdaptiveSampleSizeConfig`, `AnnualizedImpact` must land first.
- Agent-4 (M4a): Conditional power computation for adaptive N; e-value computation for FDR controller.
- Agent-3 (M3): MLRATE trigger during STARTING; panel data existence check for QUASI.
- Agent-1 (M1): StreamConfigUpdates must include SWITCHBACK and META configs.
- Agent-6 (M6): Portfolio dashboard, meta-experiment results, adaptive N zone indicator.

## New PostgreSQL Tables/Columns
- `online_fdr_controller_state`: alpha_wealth, num_tested, num_rejected, strategy, last_updated
- `adaptive_sample_size_audit`: experiment_id, interim_date, conditional_power, zone, old_n, new_n, reason
- `experiments.learning_classification`: ExperimentLearning enum value
- `experiments.learning_notes`: free-text
- `metric_results.e_value`, `metric_results.log_e_value`: double precision

## Contract Tests to Write
- M5 ↔ M4a: Adaptive N conditional power request/response
- M5 ↔ M4a: E-value submission for OnlineFdrController
- M5 ↔ M6: Portfolio data format
- M5 ↔ M6: Meta-experiment config rendering
- M5 ↔ M1: Switchback/META config in StreamConfigUpdates
- M5 ↔ M3: MLRATE STARTING trigger
