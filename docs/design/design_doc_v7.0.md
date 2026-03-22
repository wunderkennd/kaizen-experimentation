

EXPERIMENTATION PLATFORM
System Design — Post-Implementation Reference & Phase 5 Architecture Plan
SVOD-Native Architecture for Streaming Platforms
ConnectRPC · Go + Rust · Contextual Bandits · Interleaving · Open-Source Patterns
Version 7.0  —  March 2026
CONFIDENTIAL


# Table of Contents

1. Executive Summary
   1.1 Design Principles
   1.2 Open-Source Lineage
   1.3 SVOD-Specific Capabilities
   1.4 Language & Module Strategy
   1.5 Implementation Status
   1.6 Phase 5 Overview (NEW)
2. Open-Source Architectural Patterns
   2.1 Cargo Workspace with Crate Layering
   2.2 Crash-Only Design
   2.3 LMAX-Inspired Single-Threaded Policy Core
   2.4 Fail-Fast Data Integrity
   2.5 Component State Machine
   2.6 SDK Provider Abstraction
   2.7 SQL Transparency & Notebook Export
   2.8 Group Sequential Tests
   2.9 Automated Bucket Reuse
   2.10 Guardrails Default to Auto-Pause
3. Proto Schema Architecture
   3.1 Repository Structure
   3.2 Experiment State Machine
   3.3 SVOD-Specific Protos
   3.4 Sequential Testing Config
   3.5 Guardrail Action Config
   3.6 Phase 5 Proto Extensions (NEW)
4. Module 1: Assignment Service (Rust)
5. Module 2: Event Pipeline (Rust + Go)
6. Module 3: Metric Computation Engine (Go)
7. Module 4a: Statistical Analysis Engine (Rust)
8. Module 4b: Bandit Policy Service (Rust)
9. Module 5: Experiment Management Service (Go)
10. Module 6: Decision Support UI (TypeScript, UI Only)
11. Module 7: Feature Flag Service (Go)
12. Cross-Cutting Concerns
   12.1 Observability
   12.2 Infrastructure
   12.3 Documentation Infrastructure
13. Deployment Architecture
   13.1 Service Inventory
   13.2 Deployment Topologies
   13.3 Scaling Strategy
   13.4 Disaster Recovery
14. Implementation Record
   14.1 Phase 0: Schema & Toolchain
   14.2 Phase 1: Foundation
   14.3 Phase 2: Analysis & UI
   14.4 Phase 3: SVOD-Native + Bandits
   14.5 Phase 4: Advanced & Polish
   14.6 Cross-Cutting Fixes
15. Agent Coordination Protocol
   15.1 Multi-Agent Development Model
   15.2 Pair Integration Results
   15.3 Contract Testing Strategy
16. Client SDK Architecture
17. Phase 5: Architecture Evolution (NEW)
   17.1 Statistical Methodology Cluster
   17.2 Multi-Stakeholder Optimization Cluster
   17.3 Bandit & RL Advances Cluster
   17.4 Quasi-Experimental Designs Cluster
   17.5 Platform Operations Cluster
   17.6 Language Migration Cluster (NEW)
   17.7 Per-Module Impact Summary
   17.8 New Data Infrastructure
   17.9 Implementation Sequence
18. Appendix
   18.1 References
   18.2 Glossary
   18.3 Changelog from v6.0

Figures
Figure 1: System Architecture Overview (Section 1.4)
Figure 2: Cargo Workspace Crate Dependency Graph (Section 2.1)
Figure 3: LMAX-Inspired Bandit Policy Threading Model (Section 2.3)
Figure 4: Experiment Lifecycle State Machine (Section 2.5)
Figure 5: SDK Provider Fallback Chain (Section 2.6)
Figure 6: End-to-End Data Flow Pipeline (Section 5)
Figure 7: Deployment Topology Options (Section 13.2)
Figure 8: Phase 5 ADR Dependency Graph (Section 17.9) (NEW)
Figure 9: Phase 5 Per-Module Impact Heatmap (Section 17.7) (NEW)


# 1. Executive Summary

This document is the post-implementation reference and forward architecture plan for the Kaizen Experimentation Platform, a full-stack experimentation system purpose-built for SVOD streaming platforms. All four initial implementation phases are complete, with 30 Phase 1 milestones, all Phase 2–4 milestones, and 10 pair integration test suites merged to main. The platform is operational and validated through chaos engineering, load testing, and statistical golden-file verification.

Unlike general-purpose experimentation tools, this platform treats streaming-specific concerns as first-class architectural primitives: interleaving experiments, surrogate metrics for churn prediction, novelty effect detection, content catalog interference, subscriber lifecycle segmentation, session-level randomization, playback quality experimentation, and content cold-start bandits.

The architecture incorporates patterns from production open-source systems — NautilusTrader (Rust/Python high-performance trading), GrowthBook (warehouse-native experimentation), and Spotify Confidence (statistical analysis at scale) — informing the Rust crate structure, crash recovery design, threading model, SDK abstraction, and statistical validation strategy.

> **Changelog from v6.0**: This version (v7.0) introduces the Phase 5 architecture plan driven by a 2024–2026 experimentation advances gap analysis. Fifteen proposed ADRs (011–025) across six capability clusters define the next evolution: multi-stakeholder optimization (multi-objective bandits, LP constraint layer, meta-experiments, provider metrics), statistical methodology unification (anytime-valid regression adjustment, e-value framework, adaptive sample size), bandit and RL advances (slate-level optimization, offline RL for long-term causal estimation), quasi-experimental designs (switchback experiments, synthetic control methods), platform operations (portfolio optimization, feedback loop interference detection), and language migration (M7 Go→Rust port eliminating the FFI crate, conditional M5 Go→Rust port). The coordination model has been updated for Phase 5: Multiclaude for persistent sprint-level orchestration with per-agent git worktrees, Agent Teams for ad-hoc cross-agent collaboration. Section 17 details the full Phase 5 architecture with per-module impact analysis, new data infrastructure, and the recommended implementation sequence. See Section 18.3 for a full diff from v6.0.


## 1.1 Design Principles

**SVOD-Native**: Streaming-specific experiment types (interleaving, session-level, playback QoE), metric taxonomies, and analysis methods are built in, not bolted on.

**Adaptive**: Contextual bandits and content cold-start bandits enable real-time optimization during experiments.

**Predictive**: Surrogate metric framework projects long-horizon outcomes (churn) from short-term signals.

**Interference-Aware**: Content catalog spillover detection and content holdout designs account for finite-catalog SVOD dynamics.

**Crash-Only (from NautilusTrader)**: Stateless services share startup and recovery code paths. No separate graceful-shutdown logic that goes untested.

**Fail-Fast (from NautilusTrader)**: Invalid data (NaN, overflow, negative durations) triggers immediate failure rather than silent propagation to treatment effect estimates. The `assert_finite!()` macro from `experimentation-core` is enforced across all floating-point computation paths.

**Schema-First**: All interfaces defined in Protobuf with buf toolchain enforcement. The buf v2 configuration uses the STANDARD lint category.

**Language-Appropriate**: Rust for hot paths (assignment, ingestion, statistical computation, bandit policy). Go for orchestration (management, metric job scheduling, feature flags). TypeScript exclusively for browser-rendered UI — TypeScript never performs statistical computation, bandit policy evaluation, or metric aggregation.

**Crate-Layered (from NautilusTrader)**: Rust services share a Cargo workspace with 13 focused crates, feature-flagged bindings, and explicit dependency boundaries.

**SDK-Abstracted (from GrowthBook)**: Client SDKs across five platforms implement a provider interface with Remote, Local, and Mock backends for testability and resilience. All LocalProviders use hash-based variant assignment matching the Rust service exactly.

**Guardrails-Default-Safe (from Spotify)**: Guardrail breaches auto-pause experiments by default; explicit override required to continue. The override is an audited action.


## 1.2 Open-Source Lineage

| Pattern | Source Project | Applied To | Implementation Status |
| --- | --- | --- | --- |
| Cargo workspace with crate layering & feature flags | NautilusTrader | All Rust services: shared core/hash/stats/bandit/ingest crates | ✅ 13 crates, 4 feature flags |
| Crash-only design with externalized state | NautilusTrader | Assignment Service, Event Ingestion (stateless crash recovery) | ✅ Chaos-tested (kill -9 under load) |
| LMAX-inspired single-threaded event loop | NautilusTrader | Bandit Policy Service policy core (channel-fed, lock-free) | ✅ p99 < 15ms at 10K rps verified |
| Fail-fast data integrity with property-based tests | NautilusTrader | Statistical Analysis Engine (proptest invariants) | ✅ 10K-case nightly proptest |
| Component state machine with transitional states | NautilusTrader | Experiment lifecycle (STARTING, CONCLUDING transitions) | ✅ TOCTOU-safe with SELECT FOR UPDATE |
| SDK provider abstraction with fallback chain | GrowthBook | Client SDKs (Remote > Local cache > Static defaults) | ✅ 5 platforms, 206+ SDK tests |
| SQL transparency and notebook export | GrowthBook | Metric Computation Engine (View SQL, Export to Notebook) | ✅ Databricks notebook export |
| Group sequential tests alongside mSPRT | Spotify Confidence | Statistical Analysis Engine (GST for fixed-schedule reviews) | ✅ Validated against R gsDesign + scipy |
| Automated bucket reuse on experiment conclusion | Spotify Confidence | Assignment Service layer allocation recycling | ✅ 24h cooldown, overlap detection |
| Guardrails default to auto-pause | Spotify Confidence | Experiment Management Service guardrail policy | ✅ Kafka consumer, < 60s latency |


## 1.3 SVOD-Specific Capabilities

| Capability | Problem Solved | Module(s) | Status |
| --- | --- | --- | --- |
| Interleaving Experiments | A/B tests need millions of users; interleaving is 10-100x more sensitive | M1, M3, M4a, M5, M6 | ✅ Team Draft + Optimized + Multileave |
| Surrogate Metrics | Churn takes 30-90 days; surrogates predict impact in days | M3, M4a, M5, M6 | ✅ MLflow integration + calibration |
| Novelty Effect Detection | Rec changes show transient spikes; prevents shipping based on fading lift | M4a, M6 | ✅ Gauss-Newton with LM damping |
| Content Interference | Finite catalog creates spillover between treatment/control | M3, M4a, M6 | ✅ JSD, Jaccard, Gini, BH correction |
| Lifecycle Segmentation | Trial/new/established/at-risk subscribers respond differently | M1, M3, M4a, M5, M6 | ✅ CATE + Cochran Q heterogeneity |
| Session-Level Experiments | Some questions are about sessions; requires clustered analysis | M1, M3, M4a, M5 | ✅ HC1 sandwich estimator |
| Playback QoE Experiments | ABR/CDN/encoding have distinct metrics (rebuffer, TTFF, bitrate) | M2, M3, M4a, M6 | ✅ QoE-engagement correlation |
| Content Cold-Start Bandit | New content has no data; bandit learns optimal user targeting | M4b, M3, M6 | ✅ Affinity score export |
| Cumulative Holdout Groups | Measures total algorithmic lift over time | M1, M4a, M5, M6 | ✅ Fail-closed holdout assignment |
| Multi-Stakeholder Optimization | Balances engagement, provider fairness, catalog diversity in bandits | M4b, M3, M4a, M5, M6 | 📋 Proposed (ADR-011–014) |
| Sequential Variance Reduction | Combines CUPED with anytime-valid monitoring (AVLM) | M4a, M3 | 📋 Proposed (ADR-015) |
| Slate-Level Bandits | Optimizes full recommendation slates with cross-item interactions | M4b, M4a, M1 | 📋 Proposed (ADR-016) |
| Long-Term Causal Estimation | Offline RL corrects surrogate paradigm for continual treatments | M4a, M3 | 📋 Proposed (ADR-017) |
| E-Value Framework | Anytime-valid inference with online FDR control across experiments | M4a, M5 | 📋 Proposed (ADR-018) |
| Portfolio Optimization | Program-level experiment optimization and impact tracking | M5, M4a, M6 | 📋 Proposed (ADR-019) |
| Adaptive Sample Size | Mid-experiment sample size recalculation via promising-zone designs | M4a, M5 | 📋 Proposed (ADR-020) |
| Feedback Loop Detection | Detects model retraining contamination across experiment groups | M4a, M2, M3 | 📋 Proposed (ADR-021) |
| Switchback Experiments | Temporal alternation designs for interference-prone treatments | M1, M4a, M5, M6 | 📋 Proposed (ADR-022) |
| Synthetic Control | Quasi-experimental evaluation for non-randomizable interventions | M4a, M3, M5, M6 | 📋 Proposed (ADR-023) |


## 1.4 Language & Module Strategy

| Module | Language | Agent | Responsibility |
| --- | --- | --- | --- |
| M1: Assignment | Rust | Agent-1 | Variant allocation, interleaving list construction, bandit arm delegation, SDK bindings |
| M2: Event Pipeline | Rust+Go | Agent-2 | Rust: event validation/dedup/Kafka publish. Go: job orchestration/alerting |
| M3: Metric Computation | Go | Agent-3 | Spark SQL orchestration: per-user metrics, interleaving scores, surrogates, QoE |
| M4a: Statistical Analysis | Rust | Agent-4 | All statistical computation: frequentist, Bayesian, mSPRT, GST, CUPED, novelty, interference, CATE, IPW, clustered SEs |
| M4b: Bandit Policy | Rust | Agent-4 | All bandit computation: Thompson, LinUCB, Neural (Candle), cold-start. LMAX single-thread core |
| M5: Experiment Mgmt | Go | Agent-5 | CRUD, lifecycle state machine, RBAC, auto-pause guardrails, bucket reuse mgmt |
| M6: Decision Support UI | TypeScript (UI only) | Agent-6 | Frontend dashboards and visualization only. No backend computation. 355+ tests. |
| M7: Feature Flags | Go → Rust (ADR-024) | Agent-7 | Progressive delivery, flag evaluation, all-types promotion, reconciler |

Figure 1: System Architecture Overview
See accompanying file: `docs/design/system_architecture.mermaid`


## 1.5 Implementation Status

As of March 2026, all four initial implementation phases are complete. Phase 5 (Architecture Evolution) is proposed, driven by 13 ADRs from the 2024–2026 gap analysis:

| Phase | Scope | Milestones | Status |
| --- | --- | --- | --- |
| Phase 0 | Schema & Toolchain | 10 | ✅ Pre-seeded |
| Phase 1 | Foundation | 30 | ✅ All merged |
| Phase 2 | Analysis & UI | 11 | ✅ All merged |
| Phase 3 | SVOD-Native + Bandits | 8 | ✅ All merged |
| Phase 4 | Advanced & Polish | 8 | ✅ All merged |
| Phase 5 | Architecture Evolution | 15 ADRs | 📋 Proposed (ADR-011 through ADR-025) |

All 10 pair integration test suites are green. Cross-cutting review (Devin AI + human review) addressed 12 bugs across 6 crates/services. 163 PRs merged to main.

| Agent | Module | Status | Test Count |
| --- | --- | --- | --- |
| Agent-1 | M1 Assignment | ✅ All Phases Complete (polish: mobile SDK CI) | 206+ SDK tests, 15 E2E, 10 contract |
| Agent-2 | M2 Pipeline | ✅ All Phases Complete | 119 Rust tests, 7-phase E2E |
| Agent-3 | M3 Metrics | ✅ All Phases Complete | Prometheus 7 metrics, Grafana 6 panels |
| Agent-4 | M4a + M4b | ✅ All Phases Complete | Golden files (Bayesian, IPW, clustering), 12 M4a-M6 contract tests |
| Agent-5 | M5 Management | ✅ All Phases Complete | 11 M5-M6 contract tests, 10 M1-M5 contract tests |
| Agent-6 | M6 UI | ✅ All Phases Complete | 355 tests, 27 wire-format contract tests |
| Agent-7 | M7 Flags | ✅ All Phases Complete | 13 chaos tests, 20K rps load test |


## 1.6 Phase 5 Overview (NEW)

A comprehensive gap analysis of 2024–2026 experimentation research (50+ papers, 10+ industry platforms) identified capability gaps organized into six clusters. These are documented as proposed ADRs 011–025 and constitute Phase 5 development:

| Cluster | ADRs | Core Capability Gap |
| --- | --- | --- |
| A: Multi-Stakeholder Optimization | 011, 012, 013, 014 | Bandits optimize single scalar reward; SVOD needs engagement + provider fairness + catalog diversity simultaneously |
| B: Statistical Methodology | 015, 018, 020 | CUPED and mSPRT cannot be combined; no cross-experiment FDR control; fixed sample sizes waste traffic |
| C: Bandit & RL Advances | 016, 017 | Single-arm bandits miss cross-item slate interactions; surrogacy assumption violated for continual treatments |
| D: Quasi-Experimental Designs | 022, 023 | No support for interventions that cannot be user-level randomized (pricing, catalog, CDN changes) |
| E: Platform Operations | 019, 021 | No portfolio-level optimization; feedback loop interference from model retraining undetectable |
| F: Language Migration | 024, 025 | FFI crate exists solely for Go M7; expanding Go→Rust computation surface in M5 creates growing RPC/FFI pressure |

The single highest-ROI item is ADR-015 (Anytime-Valid Regression Adjustment), which unifies Kaizen's two most valuable statistical features — CUPED variance reduction and mSPRT sequential monitoring — into a single framework. The highest-urgency item is ADR-017 Phase 1 (TC/JIVE surrogate calibration), which corrects a theoretical error in the production surrogate framework for continual treatments.

Section 17 provides the full Phase 5 architecture plan with per-module impact, new data infrastructure requirements, proto schema extensions, and the recommended implementation sequence.


# 2. Open-Source Architectural Patterns

## 2.1 Cargo Workspace with Crate Layering (from NautilusTrader)

The Rust codebase is organized as a single Cargo workspace with 13 crates across four layers. The workspace root `Cargo.toml` pins all internal crate versions and shared dependencies (Rust edition 2021, minimum version 1.80).

```
crates/
  experimentation-core/          # Timestamps, config types, error types, tracing, assert_finite!()
  experimentation-hash/          # MurmurHash3, bucketing logic
                                   features: [wasm, uniffi, ffi, python]
  experimentation-proto/         # tonic-build generated code from .proto files
  experimentation-stats/         # Bootstrap (BCa/percentile), CUPED, mSPRT, GST, novelty (Gauss-Newton),
                                   interference (JSD/Jaccard/Gini), CATE, IPW (Hájek), clustered SEs (HC1),
                                   Bayesian (Beta-Binomial, Normal-Normal), BH-FDR
                                   features: [simd, python]
  experimentation-bandit/        # Thompson Sampling (MC propensity), LinUCB (Sherman-Morrison),
                                   Neural Contextual (Candle 2-layer MLP), cold-start
                                   features: [gpu]
  experimentation-interleaving/  # Team Draft, Optimized (softmax), Multileave (N-way Team Draft)
  experimentation-ingest/        # Event validation, Bloom filter dedup (rotating hourly)
  experimentation-ffi/           # CGo bindings via cbindgen (for Go hash interop)
  experimentation-assignment/    # M1 Assignment service binary
  experimentation-analysis/      # M4a Analysis service binary (all 5 RPCs wired + PG caching)
  experimentation-pipeline/      # M2 Ingestion service binary
  experimentation-policy/        # M4b Bandit policy service binary
```

| Crate Category | Crates | Purpose |
| --- | --- | --- |
| Foundation | core, hash, proto | Primitives, hashing, generated types |
| Algorithms | stats, bandit, interleaving | Statistical methods, policy algorithms, list construction |
| Infrastructure | ingest, ffi | Event processing, cross-language bindings |
| Services | assignment, analysis, pipeline, policy | Service binaries (thin wrappers over algorithm crates) |

Feature flags control cross-language binding generation: `--features wasm` produces WebAssembly via wasm-bindgen, `--features ffi` produces C headers via cbindgen, `--features uniffi` produces Swift and Kotlin bindings, `--features python` produces PyO3 bindings. The same Rust source produces all targets.

**Implementation note (v6.0)**: The `gpu` feature flag on `experimentation-bandit` now uses Candle (v0.8) instead of tch-rs, eliminating the libtorch system dependency. The migration (PR #159) maintains API compatibility; the 2-layer MLP architecture and dropout behavior are identical.

Figure 2: Cargo Workspace Crate Dependency Graph
See accompanying file: `docs/design/crate_graph.mermaid`


## 2.2 Crash-Only Design (from NautilusTrader)

| Service | State Model | Crash Recovery Strategy | Verified SLA |
| --- | --- | --- | --- |
| Assignment Service | Stateless (config from Management Service) | Restart, re-fetch config snapshot. Identical assignments immediately. | < 2 seconds |
| Event Ingestion | Stateless (Kafka producer) | Restart, reconnect to Kafka. At-least-once delivery. | < 1 second |
| Statistical Analysis | Stateless (reads Delta Lake, writes to Postgres) | Restart, re-run analysis. Idempotent. | N/A (batch) |
| Bandit Policy Service | Stateful (policy parameters in RocksDB) | Snapshot to RocksDB on every reward update. Recovery: load last snapshot, replay from Kafka offset. | < 10 seconds |

All SLAs validated via weekly chaos engineering (kill -9 under load, `weekly-chaos.yml`). Agent-4 crash recovery integration tests verify multi-experiment concurrent restore, Kafka offset verification, high-volume (2200+ rewards) recovery, and timing < 10s SLA.


## 2.3 LMAX-Inspired Single-Threaded Policy Core (from NautilusTrader)

The Bandit Policy Service (M4b) uses a three-thread architecture:

1. **Thread 1 (tokio async)**: gRPC server receives SelectArm requests → sends `(context, oneshot_tx)` into bounded `policy_channel`
2. **Thread 2 (tokio async)**: Kafka consumer receives RewardEvents from `reward_events` topic → sends events into bounded `reward_channel`
3. **Thread 3 (dedicated, single-threaded)**: Policy Core event loop uses `select!` on both channels. All state mutations (posterior updates, Sherman-Morrison rank-1 updates, RocksDB snapshots) occur here with zero mutex contention.

**Implementation note (v6.0)**: Thompson Sampling arm selection now uses Monte Carlo simulation (1000 draws per arm) to compute correct IPW propensity scores. This replaces the closed-form probability computation which was inaccurate for multi-arm cases. The MC approach adds ~50μs per SelectArm call but ensures downstream IPW-adjusted analysis in M4a receives correct assignment probabilities. This change was identified during cross-cutting review (PR #161).

Figure 3: LMAX-Inspired Bandit Policy Threading Model
See accompanying file: `docs/design/lmax_threading.mermaid`


## 2.4 Fail-Fast Data Integrity (from NautilusTrader)

The `assert_finite!()` macro from `experimentation-core` is called on every intermediate floating-point result across all statistical and bandit computation. Violations trigger an immediate panic with context, caught by the crash-only design.

Specific fail-fast patterns implemented:
- NaN or Infinity in any metric value → panic
- Negative watch-time durations → panic during event validation
- Arithmetic overflow in bootstrap accumulators → `checked_add` with panic on overflow
- Division by zero in metric ratios → explicit `Result::Err`
- Negative nanosecond timestamps → guard added (PR #161)

Property-based testing via proptest validates invariants. Nightly CI runs 10,000 cases (vs 256 default in PR builds) with a 30-minute timeout.


## 2.5 Component State Machine (from NautilusTrader)

| State | Type | Description |
| --- | --- | --- |
| DRAFT | Stable | Experiment configured but not yet validated or started. |
| STARTING | Transitional | Validating config, warming bandit policy, confirming metric availability, checking segment power. M1 blocks assignment serving. |
| RUNNING | Stable | Actively collecting data. For bandits, policy is adapting. Includes auto-pause on guardrail breach. |
| CONCLUDING | Transitional | Running final analysis, creating policy snapshots, computing surrogate projections. Result queries return 503. |
| CONCLUDED | Stable | Analysis complete, results available. |
| ARCHIVED | Stable | Retained for historical reference. Bucket allocation released with cooldown. |

**Implementation note (v6.0)**: State transitions use PostgreSQL `UPDATE ... WHERE state = $expected` with `RowsAffected() == 1` for TOCTOU safety. The Pause/Resume transitions were hardened against TOCTOU races (PR #161): concurrent pause and resume operations on the same experiment are serialized through the state precondition check.

Figure 4: Experiment Lifecycle State Machine
See accompanying file: `docs/design/state_machine.mermaid`


## 2.6 SDK Provider Abstraction (from GrowthBook)

All five client SDKs (Web/TypeScript, iOS/Swift, Android/Kotlin, Server-Go, Server-Python) implement the same provider interface with three concrete backends:

- **RemoteProvider**: JSON HTTP POST to Assignment Service. Primary path for production.
- **LocalProvider**: Hash-based variant assignment using cached experiment configs. The variant selection algorithm replicates the Rust service exactly: `relative_bucket = bucket - start; cumulative += fraction * alloc_size; if relative < cumulative → return variant; fallthrough → last variant`. Web SDK uses pure-TS MurmurHash3 (or optional WASM); mobile SDKs use UniFFI bindings (guarded by `#if canImport` on iOS and conditional source set on Android); Go SDK uses CGo FFI (with pure-Go fallback behind `!cgo || !has_ffi` build tag); Python SDK uses `mmh3` library.
- **MockProvider**: Returns deterministic, configurable assignments for unit testing.

A **ResilientProvider** (or equivalent `ExperimentClient` with fallback) wraps these in a fallback chain: Remote → Local cache → null/default.

**Implementation note (v6.0)**: All SDK RemoteProviders use JSON HTTP (ConnectRPC JSON mode) rather than binary gRPC, simplifying client-side requirements. The endpoint pattern is `POST /experimentation.assignment.v1.AssignmentService/GetAssignment` with `Content-Type: application/json`. LocalProviders in all five SDKs have been validated against the first 10 test vectors from `test-vectors/hash_vectors.json`. Total SDK test count: 206+ across all platforms.

Figure 5: SDK Provider Fallback Chain
See accompanying file: `docs/design/sdk_provider.mermaid`


## 2.7 SQL Transparency & Notebook Export (from GrowthBook)

Every metric computation logs the Spark SQL it executed to the `query_log` table in PostgreSQL, keyed by `(experiment_id, metric_id, computation_timestamp)`. The UI exposes a "View SQL" button on every metric result. An "Export to Notebook" button generates a Databricks/Jupyter `.ipynb` file with the SQL queries, data loading code, and analysis pipeline.

**Implementation note (v6.0)**: Notebook export uses a Web Worker for base64 decode to avoid blocking the UI main thread (PR #108). SQL syntax highlighting uses `prism-react-renderer` with lazy loading.


## 2.8 Group Sequential Tests (from Spotify Confidence)

Both sequential testing methods are implemented in M4a (PR #25):

| Method | When to Use | Power | Flexibility |
| --- | --- | --- | --- |
| mSPRT (always-valid) | Continuous monitoring, no fixed schedule | Lower | Arbitrary peeking |
| GST O'Brien-Fleming | Fixed weekly reviews, conservative early stopping | Higher | Pre-commit to N looks |
| GST Pocock | Fixed reviews, equal stopping probability each look | Moderate | Pre-commit to N looks |

GST boundaries are validated against both R's `gsDesign` package (to 4 decimal places, PR #136) and scipy's numerical integration (recursive integration, PR #125).


## 2.9 Automated Bucket Reuse (from Spotify Confidence)

When an experiment transitions to CONCLUDED, its hash-space allocation is automatically returned to the layer's available pool after a configurable cooldown period (default 24 hours). The cooldown prevents late-arriving exposure events from being associated with the wrong experiment.

**Implementation note (v6.0)**: The allocator now includes overlap detection with an `ErrOverlappingRanges` sentinel error (PR #161). When a new experiment requests bucket space that overlaps with an occupied range (active or cooling down), the allocation is rejected with a clear error rather than silently overlapping.


## 2.10 Guardrails Default to Auto-Pause (from Spotify)

Guardrail breaches auto-pause experiments by default:

1. M3 detects breach during hourly guardrail computation
2. M3 publishes `GuardrailAlert` to `guardrail_alerts` Kafka topic
3. M5 consumes alert within 60 seconds and transitions experiment to paused state
4. Experiment owner receives notification with breach details
5. To continue despite a breach, the owner must explicitly set `guardrail_action: ALERT_ONLY` — an audited action

**Implementation note (v6.0)**: The guardrail alert consumer in M5 checks the `guardrail_action` field on the experiment before pausing (PR #18). The `consecutive_breaches_required` field provides graduated sensitivity. The guardrail override creates an audit trail entry, and M5's RBAC interceptor ensures only Experimenter-level or above can perform overrides (PR #71).


# 3. Proto Schema Architecture

All modules share a Protobuf schema layer managed by the buf toolchain (v2, STANDARD lint category). The schema includes 17 `.proto` files across 8 packages with SVOD-specific message types.


## 3.1 Repository Structure

```
proto/experimentation/
  common/v1/
    experiment.proto        # Experiment, Variant, ExperimentState (with STARTING/CONCLUDING)
    metric.proto            # MetricDefinition (14 fields: surrogate, QoE, lifecycle, CUPED support)
    event.proto             # ExposureEvent, MetricEvent, RewardEvent, GuardrailAlert
    layer.proto             # Layer, LayerAllocation (with bucket reuse cooldown)
    targeting.proto         # TargetingRule predicate tree (AND of ORs)
    bandit.proto            # BanditConfig, BanditArm, PolicySnapshot, ArmSelection
    interleaving.proto      # InterleavingConfig, InterleavingScore, CreditAssignment
    surrogate.proto         # SurrogateModelConfig, SurrogateProjection
    lifecycle.proto         # LifecycleSegment (6 values), LifecycleStratificationConfig
    qoe.proto               # QoEEvent, PlaybackMetrics (8 fields)
  assignment/v1/              # AssignmentService: GetAssignment, GetAssignments, GetInterleavedList, StreamConfigUpdates
  pipeline/v1/                # EventIngestionService: 7 RPCs (single + batch for each event type)
  metrics/v1/                 # MetricComputationService: ComputeMetrics, ComputeGuardrailMetrics, ExportNotebook, GetQueryLog
  analysis/v1/                # AnalysisService: RunAnalysis, GetAnalysisResult, GetInterleavingAnalysis, GetNoveltyAnalysis, GetInterferenceAnalysis
  bandit/v1/                  # BanditPolicyService: SelectArm, CreateColdStartBandit, ExportAffinityScores, GetPolicySnapshot, RollbackPolicy
  management/v1/              # ExperimentManagementService: 16 RPCs (CRUD, lifecycle, metrics, layers, targeting, surrogates)
  flags/v1/                   # FeatureFlagService: CRUD, EvaluateFlag, EvaluateFlags, PromoteToExperiment
```


## 3.2 Experiment State Machine

```protobuf
enum ExperimentState {
  EXPERIMENT_STATE_UNSPECIFIED = 0;
  EXPERIMENT_STATE_DRAFT = 1;
  EXPERIMENT_STATE_STARTING = 2;      // Transitional: validating, warming up
  EXPERIMENT_STATE_RUNNING = 3;
  EXPERIMENT_STATE_CONCLUDING = 4;    // Transitional: running final analysis
  EXPERIMENT_STATE_CONCLUDED = 5;
  EXPERIMENT_STATE_ARCHIVED = 6;
}
```


## 3.3 SVOD-Specific Protos

**Interleaving**: `InterleavingMethod` (TEAM_DRAFT, OPTIMIZED, MULTILEAVE), `CreditAssignment` (BINARY_WIN, PROPORTIONAL, WEIGHTED), `InterleavingConfig`, `InterleavingScore`.

**Surrogate**: `SurrogateModelType` (LINEAR, GRADIENT_BOOSTED, NEURAL), `SurrogateModelConfig` (with `calibration_r_squared`, `mlflow_model_uri`), `SurrogateProjection`.

**Lifecycle**: `LifecycleSegment` (TRIAL=1, NEW=2, ESTABLISHED=3, MATURE=4, AT_RISK=5, WINBACK=6), `LifecycleStratificationConfig`.

**QoE**: `PlaybackMetrics` (time_to_first_frame_ms, rebuffer_count, rebuffer_ratio, avg_bitrate_kbps, resolution_switches, peak_resolution_height, startup_failure_rate, playback_duration_ms), `QoEEvent`.


## 3.4 Sequential Testing Config

```protobuf
enum SequentialMethod {
  SEQUENTIAL_METHOD_UNSPECIFIED = 0;
  SEQUENTIAL_METHOD_MSPRT = 1;
  SEQUENTIAL_METHOD_GST_OBF = 2;
  SEQUENTIAL_METHOD_GST_POCOCK = 3;
}

message SequentialTestConfig {
  SequentialMethod method = 1;
  int32 planned_looks = 2;     // For GST: >= 2
  double overall_alpha = 3;    // Default 0.05
}
```


## 3.5 Guardrail Action Config

```protobuf
enum GuardrailAction {
  GUARDRAIL_ACTION_UNSPECIFIED = 0;
  GUARDRAIL_ACTION_AUTO_PAUSE = 1;    // DEFAULT
  GUARDRAIL_ACTION_ALERT_ONLY = 2;    // Requires explicit opt-in
}
```


## 3.6 Phase 5 Proto Extensions (NEW)

Phase 5 ADRs introduce significant proto schema additions. Key new messages and enums (see individual ADRs for full definitions):

**Experiment Types** (ADR-013, 022, 023):
```protobuf
enum ExperimentType {
  // ... existing types ...
  EXPERIMENT_TYPE_META = 9;        // ADR-013: randomize over objective parameterizations
  EXPERIMENT_TYPE_SWITCHBACK = 10; // ADR-022: temporal alternation design
  EXPERIMENT_TYPE_QUASI = 11;      // ADR-023: synthetic control (non-randomized)
}
```

**Bandit Extensions** (ADR-011, 012, 016, 018): `RewardObjective`, `RewardConstraint`, `RewardCompositionMethod` for multi-objective reward composition. `ArmConstraint`, `GlobalConstraint` for LP post-processing layer. `SlateConfig`, `PositionBiasModel`, `SlateInteractionModel` for slate-level bandits. New algorithms `BANDIT_ALGORITHM_SLATE_FACTORIZED_TS` and `BANDIT_ALGORITHM_SLATE_GENERATIVE`. Field `BanditConfig.mad_randomization_fraction` for MAD e-process mixing.

**Metric Extensions** (ADR-014): `MetricStakeholder` enum (USER, PROVIDER, PLATFORM) and `MetricAggregationLevel` enum (USER, EXPERIMENT, PROVIDER) on `MetricDefinition`.

**Analysis Extensions** (ADR-015, 017, 018, 020): `SEQUENTIAL_METHOD_AVLM` for anytime-valid linear model. `VarianceReductionConfig` for ML-assisted covariates. `SurrogateModelConfig` gains TC/JIVE calibration fields. `AdaptiveSampleSizeConfig` for promising-zone designs. `e_value` and `log_e_value` columns on `metric_results`.

**Management Extensions** (ADR-019, 021): `ExperimentLearning` enum for EwL classification. `AnnualizedImpact` for per-experiment impact projection. `InterferenceAnalysisResult` gains feedback loop fields. `ModelRetrainingEvent` as a new event type.

**New RPCs** (ADR-016): `AssignmentService.GetSlateAssignment` for slate-level assignment.


# 4. Module 1: Assignment Service (Agent-1, Rust)

## 4.1 Purpose & Crash-Only Design

Entirely stateless. Fetches experiment configuration from M5 via streaming RPC (`StreamConfigUpdates`), caches in-process using `Arc<RwLock<HashMap<experiment_id, Config>>>` with a watch channel for notifications. On crash, a restarted instance re-fetches config and produces identical assignments immediately.

Crate dependencies: `experimentation-assignment` depends on `experimentation-hash`, `experimentation-proto`, `experimentation-interleaving`, `experimentation-core`.

## 4.2 Assignment Modes

| Mode | Experiment Types | Description |
| --- | --- | --- |
| Static User-Level | A/B, Multivariate, Holdout, Cumulative Holdout | MurmurHash3 bucketing. `bucket = hash(user_id + "\x00" + salt, seed=0) % total_buckets`. |
| Session-Level | Session-Level | `hash(session_id + "\x00" + salt)`. `allow_cross_session_variation` flag controls whether user_id or session_id is hashed. |
| Interleaving | Interleaving | Constructs merged list from 2+ algorithm outputs via Team Draft, Optimized (softmax), or Multileave (N-way). |
| Bandit | MAB, Contextual Bandit, Cold-Start | Calls M4b `SelectArm` via low-latency gRPC. Timeout at 10ms falls back to uniform random. |

## 4.3 Cumulative Holdout Assignment (v6.0 update)

Holdout assignment is **fail-closed**: if the holdout allocation lookup fails (e.g., database error during config fetch), the user is blocked from the layer entirely rather than leaking into treatment. This prevents holdout contamination from transient infrastructure failures (PR #161).

## 4.4 Bucket Reuse

When the config snapshot indicates an experiment has transitioned to CONCLUDED and the cooldown period has elapsed, the Assignment Service stops serving that experiment's assignments. The config cache auto-registers layers on first encounter and the allocator includes overlap detection.

## 4.5 SDK Architecture

Five complete SDK implementations with RemoteProvider (JSON HTTP), LocalProvider (hash-based), and MockProvider:

| SDK | Language | Hash Implementation | RemoteProvider Transport |
| --- | --- | --- | --- |
| Web | TypeScript | Pure-TS MurmurHash3 (or optional WASM) | fetch API |
| iOS | Swift | UniFFI (guarded by `#if canImport`) | URLSession |
| Android | Kotlin | UniFFI (conditional source set) | HttpURLConnection |
| Server-Go | Go | CGo FFI (pure-Go fallback) | net/http |
| Server-Python | Python | mmh3 library | httpx.AsyncClient |

## 4.6 Verified Performance

| Metric | Target | Achieved |
| --- | --- | --- |
| GetAssignment p99 | < 5ms | ✅ at 50K rps (PR #138) |
| GetInterleavedList p99 | < 15ms | ✅ |
| Crash recovery | < 2 seconds | ✅ Chaos-tested |
| Hash parity | 10,000 vectors | ✅ Rust, WASM, CGo, Python, TS |

## 4.7 Phase 5 Planned (ADR-016, 022)

- **Slate assignment** (ADR-016): New `GetSlateAssignment` RPC. M1 forwards candidate items to M4b's slate bandit, receives an ordered slate with per-slot probabilities. SDKs gain a `getSlate()` method.
- **Switchback assignment** (ADR-022): Time-based assignment logic replaces hash-based bucketing for `EXPERIMENT_TYPE_SWITCHBACK`. Assignment determined by `(current_time, block_duration, cluster_attribute)` rather than `hash(user_id)`. Washout period exclusion.
- **Meta-experiment routing** (ADR-013): `GetAssignment` for META experiments hashes user to variant, then delegates to M4b with variant-specific objective configuration.


# 5. Module 2: Event Pipeline (Agent-2, Rust + Go)

## 5.1 Crash-Only Ingestion (Rust)

Stateless. Validates events, deduplicates via Bloom filter (rotating hourly, sized for 100M events/day at 0.1% FPR), publishes to Kafka with idempotent producer config. On crash, restarts with empty Bloom filter; brief duplicate window absorbed by downstream dedup.

## 5.2 Event Types & Kafka Topics

| Topic | Partitions | Key Consumers |
| --- | --- | --- |
| `exposures` | 64 | Metric Engine, Monitoring |
| `metric_events` | 128 | Metric Engine, Monitoring |
| `reward_events` | 32 | Bandit Policy Service, Metric Engine |
| `qoe_events` | 64 | Metric Engine, QoE Dashboard |
| `guardrail_alerts` | 8 | Management Service (auto-pause trigger), UI |
| `surrogate_recalibration_requests` | 4 | Metric Engine |
| `model_retraining_events` | 8 | Metric Engine, Analysis Engine | 📋 Phase 5 (ADR-021) |

## 5.3 E2E Validation

A comprehensive 7-phase E2E test (`test_full_pipeline_e2e.sh`, PR #124) validates the M1→M2→Kafka→M3→M4a data flow with ~24 individual checks. Session pipeline and QoE pipeline have dedicated E2E harnesses (`test_session_pipeline_e2e.sh`, `test_qoe_pipeline_e2e.sh`).

Figure 6: End-to-End Data Flow Pipeline
See accompanying file: `docs/design/data_flow.mermaid`


# 6. Module 3: Metric Computation Engine (Agent-3, Go)

## 6.1 Metric Types

| Type | Implementation | SQL Template |
| --- | --- | --- |
| MEAN | Spark SQL | `standard_metric.sql.tmpl` |
| PROPORTION | Spark SQL | `standard_metric.sql.tmpl` |
| COUNT | Spark SQL | `standard_metric.sql.tmpl` |
| RATIO | Spark SQL + delta method | `ratio_metric.sql.tmpl` |
| PERCENTILE | Spark SQL | `percentile_metric.sql.tmpl` |
| CUSTOM | User-provided SQL | `custom_metric.sql.tmpl` |

## 6.2 SVOD-Specific Computations

All SVOD computations are implemented as Go-orchestrated Spark SQL jobs with full query logging:

- **Interleaving Scoring**: Joins exposure provenance with engagement events, applies credit assignment method.
- **Surrogate Metrics**: Loads MLflow models, computes projections. Kafka-driven recalibration consumer (PR #113).
- **Lifecycle Segmentation**: Redis feature store classification, per-segment metric computation.
- **Content Consumption**: Per-variant title-level distributions for interference analysis.
- **Session-Level Aggregation**: Preserves user_id linkage for clustered SE computation.
- **QoE Metrics**: PlaybackMetrics aggregation, QoE-engagement Pearson correlation.
- **Notebook Export**: Databricks `.ipynb` generation from SQL templates.

## 6.3 Observability (v6.0 update)

M3 exposes 7 Prometheus metrics on a dedicated `:50059` metrics endpoint (PR #148). A Grafana dashboard with 6 M3-specific panels and alert rules is provisioned automatically (PR #149).

## 6.4 Phase 5 Planned (ADR-014, 015, 017, 021, 022, 023)

M3 gains the largest data infrastructure expansion in Phase 5. New Delta Lake tables and computation pipelines:

**New Delta Lake Tables**:
- `content_catalog` (ADR-014): Dimension table populated from the content management system ETL. Columns: `content_id`, `provider_id`, `provider_name`, `genre_primary`, `genres_all`, `content_type`, `catalog_tier`, `embedding_vector`. M3 joins this with exposures during provider-side metric computation. Freshness validation: most recent `updated_at` must be < 24 hours.
- `experiment_level_metrics` (ADR-014): Provider-side metrics aggregated per experiment per time window — catalog coverage, Gini coefficient, entropy, long-tail impression share. Bootstrap CIs computed for these (one observation per time window, not per user).
- `user_trajectories` (ADR-017): MDP trajectory data for offline RL estimation. Partitioned by `experiment_id`. Columns: `user_id`, `trajectory_step`, `state_features`, `action_id`, `reward`, `next_state_features`, `logging_probability`, `timestamp`. Constructed by joining exposures, metrics, and assignments along the time axis per user. Adds ~30 minutes to daily metric computation for ORL-enabled experiments.
- `quasi_experiment_panel` (ADR-023): Panel data view for synthetic control analysis — `unit_id × time_period` aggregated outcome metrics. Constructed from existing `metric_summaries` with unit-level grouping.

**New Computation Pipelines**:
- **Provider-side metrics** (ADR-014): Catalog-level (coverage, Gini, entropy, long-tail share) and user-level (genre entropy, discovery rate, provider diversity, intra-list distance). Requires exposure → content_catalog join. Estimated +15–30 minutes to daily metric computation.
- **MLRATE cross-fitting** (ADR-015 Phase 2): Trains LightGBM/XGBoost predicting primary metric from pre-experiment features. K-fold cross-fitting produces independent predictions stored as a new column in `metric_summaries`. Runs during STARTING phase, adds 5–15 minutes to STARTING→RUNNING transition.
- **Switchback block aggregation** (ADR-022): Aggregates user-level metrics to block-level outcomes for switchback experiments. Groups by `(block_index, cluster_id, variant_id)`.
- **Feedback loop contamination quantification** (ADR-021): Joins `model_retraining_events` with exposure data to compute training data contamination fractions per retraining event.

M3 remains in Go throughout Phase 5. All new computation is Spark SQL orchestration — Go templates SQL, submits to Spark, writes results to Delta Lake. No Rust dependency is introduced.


# 7. Module 4a: Statistical Analysis Engine (Agent-4, Rust)

## 7.1 Core Statistical Methods

All implemented in `experimentation-stats` crate, executed by M4a binary. No statistical computation in Go or TypeScript.

| Method | Implementation | Validation |
| --- | --- | --- |
| Welch's t-test | `ttest.rs` | R `t.test()` to 6 decimal places |
| SRM chi-squared | `srm.rs` | R `chisq.test()` to 6 decimal places |
| CUPED variance reduction | `cuped.rs` | Golden files, proptest: never increases variance |
| Bootstrap CI (BCa + percentile) | `bootstrap.rs` | Coverage validation: 93-97% on 1000 synthetic datasets (PR #140) |
| mSPRT confidence sequences | `msprt.rs` | — |
| GST O'Brien-Fleming | `gst.rs` | R gsDesign + scipy to 4 decimal places (PR #136) |
| GST Pocock | `gst.rs` | R gsDesign + scipy to 4 decimal places |
| BH-FDR correction | `correction.rs` | — |
| Bayesian (Beta-Binomial, Normal-Normal) | `bayesian.rs` | 4 golden files (PR #156) |
| IPW-adjusted (Hájek estimator) | `ipw.rs` | 3 golden files, sandwich variance (PR #156) |
| HC1 clustered standard errors | `clustered.rs` | 3 golden files (PR #156) |
| Novelty detection | `novelty.rs` | Gauss-Newton with LM damping, golden-file validated (PR #38) |
| Interference analysis | `interference.rs` | JSD, Jaccard, Gini, title spillover with BH correction (PR #38) |
| Interleaving analysis | `interleaving.rs` | Sign test, Bradley-Terry MM, position analysis (PR #38) |
| CATE heterogeneous effects | `cate.rs` | Cochran Q + BH-FDR, 28 tests (PR #145) |

## 7.2 Analysis Service Architecture (v6.0 update)

The analysis service (PR #107) wires all 5 RPCs with PostgreSQL caching via `AnalysisStore` (sqlx):

- `RunAnalysis` → compute and write-through to PostgreSQL
- `GetAnalysisResult` → cache-first read from PostgreSQL
- `GetInterleavingAnalysis`, `GetNoveltyAnalysis`, `GetInterferenceAnalysis` → write-through

Total: 36 integration tests for the service layer.

## 7.3 Phase 5 Planned (ADR-015, 017, 018, 020, 021, 022, 023)

M4a receives the largest expansion in statistical method families — seven new analysis capabilities in `experimentation-stats`:

| Method Family | ADR | New Modules | Key Algorithms |
| --- | --- | --- | --- |
| Anytime-Valid Regression Adjustment | 015 | `avlm.rs` | GROW/REGROW confidence sequences with OLS regression adjustment. O(1) incremental updates via 6 running sufficient statistics. Subsumes CUPED + mSPRT into unified framework. |
| ML-Assisted Variance Reduction | 015 P2 | `mlrate.rs` | Cross-fitted LightGBM/XGBoost control variates. M3 trains the model; M4a uses the predictions as covariate X in AVLM. |
| E-Value Computation | 018 | `evalue.rs` | GROW martingale e-values alongside p-values. E-BH for within-experiment correction under arbitrary dependence. Stored in `metric_results.e_value`. |
| MAD E-Processes | 018 P3 | `mad.rs` | Mixture Adaptive Design e-processes for bandit experiments. Uses uniformly-randomized subset of observations for valid sequential inference. |
| Offline RL Estimation | 017 | `orl.rs` | Phase 1: TC/JIVE de-biased surrogate calibration (cross-fold IV estimation). Phase 2: Doubly-robust MDP estimator — Q-function (XGBoost) + density ratio estimation from `user_trajectories`. |
| Synthetic Control | 023 | `synthetic_control.rs` | Four methods: classic SCM, augmented SCM (Ridge de-biased), synthetic DiD, CausalImpact (Bayesian structural time series). Placebo permutation inference. |
| Switchback Analysis | 022 | `switchback.rs` | HAC standard errors (Newey-West with Andrews automatic bandwidth). Randomization inference (exact or 10K-permutation Monte Carlo). Carryover diagnostic (lag-1 autocorrelation test). |
| Adaptive Sample Size | 020 | `adaptive_n.rs` | Conditional power computation. Promising-zone classification (favorable/promising/futile). Blinded pooled variance re-estimation. GST spending function re-allocation for extended experiments. |
| Feedback Loop Detection | 021 | `feedback_loop.rs` | Pre/post retraining effect comparison (paired t-test across retraining events). Bias-corrected treatment effect extrapolation. Training data contamination correlation. |

**Validation requirements**: Each new method family requires golden-file validation against reference implementations. AVLM validates against the `avlm` R package (michaellindon.r-universe.dev). Synthetic control validates against `augsynth` R package. E-values validate against the Ramdas/Wang monograph examples. Proptest invariants: AVLM confidence sequences never exclude the true parameter at stated alpha (coverage ≥ 1 - α); e-values are non-negative; SCM donor weights are non-negative and sum to 1.

**Interaction matrix** (methods that must be aware of each other):

| | AVLM | E-Values | Adaptive N | GST |
|---|---|---|---|---|
| **AVLM** | — | AVLM produces e-value variant via `e_value_avlm()` | Adaptive N uses AVLM regression-adjusted variance | AVLM at planned looks recovers GST power |
| **E-Values** | | — | E-values do not interact with adaptive N (anytime-valid by construction) | E-values partially supersede GST p-values |
| **Adaptive N** | | | — | Adaptive N re-spends remaining GST alpha after extension |
| **GST** | | | | — |


# 8. Module 4b: Bandit Policy Service (Agent-4, Rust)

## 8.1 Algorithms

| Algorithm | Implementation | Feature Flag |
| --- | --- | --- |
| Thompson Sampling | Beta posterior (binary), Normal posterior (continuous). **MC simulation (1000 draws) for IPW propensity** (v6.0). | Default |
| Linear UCB | Ridge regression with Sherman-Morrison rank-1 updates. **Frobenius norm regularization** (v6.0). | Default |
| Neural Contextual | 2-layer MLP with dropout. **Candle v0.8** (migrated from tch-rs in v6.0, PR #159). | `gpu` |
| Content Cold-Start | Auto-creates experiment, runs exploration for configurable window (default 7 days), exports affinity scores. | Default |

## 8.2 Crash-Only Recovery

RocksDB snapshots on every reward update. Recovery: load last snapshot + replay from Kafka consumer group's committed offset. Multi-experiment concurrent restore verified in integration tests. Snapshot pruning: last 10 per experiment.

## 8.3 Verified Performance

| Metric | Target | Achieved |
| --- | --- | --- |
| SelectArm p99 | < 15ms at 10K rps | ✅ |
| Crash recovery | < 10 seconds | ✅ (2200+ reward high-volume test) |
| Zero mutex contention | Verified via tokio-console | ✅ |

## 8.4 Phase 5 Planned (ADR-011, 012, 016, 018)

The LMAX policy core receives four major extensions, all running on the dedicated Thread 3:

**Multi-Objective Reward Composition (ADR-011)**: The policy core's reward update path extends from `reward = scalar` to `reward = compose(r_1, ..., r_k)` using three selectable strategies — weighted scalarization (Σ w_i × normalized(r_i)), epsilon-constraint (maximize primary subject to floors on secondaries via Lagrangian relaxation), or Tchebycheff (minimize max weighted deviation from ideal point for Pareto-optimality on non-convex frontiers). Per-metric running normalization (EMA mean/variance) persisted in RocksDB alongside posterior parameters. ~200 bytes additional state per metric per experiment.

**LP Constraint Post-Processing (ADR-012)**: A deterministic layer between raw arm probabilities **p** and final selection. Solves KL(q || p) minimization over a constraint polytope (per-arm floors/ceilings, general linear constraints). O(K log K) for simple constraints, <50μs for general linear via warm-started interior point. Population-level constraints enforced via running impression counts with EMA decay. The LP runs on the same LMAX thread — constraint state (running counts) updated atomically with policy state, zero synchronization. The *adjusted* probabilities **q** are logged as `assignment_probability` for IPW validity.

**Slate-Level Bandits (ADR-016)**: Two new algorithms join the policy core. Slot-wise factorized Thompson Sampling (default): sequential per-slot posterior sampling with shared context propagation across slots, O(L × K) inference for L slots and K candidates, sub-millisecond for typical SVOD (L=10, K=100). GeMS VAE slate generation (behind `gpu` feature flag): continuous latent space slate optimization via Candle. Policy state expands from `HashMap<ExperimentId, PolicyState>` to accommodate `Vec<HashMap<ItemId, ArmPosterior>>` per-slot posteriors. RocksDB snapshots include full slate policy state. Three reward attribution models: clicked-slot only, position-weighted, counterfactual leave-one-out.

**MAD Randomization Mixing (ADR-018 Phase 3)**: When `BanditConfig.mad_randomization_fraction > 0`, SelectArm mixes uniform randomization at rate ε with the bandit policy. The ε-fraction of uniformly randomized observations forms a valid basis for e-process computation in M4a, enabling anytime-valid inference from bandit experiments. The response includes a flag indicating whether the observation was from the bandit or the uniform component.

**Estimated LMAX core state growth**: For a typical Phase 5 experiment (multi-objective, LP constraints, 10-arm slate bandit with MAD), policy state per experiment grows from ~5KB to ~50KB. At 100 concurrent experiments, total RocksDB snapshot size grows from ~500KB to ~5MB — well within budget.


# 9. Module 5: Experiment Management Service (Agent-5, Go)

## 9.1 RBAC (v6.0 update)

ConnectRPC auth interceptor with 4-level role hierarchy (PR #71):

| Role | Level | Permissions |
| --- | --- | --- |
| Viewer | 0 | Read experiments, results, metrics, layers |
| Analyst | 1 | Viewer + create metric definitions, targeting rules |
| Experimenter | 2 | Analyst + create/edit experiments, lifecycle transitions |
| Admin | 3 | All permissions + archive, manage layers, manage users |

Audit trail records real actor identity from auth context.

## 9.2 Experiment Type Support (complete)

| Type | STARTING Validation | CONCLUDING Behavior |
| --- | --- | --- |
| A/B, Multivariate | Variant count >= 2, primary metric, fractions sum to 1.0 | M4a fixed-horizon or sequential analysis |
| INTERLEAVING | InterleavingConfig, 2+ algorithm_ids | M4a sign test + Bradley-Terry |
| SESSION_LEVEL | SessionConfig, session_id_attribute resolves | M4a clustered analysis (naive + HC1) |
| PLAYBACK_QOE | At least one QoE guardrail metric | M4a QoE + engagement cross-reference |
| CONTEXTUAL_BANDIT | BanditConfig, reward_metric, context features | M4a IPW-adjusted analysis |
| CUMULATIVE_HOLDOUT | Holdout 1-5%, no conclusion_date | No auto-conclusion; periodic lift report |

## 9.3 MetricType Filter (v6.0 update)

`ListMetricDefinitions` supports `type_filter` parameter (PR #157) for filtering by MEAN, PROPORTION, RATIO, COUNT, PERCENTILE, or CUSTOM.

## 9.4 Phase 5 Planned (ADR-013, 018, 019, 020, 021, 022, 023, 025)

M5 gains the most new responsibilities of any module in Phase 5:

**Three new experiment types** with dedicated STARTING validation:

| Type | STARTING Validation | CONCLUDING Behavior |
| --- | --- | --- |
| META (ADR-013) | `MetaExperimentConfig.base_algorithm` set; all variant `payload_json` valid; metric_ids resolve; reward weights sum to 1.0; primary metric ≠ reward metric (warns) | Cross-variant business outcome analysis in M4a |
| SWITCHBACK (ADR-022) | `planned_cycles >= 4`; `block_duration >= 1h`; cluster_attribute resolves (if clustered) | M4a switchback analysis (HAC + randomization inference) |
| QUASI (ADR-023) | Panel data exists for treated unit and donors; pre-treatment period sufficient | M4a synthetic control analysis |

**Portfolio-level management (ADR-019)**: New `/portfolio` data served to M6 — win rate, learning rate, annualized impact, traffic utilization, experiment throughput, power distribution. `ExperimentLearning` classification required at CONCLUDED→ARCHIVED transition. Optimal alpha recommendation during experiment creation. Traffic allocation optimizer across concurrent experiments. Decision rule evaluation as monthly batch job.

**Online FDR controller (ADR-018)**: Platform-level singleton `OnlineFdrController` persisted in PostgreSQL. On each experiment CONCLUDED transition, M5 submits the primary metric's e-value and receives a reject/don't-reject decision. Controller state (alpha wealth, rejection count, test history) checkpointed after every decision.

**Adaptive sample size triggers (ADR-020)**: M5 schedules interim analysis at `interim_fraction × planned_duration`. On trigger, requests conditional power from M4a. Zone classification: favorable (no action), promising (extends experiment duration, adjusts GST boundaries), futile (sends early-termination recommendation to owner). Audit trail records all recalculations.

**Feedback loop alerting (ADR-021)**: Consumes `ModelRetrainingEvent` from Kafka, correlates with running experiments, triggers M4a feedback loop analysis when retraining events overlap with active experiments. Surfaces mitigation recommendations to M6.

**Language migration (ADR-025)**: M5 is conditionally scheduled for Go→Rust port if ≥ 3 of {ADR-015 P2, ADR-018, ADR-019, ADR-020, ADR-021} are committed. The port eliminates M5→M4a RPCs for statistical computation (power analysis, e-value thresholding, conditional power) by directly importing `experimentation-stats` as a workspace crate dependency. See ADR-025 for trigger criteria and implementation plan.


# 10. Module 6: Decision Support UI (Agent-6, TypeScript)

## 10.1 Architecture

Next.js 14 + React 18 application with Tailwind CSS, shadcn/ui components, Recharts for standard charts, and D3 for custom visualizations. ConnectRPC client via `@connectrpc/connect-web`.

## 10.2 Complete Feature Set

| Feature | Phase | PR(s) | Tests |
| --- | --- | --- | --- |
| Experiment list with search, filter, sort | 1 | #30, #90 | ✅ |
| Experiment detail with state indicator | 1 | #30 | ✅ |
| Create/edit experiment form | 1 | #49 | ✅ |
| View SQL modal + notebook export | 1 | #30 | ✅ |
| Results dashboard (treatment effects, CI) | 2 | #56 | ✅ |
| Sequential testing boundary plots | 2 | #81 | ✅ |
| Bandit arm allocation dashboard | 3 | #60 | ✅ |
| Surrogate/holdout/guardrail visualizations | 3 | #76 | ✅ |
| CATE lifecycle segment forest plot | 3 | #80 | ✅ |
| QoE dashboard + novelty decay curve | 3 | #81 | ✅ |
| Lorenz curve + GST boundary | 3 | #81 | ✅ |
| Session-level analysis panel | 4 | #137 | ✅ |
| Metric definition browser (/metrics) | 4 | #154 | ✅ |
| Error boundary + chaos resilience | 4 | #143 | ✅ |
| Performance targets (< 1s dashboard) | 4 | #108 | ✅ |
| Proto-to-UI type alignment adapters | 4 | #147 | ✅ |
| RBAC-aware UI controls | 4 | ✅ | ✅ |

## 10.3 Performance Optimizations (v6.0)

- In-memory RPC cache with 30s TTL
- Code-split dynamic imports for 11 tab/chart components
- `React.memo` on 7 heavy components
- `prism-react-renderer` for SQL highlighting
- Web Worker base64 decode for notebook export

Total: 355 tests (317 after error boundary + 37 contract tests + 11 metric browser).

## 10.4 Phase 5 Planned UI (ADR-011–023)

M6 gains significant new pages and visualization components:

**New pages**:
- `/portfolio` (ADR-019): Experiment portfolio dashboard — win rate, learning rate (EwL), annualized impact, traffic utilization, experiment throughput, power distribution histogram, false discovery estimate, optimal alpha recommendation widget.
- `/portfolio/provider-health` (ADR-014): Provider-side metrics across all running experiments — time series of catalog coverage, provider Gini, long-tail impression share. Filterable by provider.

**New experiment results tabs**:
- **Provider metrics tab** (ADR-014): Provider-side treatment effects alongside user-side metrics. Experiment-level metrics show bootstrap CIs. Guardrail rendering identical to user-side.
- **Feedback loop analysis tab** (ADR-021): Retraining timeline on daily treatment effect plot, pre/post retraining comparison box plots, contamination fraction bar chart, bias-corrected estimate comparison, mitigation recommendation matrix. Shown only when `model_retraining_events` data available.
- **Switchback results tab** (ADR-022): Block timeline with alternating treatment/control bands, block-level outcome time series, ACF carryover diagnostic, randomization test distribution histogram.
- **Quasi-experiment results tab** (ADR-023): Treated vs. synthetic control time series with confidence band, pointwise effect plot, cumulative effect plot, donor weight table, placebo test small-multiple panel, pre-treatment fit RMSPE diagnostic.

**Enhanced existing views**:
- **Create experiment form**: Multi-objective reward configuration (ADR-011), LP constraint specification (ADR-012), slate bandit configuration (ADR-016), switchback parameters (ADR-022), quasi-experiment setup (ADR-023). Optimal alpha recommendation displayed during creation (ADR-019).
- **Results dashboard**: E-value column alongside p-values (ADR-018). AVLM confidence sequence boundary plot replacing separate mSPRT/CUPED views (ADR-015). Adaptive sample size zone indicator and extended timeline (ADR-020).
- **Bandit dashboard**: Multi-objective reward decomposition per arm (ADR-011). LP constraint adjustment visualization (ADR-012). Slate-level allocation heatmap (ADR-016). MAD randomization fraction indicator (ADR-018).
- **Meta-experiment results** (ADR-013): Objective comparison table, business outcome comparison, ecosystem health comparison, bandit efficiency per variant, Pareto frontier visualization for 3+ variants.


# 11. Module 7: Feature Flag Service (Agent-7, Go)

## 11.1 Complete Feature Set

| Feature | Description | Status |
| --- | --- | --- |
| Boolean/String/Numeric/JSON flags | Full CRUD via ConnectRPC | ✅ |
| CGo hash bridge | 10K vectors match, 280ns/call overhead | ✅ |
| Percentage rollout | Monotonic guarantee (no user eviction) | ✅ |
| Multi-variant flags | Traffic fraction-based allocation | ✅ |
| PromoteToExperiment | All experiment types supported | ✅ |
| Flag audit trail | Action history with actor identity | ✅ |
| Stale flag detection | SQL view: >90 days unchanged at 100% | ✅ |
| Experiment reconciler | Auto-resolve flag when experiment concludes | ✅ (PR #123) |

## 11.2 Verified Performance

| Metric | Target | Achieved |
| --- | --- | --- |
| EvaluateFlag p99 | < 10ms | ✅ at 20K rps |
| Bulk EvaluateFlags p99 | < 50ms | ✅ |
| CGo bridge overhead | < 1μs | ✅ (280ns) |

## 11.3 Phase 5 Planned: Rust Port (ADR-024)

M7 is unconditionally scheduled for Go→Rust port. The business case: the `experimentation-ffi` crate, cbindgen step, CGo cross-compilation CI, and 10K-vector parity validation exist solely because M7 is the only Go service that needs to call `experimentation-hash`. Porting M7 to Rust eliminates the FFI crate entirely — hash parity becomes guaranteed by construction (same source, same binary).

New `crates/experimentation-flags/` joins the Cargo workspace. tonic-web serves JSON HTTP matching the existing ConnectRPC wire format — no client changes required. PostgreSQL via sqlx. Kafka consumer (rdkafka) for the experiment reconciler.

Estimated: ~3 weeks. Deletes ~3,100 lines of Go/C/build config, produces ~2,000 lines of Rust. Expected p99 improvement from < 10ms to < 5ms due to eliminated CGo bridge overhead.

See ADR-024 for full implementation plan and validation strategy.


# 12. Cross-Cutting Concerns

## 12.1 Observability

- **Go**: OpenTelemetry Go SDK + connect-go interceptors for automatic RPC tracing and Prometheus metrics
- **Rust**: `tracing` crate + opentelemetry-rust + tonic interceptors. Spans include crate name.
- **Cross-language**: W3C Trace Context propagation through Connect/gRPC metadata
- **M3 Prometheus**: 7 metrics on dedicated `:50059` endpoint. Grafana dashboard with 6 panels + alert rules.
- **Monitoring stack**: Grafana + Prometheus + Jaeger via `docker-compose.monitoring.yml`

## 12.2 Infrastructure

| Component | Technology | Notes |
| --- | --- | --- |
| Go Services | connect-go on K8s | Management (:50055), Metrics (:50056), Flags (:50057), Orchestration (:50058) |
| Rust Services | tonic + tonic-web on K8s | Assignment (:50051), Pipeline (:50052), Analysis (:50053), Policy (:50054) |
| Schema | Buf CLI v2 + BSR | STANDARD lint category, WIRE_JSON breaking detection |
| Kafka | MSK/Confluent | 6 topics, 4-128 partitions |
| Lakehouse | Delta Lake on S3/GCS | QoE tables, surrogate artifacts |
| Surrogate Models | MLflow on S3 | Model registry, versioning, calibration tracking |
| Feature Store | Redis Cluster | User attributes, lifecycle segment, bandit context |
| Database | PostgreSQL 16 | Config, results, audit, query_log. See `sql/migrations/001_schema.sql` through `005_flag_resolved_at.sql`. |
| Policy Store | RocksDB (embedded) | Bandit policy snapshots — crash-only persistence |
| UI | Next.js 14 (:3000) | CDN-deployable, SSR for static assets |

## 12.3 Documentation Infrastructure (NEW)

A self-hosted DocMost documentation wiki (PR #153) provides 8 documentation spaces:

| Space | Content |
| --- | --- |
| General | Platform overview, contributing guide, development workflow |
| Architecture | Design doc v5.1, Mermaid diagrams, patterns |
| Modules | Module documentation (M1-M7) |
| Architecture Decision Records | All 10 ADRs |
| Agent Onboarding | Per-agent quickstart guides (Agent-0 through Agent-7) |
| Project Coordination | Status tracker, coordinator playbook, agent prompts |
| Integration Guide | SDK guides, API reference, event pipeline, feature flags, experiment types, deployment, security |
| User Experience Guide | Dashboard walkthrough, experiment workflows, tips |


# 13. Deployment Architecture (NEW)

A comprehensive deployment architecture guide is available at `docs/design/deployment_architecture.md`. Key points summarized here.

## 13.1 Service Inventory

| Module | Service | Language | Port | Protocol | Stateful? | Latency SLA |
| --- | --- | --- | --- | --- | --- | --- |
| M1 | Assignment | Rust | 50051 | gRPC + HTTP JSON | No | p99 < 5ms |
| M2 | Pipeline | Rust | 50052 | gRPC | No | p99 < 10ms |
| M2-Orch | Orchestration | Go | 50058 | ConnectRPC | No | N/A (batch) |
| M3 | Metrics | Go | 50056 | gRPC | No | N/A (batch) |
| M4a | Analysis | Rust | 50053 | gRPC | No | N/A (batch) |
| M4b | Policy | Rust | 50054 | gRPC | Yes (RocksDB) | p99 < 15ms |
| M5 | Management | Go | 50055 | ConnectRPC | No (uses PG) | p99 < 50ms |
| M6 | UI | TypeScript | 3000 | HTTP | No | N/A (frontend) |
| M7 | Flags | Go → Rust (ADR-024) | 50057 | gRPC | No | p99 < 10ms (< 5ms post-port) |

## 13.2 Deployment Topologies

| Topology | Best For | Monthly Cost (Medium Scale) |
| --- | --- | --- |
| **Fly.io** (all services) | Dev/staging, small-scale | ~$360 |
| **AWS** (ECS Fargate + EC2 for M4b) | Production at scale, compliance | ~$1,625 |
| **GCP** (Cloud Run + GKE for M4b) | Production, auto-scale-to-zero | ~$1,260 |
| **Hybrid** (Fly.io edge + AWS backend) | Global low-latency + managed backends | ~$800 |

M4b (Bandit Policy) is the only service requiring special deployment consideration due to its RocksDB persistent volume and LMAX single-threaded design. It cannot be horizontally scaled; scaling strategies include vertical scaling, reducing MC_SIMULATIONS, or sharding by experiment_id.

## 13.3 Scaling Strategy

| Service | Scaling Dimension | Strategy | Min | Max |
| --- | --- | --- | --- | --- |
| M1 (Assignment) | Requests/sec | Horizontal (stateless) | 2 | 20 |
| M2 (Pipeline) | Events/sec | Horizontal (stateless) | 2 | 10 |
| M3 (Metrics) | Experiments count | Vertical (Spark workers) | 1 | 5 |
| M4a (Analysis) | Experiments count | Horizontal (job queue) | 1 | 5 |
| M4b (Policy) | Cannot scale horizontally | Vertical only (LMAX) | 1 | 1 |
| M5 (Management) | API requests | Horizontal (stateless) | 1 | 5 |
| M6 (UI) | Page views | Horizontal (stateless) | 1 | 3 |
| M7 (Flags) | Evaluations/sec | Horizontal (stateless) | 2 | 10 |

## 13.4 Disaster Recovery

| Component | RPO | RTO | Strategy |
| --- | --- | --- | --- |
| M1, M2 (stateless) | 0 | < 10s | Restart |
| M4b (Policy) | Last RocksDB snapshot | < 60s | Load snapshot + Kafka replay |
| PostgreSQL | < 1 min (WAL) | < 5 min | Multi-AZ failover |
| Kafka | 0 (replication=3) | < 30s | Leader election |


# 14. Implementation Record

## 14.1 Phase 0: Schema & Toolchain (Week 1) — Complete

10 pre-seeded milestones: Proto schema (17 files), PostgreSQL DDL, Delta Lake tables, Kafka topics, Cargo workspace (13 crates with working MurmurHash3/t-test/SRM/Thompson/Team Draft), Go modules (4 service shells), Next.js scaffold, 10,000 hash test vectors, Docker Compose + CI/CD, SDK scaffolding (5 platforms).

## 14.2 Phase 1: Foundation (Weeks 2–7) — Complete (30 milestones)

All 30 milestones merged. Highlights:
- M1: Hash WASM + FFI (PR #4), GetAssignment (PR #11), config cache, targeting, session-level + layer-aware assignment
- M2: All 4 event types + Bloom filter + Go orchestration (PRs #1, #8)
- M3: MEAN/PROPORTION/COUNT/RATIO + CUPED + guardrails (PRs #3, #5, #9, #16)
- M4: t-test/SRM golden files + CUPED + mSPRT/GST + Thompson/LMAX/RocksDB (PRs #2, #14, #25)
- M5: CRUD + state machine + layers + bucket reuse + StreamConfigUpdates + guardrail consumer + metric CRUD (PRs #7, #10, #15, #18, #24)
- M6: Experiment list/detail + state indicator + View SQL (PR #30)
- M7: Boolean flag CRUD + CGo bridge + percentage rollout + PromoteToExperiment (PR #13)

## 14.3 Phase 2: Analysis & UI (Weeks 6–11) — Complete

- M4a: GST (implemented with mSPRT in PR #25), Bootstrap CI + BH-FDR (PR #29), Novelty/Interference/Interleaving analysis (PR #38)
- M1: GetInterleavedList (Team Draft + Optimized + Multileave)
- M3: Surrogate metric framework (PR #35), SVOD-specific metrics (PR #26)
- M6: Results dashboard, notebook export, analysis tabs (PRs #56, #76, #80, #81)

## 14.4 Phase 3: SVOD-Native + Bandits (Weeks 10–17) — Complete

- M4b: LinUCB contextual bandit (PR #54), content cold-start bandit (PR #62, gRPC wired PR #72)
- M6: Bandit dashboard (PR #60), CATE forest plot (PR #80), QoE/novelty/GST/Lorenz (PR #81)
- Session pipeline: Agent-2 (session_id keyed events), Agent-3 (session_level_mean.sql.tmpl), Agent-1 (allow_cross_session_variation)
- QoE pipeline: Agent-2 (QoE validation), Agent-3 (qoe_metric.sql.tmpl + correlation)
- M5: Cumulative holdout support (PR #57)

## 14.5 Phase 4: Advanced & Polish (Weeks 16–22) — Complete

- M4a: CATE heterogeneous treatment effects + Cochran Q (PR #145), analysis service all RPCs wired + PG caching (PR #107), Bayesian/IPW/clustered SEs (PR #156)
- M4b: Neural bandit migrated to Candle (PR #159), PGO-optimized builds (PR #133)
- M1: PGO build (PR #116), 50K rps load test (PR #138), SDK LocalProvider + RemoteProviders (PRs #144, #150)
- M5: RBAC (PR #71)
- M6: Performance targets (PR #108), live API integration (PR #130), error boundary (PR #143), metric browser (PR #154)
- M7: All-types promote + reconciler (PR #123), k6 load test (PR #129)
- Chaos: All agents contributed chaos scripts and recovery tests

## 14.6 Cross-Cutting Fixes (v6.0)

PR #161 (Devin AI + human review) addressed 12 bugs across 6 crates/services:
1. **Thompson MC propensity**: SelectArm now uses 1000-draw MC simulation for correct IPW probabilities
2. **Pause/Resume TOCTOU**: Concurrent lifecycle transitions serialized via state precondition
3. **Holdout fail-closed**: Failed holdout lookup blocks layer instead of leaking to treatment
4. **Negative nanos guard**: Timestamp validation rejects negative nanosecond values
5. **Config cache layer auto-registration**: Layers from StreamConfigUpdates auto-registered in cache
6. **Allocator overlap detection**: `ErrOverlappingRanges` sentinel on occupied range collision
7. **LinUCB Frobenius norm**: Regularization uses Frobenius norm for numerical stability
8. **Golden file significance check**: Statistical tests verify significance aligns with p-value

PR #162 addressed documentation/SDK cleanup: port mismatches in docs, iOS SDK dedup, Python SDK module drift, Rust API `#[doc(hidden)]`.


# 15. Agent Coordination Protocol

## 15.1 Multi-Agent Development Model

The platform is developed by 7 specialized AI agents, each owning specific modules. Agent ownership boundaries are documented in Multiclaude agent definitions (`.multiclaude/agents/agent-{N}-*.md`) and per-agent onboarding guides (`docs/coordination/prompts/agent-{N}-*.md`).

**Phases 0–4 (completed)** used a human-coordinated model:
- Per-agent status files (`docs/coordination/status/agent-N-status.md`) to eliminate merge conflicts
- Continuation prompt templates for advancing agents between milestones
- Coordinator playbook for the merge → resolve → advance cycle
- Branch naming: `agent-N/<type>/<description>` with conventional commits scoped by module/crate

**Phase 5 (current)** uses a hybrid autonomous model:
- **Multiclaude** for persistent sprint-level orchestration. Each agent runs as a Multiclaude worker with its own git worktree and tmux window. The supervisor daemon health-checks workers, routes messages, and refreshes worktrees every 2 minutes. A CI-gated merge queue auto-merges PRs when tests pass (multiplayer mode: human review required first). Workers self-destruct on task completion; supervisor spawns new workers for the next milestone.
- **Agent Teams** for ephemeral ad-hoc collaboration. Cross-agent contract test debugging, proto schema design sessions, and interactive PR review use Claude Code Agent Teams (2–4 teammates, peer-to-peer messaging, shared task list). Sessions are short-lived and do not replace the Multiclaude workflow.
- **Per-agent status files** continue at `docs/coordination/status/agent-N-status.md`. Each agent writes only their own status file and reads others for dependency tracking. Workers update status files in their worktree; the merge queue propagates to main.
- **Multiclaude agent definitions** at `.multiclaude/agents/` persist the agent's role, ADR responsibilities, coding standards, dependencies, and contract test obligations. These replace the Phase 1–4 continuation prompt templates.

See `docs/coordination/phase5-playbook.md` for the full operational guide and `docs/coordination/sprint-prompts.md` for pre-written worker create commands per sprint.

## 15.2 Pair Integration Results (Phases 1–4, all 10 green)

| Week | Pair | Tests | Notes |
| --- | --- | --- | --- |
| 3 | Agent-5 ↔ Agent-6 | 11 + 37 contract tests | camelCase, enum prefixes, RFC 3339, RBAC 403 |
| 3 | Agent-1 ↔ Agent-5 | 10 contract tests | Version monotonicity, hash_salt stability, holdout flag |
| 4 | Agent-2 ↔ Agent-3 | 40 contract tests | Delta schema alignment, SQL template field coverage |
| 4 | Agent-1 ↔ Agent-7 | CGo bridge parity | 10K vectors, `just test-flags-cgo` |
| 5 | Agent-3 ↔ Agent-4 | ~50 contract tests | All 4 output tables + ratio delta method components |
| 5 | Agent-5 ↔ Agent-3 | 3 schema + Kafka roundtrip | Field symmetry, bidirectional deser |
| 5 | Agent-2 ↔ Agent-4 | 24 integration tests | 17 protobuf contract + 7 Kafka roundtrip |
| 6 | Agent-3 ↔ Agent-5 | 22 tests, 49 subtests | Wire-format for all definition types |
| 6 | Agent-1 ↔ Agent-4 | 10 contract tests | Thompson roundtrip, LinUCB context, cold-start lifecycle |
| 6 | Agent-4 ↔ Agent-6 | 12 + 27 contract tests | 14-field MetricResult, SRM, lifecycle enums, adapters |

## 15.3 Phase 5 Contract Tests (planned)

| Sprint | Pair | Validates |
| --- | --- | --- |
| 5.1 | Agent-3 ↔ Agent-4 | MetricStakeholder, MetricAggregationLevel, experiment_level_metrics schema |
| 5.1 | Agent-7 (Rust) ↔ Agent-6 | All M7 RPCs produce identical JSON in Rust vs. Go |
| 5.2 | Agent-4 ↔ Agent-5 | M5 requests conditional power; M4a returns zone classification |
| 5.2 | Agent-4 ↔ Agent-6 | AvlmResult proto rendering in results dashboard |
| 5.3 | Agent-1 ↔ Agent-4 | SelectArm with LP constraints returns adjusted probabilities |
| 5.3 | Agent-1 ↔ Agent-5 | SWITCHBACK assignment mode matches M5 config |
| 5.4 | Agent-1 ↔ Agent-4 | GetSlateAssignment returns ordered slate with per-slot probabilities |
| 5.4 | Agent-5 ↔ Agent-6 | Portfolio dashboard renders M5's program metrics |
| 5.5 | Agent-4 ↔ Agent-5 | M5 submits e-value, receives FDR decision from OnlineFdrController |

## 15.4 Contract Testing Strategy

- **Proto schema**: `buf breaking` against main branch in CI
- **Wire-format**: JSON serialization/deserialization parity tests between producer and consumer
- **Proto3 zero-value**: Tests verify optional sub-messages omit correctly
- **Enum prefix stripping**: UI adapters strip `LIFECYCLE_SEGMENT_` and `SEQUENTIAL_METHOD_` prefixes
- **int64 coercion**: SRM counts and segment sample sizes handle proto3 int64-as-string in JSON
- **Phase 5 addition**: Consumer agent writes the contract test; producer agent's code must pass it. Cross-module PRs blocked by CI until contract tests pass.


# 16. Client SDK Architecture (NEW)

All SDKs share an identical assignment algorithm that matches the Rust service:

```
key = user_id + "\x00" + salt
hash = murmurhash3_x86_32(key, seed=0)
bucket = hash % total_buckets

if bucket < allocation_start or bucket > allocation_end:
    return null  // not in allocation

relative_bucket = bucket - allocation_start
alloc_size = allocation_end - allocation_start + 1
cumulative = 0.0
for variant in variants:
    cumulative += variant.traffic_fraction * alloc_size
    if relative_bucket < cumulative:
        return variant

return variants.last()  // FP rounding fallback
```

This algorithm is validated against the first 10 test vectors in every SDK's test suite:

| user_id | salt | total_buckets | expected_bucket |
| --- | --- | --- | --- |
| user_000000 | experiment_default_salt | 10000 | 3913 |
| user_000001 | experiment_default_salt | 10000 | 4234 |
| user_000002 | experiment_default_salt | 10000 | 5578 |
| user_000003 | experiment_default_salt | 10000 | 8009 |
| user_000004 | experiment_default_salt | 10000 | 2419 |
| ... | ... | ... | ... |

Go SDK test: `TestBucketParity`. Python SDK test: `test_bucket_parity`. Web SDK test: `bucket parity with test vectors`. iOS/Android: verified via LocalProvider integration tests.


# 17. Phase 5: Architecture Evolution (NEW)

Phase 5 is driven by a systematic gap analysis of 2024–2026 experimentation research across 50+ papers and 10+ industry platform reports (Netflix, Spotify, Meta, Etsy, LinkedIn, DoorDash, Amazon). Thirteen proposed ADRs (011–023) plus two language migration ADRs (024–025) define the next architectural evolution. This section provides the integrated view — how the ADRs compose, where they impact the codebase, what new infrastructure they require, and in what order they should be built.


## 17.1 Statistical Methodology Cluster (ADR-015, 018, 020)

**Problem**: Kaizen forces a choice between CUPED (variance reduction, fixed-horizon only) and mSPRT (sequential monitoring, no variance reduction). Cross-experiment FDR is uncontrolled. Sample sizes are fixed at experiment creation.

**Solution architecture**:

ADR-015 (AVLM) is the keystone. It implements Lindon et al.'s (Netflix/HBS, 2025) anytime-valid linear model in `experimentation-stats`, using O(1) incremental sufficient statistics (6 scalars) to produce regression-adjusted confidence sequences. This subsumes both CUPED and mSPRT into a single framework. Phase 2 extends to ML-assisted covariates (MLRATE) with cross-fitted LightGBM, trained by M3 during STARTING. Phase 3 adds in-experiment covariates (Etsy KDD 2024).

ADR-018 (E-Values) adds a parallel inference track. E-values are computed alongside p-values for every metric result and stored in `metric_results.e_value`. The `OnlineFdrController` in M5 consumes e-values at experiment conclusion to maintain platform-level FDR control via e-LOND. Phase 3 introduces MAD e-processes for valid sequential inference from bandit experiments, requiring M4b to mix uniform randomization at a configurable rate.

ADR-020 (Adaptive Sample Size) layers on top of AVLM and GST. At a configured interim point, M5 triggers M4a to compute conditional power using the AVLM regression-adjusted variance estimate. The promising-zone design classifies experiments into favorable/promising/futile zones, automatically extending promising experiments while flagging futile ones for early termination.

**Key interaction**: AVLM's regression-adjusted variance feeds into both ADR-020's conditional power computation and ADR-018's e-value confidence sequence width. The three ADRs form a coherent statistical stack where each layer depends on the one below.


## 17.2 Multi-Stakeholder Optimization Cluster (ADR-011, 012, 013, 014)

**Problem**: Bandits optimize a single scalar reward. SVOD recommendation is a three-sided market (subscribers, content providers, platform) with competing objectives. No provider-side metrics exist as first-class experiment measures.

**Solution architecture**:

ADR-014 (Provider Metrics) provides the measurement foundation. New Delta Lake tables (`content_catalog`, `experiment_level_metrics`) and new metric types (catalog coverage, provider Gini, genre entropy, discovery rate) become available as guardrails, secondary metrics, and bandit reward components. Guardrail beta-correction (Bonferroni on power side) replaces the incorrect BH-FDR application to guardrails.

ADR-011 (Multi-Objective Reward) extends the LMAX policy core to compose rewards from multiple stakeholder objectives. Three strategies: weighted scalarization, epsilon-constraint (Lagrangian), Tchebycheff (Pareto-aware). Running normalization (EMA mean/variance) handles cross-metric scale differences. All composition runs on the existing dedicated LMAX thread.

ADR-012 (LP Constraints) adds a deterministic post-processing layer between the bandit's raw arm probabilities and final selection. Solves KL-divergence minimization over a constraint polytope in <50μs. Population-level constraints (provider exposure guarantees) enforced via running impression counts with EMA decay. The *adjusted* probabilities are logged for IPW validity.

ADR-013 (Meta-Experiments) introduces `EXPERIMENT_TYPE_META` — randomizing users over different objective parameterizations while holding the algorithm constant. Isolated policy state per meta-variant on the LMAX thread. Two-level IPW in M4a. The primary metric must be a business outcome (retention, LTV), not one of the bandit's reward objectives.


## 17.3 Bandit & RL Advances Cluster (ADR-016, 017)

**Problem**: Bandits select single items; SVOD presents ordered slates with cross-item interactions. The surrogate metric framework assumes surrogacy — violated by construction for continual treatments like recommendation algorithm changes.

**Solution architecture**:

ADR-017 (Offline RL) has the highest urgency. Phase 1 (TC/JIVE) is a targeted fix to surrogate calibration — replacing biased R²-based calibration with Jackknife Instrumental Variables Estimation, eliminating correlated measurement error bias. This uses existing infrastructure (historical experiments, MLflow models) with minimal new code. Phase 2 introduces a full doubly-robust MDP estimator requiring the new `user_trajectories` Delta Lake table. M4a fits Q-functions (XGBoost) and density ratios to estimate long-term causal effects from short-duration experiments.

ADR-016 (Slate Bandits) extends M4b to optimize full recommendation slates. Slot-wise factorized Thompson Sampling (default) achieves sub-millisecond inference. GeMS VAE (behind `gpu` flag) enables holistic slate encoding. New `GetSlateAssignment` RPC on M1. LIPS off-policy evaluator in M4a enables offline slate policy evaluation before deployment. Three reward attribution models handle credit assignment across slate positions.


## 17.4 Quasi-Experimental Designs Cluster (ADR-022, 023)

**Problem**: Kaizen has no mechanism for interventions that cannot be user-level randomized — pricing changes, catalog launches, CDN routing, market-level rollouts.

**Solution architecture**:

ADR-022 (Switchback) adds temporal alternation designs for interference-prone treatments. Time-based assignment in M1 (replacing hash-based). HAC (Newey-West) standard errors in M4a for temporal autocorrelation. Randomization inference (permutation-based) for distribution-free validity. Adaptive block length selection after 2 cycles. Three design variants: simple alternating, regular balanced, randomized.

ADR-023 (Synthetic Control) adds quasi-experimental evaluation for non-randomizable interventions. Analysis-only — no assignment serving, no traffic allocation. Four methods: classic SCM, augmented SCM (default, Ridge de-biased), synthetic DiD (doubly robust), CausalImpact (Bayesian structural time series). Placebo permutation inference. Panel data constructed from existing `metric_summaries` with unit-level grouping.

These are the only ADRs that introduce entirely new experiment types to the lifecycle state machine.


## 17.5 Platform Operations Cluster (ADR-019, 021)

**Problem**: No portfolio-level optimization. Feedback loop interference from model retraining undetectable.

**Solution architecture**:

ADR-021 (Feedback Loop Detection) adds a new detection mechanism orthogonal to existing interference analysis (JSD, Jaccard, Gini). Ingests `ModelRetrainingEvent` from a new Kafka topic. Correlates daily treatment effect drift with retraining timestamps. Quantifies training data contamination. Provides bias-corrected treatment effect estimates and surfaces mitigation recommendations (data diversion, holdout retraining, freeze, bias correction, weighted training).

ADR-019 (Portfolio Optimization) adds program-level analytics. Experiment learning classification (EwL). Optimal alpha recommendation (often 0.10–0.20 for low-cost treatments). Annualized impact projection (3 methods). Traffic allocation optimizer for concurrent experiments. Decision rule evaluation via monthly data-split batch job. This is the highest-dependency ADR — requires ADR-017 (annualization method) and ADR-018 (FDR control) as inputs.


## 17.6 Language Migration Cluster (ADR-024, 025)

**Problem**: The `experimentation-ffi` crate, CGo build infrastructure, and 10K-vector parity validation exist solely because M7 (Go) needs to call one Rust hash function. As Phase 5 ADRs add statistical computation to M5's management decisions (e-value thresholding, conditional power, portfolio optimization), the Go→Rust RPC boundary becomes increasingly costly.

**Solution architecture**:

ADR-024 (M7 Rust Port) is unconditional. M7 is the simplest Go service (~2,500 lines). Porting it to Rust eliminates the FFI crate entirely — hash parity becomes guaranteed by construction (same source, same binary). The new `crates/experimentation-flags/` uses tonic-web for JSON HTTP mode (wire-format compatible with existing M6 and SDK consumers). Expected p99 improvement from <10ms to <5ms due to eliminated CGo bridge overhead. ~3 weeks effort, deletes ~3,100 lines of Go/C/build config.

ADR-025 (M5 Conditional Rust Port) triggers only when ≥ 3 of {ADR-015 P2, ADR-018, ADR-019, ADR-020, ADR-021} are committed. Below that threshold, M5→M4a RPCs for statistical computation are acceptable. Above it, M5's management decisions depend on statistical computations so frequently that the RPC boundary becomes architecturally significant. The port enables `experimentation-management` to import `experimentation-stats` directly — power analysis, e-value thresholding, and conditional power become ~1μs function calls instead of ~5ms RPC round-trips.

Post-migration, only M2-Orch and M3 remain in Go — both purely I/O-bound Spark/Kafka orchestration with zero Rust computation dependencies. The `experimentation-ffi` crate is deleted entirely.


## 17.7 Per-Module Impact Summary

| Module | Phase 5 ADR Impact | Estimated Effort |
| --- | --- | --- |
| M1 (Assignment) | GetSlateAssignment RPC (016), switchback time-based assignment (022), meta-experiment routing (013) | Medium |
| M2 (Pipeline) | `model_retraining_events` Kafka topic and ingestion (021) | Low |
| M3 (Metrics) | `content_catalog` + `experiment_level_metrics` + `user_trajectories` + `quasi_experiment_panel` tables; provider metrics, MLRATE, switchback aggregation, feedback loop contamination pipelines (014, 015, 017, 021, 022, 023) | **High** |
| M4a (Analysis) | 7 new method families: AVLM, e-values, SCM, switchback HAC, ORL, adaptive N, feedback loops (015, 017, 018, 020, 021, 022, 023) | **Very High** |
| M4b (Bandit) | Multi-objective reward, LP constraints, slate bandits, MAD mixing (011, 012, 016, 018) | **High** |
| M5 (Management) | 3 new experiment types, portfolio dashboard, FDR controller, adaptive N, learning classification, feedback loop alerting (013, 018, 019, 020, 021, 022, 023). Conditional Rust port (025). | **Very High** |
| M6 (UI) | Portfolio page, provider health page, 4 new results tabs, enhanced create form, meta-experiment results, Pareto visualization (all ADRs) | **High** |
| M7 (Flags) | Rust port only (024) | Medium |

Figure 9: Phase 5 Per-Module Impact Heatmap
See accompanying file: `docs/design/phase5_impact.mermaid`


## 17.8 New Data Infrastructure

Phase 5 introduces 4 new Delta Lake tables and 1 new Kafka topic:

| Asset | Type | ADR | Owner | Estimated Size |
| --- | --- | --- | --- | --- |
| `content_catalog` | Delta Lake dimension table | 014 | External ETL → M3 reads | ~100K rows (SVOD catalog) |
| `experiment_level_metrics` | Delta Lake fact table | 014 | M3 writes | ~1K rows/day |
| `user_trajectories` | Delta Lake fact table | 017 | M3 writes | ~100M rows/experiment (ORL-enabled only) |
| `quasi_experiment_panel` | Delta Lake view | 023 | M3 materializes | ~10K rows/quasi-experiment |
| `model_retraining_events` | Kafka topic (8 partitions) | 021 | External ML pipeline publishes; M3 consumes | ~1-7 events/week |

PostgreSQL schema additions: `metric_results.e_value`, `metric_results.log_e_value` (ADR-018); `experiments.learning_classification`, `experiments.learning_notes` (ADR-019); `online_fdr_controller_state` table (ADR-018); `adaptive_sample_size_audit` table (ADR-020).

New `experimentation-stats` module count: 9 new `.rs` files (`avlm.rs`, `mlrate.rs`, `evalue.rs`, `mad.rs`, `orl.rs`, `synthetic_control.rs`, `switchback.rs`, `adaptive_n.rs`, `feedback_loop.rs`).

New `experimentation-bandit` additions: multi-objective reward composition, LP constraint solver, slate policy state, MAD mixing — all on the LMAX core thread.


## 17.9 Implementation Sequence

Recommended priority order considering dependencies, ROI, and risk:

**P0 — Immediate (highest ROI, standalone or corrective)**:
- ADR-015 Phase 1 (AVLM): Unifies CUPED + mSPRT. #1 ROI item. No dependencies beyond existing ADR-004.
- ADR-017 Phase 1 (TC/JIVE): Fixes theoretical error in production surrogate calibration. Standalone.
- ADR-024 (M7 Rust port): Eliminates FFI crate. Standalone. ~3 weeks.

**P1 — Near-term (foundations for later clusters)**:
- ADR-014 (Provider Metrics): Measurement foundation for Cluster A. No dependencies.
- ADR-018 Phase 1 (E-Values): Parallel e-value computation alongside p-values. No breaking changes.
- ADR-021 (Feedback Loops): Standalone detection capability. Immediate diagnostic value.

**P2 — Mid-term (builds on P0/P1)**:
- ADR-011 (Multi-Objective Reward): Requires ADR-002 (existing). Enables ADR-012.
- ADR-020 (Adaptive Sample Size): Requires ADR-015 (AVLM variance). Reduces wasted traffic.
- ADR-023 (Synthetic Control): Analysis-only; no assignment changes. Standalone.
- ADR-018 Phase 2 (e-LOND FDR controller in M5): Requires Phase 1 e-values.

**P3 — Later (higher dependency, more complex)**:
- ADR-012 (LP Constraints): Requires ADR-011 + ADR-014.
- ADR-022 (Switchback): New experiment type; significant M1 assignment changes.
- ADR-016 (Slate Bandits): Large scope; extends M4b LMAX core with new policy state structure.
- ADR-025 (M5 Rust port): Trigger: ≥ 3 of {015 P2, 018, 019, 020, 021} committed.

**P4 — Strategic (highest dependency count)**:
- ADR-013 (Meta-Experiments): Requires ADR-011 + ADR-012.
- ADR-019 (Portfolio): Requires ADR-017 (annualization) + ADR-018 (FDR). Needs 6+ months of historical experiment data.
- ADR-017 Phase 2 (Full ORL/MDP): Most complex; requires `user_trajectories` table.
- ADR-018 Phase 3 (MAD): Requires M4b changes for bandit randomization mixing.
- ADR-015 Phases 2–3 (MLRATE, in-experiment covariates): Requires M3 model training infrastructure.

Figure 8: Phase 5 ADR Dependency Graph
See accompanying file: `docs/design/phase5_dependencies.mermaid`


# 18. Appendix

## 18.1 References

**Foundational (Phases 0–4)**:
- NautilusTrader (nautilustrader.io): Crate layering, crash-only, fail-fast, LMAX threading
- GrowthBook (growthbook.io): SDK abstraction, SQL transparency
- Spotify Confidence (confidence.spotify.com): mSPRT + GST, bucket reuse, guardrails
- Candle (github.com/huggingface/candle): Pure-Rust ML framework (replaced tch-rs for neural bandit)
- Chapelle et al. (2012): Interleaved Search Evaluation
- Radlinski & Craswell (2010): Optimized Interleaving
- Schuth et al. (2015): Multileave Comparisons
- Athey & Imbens (2016): Surrogate metrics in experimentation
- Li et al. (2010): LinUCB. Agrawal & Goyal (2013): Thompson Sampling.
- Tang et al. (2010): Google Overlapping Experiment Infrastructure
- Martin Fowler: LMAX Architecture
- Candea & Fox (2003): Crash-Only Software (USENIX HotOS)

**Phase 5 (ADR-011 through ADR-025)**:
- Lindon, Ham, Tingley, Bojinov: Anytime-Valid Linear Models (Netflix/HBS, 2025) — ADR-015
- Bibaut, Kallus, Lindon: Delayed-start normal-mixture SPRT guarantees (Netflix, 2024) — ADR-015
- Guo et al.: MLRATE (Meta, NeurIPS 2021) — ADR-015
- Etsy CUPAC: LightGBM control variates (December 2025) — ADR-015
- Tran, Bibaut, Kallus: Offline RL for long-term causal effects (Netflix, ICML 2024) — ADR-017
- Bibaut, Chou, Ejdemyr, Kallus: TC/JIVE proxy metric calibration (Netflix, KDD 2024) — ADR-017
- Kallus & Mao: Relaxed surrogacy bounds (JRSS-B, 2025) — ADR-017
- Ramdas & Wang: Hypothesis Testing with E-values (FnTnS, 2025) — ADR-018
- Xu & Ramdas: e-LOND online FDR (AISTATS 2024) — ADR-018
- Liang & Bojinov: MAD for bandit inference (HBS, 2024) — ADR-018
- Kiyohara, Nomura, Saito: LIPS slate OPE (WWW 2024) — ADR-016
- Goyal et al.: Slot-wise factorized slate bandits (UAI 2025) — ADR-016
- Deffayet et al.: GeMS VAE slates (WSDM 2023) — ADR-016
- Netflix incrementality bandits (Data Council 2025) — ADR-016
- Qassimi et al.: MOC-MAB (Scientific Reports, 2025) — ADR-011
- LinkedIn BanditLP (KDD 2025) — ADR-012
- Chen et al.: Interpolating fairness (NeurIPS 2024) — ADR-012
- Spotify calibrated bandits (RecSys 2025) — ADR-011, 014
- Netflix EC '25: Optimizing experimentation program returns — ADR-019
- Netflix KDD '25: Decision rule evaluation (Best Paper) — ADR-019
- Spotify EwL framework (September 2025) — ADR-019
- Schultzberg et al.: Risk-aware guardrail decisions (JSPI, 2024) — ADR-014
- Mehta & Pocock: Promising-zone adaptive designs (SiM, 2011) — ADR-020
- Bojinov, Simchi-Levi, Zhao: Switchback experiments (Management Science, 2023) — ADR-022
- Ben-Michael, Feller, Rothstein: Augmented Synthetic Control (JASA, 2021) — ADR-023
- Arkhangelsky et al.: Synthetic DiD (AER, 2021) — ADR-023
- Brodersen et al.: CausalImpact (Google, AoAS, 2015) — ADR-023
- arXiv 2310.17496v4: Feedback loop interference in A/B tests (2024) — ADR-021
- Brennan et al.: Symbiosis bias (WWW 2025) — ADR-021
- Jannach & Abdollahpouri: Multi-stakeholder RecSys survey (Frontiers, 2023) — ADR-011, 014

## 18.2 Glossary

| Term | Definition |
| --- | --- |
| Crash-Only Design | Recovery and startup share one code path. No separate graceful-shutdown persistence. |
| Fail-Fast | Panic on invalid data (NaN, overflow) rather than silent propagation. `assert_finite!()` macro. |
| LMAX Disruptor | Single-threaded event loop for core state; async I/O on separate threads via channels. |
| Provider Abstraction | SDK interface with pluggable backends (Remote, Local, Mock) and fallback chain. |
| Bucket Reuse | Recycling hash-space from concluded experiments after cooldown. Overlap detection via `ErrOverlappingRanges`. |
| Auto-Pause Guardrail | Default behavior: experiment paused on guardrail breach. Override requires explicit config and audit. |
| Group Sequential Test | Pre-specified analysis schedule with alpha spending (O'Brien-Fleming or Pocock). |
| proptest | Rust property-based testing. 10K cases in nightly CI. |
| Interleaving | Mixing items from 2+ algorithms in one list; 10-100x more sensitive than A/B. |
| Team Draft | Interleaving method: algorithms alternate picks like sports captains. |
| Surrogate Metric | Short-term metric validated to predict long-term outcomes via MLflow model. |
| Novelty Effect | Transient engagement spike that decays to steady-state. Detected via Gauss-Newton + LM damping. |
| Content Interference | Spillover from treatment redirecting consumption. Measured via JSD + Jaccard + Gini. |
| Lifecycle Segment | User class by subscription maturity: TRIAL, NEW, ESTABLISHED, MATURE, AT_RISK, WINBACK. |
| Clustered SE | HC1 sandwich estimator for correlated sessions within users. |
| QoE | Quality of Experience: TTFF, rebuffer, bitrate, resolution switches. |
| CUPED | Variance reduction via pre-experiment covariates (theta-hat estimator). |
| mSPRT | Sequential testing with always-valid p-values and arbitrary peeking. |
| CATE | Conditional Average Treatment Effect — heterogeneous effects across lifecycle segments. |
| IPW | Inverse Propensity Weighting (Hájek estimator) for bandit experiments with adaptive allocation. |
| MC Propensity | Monte Carlo simulation (1000 draws) for computing Thompson Sampling assignment probabilities. |
| Candle | Pure-Rust ML framework from Hugging Face, used for neural contextual bandit (replaced tch-rs). |
| AVLM | Anytime-Valid Linear Model — confidence sequences with regression adjustment (Phase 5, ADR-015). |
| E-value | Measure of evidence against the null; can be safely multiplied across experiments (Phase 5, ADR-018). |
| e-LOND | Online FDR control via e-values under arbitrary dependence (Phase 5, ADR-018). |
| MAD | Mixture Adaptive Design — mixing bandit with uniform randomization for valid inference (Phase 5, ADR-018). |
| MLRATE | ML-assisted variance reduction using cross-fitted model predictions as control variates (Phase 5, ADR-015). |
| TC/JIVE | Treatment-effect Correlation / Jackknife IV Estimation — de-biased surrogate calibration (Phase 5, ADR-017). |
| ORL | Offline Reinforcement Learning — MDP-based long-term causal effect estimation (Phase 5, ADR-017). |
| LIPS | Latent IPS — off-policy evaluation for slate recommendations in abstraction space (Phase 5, ADR-016). |
| LP Post-Processing | Linear program adjusting bandit arm probabilities to satisfy hard constraints (Phase 5, ADR-012). |
| Meta-Experiment | Experiment randomizing over objective function parameterizations (Phase 5, ADR-013). |
| Switchback | Temporal alternation design — platform alternates treatment/control over time blocks (Phase 5, ADR-022). |
| Synthetic Control | Counterfactual estimation from weighted donor units for non-randomizable interventions (Phase 5, ADR-023). |
| EwL | Experiments with Learning — classification of experiment outcomes as learning or no-learning (Phase 5, ADR-019). |
| Promising Zone | Conditional power region (30–90%) where adaptive sample size extension is warranted (Phase 5, ADR-020). |

## 18.3 Changelog from v6.0

| Section | Change | Rationale |
| --- | --- | --- |
| Header | Version 6.0 → 7.0; subtitle adds "Phase 5 Architecture Plan" | Document now includes forward-looking architecture |
| 1 | Executive summary updated with Phase 5 context | 15 proposed ADRs across 6 clusters; coordination model updated |
| 1.3 | 11 planned capabilities added to SVOD-Specific Capabilities table | ADR-011 through ADR-023 |
| 1.4 | M7 language updated: Go → "Go → Rust (ADR-024)" | Reflects unconditional Rust port |
| 1.5 | Phase 5 row added to implementation status table | Proposed status, 15 ADRs |
| 1.6 | NEW: Phase 5 Overview section | 6 cluster summary table (including Cluster F: Language Migration), #1 ROI item, highest-urgency item |
| 3.6 | NEW: Phase 5 Proto Extensions section | All new experiment types, bandit extensions, metric extensions, analysis extensions, management extensions, new RPCs |
| 4.7 | NEW: M1 Phase 5 Planned | Slate assignment, switchback, meta-experiment routing |
| 5.2 | `model_retraining_events` Kafka topic added | ADR-021 feedback loop detection |
| 6.4 | NEW: M3 Phase 5 Planned | 4 new Delta Lake tables, 4 new computation pipelines |
| 7.3 | NEW: M4a Phase 5 Planned | 9 new statistical modules, validation requirements, interaction matrix |
| 8.4 | NEW: M4b Phase 5 Planned | Multi-objective reward, LP constraints, slate bandits, MAD mixing; LMAX core state growth estimate |
| 9.4 | NEW: M5 Phase 5 Planned | 3 new experiment types, portfolio, FDR controller, adaptive N, learning classification, conditional Rust port (ADR-025) |
| 10.4 | NEW: M6 Phase 5 Planned UI | 2 new pages, 4 new results tabs, enhanced existing views |
| 11.3 | NEW: M7 Phase 5 Planned (Rust port) | ADR-024 unconditional port; eliminates experimentation-ffi crate |
| 13.1 | M7 service inventory updated | Language: Go → Rust (ADR-024); latency target: < 5ms post-port |
| 15.1 | Coordination model rewritten for Phase 5 | Hybrid Multiclaude + Agent Teams; per-agent worktree isolation; CI-gated merge queue; Multiclaude agent definitions replace continuation prompts |
| 15.3 | NEW: Phase 5 Contract Tests table | 9 planned cross-agent contract test pairs mapped to sprints |
| 15.4 | Contract testing strategy expanded | Phase 5 addition: consumer writes test, CI blocks incompatible merges |
| 17 | NEW: Phase 5 Architecture Evolution (entire section) | 6 cluster architectures (including 17.6 Language Migration), per-module impact summary, new data infrastructure, implementation sequence |
| 17.6 | NEW: Language Migration Cluster | ADR-024/025 architecture, post-migration language surface, FFI elimination |
| 18.1 | References expanded with Phase 5 sources | 30+ new academic and industry references |
| 18.2 | Glossary expanded with Phase 5 terms | 14 new terms |
| 18.3 | Changelog comprehensive for v6.0 → v7.0 | This table |

---

*End of document. Version 7.0 — March 2026.*
