# Phase 5 Release Notes and CHANGELOG

**Phase**: 5 (Sprint 5.0–5.5)
**ADRs**: 011–025 (15 decisions, 6 clusters)
**Date range**: 2026-03-19 – ongoing
**Base version**: Phase 4 complete (PR #228 merge queue, 163 PRs merged, 10 pair integration suites green)

---

## Summary

Phase 5 extends the Kaizen Experimentation Platform from a best-in-class A/B testing system into a full-spectrum causal inference and adaptive optimization platform. Fifteen architecture decisions (ADRs 011–025) are organized into six capability clusters:

- **Cluster A (Multi-Stakeholder)**: Provider-side diversity metrics, multi-objective bandit reward composition, LP constraint post-processing, and meta-experiments on objective function parameters — enabling the platform to balance subscriber engagement, content provider fairness, and platform economics simultaneously.
- **Cluster B (Statistical Methods)**: Anytime-valid regression adjustment (AVLM — sequential CUPED), e-value framework with online FDR control, and adaptive sample size recalculation via promising-zone designs — eliminating the forced choice between variance reduction and continuous monitoring.
- **Cluster C (Bandit & RL)**: Slate-level bandit optimization with off-policy evaluation (LIPS), and offline RL with TC/JIVE surrogate calibration fix — correcting a fundamental theoretical error in the platform's surrogate metric framework for continual treatments.
- **Cluster D (Quasi-Experimental)**: Switchback experiment designs for interference-prone treatments, and synthetic control methods for non-randomizable interventions — expanding the platform to market-level launches, pricing experiments, and post-hoc causal evaluation.
- **Cluster E (Platform Operations)**: Portfolio-level experiment program optimization and feedback loop interference detection — treating the experimentation program as a resource allocation problem and detecting model-retraining contamination that deflates treatment effect estimates.
- **Cluster F (Language Migration)**: Unconditional port of M7 Feature Flag Service from Go to Rust (eliminating the `experimentation-ffi` crate entirely), and a conditional port of M5 Experiment Management Service triggered when ≥3 of the Phase 5 statistical ADRs are implemented.

---

## Per-Cluster Breakdown

### Cluster A: Multi-Stakeholder Optimization

| ADR | Title | Status | PR(s) | Description |
|-----|-------|--------|-------|-------------|
| ADR-011 | Multi-Objective Bandit Reward | **Complete** | #221, #228 | Three reward composition strategies (weighted scalarization, epsilon-constraint Lagrangian, Tchebycheff) on LMAX single-threaded core; EMA metric normalization; RocksDB-persisted composer state. |
| ADR-012 | Constrained Arm Selection via LP | **In Progress** | — | KL-divergence minimization over constraint polytope for provider exposure guarantees; O(K log K) for per-arm, <50μs interior-point for general linear; LMAX integration with EMA running counts. |
| ADR-013 | Meta-Experiments on Objective Functions | **Planned** (Sprint 5.4) | — | Experiments that test different bandit reward formulations; isolated M4b policy state per (experiment, variant); two-level IPW analysis. |
| ADR-014 | Provider-Side Metrics | **Complete** | #208, #209, #211, #212 | Ten SQL templates for catalog diversity (Gini, entropy, coverage, longtail share) and user diversity (genre entropy, discovery rate, provider variety, ILD); freshness validation on `delta.content_catalog`; M6 provider health dashboard. |

### Cluster B: Statistical Methods

| ADR | Title | Status | PR(s) | Description |
|-----|-------|--------|-------|-------------|
| ADR-015 Phase 1 | AVLM Sequential CUPED | **Complete** | #199, #226 | Anytime-valid linear model (Lindon et al. 2025) combining CUPED variance reduction with mSPRT sequential validity; O(1) update with 6 running sufficient statistics; wired into M4a `RunAnalysis` as `SEQUENTIAL_METHOD_AVLM=4`. |
| ADR-015 Phase 2 | MLRATE Cross-Fitting | **Planned** (Sprint 5.5) | — | ML-predicted control variates via LightGBM with K-fold cross-fitting; M3 model training during STARTING phase; ~19% additional variance reduction over linear CUPED. |
| ADR-018 Phase 1 | E-Value Computation | **Complete** | #200 | GROW martingale e-values for sequential mean comparisons; regression-adjusted AVLM variant; `e_value` and `log_e_value` columns added to `metric_results`; golden-file validated against Ramdas/Wang monograph to 6 decimal places. |
| ADR-018 Phase 2 | Online FDR (e-LOND) | **Planned** (Sprint 5.3) | — | Platform-level `OnlineFdrController` singleton in M5; alpha-wealth management with geometric decay; PostgreSQL persistence; per-experiment FDR decision at CONCLUDED transition. |
| ADR-018 Phase 3 | MAD E-Processes | **Planned** (Sprint 5.5) | — | Mixture Adaptive Design for valid sequential inference from bandit experiments; uniform randomization at configurable fraction ε; e-process from uniform-component observations only. |
| ADR-020 | Adaptive Sample Size | **Complete** | #227 | Promising-zone design (Mehta-Pocock 2011): conditional power classification into favorable/promising/futile zones at interim look; blinded pooled variance re-estimation; GST spending function reallocation on extension; type I error preserved. |

### Cluster C: Bandit & RL

| ADR | Title | Status | PR(s) | Description |
|-----|-------|--------|-------|-------------|
| ADR-016 | Slate Bandit Optimization | **Planned** (Sprint 5.4) | — | Slot-wise factorized Thompson Sampling for ordered recommendation slates; LIPS off-policy evaluator (Kiyohara et al. WWW 2024); `GetSlateAssignment` RPC on M1; three reward attribution models. |
| ADR-017 Phase 1 | TC/JIVE Surrogate Calibration Fix | **Complete** | #198 | Replaces R²-based surrogate calibration with Jackknife IV Estimation (Bibaut et al. KDD 2024); K-fold cross-fold procedure eliminates correlated measurement error bias; `InstrumentStrength` diagnostics; golden-file validates 3 Netflix KDD 2024 Table 2 scenarios. |
| ADR-017 Phase 2 | Offline RL (ORL) Estimator | **Planned** (Sprint 5.5) | — | Doubly-robust MDP estimator (Tran, Bibaut, Kallus ICML 2024) for long-term causal effects; Q-function fitting + density ratio estimation; M3 `user_trajectories` Delta Lake table. |

### Cluster D: Quasi-Experimental

| ADR | Title | Status | PR(s) | Description |
|-----|-------|--------|-------|-------------|
| ADR-022 | Switchback Experiment Designs | **Planned** (Sprint 5.3) | — | `EXPERIMENT_TYPE_SWITCHBACK=10`; time-based assignment alternating entire platform or clusters; HAC standard errors (Newey-West); randomization inference; carryover test; data-driven block length adaptation. |
| ADR-023 | Synthetic Control Methods | **Planned** (Sprint 5.3) | — | `EXPERIMENT_TYPE_QUASI=11`; four SCM variants (Classic, Augmented, SDiD, CausalImpact); placebo permutation inference; M3 panel data aggregation; M6 treated vs. synthetic control visualization with donor weight table. |

### Cluster E: Platform Operations

| ADR | Title | Status | PR(s) | Description |
|-----|-------|--------|-------|-------------|
| ADR-019 | Portfolio Experiment Optimization | **Planned** (Sprint 5.4) | — | Portfolio dashboard (win rate, learning rate, annualized impact, traffic utilization); `ExperimentLearning` classification; optimal alpha recommendation (Netflix EC '25); traffic allocation optimizer; decision rule evaluation (Netflix KDD '25). |
| ADR-021 | Feedback Loop Interference | **Complete** | #209, #222 | Detects model-retraining contamination via paired t-test on pre/post retraining treatment effects; `ModelRetrainingEvent` Kafka topic ingestion (M2); `FeedbackLoopDetector` in `experimentation-stats`; M3 contamination SQL; bias-corrected treatment effect estimation. |

### Cluster F: Language Migration

| ADR | Title | Status | PR(s) | Description |
|-----|-------|--------|-------|-------------|
| ADR-024 | M7 Rust Port | **Complete** | #197, #215 | Full port of Feature Flag Service from Go to Rust (`crates/experimentation-flags/`); tonic-web JSON HTTP wire-format compatible with ConnectRPC; direct `experimentation-hash` import (no FFI); rdkafka reconciler; `experimentation-ffi` crate deleted; 13 chaos tests + 20K rps load test at p99 < 5ms. |
| ADR-025 | M5 Conditional Rust Port | **Conditional** | — | Trigger: ≥3 of {ADR-015 P2, ADR-018, ADR-019, ADR-020, ADR-021} implemented. Currently: ADR-020 ✅, ADR-021 ✅, ADR-018 P1 ✅ (P2 pending). `experimentation-management` crate plan ready; direct `experimentation-stats` imports replace M4a RPCs for management decisions. |

---

## Breaking Changes

### Proto Schema Additions (PR #196)

All changes are **additive** (no field removals or renumbers). Clients using proto3 binary encoding are backward compatible. JSON clients may need to handle new enum values.

#### `experiment.proto`

```protobuf
// New ExperimentType enum values
EXPERIMENT_TYPE_META       = 9;   // ADR-013: experiments on objective functions
EXPERIMENT_TYPE_SWITCHBACK = 10;  // ADR-022: temporal alternation designs
EXPERIMENT_TYPE_QUASI      = 11;  // ADR-023: quasi-experimental (SCM)

// New SequentialMethod enum value
SEQUENTIAL_METHOD_AVLM = 4;  // ADR-015: anytime-valid linear model

// New Experiment fields (27–32)
ExperimentLearning learning_classification = 27;  // ADR-019
string learning_notes = 28;
AdaptiveSampleSizeConfig adaptive_sample_size = 29;  // ADR-020
MetaExperimentConfig meta_config = 30;              // ADR-013
SwitchbackConfig switchback_config = 31;            // ADR-022
QuasiExperimentConfig quasi_config = 32;            // ADR-023

// New top-level messages
MetaExperimentConfig, MetaVariantObjective, SwitchbackConfig
QuasiExperimentConfig, AdaptiveSampleSizeConfig, VarianceReductionConfig
AnnualizedImpact, ExperimentLearning (enum)
```

#### `bandit.proto`

```protobuf
// New BanditAlgorithm enum values
BANDIT_ALGORITHM_SLATE_FACTORIZED_TS = 5;  // ADR-016
BANDIT_ALGORITHM_SLATE_GENERATIVE    = 6;  // ADR-016

// New BanditConfig fields (8–13)
repeated RewardObjective reward_objectives = 8;    // ADR-011
RewardCompositionMethod composition_method = 9;    // ADR-011
repeated ArmConstraint arm_constraints = 10;       // ADR-012
repeated GlobalConstraint global_constraints = 11; // ADR-012
SlateConfig slate_config = 12;                     // ADR-016
double mad_randomization_fraction = 15;            // ADR-018

// New top-level messages and enums
RewardObjective, ArmConstraint, GlobalConstraint, SlateConfig
RewardCompositionMethod, PositionBiasModel, SlateInteractionModel
```

#### `metric.proto`

```protobuf
// New MetricDefinition fields (15–16)
MetricStakeholder stakeholder = 15;         // ADR-014: USER, PROVIDER, PLATFORM
MetricAggregationLevel aggregation_level = 16;  // ADR-014: USER, EXPERIMENT, PROVIDER
```

#### `analysis_service.proto`

```protobuf
// New RPCs
rpc GetSyntheticControlAnalysis(...) returns (...);  // ADR-023
rpc GetSwitchbackAnalysis(...) returns (...);         // ADR-022

// New MetricResult fields
double e_value = 19;      // ADR-018
double log_e_value = 20;  // ADR-018

// New InterferenceAnalysisResult fields (11–14)
bool feedback_loop_detected = 11;           // ADR-021
double feedback_loop_bias_estimate = 12;    // ADR-021
double contamination_effect_correlation = 13; // ADR-021
Timestamp feedback_loop_computed_at = 14;  // ADR-021
```

#### `event.proto`

```protobuf
// New message
ModelRetrainingEvent  // ADR-021: published by ML training pipeline
```

#### `assignment_service.proto`

```protobuf
// New RPC
rpc GetSlateAssignment(GetSlateAssignmentRequest)
    returns (GetSlateAssignmentResponse);  // ADR-016
```

#### `surrogate.proto`

```protobuf
// New SurrogateModelConfig fields (10–13)
double treatment_effect_correlation = 10;  // ADR-017 TC
double jive_coefficient = 11;              // ADR-017 JIVE
double jive_r_squared = 12;               // ADR-017 JIVE
int32 calibration_experiment_count = 13;  // ADR-017
```

### New Crates

| Crate | ADR | Notes |
|-------|-----|-------|
| `experimentation-flags` | ADR-024 | Replaces `services/flags/` (Go). Added to Cargo workspace. tonic-web + sqlx + rdkafka. |
| `experimentation-management` | ADR-025 | Conditional — only when trigger threshold met. Replaces `services/management/` (Go). |

### Removed Crates and Files

| Artifact | ADR | Removed By | Replacement |
|----------|-----|------------|-------------|
| `crates/experimentation-ffi/` (~400 LOC) | ADR-024 | PR #215 | Direct workspace dependency |
| `services/flags/` (Go, ~2,500 LOC) | ADR-024 | PR #215 | `crates/experimentation-flags/` |
| CGo build tags in CI | ADR-024 | PR #215 | Removed |
| `just test-flags-cgo` recipe | ADR-024 | PR #215 | Unnecessary (same binary) |
| Go SDK CGo bridge (~150 LOC) | ADR-024 | PR #215 | Pure-Go MurmurHash3 (already validated) |

### New Database Migrations

| File | Description |
|------|-------------|
| `sql/migrations/006_evalue_columns.sql` | Adds `e_value`, `log_e_value` to `metric_results` |
| `sql/migrations/007_metric_stakeholder_aggregation.sql` | Adds `stakeholder`, `aggregation_level` to `metric_definitions` |
| `sql/migrations/008_adaptive_sample_size_audit.sql` / `008_feedback_loop_results.sql` | Adaptive N audit trail; feedback loop results table |

> **Note**: Migrations 008 have a numbering conflict between Agent-4 (feedback_loop_results) and Agent-5 (adaptive_sample_size_audit). Resolve before merging both to main by renumbering one to 009.

### New Delta Lake Tables

| Table | ADR | Purpose |
|-------|-----|---------|
| `delta.content_catalog` | ADR-014 | Provider/genre metadata; freshness ≤24h enforced |
| `delta.experiment_level_metrics` | ADR-014 | Catalog-level diversity metrics (Gini, entropy, coverage) |
| `delta.feedback_loop_contamination` | ADR-021 | Pre/post retraining treatment effects per experiment |

### M5 Validation Enforcement (Breaking for Malformed Configs)

Experiments created after PR #212 merges are subject to new validation:
- `MetricDefinition.stakeholder` and `aggregation_level` are **required** (not UNSPECIFIED).
- Bandit reward metrics must have `aggregation_level = USER`.
- Guardrail metrics must have `aggregation_level = USER` or `EXPERIMENT`.
- Existing metric definitions will need backfill migration if null.

---

## Migration Guide

### 1. Using AVLM Sequential Method

AVLM replaces the forced choice between CUPED (variance reduction) and mSPRT (continuous monitoring). It is the recommended default for experiments that want both.

**Proto configuration:**

```protobuf
RunAnalysisRequest {
  experiment_id: "exp-001",
  sequential_method: SEQUENTIAL_METHOD_AVLM,  // field 2 — new
  tau_sq: 0.5,                                 // field 3 — mixing variance (default 0.5)
  // cuped_covariate_metric_id must be set on the MetricDefinition for full benefit
}
```

**Rust (direct stats usage):**

```rust
use experimentation_stats::avlm::{AvlmSequentialTest, AvlmConfig};

let mut test = AvlmSequentialTest::new(AvlmConfig {
    rho: 1.0 / n_planned as f64,  // unit-information prior
    alpha: 0.05,
});

// O(1) per observation
for obs in observations {
    test.update(obs.y, obs.x, obs.is_treatment);
}

let (estimate, lower, upper) = test.confidence_sequence();
let significant = test.is_significant();
```

**When to use**: Whenever both sequential monitoring (`SEQUENTIAL_METHOD_AVLM`) and variance reduction (CUPED covariate configured) are desired. AVLM is a Pareto improvement over CUPED-only and mSPRT-only.

**Golden-file validation**: Validated against R `avlm` package (michaellindon.r-universe.dev/avlm) to 4 decimal places.

### 2. Using E-Values

E-values are computed alongside p-values. They are available in `MetricResult.e_value` and `MetricResult.log_e_value` after any analysis run.

**Interpretation**: `e_value > 1/alpha` rejects the null at level `alpha`. `e_value = 15` → reject at any `alpha ≥ 1/15 ≈ 0.067`. Values close to 1 are uninformative.

**Safe multiplication** (combining evidence from sequential experiments):
```rust
use experimentation_stats::evalue::e_value_grow;

let ev1 = e_value_grow(&control_batch1, &treatment_batch1, mixing_variance);
let ev2 = e_value_grow(&control_batch2, &treatment_batch2, mixing_variance);

// Safe to multiply — no multiple-testing penalty
let combined = ev1.e_value * ev2.e_value;
```

**With AVLM covariate adjustment:**
```rust
use experimentation_stats::evalue::e_value_avlm;

let result = e_value_avlm(
    &control, &treatment,
    &covariate_control, &covariate_treatment,
    0.5,  // mixing variance
);
```

**Online FDR** (Phase 2, Sprint 5.3): Once `OnlineFdrController` is deployed in M5, every experiment conclusion will submit its primary metric's e-value and receive a platform-level FDR-controlled reject/don't-reject decision. Enable via `ExperimentConfig.enable_online_fdr = true`.

### 3. Using Portfolio Optimization (Sprint 5.4)

Portfolio optimization (ADR-019) will expose the following RPCs on M5's ManagementService when implemented:

```protobuf
// Get portfolio-level analytics
rpc GetPortfolioSummary(GetPortfolioSummaryRequest)
    returns (GetPortfolioSummaryResponse);

// Get optimal alpha recommendation for a new experiment
rpc GetAlphaRecommendation(GetAlphaRecommendationRequest)
    returns (AlphaRecommendation);

// Get traffic allocation recommendations for a layer
rpc OptimizeTrafficAllocation(OptimizeTrafficAllocationRequest)
    returns (OptimizeTrafficAllocationResponse);
```

At experiment conclusion, call `ConcludeExperiment` with the new `learning_classification` field set to one of: `WIN`, `REVERT`, `NEUTRAL`, `PIVOT`, `NO_LEARNING`. This populates the portfolio learning rate metric.

### 4. Using Adaptive Sample Size

Enable via `AdaptiveSampleSizeConfig` on experiment creation:

```protobuf
AdaptiveSampleSizeConfig {
  enabled: true,
  interim_fraction: 0.5,        // 50% of planned duration
  promising_zone_lower: 0.30,   // below = futile
  promising_zone_upper: 0.90,   // above = favorable (no action)
  max_sample_multiplier: 2.0,   // cap at 2× planned duration
  blinded: true,                // preserve integrity
}
```

M5 will automatically trigger the interim analysis at the configured fraction, extend promising-zone experiments, and send notifications for futile-zone experiments. The zone classification and (if extended) the new planned duration appear in the M6 experiment detail page.

### 5. Feedback Loop Interference

No configuration required. Detection runs automatically when `ModelRetrainingEvent` events are available on the `model_retraining_events` Kafka topic.

**Required integration**: The recommendation model training pipeline must publish `ModelRetrainingEvent` events:

```protobuf
ModelRetrainingEvent {
  event_id: "retrain-2026-03-24",
  model_id: "rec-v3",
  training_data_start: "2026-03-17T00:00:00Z",
  training_data_end: "2026-03-24T00:00:00Z",
  includes_experiment_data: true,
  active_experiment_ids: ["exp-001", "exp-002"],
  retrained_at: "2026-03-24T06:00:00Z",
}
```

Publish to topic: `model_retraining_events` (8 partitions). M2's ingest pipeline validates and deduplicates via Bloom filter.

When interference is detected (paired t-test p < 0.05 AND |correlation| > 0.5), M6's interference tab shows the pre/post retraining comparison, contamination fraction, bias-corrected treatment effect, and mitigation recommendations.

### 6. Multi-Objective Bandit Rewards

```protobuf
BanditConfig {
  algorithm: BANDIT_ALGORITHM_THOMPSON_SAMPLING,
  reward_objectives: [
    { metric_id: "watch_time_minutes", weight: 0.7 },
    { metric_id: "content_diversity_score", weight: 0.3 },
  ],
  composition_method: REWARD_COMPOSITION_WEIGHTED_SUM,
}
```

Three composition strategies available:
- `WEIGHTED_SUM`: Σ wᵢ × normalized(rᵢ) — convex objectives
- `EPSILON_CONSTRAINT`: Lagrangian relaxation with per-metric floor thresholds — one primary objective plus constraint secondaries
- `TCHEBYCHEFF`: −maxᵢ{wᵢ × max(0, idealᵢ − normalized(rᵢ))} — non-convex Pareto-optimal

Metric normalization uses EMA (α = 0.01) to maintain running mean/variance. Composer state is persisted in RocksDB alongside posterior parameters, surviving crashes and restarts.

### 7. Provider-Side Metrics

When the `delta.content_catalog` table is populated and freshness is within 24h, provider-side metrics are available automatically for experiments. Configure via `MetricDefinition`:

```protobuf
MetricDefinition {
  metric_id: "catalog_gini_coefficient",
  stakeholder: METRIC_STAKEHOLDER_PROVIDER,
  aggregation_level: METRIC_AGGREGATION_LEVEL_EXPERIMENT,
  // ... existing fields
}
```

Metrics with `aggregation_level = EXPERIMENT` (Gini, entropy, coverage, longtail share, provider exposure) are stored in `delta.experiment_level_metrics`. User-level diversity metrics (genre entropy, discovery rate, provider diversity, ILD) are stored in `delta.metric_summaries` per user.

---

## Performance Characteristics

### Verified Benchmarks (Implementation Complete)

| Component | Target | Status | Notes |
|-----------|--------|--------|-------|
| AVLM `update()` | O(1) per observation | **Verified** | 6 scalar running sufficient statistics; no allocation |
| AVLM `confidence_sequence()` | O(1) | **Verified** | Closed-form GROW/REGROW boundary |
| M7 `EvaluateFlag` p99 | <5ms at 20K rps | **Verified** | PR #215: load test passing; eliminates 280ns/call CGo overhead |
| E-value GROW computation | <5μs per call | **Verified** | Direct arithmetic; no allocation |
| E-value AVLM computation | <5μs per call | **Verified** | Gaussian mixture closed-form |
| Feedback loop `detect()` | O(N) in retraining events | **Verified** | N typically < 10 per experiment |
| TC/JIVE K-fold calibration | O(K × E²) in experiments | **Verified** | Batched via historical experiments; offline |
| Adaptive N conditional power | O(1) | **Verified** | Closed-form at interim look |

### Specification Targets (Implementation Planned)

| Component | Target | ADR | Sprint |
|-----------|--------|-----|--------|
| LP constraint solver | <50μs (general linear) | ADR-012 | 5.3 |
| Slate bandit inference | <1ms for L=10, K=100 | ADR-016 | 5.4 |
| Switchback HAC SE | <1ms for N<100 blocks | ADR-022 | 5.3 |
| SCM (Augmented, 30 donors, 365 periods) | <10ms | ADR-023 | 5.3 |
| ORL Q-function fit | <30min via Spark (offline) | ADR-017 | 5.5 |

### Crate-Level Test Counts (Post Phase 5 PRs)

| Crate/Service | Tests | Notes |
|---------------|-------|-------|
| `experimentation-stats` | 192+ | +AVLM, +TC/JIVE, +e-values, +adaptive-N, +feedback loop |
| `experimentation-bandit` | 46+ | +multi-objective (18 unit + 4 proptest) |
| `experimentation-analysis` | 52+ | +AVLM integration |
| `experimentation-flags` | 13 chaos + load | ADR-024 port |
| M6 UI | 400+ | +AVLM/adaptive-N UI, +provider health |
| services/metrics (Go) | Spark SQL tests | +provider metrics, +feedback loop |

---

## Known Gaps and Future Work

The following items are scoped for later Phase 5 sprints and are **not yet implemented**:

- **ADR-012 LP Constraints** (Sprint 5.3): Provider exposure guarantees via LP post-processing on LMAX core.
- **ADR-013 Meta-Experiments** (Sprint 5.4): Testing different bandit objective functions as experiment variants.
- **ADR-015 Phase 2 MLRATE** (Sprint 5.5): LightGBM/XGBoost cross-fitted control variates for ~19% additional variance reduction.
- **ADR-016 Slate Bandits** (Sprint 5.4): Slot-wise factorized Thompson Sampling + LIPS OPE; `GetSlateAssignment` RPC.
- **ADR-017 Phase 2 ORL** (Sprint 5.5): Doubly-robust MDP estimator for long-term causal effects.
- **ADR-018 Phases 2–3** (Sprints 5.3, 5.5): e-LOND OnlineFdrController singleton; MAD e-processes for bandit inference.
- **ADR-019 Portfolio Optimization** (Sprint 5.4): Experiment program analytics, optimal alpha, traffic allocation optimizer.
- **ADR-022 Switchback Designs** (Sprint 5.3): Temporal alternation experiments for platform-wide treatments.
- **ADR-023 Synthetic Control** (Sprint 5.3): Quasi-experimental analysis for non-randomizable interventions.
- **ADR-025 M5 Conditional Port**: Trigger evaluation at Sprint 5.5 end — currently ADR-020 ✅, ADR-021 ✅, ADR-018 P1 ✅ (P2 needed to reach threshold).

---

## References

- Phase 5 ADR index: `docs/adrs/README.md`
- Sprint prompts: `docs/coordination/sprint-prompts.md`
- Coordinator playbook: `docs/coordination/phase5-playbook.md`
- Phase 5 implementation plan: `docs/coordination/phase5-implementation-plan.md`
- Per-agent status files: `docs/coordination/status/agent-N-status.md`
