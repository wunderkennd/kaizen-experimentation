# Agent-4 Status — Phase 5

**Module**: M4a Analysis + M4b Bandit
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.0
Focus: ADR-015 AVLM, ADR-017 TC/JIVE, ADR-018 E-values, ADR-014 Guardrail beta-correction, ADR-011 Multi-objective, ADR-012 LP constraints, ADR-022 Switchback
Branch: work/eager-bear, work/happy-squirrel

Focus: ADR-015 AVLM, ADR-017 TC/JIVE, ADR-018 E-values, ADR-014 Guardrail beta-correction, ADR-011 Multi-objective, ADR-012 LP constraints
Branch: work/clever-deer

Focus: ADR-023 Synthetic Control Methods
Branch: work/happy-rabbit

Sprint: 5.1
Focus: ADR-016 Slate bandit optimization
Branch: work/silly-dolphin

Branch: work/gentle-owl

## In Progress

_None._

## Completed (this sprint)

- [x] **ADR-023 — Synthetic Control Methods** (2026-03-24, work/happy-rabbit)
  - `crates/experimentation-stats/src/synthetic_control.rs` (~800 LOC)
  - `SyntheticControlResult { method, att, ci_lower, ci_upper, donor_weights, placebo_p_value }`
  - `SyntheticControlInput` + `synthetic_control(input, method)` dispatch API
  - **Classic SCM**: projected gradient descent on simplex (Duchi O(n log n) projection), adaptive lr
  - **Augmented SCM**: classic weights + Ridge bias correction (auto-lambda), conformal CI from pre-treatment residual SE
  - **Synthetic DiD**: time weights + unit weights (both via projected gradient), jackknife CI over post periods
  - **CausalImpact**: Ridge regression + Kalman filter (local linear trend F=[[1,1],[0,1]]), open-loop post-period prediction, prediction-variance CI
  - **Placebo permutation**: leave-one-out, p = (1+n_extreme)/(1+n_donors)
  - 26 unit tests + 5 proptest invariants (weights sum to 1.0 +-1e-9 for Classic, Augmented, SDiD)
  - 3 golden files + integration test (`synthetic_control_golden.rs`)
  - All 335 experimentation-stats tests green (0 failures)

## Completed (this session)

- [x] **ADR-021 feedback loop API completion** (2026-03-24, work/eager-badger, PR #241)
  - Added `InterferenceResult` struct with `From<FeedbackLoopResult>` conversion
  - Added `contamination_flag(p_value, threshold) -> bool` standalone function
  - Added `FeedbackLoopDetector::bias_corrected_effect(raw) -> f64` method (pre/post ratio)
  - Added proptest `null_no_detection`: pre==post → detected=false, p=1.0 always
  - All 27 feedback_loop tests + 192 experimentation-stats tests green

- [x] **ADR-011 integration test — multi-objective weighted-sum convergence** (2026-03-24, work/witty-owl)
  - Added `multi_objective_weighted_sum_converges_to_optimal_arm` to `crates/experimentation-bandit/tests/convergence.rs`
  - Uses `ThompsonSamplingPolicy::new_multi_objective` + `update_multi_objective` API
  - 2-arm bandit with 2-metric weighted-sum (engagement w=0.6, quality w=0.4); arm "high" (0.8, 0.7) vs arm "low" (0.3, 0.4)
  - Verifies arm "high" selected >60% after 1000 rounds with Gaussian noise on ground-truth metrics
  - All 51 bandit crate tests green (46 unit + 5 integration)

## Completed (Phase 5) — latest first

- [x] **ADR-017 Phase 1 — TC/JIVE verification pass** (2026-03-24, work/clever-koala)
  - Verified `kfold_iv_calibrate()` in `crates/experimentation-stats/src/orl.rs` is fully operational
  - All 3 Netflix KDD 2024 Table 2 golden tests pass (`orl_golden.rs`): no-confounding, confounded, weak-instrument
  - Proptest invariants `iv_result_all_finite` and `bias_correction_sign_with_positive_confounder` confirmed green
  - Full `experimentation-stats` test suite: 0 failures
  - No code changes required — implementation landed in PR #198

- [x] **ADR-022 — Switchback Experiment Analysis** (2026-03-23, work/happy-squirrel)
  - `crates/experimentation-stats/src/switchback.rs`: `SwitchbackAnalyzer` (~450 LOC)
  - **`BlockOutcome`**: block_index, cluster_id, is_treatment, metric_value, user_count, in_washout
  - **`SwitchbackResult`**: effect, hac_se, ci_lower/upper, randomization_p_value, effective_blocks, lag1_autocorrelation, carryover_test_p_value, hac_bandwidth
  - **`SwitchbackAnalyzer::new()`**: validates ≥4 non-washout blocks, filters washout, sorts by block_index
  - **`SwitchbackAnalyzer::analyze(alpha, n_permutations, rng_seed)`**: OLS + HAC SE + randomization inference + carryover diagnostic
  - **`SwitchbackAnalyzer::randomization_test(n_permutations)`**: standalone randomization inference
  - **`SwitchbackAnalyzer::carryover_test(alpha)`**: standalone lag-1 autocorrelation test
  - **HAC SE**: Newey-West Bartlett-kernel estimator; Andrews (1991) AR(1) automatic bandwidth ĥ = ⌈1.1447·(2ρ̂²/(1−ρ̂²))^{1/3}·T^{1/3}⌉
  - **Randomization inference**: exact enumeration when C(T,k) ≤ n_permutations; Fisher-Yates MC otherwise
  - **Carryover diagnostic**: t-test on lag-1 autocorrelation of OLS residuals, approx t(T−2)
  - 20 unit tests + 4 proptest invariants (p_all_outputs_valid, p_effect_seed_independent, p_randomization_p_in_range, p_lag1_autocorr_in_range)
  - Registered in `crates/experimentation-stats/src/lib.rs` as `pub mod switchback`
  - All 243 experimentation-stats tests green; full workspace 0 failures

- [x] **ADR-014 Phase 2 — GuardrailCorrector module** (2026-03-24, work/clever-deer)
  - `crates/experimentation-stats/src/guardrail.rs` (new, 389 LOC)
    - `GuardrailCorrector { new(alpha, k), corrected_alpha(), is_violated(p) }`:
      Bonferroni correction — each of K guardrails at alpha/K so FWER ≤ alpha
    - `MetricStakeholder` enum: Unspecified/User/Provider/Platform (mirrors proto)
    - `MetricAggregationLevel` enum: Unspecified/User/Experiment/Provider (mirrors proto)
    - `validate_bandit_reward_aggregation()`: enforces USER-only for bandit rewards
    - `validate_guardrail_aggregation()`: enforces USER or EXPERIMENT for guardrails
  - 13 unit tests + 10K FWER Monte Carlo (K=1,2,3,5,10,20) + 5 proptest invariants
  - All 32 guardrail tests + full experimentation-stats suite green

- [x] **ADR-016 — Slot-wise factorized Thompson Sampling slate bandit** (2026-03-24, work/silly-dolphin)
  - `crates/experimentation-bandit/src/slate.rs` (new, ~420 LOC)
  - `SlatePolicy`: per-slot Beta posteriors, `Vec<HashMap<ArmId, BetaArm>>` (O(L×K) state)
  - `select_slate(candidates, n_slots, rng)`: sequential slot-filling with context propagation
    - O(L×K) inference: for each slot, argmax over remaining candidates' Beta draws
    - Enforces item uniqueness across slots (remove-from-pool after selection)
    - Graceful fallback to Beta(1,1) prior for arms added after policy creation
  - Three reward attribution models:
    - `AttributionModel::ClickedSlot`: full credit to clicked position only
    - `AttributionModel::PositionWeighted`: reciprocal-rank discount (1/(pos+1)) across all slots
    - `AttributionModel::Counterfactual`: IPS correction `(reward / propensity).clamp(0,1)` at clicked slot
  - `lips_estimate(logged: &[SlateLog]) -> f64`: LIPS OPE estimator (IPS-weighted average reward)
    - Skips zero-propensity observations; asserts finite outputs
    - Reference: Kiyohara, Nomura, Saito (WWW 2024)
  - `SlateLog` struct: slate, clicked, clicked_position, propensity, reward
  - RocksDB snapshot: `to_bytes()` / `from_bytes()` using `bincode` (full state roundtrip)
  - Added `bincode = "1.3"` to workspace `Cargo.toml` and `experimentation-bandit/Cargo.toml`
  - Registered as `pub mod slate` in `lib.rs`
  - **21 new tests** covering:
    - Uniform prior creation, select_slate length/uniqueness/truncation
    - Dominant arm preference, context propagation across 50 runs
    - All three attribution models (click-only, position-weighted, counterfactual/IPS)
    - bincode roundtrip for all 3 attribution models
    - LIPS estimator: empty, full propensity, partial propensity, mixed, zero-propensity skip
    - Convergence: arm-0 dominates slot-0 after 500 training rounds (>70% win rate)
  - All 68 bandit crate tests green (21 new + 47 existing)

## Completed This Session (2026-03-24)

- [x] **Phase 5 E2E integration test suite** (2026-03-24, work/swift-elephant)
  - `crates/experimentation-analysis/tests/phase5_e2e.rs` — 9 tests, all green
  - `crates/experimentation-analysis/Cargo.toml` — added `experimentation-bandit` + `rand` to dev-dependencies
  - **Switchback path** (ADR-022): `test_switchback_rpc_unimplemented_and_run_analysis_fallback`
    - Asserts `GetSwitchbackAnalysis` → `Code::Unimplemented` (stub, pending ADR-022)
    - Runs `RunAnalysis` on 100 switchback-period-structured observations (5 control × 5 treatment periods, 10 users each); verifies MetricResult fields, positive effect, CI contains estimate, no SRM
    - ADR-018 e_value / log_e_value fields verified as finite on MetricResult
  - **Quasi-experiment path** (ADR-023): `test_quasi_experiment_synthetic_control_rpc_unimplemented` + `test_quasi_experiment_run_analysis_with_donor_unit_data`
    - `GetSyntheticControlAnalysis` → `Code::Unimplemented` (stub, pending ADR-023)
    - `RunAnalysis` on 40-unit donor/treated data (mu_ctrl=5.0, mu_trt=6.0, ATT≈+1.0); p-value < 0.10 confirmed
  - **E-value sequential test** (ADR-018): 3 unit tests on `e_value_grow`
    - `test_evalue_grow_safe_start_and_trajectory_length`: λ₁=0 → log_wealth[0]=0.0; trajectory length = n; all entries finite
    - `test_evalue_grow_accumulates_evidence_under_h1`: obs=2.0×30, e_value > 1/α=20, reject=true, log_wealth grows t5→t30
    - `test_evalue_grow_no_rejection_under_null`: alternating ±0.1, reject=false, |log_wealth| < 2.0
    - `test_run_analysis_metric_result_has_evalue_fields`: e_value and log_e_value on MetricResult are finite; exp(log_e_value)=e_value invariant checked
  - **Slate bandit roundtrip** (ADR-016): 2 tests on `experimentation_bandit::thompson::select_arm`
    - `test_slate_bandit_per_slot_selection_probabilities`: 8-candidate, 4-slot; item_00 (400/500 successes) wins slot 0 with prob > 0.70; all probabilities ≥ 0, sum to 1.0 (±1e-3), arm_id ∈ candidates
    - `test_slate_bandit_posterior_update_shifts_selection`: 200 successes on arm_0; arm_0 selection prob > 0.80; probability sum invariant holds

## Completed (This Session)

- [x] **ADR-017 TC/JIVE golden-file verification** (2026-03-24, work/gentle-owl)
  - Verified all 3 existing golden tests pass (orl_kdd2024_no_confounding, _confounded, _weak_instrument)
  - Created `test-vectors/tc_jive_vectors.json` with Netflix KDD 2024 Table 2 values:
    - Scenario A (ρ_UY=0): jive_coefficient=0.3, treatment_effect_correlation=1.0, first_stage_r²=1.0
    - Scenario B (ρ_UY≈0.5): jive_coefficient=0.2974, ols_naive=0.4373, treatment_effect_correlation=0.8834
  - Added `tc_jive_kdd2024_table2_vectors` test in `orl.rs` validating to 4 decimal places (tolerance=1e-4)
    against both jive_coefficient, ols_naive_estimate, treatment_effect_correlation, first_stage_r_squared
  - Added missing proptest invariant: `jive_coefficient in (-1, 1)` in `iv_result_all_finite`
  - Added `shrinkage_calibrated_le_naive_positive_confounding` proptest: with valid instrument
    (block design, Cov(Z,η)=0) and positive confounding (δ∈[0.3,0.8]), JIVE ≤ OLS always holds
  - 11 lib tests + 3 golden integration tests: all green

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
- [x] **ADR-018 Phase 1 — E-value computation** (2026-03-24, work/eager-bear)
  - `crates/experimentation-stats/src/evalue.rs`:
    - `e_value_grow()`: Sequential GROW martingale (causal plug-in λ_t = μ̂_{t-1}/σ², safe-start λ_1=0)
    - `e_value_avlm()`: CUPED-adjusted Gaussian mixture e-value (Ramdas & Wang §3.1)
    - `EValueResult`: e_value, log_e_value, reject, log_wealth_trajectory
  - `sql/migrations/006_evalue_columns.sql`: ALTER TABLE metric_results ADD COLUMN e_value, log_e_value
  - `crates/experimentation-stats/tests/evalue_golden.rs`: 8 golden-file tests (6dp tolerance)
  - 8 golden JSON files in tests/golden/: 4 GROW (analytic), 4 AVLM (formula-validated to 6dp)
  - 4 proptest invariants: grow_outputs_always_finite, grow_reject_consistent, avlm_outputs_always_finite, avlm_reject_consistent
  - 12 unit tests in evalue.rs
  - All 144+ experimentation-stats tests green (0 failures)

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
