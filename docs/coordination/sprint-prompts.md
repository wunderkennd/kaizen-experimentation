# Phase 5 Sprint Prompt Templates

Pre-written `multiclaude worker create` commands for each sprint. Copy-paste to launch workers.

---

## Sprint 5.0 — Schema & Foundations (Weeks 1–3)

```bash
# Proto schema (BLOCKS ALL OTHER WORK — run first)
multiclaude worker create \
  "Land all Phase 5 proto schema extensions. Read design_doc_v7.0.md Section 3.6. \
   Changes: ExperimentType += META(9), SWITCHBACK(10), QUASI(11). \
   BanditConfig: RewardObjective, RewardConstraint, RewardCompositionMethod, ArmConstraint, \
   GlobalConstraint, SlateConfig, PositionBiasModel, SlateInteractionModel, mad_randomization_fraction. \
   MetricDefinition: MetricStakeholder, MetricAggregationLevel enums. \
   SequentialMethod += AVLM(4). VarianceReductionConfig, AdaptiveSampleSizeConfig. \
   SurrogateModelConfig += TC/JIVE fields. InterferenceAnalysisResult += feedback loop fields. \
   ModelRetrainingEvent. ExperimentLearning enum. AnnualizedImpact. \
   AssignmentService: GetSlateAssignment RPC. SyntheticControlMethod enum. \
   QuasiExperimentConfig, SwitchbackConfig, MetaExperimentConfig. \
   Run: buf lint proto/ && buf breaking proto/ --against .git#branch=main \
   Write status to docs/coordination/status/agent-4-status.md."

# AVLM (P0 — #1 ROI)
multiclaude worker create \
  "Implement AVLM (ADR-015 Phase 1) in crates/experimentation-stats/src/avlm.rs. \
   Read docs/adrs/015-anytime-valid-regression-adjustment.md. \
   AvlmSequentialTest struct with 6 running sufficient statistics (sum_x, sum_y, sum_xy, sum_xx, sum_yy, n). \
   O(1) update() per observation. confidence_sequence() returns (estimate, lower, upper). \
   is_significant() checks null exclusion from CS. \
   GROW/REGROW mixing boundary with configurable rho (default: unit-information prior). \
   Golden-file tests against R avlm package to 4 decimal places. \
   Proptest: CS covers true parameter at rate >= (1-alpha) over 10K sims. \
   Add pub mod avlm to lib.rs. \
   Write status to docs/coordination/status/agent-4-status.md."

# TC/JIVE (P0 — corrects theoretical error)
multiclaude worker create \
  "Implement TC/JIVE surrogate calibration fix (ADR-017 Phase 1) in crates/experimentation-stats/src/orl.rs. \
   Read docs/adrs/017-offline-rl-long-term-effects.md. \
   Replace R²-based calibration with Jackknife IV Estimation. \
   SurrogateCalibrator struct: K-fold cross-fold procedure. \
   For each fold k: train predictive model on K-1 folds, predict on held-out fold. \
   Use cross-fold predictions as instruments in 2SLS regression. \
   Output: treatment_effect_correlation (TC), jive_coefficient, jive_r_squared. \
   Update SurrogateModelConfig proto with JIVE fields. \
   Golden-file: reproduce Netflix KDD 2024 Table 2 results. \
   Write status to docs/coordination/status/agent-4-status.md."

# E-value computation (P1)
multiclaude worker create \
  "Implement e-value computation (ADR-018 Phase 1) in crates/experimentation-stats/src/evalue.rs. \
   Read docs/adrs/018-e-value-framework-online-fdr.md. \
   e_value_grow(): GROW martingale e-value for two-sample mean comparison. \
   e_value_avlm(): regression-adjusted e-value (pairs with AVLM). \
   EValueResult { e_value, log_e_value, implied_level }. \
   SQL migration: ALTER TABLE metric_results ADD COLUMN e_value DOUBLE PRECISION, ADD COLUMN log_e_value DOUBLE PRECISION. \
   Golden-file: Ramdas/Wang monograph examples to 6 decimal places. \
   Proptest: e-values non-negative; product under null has E[e] <= 1. \
   Write status to docs/coordination/status/agent-4-status.md."

# M7 Rust port scaffold
multiclaude worker create \
  "Begin M7 Rust port (ADR-024 Phase 1). \
   Read docs/adrs/024-m7-rust-port.md. \
   Create crates/experimentation-flags/ in workspace. Add to Cargo.toml workspace members. \
   Dependencies: tonic, tonic-web, sqlx (postgres), experimentation-hash, experimentation-proto, experimentation-core. \
   Implement tonic service with tonic-web JSON HTTP mode. \
   Migrate 5 SQL migrations from services/flags/ to sqlx format. \
   Flag CRUD RPCs: CreateFlag, GetFlag, ListFlags, UpdateFlag, DeleteFlag. \
   Wire-format contract test: compare JSON output with Go M7 for CreateFlag + GetFlag. \
   Run cargo test -p experimentation-flags. \
   Write status to docs/coordination/status/agent-7-status.md."
```

---

## Sprint 5.1 — Measurement Foundations (Weeks 4–6)

```bash
# Provider metrics: Delta Lake tables + SQL templates
multiclaude worker create \
  "Implement provider-side metrics (ADR-014). \
   Read docs/adrs/014-provider-side-metrics.md. \
   Create delta/content_catalog.sql and delta/experiment_level_metrics.sql DDL. \
   Implement SQL templates in services/metrics/templates/: \
   catalog_coverage_rate, catalog_gini_coefficient, catalog_entropy, longtail_impression_share, \
   provider_exposure_gini, provider_exposure_parity, user_genre_entropy, user_discovery_rate, \
   user_provider_diversity, intra_list_distance. \
   Add freshness validation: content_catalog.updated_at must be < 24h. \
   All queries logged to query_log. \
   Write status to docs/coordination/status/agent-3-status.md." \
  --agent agent-3-metrics

# Guardrail beta-correction
multiclaude worker create \
  "Implement guardrail beta-correction (ADR-014 guardrail section). \
   In experimentation-stats: Bonferroni on power side for guardrail metrics. \
   Each guardrail runs at significance alpha/K where K = number of guardrails. \
   Update M5 validation: MetricStakeholder and MetricAggregationLevel enforcement. \
   Bandit rewards require USER aggregation; guardrails accept USER or EXPERIMENT. \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

# ModelRetrainingEvent ingestion
multiclaude worker create \
  "Implement ModelRetrainingEvent ingestion (ADR-021). \
   Read docs/adrs/021-feedback-loop-interference.md. \
   Add model_retraining_events Kafka topic (8 partitions) to docker-compose and topic config. \
   Implement event validation in experimentation-ingest: model_id, training_data_start/end required. \
   Bloom filter dedup integration. \
   Contract test with M3: Kafka roundtrip serialization. \
   Write status to docs/coordination/status/agent-2-status.md." \
  --agent agent-2-pipeline

# M7 Rust port: business logic + cutover
multiclaude worker create \
  "Complete M7 Rust port (ADR-024 Phases 2-4). \
   Percentage rollout: native experimentation_hash::murmur3_x86_32(). \
   Multi-variant traffic fraction allocation. \
   PromoteToExperiment: tonic client to M5. \
   Audit trail: sqlx insert on mutation. \
   Stale flag detection: SQL view. \
   Kafka reconciler: rdkafka consumer for experiment conclusion events. \
   Port all 13 chaos tests. k6 load test: 20K rps, p99 < 5ms. \
   100% wire-format contract tests vs Go M7. \
   After validation: DELETE crates/experimentation-ffi/, services/flags/, just test-flags-cgo. \
   Update Go SDK: pure-Go MurmurHash3 becomes primary. \
   Write status to docs/coordination/status/agent-7-status.md." \
  --agent agent-7-flags

# Provider health dashboard
multiclaude worker create \
  "Build /portfolio/provider-health page (ADR-014 M6 section). \
   Read docs/adrs/014-provider-side-metrics.md M6 rendering section. \
   Time series: catalog coverage, provider Gini, long-tail impression share. \
   Filter by provider dropdown. Recharts for time series. \
   Data source: M3 provider metric endpoints. \
   Code-split with dynamic import. React.memo on chart components. \
   Write status to docs/coordination/status/agent-6-status.md." \
  --agent agent-6-ui
```

---

## Sprint 5.2 — Statistical Core (Weeks 7–9)

```bash
# AVLM integration into M4a service
multiclaude worker create \
  "Wire AVLM into M4a analysis service (ADR-015). \
   SEQUENTIAL_METHOD_AVLM selectable in RunAnalysis. \
   When configured with cuped_covariate_metric_id + AVLM method: \
   M4a constructs AvlmSequentialTest, feeds observations incrementally, \
   returns confidence sequence in AnalysisResult. \
   Integration test: RunAnalysis with AVLM produces narrower CIs than mSPRT on golden-file data. \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

# Multi-objective reward on LMAX core
multiclaude worker create \
  "Implement multi-objective reward composition (ADR-011) on LMAX core. \
   Read docs/adrs/011-multi-objective-bandit-reward.md. \
   RewardComposer: weighted sum, epsilon-constraint (Lagrangian), Tchebycheff. \
   MetricNormalizer: EMA running mean/variance per metric, alpha=0.01. \
   When reward_objectives non-empty, compose reward before posterior update. \
   Persisted in RocksDB alongside posterior parameters. \
   Test: multi-objective bandit converges on weighted-sum reward in simulation. \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

# Adaptive sample size
multiclaude worker create \
  "Implement adaptive sample size (ADR-020). \
   Read docs/adrs/020-adaptive-sample-size-recalculation.md. \
   experimentation-stats/src/adaptive_n.rs: conditional_power(), blinded pooled variance, \
   zone classification (favorable/promising/futile). \
   GST spending function re-allocation for extended experiments. \
   M5 integration: scheduled interim trigger, M4a delegation, experiment extension on promising zone. \
   Proptest: type I error <= alpha after blinded re-estimation on 10K null sims. \
   Write status to docs/coordination/status/agent-5-status.md." \
  --agent agent-5-management

# Feedback loop detection
multiclaude worker create \
  "Implement feedback loop detection (ADR-021). \
   Read docs/adrs/021-feedback-loop-interference.md. \
   experimentation-stats/src/feedback_loop.rs: FeedbackLoopDetector. \
   Pre/post retraining effect comparison (paired t-test across events). \
   Bias-corrected treatment effect extrapolation. \
   M3 contamination SQL joining model_retraining_events with exposures. \
   Proto: InterferenceAnalysisResult += feedback loop fields. \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

# AVLM + adaptive N UI
multiclaude worker create \
  "Build AVLM and adaptive N UI components. \
   AVLM confidence sequence boundary plot (Recharts) replacing separate mSPRT/CUPED views. \
   Adaptive N zone indicator badge (favorable/promising/futile) on experiment detail. \
   Extended timeline visualization for promising-zone experiments. \
   Feedback loop analysis tab: retraining timeline, pre/post comparison, contamination chart, \
   bias-corrected estimate, mitigation recommendation matrix. \
   Write status to docs/coordination/status/agent-6-status.md." \
  --agent agent-6-ui
```

---

## Sprint 5.3 — Constraints & New Experiment Types (Weeks 10–12)

```bash
# LP constraint layer
multiclaude worker create \
  "Implement LP constraint post-processing (ADR-012) on LMAX core. \
   Read docs/adrs/012-constrained-arm-selection-lp.md. \
   KL(q||p) minimization over constraint polytope. \
   O(K log K) for per-arm constraints, <50us for general linear (interior point). \
   Population-level running counts with EMA decay on LMAX thread. \
   Feasibility checking: CONSTRAINT_INFEASIBLE fallback to raw probabilities. \
   Log adjusted q as assignment_probability (critical for IPW validity). \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

# Switchback analysis + assignment
multiclaude worker create \
  "Implement switchback analysis (ADR-022). \
   experimentation-stats/src/switchback.rs: SwitchbackAnalyzer. \
   HAC SE (Newey-West, Andrews automatic bandwidth). \
   Randomization inference (exact for small blocks, 10K-permutation MC for large). \
   Carryover diagnostic (lag-1 autocorrelation test). \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

multiclaude worker create \
  "Implement switchback assignment in M1 (ADR-022). \
   Time-based assignment: (current_time, block_duration, cluster_attribute). \
   Three designs: simple alternating, regular balanced, randomized. \
   Washout period exclusion. Block index in exposure events. \
   M5 STARTING validation: planned_cycles >= 4, block_duration >= 1h. \
   Write status to docs/coordination/status/agent-1-status.md." \
  --agent agent-1-assignment

# Synthetic control
multiclaude worker create \
  "Implement synthetic control (ADR-023). \
   Read docs/adrs/023-synthetic-control-methods.md. \
   experimentation-stats/src/synthetic_control.rs: 4 methods. \
   Classic SCM (constrained optimization, convex weights). \
   Augmented SCM (Ridge de-biased, conformal CIs). \
   Synthetic DiD (unit weights + time weights, doubly robust). \
   CausalImpact (Bayesian structural time series). \
   Placebo permutation inference. \
   Golden-file: R augsynth package to 4 decimal places. \
   M5: QUASI experiment type with simplified lifecycle. \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

# e-LOND FDR controller
multiclaude worker create \
  "Implement e-LOND OnlineFdrController (ADR-018 Phase 2). \
   M5 singleton persisted in PostgreSQL. \
   SQL migration: online_fdr_controller_state table. \
   On each CONCLUDED transition: submit e-value, receive reject/don't-reject. \
   Alpha wealth management with geometric decay. \
   Checkpoint state after every decision. \
   Write status to docs/coordination/status/agent-5-status.md." \
  --agent agent-5-management

# Switchback + SCM UI tabs
multiclaude worker create \
  "Build switchback and quasi-experiment results tabs. \
   Switchback: block timeline (colored bands), block-level outcomes, ACF carryover plot, \
   randomization test histogram. \
   Quasi: treated vs synthetic control time series with confidence band, pointwise effects, \
   cumulative effects, donor weight table, placebo small-multiples, RMSPE diagnostic. \
   Write status to docs/coordination/status/agent-6-status.md." \
  --agent agent-6-ui
```

---

## Sprint 5.4 — Slate Bandits & Meta-Experiments (Weeks 13–15)

```bash
# Slate bandit policy
multiclaude worker create \
  "Implement slot-wise factorized Thompson Sampling (ADR-016). \
   Read docs/adrs/016-slate-bandit-optimization.md. \
   SlatePolicy: per-slot posteriors, sequential selection with context propagation. \
   Three reward attribution models: clicked-slot, position-weighted, counterfactual. \
   LIPS OPE estimator for offline evaluation. \
   O(L*K) inference for L slots, K candidates. \
   RocksDB snapshots include full slate policy state. \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

# GetSlateAssignment RPC
multiclaude worker create \
  "Implement GetSlateAssignment RPC in M1 (ADR-016). \
   New RPC on AssignmentService. Forward candidates to M4b slate bandit. \
   Return ordered slate with per-slot probabilities and overall slate probability. \
   Contract test with M4b: slate roundtrip. \
   Write status to docs/coordination/status/agent-1-status.md." \
  --agent agent-1-assignment

# Meta-experiment support
multiclaude worker create \
  "Implement META experiment type (ADR-013). \
   Read docs/adrs/013-meta-experiments-objective-functions.md. \
   M5: STARTING validation for MetaExperimentConfig. \
   M1: hash user to variant, delegate to M4b with variant-specific config. \
   M4b: isolated policy state per (experiment_id, variant_id). \
   M4a: two-level IPW (P(variant) * P(arm|variant)). Cross-variant business outcome analysis. \
   Write status to docs/coordination/status/agent-5-status.md." \
  --agent agent-5-management

# Portfolio dashboard
multiclaude worker create \
  "Implement portfolio optimization (ADR-019). \
   Read docs/adrs/019-portfolio-experiment-optimization.md. \
   M5: portfolio data endpoints, ExperimentLearning classification, \
   optimal alpha recommendation, traffic allocation optimizer. \
   Write status to docs/coordination/status/agent-5-status.md." \
  --agent agent-5-management

# Portfolio page + meta-experiment results + enhanced bandit dashboard
multiclaude worker create \
  "Build /portfolio page, meta-experiment results view, enhanced bandit dashboard. \
   Portfolio: win rate, learning rate trends, annualized impact, traffic utilization, \
   power distribution histogram, optimal alpha widget. \
   Meta: objective comparison table, business outcome comparison, Pareto frontier (D3). \
   Bandit: multi-objective decomposition, LP constraint visualization, slate heatmap. \
   Write status to docs/coordination/status/agent-6-status.md." \
  --agent agent-6-ui
```

---

## Sprint 5.5 — Advanced & Integration (Weeks 16–18)

```bash
# ORL Phase 2 (if committed)
multiclaude worker create \
  "Implement ORL doubly-robust MDP estimator (ADR-017 Phase 2). \
   Read docs/adrs/017-offline-rl-long-term-effects.md Phase 2 section. \
   M3: user_trajectories Delta Lake table construction. \
   M4a: Q-function fitting (XGBoost via Spark), density ratio estimation, DR combination. \
   Density ratio clipping at 10x. Effective sample size reporting. \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

# MLRATE cross-fitting
multiclaude worker create \
  "Implement MLRATE cross-fitting (ADR-015 Phase 2). \
   M3: LightGBM/XGBoost training during STARTING. K-fold cross-fitted predictions \
   stored in metric_summaries. VarianceReductionConfig proto fields. \
   M5: MLRATE trigger during STARTING→RUNNING transition. \
   Write status to docs/coordination/status/agent-3-status.md." \
  --agent agent-3-metrics

# MAD e-processes
multiclaude worker create \
  "Implement MAD e-processes (ADR-018 Phase 3). \
   M4b: mix uniform randomization at mad_randomization_fraction rate. \
   Flag observations as bandit vs uniform component in SelectArm response. \
   M4a: compute e-process from uniform-component observations only. \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

# E-value column + FDR badge in UI
multiclaude worker create \
  "Add e-value display to results dashboard (ADR-018). \
   E-value column alongside p-values in treatment effects table. \
   OnlineFdrController decision badge on experiment cards. \
   Create experiment form: optimal alpha recommendation from M5. \
   Write status to docs/coordination/status/agent-6-status.md." \
  --agent agent-6-ui

# Full integration test suite
multiclaude worker create \
  "Build Phase 5 integration test suite. \
   E2E test 1: multi-objective bandit with LP constraints + provider metrics + AVLM analysis + e-values. \
   E2E test 2: switchback experiment creation → assignment → block aggregation → HAC analysis → results. \
   E2E test 3: quasi-experiment creation → panel data → SCM analysis → placebo test → results. \
   E2E test 4: meta-experiment → isolated policy per variant → cross-variant analysis. \
   All cross-agent contract tests passing. \
   cargo test --workspace && go test ./... && cd ui && npm test. \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit
```
