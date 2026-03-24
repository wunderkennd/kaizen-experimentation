# Agent-4 Status — Phase 5

**Module**: M4a Analysis + M4b Bandit
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.0
Focus: ADR-015 AVLM, ADR-017 TC/JIVE, ADR-018 E-values, ADR-014 Guardrail beta-correction, ADR-011 Multi-objective, ADR-012 LP constraints
Branch: work/bright-bear

## In Progress

- [ ] ADR-012 LP constraints

## Completed (Phase 5) — latest first

- [x] **ADR-015 M4a integration — AVLM wired into RunAnalysis** (2026-03-23, work/bright-bear)
  - `proto/experimentation/analysis/v1/analysis_service.proto`: extended `RunAnalysisRequest`
    with `sequential_method` (field 2) and `tau_sq` (field 3) — AVLM now selectable per-call
  - `crates/experimentation-analysis/src/config.rs`: added `default_tau_sq: f64` (env: `ANALYSIS_DEFAULT_TAU_SQ`, default 0.5)
  - `crates/experimentation-analysis/src/grpc.rs`:
    - Added `avlm` + `SequentialResult` imports; `SEQUENTIAL_METHOD_AVLM = 4` constant
    - `compute_avlm_result()` helper: streams observations into `AvlmSequentialTest`, uses `cov.unwrap_or(0.0)` fallback (null covariate → unadjusted mSPRT CS)
    - `compute_analysis()` branched: AVLM path when `sequential_method == 4`, CUPED path otherwise
    - AVLM results placed in `cuped_adjusted_effect/cuped_ci_lower/cuped_ci_upper/variance_reduction_pct`; `sequential_result.boundary_crossed` = `is_significant`
    - `run_analysis` handler now extracts `sequential_method` and `tau_sq` from request
    - `get_analysis_result` passes (0, 0.0) → fixed-horizon, backward compatible
  - `crates/experimentation-analysis/tests/m4a_m6_contract_test.rs`:
    - `test_avlm_narrower_ci_than_msprt_on_golden_data`: 20-obs golden dataset (Y = 2X + effect + noise, R² ≈ 0.999); confirms AVLM CI narrower than mSPRT CI, variance reduction > 80%, boundary crossed
    - All existing tests updated to `..Default::default()` for new `RunAnalysisRequest` fields
  - All 52 analysis tests + 181 stats tests + full workspace green (0 failures)

- [x] **ADR-021 — Feedback Loop Interference Detection** (2026-03-23, work/fancy-bear)
  - `crates/experimentation-stats/src/feedback_loop.rs`: `FeedbackLoopDetector`
    - `RetrainingEffectObservation`: contamination_fraction, pre/post treatment effects per retrain event
    - `FeedbackLoopResult`: paired t-test p-value, contamination-effect Pearson r, OLS bias estimate, bias-corrected effect
    - `FeedbackLoopDetector::new(observations)` — validates ≥3 events, contamination in [0,1]
    - `FeedbackLoopDetector::detect(alpha)` — paired t-test + Pearson r + OLS bias correction
    - Detection criterion: (`p < alpha` AND `|r| > 0.5`) OR `p < alpha/2`
    - 11 unit tests: validation errors, no-feedback baseline, strong detection, bias-correction golden, mean stats, uncorrelated contamination
    - 3 proptest invariants: p_value_in_range, correlation_in_range, all_outputs_finite
    - All 192 experimentation-stats tests green
  - Registered in `crates/experimentation-stats/src/lib.rs` as `pub mod feedback_loop`
  - **M3 contamination pipeline** (`services/metrics/internal/jobs/feedback_loop.go`):
    - `FeedbackLoopJob` + `FeedbackLoopResult` — orchestrates Spark SQL contamination run
    - Writes to `delta.feedback_loop_contamination` for M4a to read
  - **Spark SQL template** (`services/metrics/internal/spark/templates/feedback_loop_contamination.sql.tmpl`):
    - Joins `delta.model_retraining_events` with `delta.metric_summaries`
    - 7-day pre/post windows around each retraining event
    - Filters ARRAY_CONTAINS(active_experiment_ids, experiment_id) for scoped joins
  - **SQLRenderer** (`services/metrics/internal/spark/renderer.go`):
    - Added `RenderFeedbackLoopContamination(p TemplateParams) (string, error)`
  - **Golden SQL** (`services/metrics/testdata/golden/feedback_loop_contamination_expected.sql`):
    - exp-001 / watch_time_minutes / ctrl-001 — validated via `TestRenderFeedbackLoopContamination`
  - **SQL migration** (`sql/migrations/008_feedback_loop_results.sql`):
    - `feedback_loop_results` table for M4a to persist analysis results
    - Unique index on (experiment_id, metric_id)
    - Stores: n_retraining_events, feedback_loop_detected, paired_ttest_p_value, mean_pre/post_retrain_effect, mean_effect_shift, contamination_effect_correlation, bias_estimate, bias_corrected_effect
  - **Proto** — `InterferenceAnalysisResult` fields 11–14 already defined in prior proto PR (#196):
    - `feedback_loop_detected (11)`, `feedback_loop_bias_estimate (12)`, `contamination_effect_correlation (13)`, `feedback_loop_computed_at (14)`
  - All spark + jobs Go tests pass

- [x] **ADR-011 Multi-objective reward composition** (2026-03-23, work/bright-platypus)
  - `crates/experimentation-bandit/src/reward_composer.rs` (new, ~450 LOC)
  - `MetricNormalizer`: EMA running mean/variance per metric (α=0.01); tracks running-max ideal point for Tchebycheff; serializable alongside posterior state
  - `RewardComposer`: three strategies fully implemented:
    - `WeightedScalarization`: Σ wᵢ × normalized(rᵢ); weights validated to sum 1.0 (±1e-6)
    - `EpsilonConstraint`: Lagrangian relaxation — primary objective + credit for secondaries exceeding floor thresholds
    - `Tchebycheff`: −max_i { wᵢ × max(0, ideal_i − normalized(rᵢ)) }; Pareto-optimal for non-convex frontiers
  - `ThompsonSamplingPolicy` extended:
    - `new_multi_objective()` constructor with embedded `RewardComposer`
    - `update_multi_objective()` method: composes raw metric values → sigmoid-mapped scalar → Beta posterior update
    - Composer state persisted in `PolicyState` (RocksDB via existing snapshot mechanism)
    - Fully backward-compatible; single-objective policies unaffected
  - 18 unit tests + 4 proptest invariants (all three methods always-finite, Tchebycheff post-warmup)
  - Convergence test: 2-arm Thompson Sampling bandit with 2-metric weighted-sum reward converges to optimal arm >60% after 2000 rounds
  - All 46 bandit crate tests green (including 2 integration convergence tests)

## Completed (Phase 5)

- [x] **ADR-011 — Multi-objective reward composition** (2026-03-23, work/happy-koala)
  - `crates/experimentation-bandit/src/multi_objective.rs` (new)
    - `MetricStats`: EMA running mean/variance (α=0.01), Welford-style update
    - `MetricNormalizer`: keyed EMA normaliser over arbitrary metric names; serialize/deserialize for RocksDB
    - `RewardObjective`: metric_name, weight, `constraint_slack: Option<f64>`, reference_value
    - `CompositionMethod`: `WeightedSum`, `EpsilonConstraint` (Lagrangian), `Tchebycheff`
    - `RewardComposer`: 3-phase compose (collect → normalise → scalarise); clamps to ±10σ; serialize/deserialize
    - `sigmoid()`: maps composed real reward to (0, 1) for Beta-Bernoulli policies
  - `crates/experimentation-bandit/src/lib.rs`: added `pub mod multi_objective`
  - `crates/experimentation-policy/src/types.rs`: `RewardUpdate` gains `metric_values: Option<HashMap<String, f64>>`
  - `crates/experimentation-policy/src/snapshot.rs`: `SnapshotEnvelope` gains `reward_composer_state: Option<Vec<u8>>` (serde default for backward compat); `make_envelope_with_composer()` factory
  - `crates/experimentation-policy/src/core.rs`:
    - `PolicyCore` gains `reward_composers: HashMap<String, RewardComposer>`
    - `register_multi_objective_experiment()`: registers Thompson + composer atomically
    - `handle_reward_update()`: composes metric_values → scalar, applies sigmoid for Thompson policies
    - `write_snapshot()`: persists composer state alongside policy posteriors
    - `restore_from_snapshots()`: restores composer state on startup
  - Tests:
    - 10 unit tests in `multi_objective.rs` (MetricStats convergence, normalisation direction, WeightedSum, EpsilonConstraint penalty, Tchebycheff balance, roundtrip)
    - `test_weighted_sum_convergence`: 1000-round simulation — arm_a (E[eng]=0.8, E[qual]=0.6) beats arm_b (E[eng]=0.3, E[qual]=0.7) with w=(0.7, 0.3); arm_a selection rate >60% in final 200 rounds
    - `test_multi_objective_composer_crash_recovery`: normaliser survives crash-and-restore via RocksDB; ≥45 obs restored after 50 reward events
    - All 35 policy tests + 33 bandit tests green; workspace-wide 0 failures

- [x] **ADR-014 Phase 1 — Guardrail Bonferroni beta-correction** (2026-03-23, work/nice-lion)
  - `guardrail_bonferroni()` in `crates/experimentation-stats/src/multiple_comparison.rs`
    - Per-guardrail threshold = alpha/K; `rejected[i]` = true when p_i ≤ alpha/K
    - `GuardrailBonferroniResult` struct with `p_values`, `alpha_per_guardrail`, `rejected`, `num_guardrails`
    - 9 unit tests + 3 proptest invariants (threshold=alpha/K, rejection consistent with threshold, threshold decreases with K)
    - All 181 experimentation-stats tests green
  - M5 validation enforcement (`services/management/internal/validation/metric.go`):
    - `ValidateCreateMetricDefinition` now requires `stakeholder` and `aggregation_level` (not UNSPECIFIED)
    - PROVIDER aggregation requires PROVIDER stakeholder
    - New exported: `ValidateBanditRewardMetricAggregation(m)` — enforces USER aggregation
    - New exported: `ValidateGuardrailMetricAggregation(m)` — enforces USER or EXPERIMENT aggregation
    - 12 new ADR-014 test cases; all management tests green
  - Store layer (`services/management/internal/store/`):
    - `MetricDefinitionRow`: added `Stakeholder`, `AggregationLevel` string fields
    - `metric_convert.go`: bidirectional proto↔row mapping for new fields
    - `metric.go`: SQL updated (INSERT, SELECT, Scan) for new columns
  - DB migration: `sql/migrations/007_metric_stakeholder_aggregation.sql`
  - Handler enforcement (`services/management/internal/handlers/lifecycle.go`):
    - `validateMetricsForStart`: after existence check, validates each guardrail metric via `ValidateGuardrailMetricAggregation`
    - `validateTypeConfigForStart`: after reward metric existence check, validates via `ValidateBanditRewardMetricAggregation`
  - Infrastructure: generated missing `gen/go/go.mod` + full buf codegen (common/v1, management/v1, analysis/v1, etc.) — unblocked all Go compilation

## Completed (Phase 5)

- [x] **ADR-015 Phase 1 (AVLM)** — PR #199
  - `crates/experimentation-stats/src/avlm.rs` implemented
  - `AvlmSequentialTest` with 6 sufficient statistics per arm, O(1) `update()`
  - `confidence_sequence()` returns regression-adjusted anytime-valid CI
  - Batch convenience API `avlm_confidence_sequence()`
  - 15 tests passing: 5 golden-file tests, 8 behavioral/unit tests, 2 proptest/coverage tests
  - Proptest coverage invariant: 200-trial simulation at n=50/arm yields ≥ 90% coverage (conservative threshold)
  - Variance reduction confirmed for correlated covariates
  - Degenerate fallback (constant covariate → unadjusted mSPRT)
  - Registered in `lib.rs` as `pub mod avlm`

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
  - PR #198 merged

- [x] **Proto schema extensions** (PR #196 merged) — All Phase 5 proto additions, buf lint + breaking clean.
- [x] ADR-018 Phase 1 — E-value computation (PR open 2026-03-23)
  - `e_value_grow()`: Sequential GROW martingale (plug-in betting, valid e-process)
  - `e_value_avlm()`: CUPED-adjusted Gaussian mixture e-value
  - SQL migration 006_evalue_columns.sql: `e_value` + `log_e_value` on metric_results
  - 8 golden-file tests (GROW: 4 analytic; AVLM: 4 formula-validated to 6dp)
  - All 144+ workspace tests green

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

- ADR-015 Phase 1 (AVLM AvlmSequentialTest) — unblocked
- ADR-017 Phase 2 — Offline RL policy evaluation (doubly-robust estimator)
- ADR-011 Multi-objective bandits
- ADR-012 LP constraints

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

## Notes

- ADR-015 ADR file (`docs/adrs/015-anytime-valid-regression-adjustment.md`) created 2026-03-24; documents algorithm, sufficient statistics, API contract, golden-file and proptest validation targets.
- The `e_value_avlm` function implements the Gaussian mixture e-value from Ramdas & Wang
  (2024) §3.1, validated to 6 decimal places against analytically computable examples.
- GROW martingale uses the causal plug-in strategy (safe start λ_1=0); valid as an
  e-process since E_{H0}[exp(λX − λ²σ²/2)] = 1 for X ~ N(0,σ²).
