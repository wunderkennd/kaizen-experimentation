# Phase 5 Implementation Plan

**Status**: Sprint 5.5 in progress (2026-04-06)
**Owner**: Multiclaude (7 agents, supervisor daemon)
**ADRs**: 011–025 (15 decisions, 6 clusters)
**Sprint length**: ~3 weeks each; 6 sprints total (5.0–5.5)

---

## Sprint Overview

| Sprint | Weeks | Theme | Status |
|--------|-------|-------|--------|
| 5.0 | 1–3 | Schema & Foundations | ✅ Complete |
| 5.1 | 4–6 | Measurement Foundations | ✅ Complete |
| 5.2 | 7–9 | Statistical Core | ✅ Complete |
| 5.3 | 10–12 | Constraints & New Experiment Types | ✅ Complete |
| 5.4 | 13–15 | Slate Bandits & Meta-Experiments | ✅ Complete |
| 5.5 | 16–18 | Advanced & Integration | 🔵 In Progress |

---

## Sprint 5.0 — Schema & Foundations

| # | Task | Owner | Status | PR | Notes |
|---|------|-------|--------|-----|-------|
| 5.0.1 | Proto schema extensions (all Phase 5 ADRs) | Agent-4 | ✅ Complete | #196 | ExperimentType META/SWITCHBACK/QUASI, BanditConfig fields, MetricStakeholder, AVLM, TC/JIVE, AdaptiveSampleSizeConfig, ModelRetrainingEvent, SlateConfig, e-value columns, GetSlateAssignment RPC |
| 5.0.2 | AVLM Phase 1 (AvlmSequentialTest in experimentation-stats) | Agent-4 | ✅ Complete | #199 | 6 sufficient stats, O(1) update, golden-file vs R avlm, proptest coverage invariant |
| 5.0.3 | TC/JIVE surrogate calibration fix (ADR-017 Phase 1) | Agent-4 | ✅ Complete | #198 | K-fold IV estimation, InstrumentStrength, 3 Netflix KDD 2024 golden scenarios |
| 5.0.4 | E-value computation Phase 1 (ADR-018) | Agent-4 | ✅ Complete | #200 | e_value_grow, e_value_avlm, SQL migration 006, golden-file to 6dp |
| 5.0.5 | M7 Rust port scaffold (ADR-024 Phase 1) | Agent-7 | ✅ Complete | #197 | experimentation-flags crate, tonic-web, sqlx migrations, Flag CRUD |

---

## Sprint 5.1 — Measurement Foundations

| # | Task | Owner | Status | PR | Notes |
|---|------|-------|--------|-----|-------|
| 5.1.1 | Provider-side metrics: Delta tables + SQL templates (ADR-014) | Agent-3 | ✅ Complete | #211 | 10 SQL templates, content_catalog freshness validation, query_log integration |
| 5.1.2 | Guardrail Bonferroni beta-correction + M5 metric validation (ADR-014) | Agent-4/5 | ✅ Complete | #212 | guardrail_bonferroni() in stats, M5 stakeholder/aggregation enforcement, SQL migration 007 |
| 5.1.3 | ModelRetrainingEvent ingestion (ADR-021) | Agent-2 | ✅ Complete | #209 | model_retraining_events Kafka topic, M2 validation + Bloom dedup |
| 5.1.4 | M7 Rust port Phases 2–4 (ADR-024) | Agent-7 | ✅ Complete | #215 | Business logic, chaos tests, 20K rps load test, experimentation-ffi deleted |
| 5.1.5 | M6 provider health dashboard (ADR-014) | Agent-6 | ✅ Complete | #208 | /portfolio/provider-health, Recharts time series, provider filter |

---

## Sprint 5.2 — Statistical Core

| # | Task | Owner | Status | PR | Notes |
|---|------|-------|--------|-----|-------|
| 5.2.1 | Wire AVLM into M4a RunAnalysis service (ADR-015) | Agent-4 | ✅ Complete | #226 | SEQUENTIAL_METHOD_AVLM=4, null covariate fallback, integration test: AVLM CI narrower than mSPRT |
| 5.2.2 | Multi-objective reward composition on LMAX core (ADR-011) | Agent-4 | ✅ Complete | #221, #228 | WeightedSum, EpsilonConstraint, Tchebycheff; MetricNormalizer EMA; RocksDB persisted; 18 unit + 4 proptest |
| 5.2.3 | Adaptive sample size recalculation (ADR-020) | Agent-5 | ✅ Complete | #227 | conditional_power, blinded_pooled_variance, zone_classify, GST spending realloc, M5 scheduler |
| 5.2.4 | Feedback loop detection in experimentation-stats (ADR-021) | Agent-4 | ✅ Complete | #222 | FeedbackLoopDetector, paired t-test, OLS bias correction, M3 contamination SQL |
| 5.2.5 | AVLM + adaptive N + feedback loop UI (ADR-015/020/021) | Agent-6 | ✅ Complete | #223 | CS boundary plot, zone badge, retraining timeline, bias-corrected estimate panel |
| 5.2.6 | LP constraint layer (ADR-012) | Agent-4 | ✅ Complete | #245 | KL(q‖p) over constraint polytope, <50μs target, LMAX integration |

---

## Sprint 5.3 — Constraints & New Experiment Types

| # | Task | Owner | Status | PR | Notes |
|---|------|-------|--------|-----|-------|
| 5.3.1 | Switchback analysis (ADR-022) | Agent-4 | ✅ Complete | #258 | SwitchbackAnalyzer, HAC SE (Newey-West), randomization inference, carryover test |
| 5.3.2 | Switchback assignment in M1 (ADR-022) | Agent-1 | ✅ Complete | #229 | Time-based assignment, 3 designs (simple/balanced/randomized), washout exclusion |
| 5.3.3 | Synthetic control (ADR-023) | Agent-4 | ✅ Complete | #243, #258 | SyntheticControlAnalyzer 4 methods, placebo tests, golden-file vs R augsynth |
| 5.3.4 | e-LOND OnlineFdrController (ADR-018 Phase 2) | Agent-5 | ✅ Complete | #231 | M5 singleton, PostgreSQL persistence, alpha-wealth geometric decay |
| 5.3.5 | Switchback + SCM M6 UI tabs | Agent-6 | ✅ Complete | #252 | Block timeline, ACF plot, treated vs synthetic control, placebo panel |

---

## Sprint 5.4 — Slate Bandits & Meta-Experiments

| # | Task | Owner | Status | PR | Notes |
|---|------|-------|--------|-----|-------|
| 5.4.1 | Slate bandit policy (ADR-016) | Agent-4 | ✅ Complete | #329 | SlatePolicy slot-wise factorized TS, 3 reward attribution models, LIPS OPE estimator |
| 5.4.2 | GetSlateAssignment RPC in M1 (ADR-016) | Agent-1 | ✅ Complete | #327 | Forward candidates to M4b, return ordered slate with per-slot probabilities |
| 5.4.3 | META experiment type (ADR-013) | Agent-5 | ✅ Complete | #331 | STARTING validation for MetaExperimentConfig, M4b isolated policy per variant |
| 5.4.4 | Portfolio optimization (ADR-019) | Agent-5 | ✅ Complete | #328 | ExperimentLearning, alpha recommendation, traffic allocator, decision rule eval |
| 5.4.5 | Portfolio page + meta-experiment results + slate heatmap (M6) | Agent-6 | ✅ Complete | #330 | Win rate, learning rate, annualized impact, Pareto frontier, slate visualization |

---

## Sprint 5.5 — Advanced & Integration

| # | Task | Owner | Status | PR | Notes |
|---|------|-------|--------|-----|-------|
| 5.5.1 | ORL doubly-robust estimator (ADR-017 Phase 2) | Agent-4 | 🟡 Planned | — | Q-function, density ratio, DR combination, M3 user_trajectories table |
| 5.5.2 | MLRATE cross-fitting (ADR-015 Phase 2) | Agent-3 | 🟡 Planned | — | LightGBM K-fold cross-fitted predictions in metric_summaries |
| 5.5.3 | MAD e-processes (ADR-018 Phase 3) | Agent-4 | 🟡 Planned | — | M4b uniform mixing, MAD e-process from uniform component |
| 5.5.4 | E-value UI + FDR badge (ADR-018) | Agent-6 | 🟡 Planned | — | e-value column alongside p-values, FDR decision badge, optimal alpha widget |
| 5.5.5 | Phase 5 integration test suite | Agent-4 | 🟡 Planned | — | 4 E2E tests: multi-objective+AVLM, switchback, SCM, meta-experiment |
| 5.5.6 | ADR-025 trigger evaluation | Coordinator | 🟡 Planned | — | Count ADRs {015P2, 018, 019, 020, 021}: currently 2/5 (020✅, 021✅); need 1 more |

---

## ADR Implementation Status Summary

| ADR | Title | Cluster | Status | Sprint Done | PR(s) |
|-----|-------|---------|--------|-------------|-------|
| ADR-011 | Multi-objective bandit reward | A | ✅ Complete | 5.2 | #221, #228 |
| ADR-012 | LP constraint post-processing | A | ✅ Complete | 5.2 | #245 |
| ADR-013 | Meta-experiments | A | 🔵 In Progress | 5.4 | #249, #259 |
| ADR-014 | Provider-side metrics | A | ✅ Complete | 5.1 | #208, #211, #212 |
| ADR-015 P1 | AVLM sequential CUPED | B | ✅ Complete | 5.0/5.2 | #199, #226 |
| ADR-015 P2 | MLRATE cross-fitting | B | 🟡 Planned | 5.5 | — |
| ADR-016 | Slate bandit optimization | C | 🔵 In Progress | 5.4 | #253 |
| ADR-017 P1 | TC/JIVE calibration fix | C | ✅ Complete | 5.0 | #198 |
| ADR-017 P2 | ORL estimator | C | 🟡 Planned | 5.5 | — |
| ADR-018 P1 | E-value computation | B | ✅ Complete | 5.0 | #200 |
| ADR-018 P2 | e-LOND FDR controller | B | ✅ Complete | 5.3 | #231, #267 |
| ADR-018 P3 | MAD e-processes | B | 🟡 Planned | 5.5 | — |
| ADR-019 | Portfolio optimization | E | 🔵 In Progress | 5.4 | #250, #261 |
| ADR-020 | Adaptive sample size | B | ✅ Complete | 5.2 | #227 |
| ADR-021 | Feedback loop interference | E | ✅ Complete | 5.1/5.2 | #209, #222 |
| ADR-022 | Switchback experiments | D | ✅ Complete | 5.3 | #229, #252, #258, #259 |
| ADR-023 | Synthetic control | D | ✅ Complete | 5.3 | #243, #252, #258 |
| ADR-024 | M7 Rust port | F | ✅ Complete | 5.0/5.1 | #197, #215 |
| ADR-025 | M5 conditional Rust port | F | 🔵 Conditional | 5.5 eval | — |

**Legend**: ✅ Complete | 🔵 In Progress/Conditional | 🟡 Planned | ⚪ Blocked

---

## ADR-025 Trigger Tracker

Port M5 to Rust when ≥3 of {ADR-015 P2, ADR-018 (full), ADR-019, ADR-020, ADR-021} are complete:

| ADR | Requirement | Status |
|-----|-------------|--------|
| ADR-015 Phase 2 | MLRATE in M3 | 🟡 Planned |
| ADR-018 (all phases) | OnlineFdrController + MAD | 🔵 Phase 1 done; P2/P3 planned |
| ADR-019 | Portfolio optimization | 🟡 Planned |
| ADR-020 | Adaptive sample size | ✅ Complete |
| ADR-021 | Feedback loop interference | ✅ Complete |

**Count**: 2/5 complete. Trigger requires 3. Evaluate at end of Sprint 5.5.

---

## Proto Schema Status

All Phase 5 proto extensions landed in PR #196 (`buf lint` + `buf breaking` clean). No further proto changes anticipated except:
- `online_fdr_state` table migration (ADR-018 Phase 2)
- `user_trajectories` Delta Lake table (ADR-017 Phase 2)

---

## Cross-Agent Dependencies (Phase 5)

| Provider | Consumer | Dependency | Status |
|----------|----------|------------|--------|
| Agent-4 (ADR-012 LP) | Agent-1 (Assignment) | LP-adjusted assignment probabilities in exposure events | Blocked on ADR-012 |
| Agent-4 (ADR-016 Slate policy) | Agent-1 (GetSlateAssignment RPC) | M4b slate policy output | Blocked on ADR-016 |
| Agent-4 (ADR-020 ConditionalPowerClient) | Agent-5 (M5 scheduler) | gRPC wrapper for ComputeConditionalPower | Interface defined; Agent-4 implements server |
| Agent-2 (ModelRetrainingEvent) | Agent-4 (FeedbackLoopDetector) | Kafka events → Delta contamination table | ✅ Complete |
| Agent-3 (MLRATE predictions) | Agent-4 (AVLM Phase 2) | Cross-fitted covariate in metric_summaries | Blocked on ADR-015 P2 |
| Agent-5 (ExperimentLearning classification) | Agent-6 (Portfolio dashboard) | Learning rate computation | Blocked on ADR-019 |

---

## Risk Register

| Risk | Severity | Mitigation |
|------|----------|------------|
| SQL migration 008 numbering conflict (adaptive_n vs feedback_loop) | Medium | Renumber one to 009 before both merge to main |
| ADR-025 trigger may not be met if ADR-019 or ADR-015 P2 slip | Low | M5 remains Go with M4a RPCs; architecture remains functional |
| LP constraint solver <50μs target may require significant optimization | Medium | Interior-point solver; benchmark before PR; fallback to simplex if needed |
| Switchback carryover bias if washout is insufficient for SVOD treatment | Medium | Enforce block_duration ≥ 1h in M5 validation; carryover test in analyzer |
| M5 ConditionalPowerClient interface defined but not wired (ADR-020) | Low | Interface defined in processor.go:62; Agent-4 implements M4a server side |
