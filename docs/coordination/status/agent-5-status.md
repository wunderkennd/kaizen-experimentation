# Agent-5 Status — Phase 5

**Module**: M5 Management
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.3
Focus: ADR-013/022/023 Phase 5 experiment type lifecycle validation
Branch: work/jolly-otter
PR: (pending)

## In Progress

- [x] ADR-013/022/023 STARTING validation for META, SWITCHBACK, QUASI experiment types

## Completed (Phase 5)

- [x] ADR-020 Adaptive Sample Size (PR #227, merged)
  - `crates/experimentation-stats/src/adaptive_n.rs`: blinded_pooled_variance, conditional_power, zone_classify, gst_reallocate_spending, required_n_for_power, run_interim_analysis
  - `sql/migrations/008_adaptive_sample_size_audit.sql`
  - `services/management/internal/adaptive/`: Trigger, Processor, ConditionalPowerClient interface

- [x] Phase 5 experiment type store + lifecycle validation (this sprint)
  - `store/convert.go`: META, SWITCHBACK, QUASI type string mappings; typeConfig JSONB serialization/deserialization for MetaExperimentConfig, SwitchbackConfig, QuasiExperimentConfig
  - `validation/experiment.go`: create-time nil-config checks for META/SWITCHBACK/QUASI; STARTING validators ValidateMetaExperimentForStart, ValidateSwitchbackForStart, ValidateQuasiExperimentForStart
  - `handlers/lifecycle.go`: validateTypeConfigForStart wired to call new validators for META/SWITCHBACK/QUASI
  - `validation/experiment_test.go`: unit tests for all three validation paths
  - `gen/go/go.mod`: initialized gen/go module (was missing); proto stubs regenerated via `buf generate`

## Blocked

None.

## Validation Rules Implemented

| Type | Config Field | STARTING Rule |
|------|-------------|---------------|
| META | `meta_experiment_config` | `variant_objectives` non-empty; all `variant_id` match experiment variants |
| SWITCHBACK | `switchback_config` | `planned_cycles >= 4`; `block_duration >= 1h` |
| QUASI | `quasi_experiment_config` | `donor_unit_ids` non-empty |

## Next Up

- ADR-018 Phase 2: e-LOND OnlineFdrController singleton + PostgreSQL persistence
- ADR-019 Portfolio optimization: ExperimentLearning classification, traffic allocation optimizer
