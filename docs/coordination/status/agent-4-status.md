# Agent-4 Status — Phase 5

**Module**: M4a Analysis + M4b Bandit
**Last updated**: 2026-03-23

## Current Sprint

Sprint: 5.0
Focus: ADR-015 AVLM, ADR-017 TC/JIVE, ADR-018 E-values, ADR-011 Multi-objective, ADR-012 LP constraints
Branch: work/bright-elephant

## In Progress

- [ ] ADR-015 AVLM (sequential CUPED)
- [ ] ADR-018 E-values + online FDR
- [ ] ADR-011 Multi-objective bandits
- [ ] ADR-012 LP constraints

## Completed (Phase 5)

- [x] **ADR-017 Phase 1 — TC/JIVE surrogate calibration fix** (2026-03-23)
  - Implemented `crates/experimentation-stats/src/orl.rs`
  - `kfold_iv_calibrate()`: K-fold cross-fit IV estimation replacing R²-based calibration
  - `InstrumentStrength` enum (Strong/Moderate/Weak) based on first-stage F-stat (Stock-Yogo rule-of-thumb)
  - HC0 sandwich SE; OLS vs JIVE bias-correction reported
  - Golden files: 3 scenarios from Netflix KDD 2024 Table 2
    - Scenario A: no confounding → JIVE = OLS = true γ = 0.3 (exact)
    - Scenario B: positive confounding → OLS biased up, JIVE corrects
    - Scenario C: weak instrument (F ≈ 0.41) → `InstrumentStrength::Weak` detected
  - 141 lib tests pass + 3 golden integration tests pass (0 failures)
  - Proptest invariants: `iv_result_all_finite`, `bias_correction_sign_with_positive_confounder`
  - PR: work/bright-elephant

- [x] **Proto schema extensions** (PR: work/lively-owl) — All Phase 5 proto additions, buf lint + breaking clean.

### Proto changes landed:

**experiment.proto**
- `ExperimentType` += `META(9)`, `SWITCHBACK(10)`, `QUASI(11)` (ADR-013, 022, 023)
- `SequentialMethod` += `AVLM(4)` (ADR-015)
- New enums: `SyntheticControlMethod`, `ExperimentLearning`
- New messages: `MetaExperimentConfig`, `MetaVariantObjective`, `SwitchbackConfig`,
  `QuasiExperimentConfig`, `AdaptiveSampleSizeConfig`, `VarianceReductionConfig`,
  `AnnualizedImpact`
- `Experiment` message: fields 27–32 (phase 5 configs + `learning`)

**bandit.proto**
- `BanditAlgorithm` += `SLATE_FACTORIZED_TS(5)`, `SLATE_GENERATIVE(6)` (ADR-016)
- New enums: `RewardCompositionMethod`, `PositionBiasModel`
- New messages: `RewardObjective`, `ArmConstraint`, `GlobalConstraint`,
  `SlateInteractionModel`, `SlateConfig`
- `BanditConfig` fields 8–13: `reward_objectives`, `composition_method`,
  `arm_constraints`, `global_constraints`, `slate_config`, `mad_randomization_fraction`
  (ADR-011, 012, 016, 018)

**metric.proto**
- New enums: `MetricStakeholder` (USER/PROVIDER/PLATFORM), `MetricAggregationLevel`
  (USER/EXPERIMENT/PROVIDER) (ADR-014)
- `MetricDefinition` fields 15–16: `stakeholder`, `aggregation_level`

**analysis_service.proto**
- New RPCs: `GetSyntheticControlAnalysis`, `GetSwitchbackAnalysis`
- `MetricResult` fields 19–20: `e_value`, `log_e_value` (ADR-018)
- `InterferenceAnalysisResult` fields 11–14: feedback loop fields (ADR-021)
- New messages: `SyntheticControlAnalysisResult`, `SwitchbackAnalysisResult`
  (ADR-022, 023)

**event.proto**
- New message: `ModelRetrainingEvent` (ADR-021)

**assignment_service.proto**
- New RPC: `GetSlateAssignment` (ADR-016)
- New messages: `GetSlateAssignmentRequest`, `GetSlateAssignmentResponse`,
  `SlotProbability`

**surrogate.proto**
- `SurrogateModelConfig` fields 11–13: TC/JIVE calibration fields (ADR-017)

## Blocked

_None._

## Next Up

- ADR-017 Phase 2 — Offline RL policy evaluation (doubly-robust estimator)
- ADR-015 AVLM — sequential CUPED, depends on: none (can start)
- ADR-018 E-values — depends on: none (can start)

## Dependencies Provided to Other Agents

All Phase 5 proto schema is now unblocked. Agents 1, 2, 3, 5, 6, 7 can consume
the new types. Key dependencies:
- **Agent-1** (M1 Assignment): `GetSlateAssignment` RPC + `SlateConfig` in `BanditConfig`
- **Agent-2** (M2 Pipeline): `ModelRetrainingEvent` on `model_retraining_events` topic
- **Agent-3** (M3 Metrics): `MetricStakeholder`, `MetricAggregationLevel` on `MetricDefinition`
- **Agent-5** (M5 Management): `ExperimentType` META/SWITCHBACK/QUASI, `ExperimentLearning`,
  `AnnualizedImpact`, new config messages
- **Agent-6** (M6 UI): All new result types + `SyntheticControlAnalysisResult` +
  `SwitchbackAnalysisResult`
