# Experimentation Platform — Coordination Status

> **Last updated**: 2026-03-11 by Agent-6 (Live API integration prep — port fix, pause/resume RPCs, proto wire-format contract tests)
>
> This file is the single source of truth for multi-agent execution state.
> Update it each time a milestone merges to `main` or a blocker is identified.

## Active Phase

**Phase 1: Foundation (Weeks 2–7)** — **complete**. All 30 milestones merged.

## Agent Status

| Agent | Module | Status | Current Branch | Current Milestone | Blocked By | Notes |
|-------|--------|--------|----------------|-------------------|------------|-------|
| Agent-1 | M1 Assignment | 🔵 Phase 4 In Progress | agent-1/perf/loadtest-10k-rps | p99 < 5ms at 10K rps load test | — | M1.1–1.5 + M2.7 + M2.7b + M2.7c complete. Live bandit delegation done. Cold-start bandit done. PGO build pipeline (PR #116). k6 gRPC load test: 10K rps sustained (85% GetAssignment, 10% GetAssignments, 5% GetInterleavedList), SLA validation (p99 < 5ms assign, < 15ms interleave). `just loadtest-assignment` recipe. 95 tests. |
| Agent-2 | M2 Pipeline | 🟢 All Phases Complete | agent-2/feat/e2e-pipeline-tests | M2→M3 event contract tests | — | All phases merged (PRs #1, #8, #23, #40, #48, #59, #66, #78, #85, #99). M2→M3 contract tests: 40 tests (32 protobuf contract + 8 Kafka roundtrip) validating ExposureEvent, MetricEvent, QoEEvent data paths. Delta schema alignment, M3 SQL template field coverage, cross-topic user correlation. 119 tests pass. |
| Agent-3 | M3 Metrics | 🟢 Phase 4 Complete | agent-3/feat/latency-sla-validation | Latency SLA validation tests | — | Phase 1–3 done. Kafka publisher (PR #64). M3↔M5 contracts (PR #68). Chaos tests (PR #69). Coverage improvements (PR #77, #98). E2e pipeline tests (PR #79). Spark retry with exponential backoff (PR #86). Databricks notebook export (PR #87). CUSTOM metric (PR #91). PERCENTILE metric (PR #92). SQL template validation (PR #95). Go benchmarks (PR #101). Surrogate recalibration trigger job (PR #105). Kafka-driven recalibration consumer: reads `surrogate_recalibration_requests` from M5, triggers RecalibrationJob per experiment. **Latency SLA validation**: 7 tests — daily pipeline 42 queries (< 80 budget), guardrail 2 queries (< 20 budget), per-experiment breakdown, wall-clock timing, per-metric-type formula, linear scaling. All Phase 4 onboarding items complete. |
| Agent-4 | M4a Analysis + M4b Bandit | 🔵 Phase 4 In Progress | agent-4/feat/wire-remaining-analysis-rpcs | PostgreSQL caching for analysis results | — | M1.14–1.19 merged. M2.1–2.6, M2.10 complete. M3.1 LinUCB merged (PR #54). M3.2 cold-start merged. M4.1 CATE in PR #70. M4.2 analysis service (PR #93, #107). Chaos testing merged. **All 5 analysis RPCs wired** + **PostgreSQL caching**: RunAnalysis (always compute + cache write), GetAnalysisResult (cache-first + Delta Lake fallback), novelty/interference (compute + cache write). AnalysisStore with sqlx. 36 tests (31 active + 5 PG-gated). |
| Agent-5 | M5 Management | 🔵 Phase 4 In Progress | agent-5/feat/chaos-test-management | Chaos test script (4.5) | — | Phase 3 complete (M3.6 PR #57). M4.4 RBAC interceptor (PR #71). Phase 4: stress tests (PR #75). Guardrail override audit (PR #83). Type-specific conclude + QoE validation (PR #89). Chaos test script: crash recovery, state integrity, lifecycle verification (PR #96). |
| Agent-6 | M6 UI | 🔵 Phase 4 In Progress | agent-6/feat/phase4-next | Live API integration prep | — | M1.25–1.27, M2.8–2.9, analysis tabs (PR #56), bandit dashboard (PR #60), live API integration. Phase 3 complete: surrogate/holdout/guardrail (PR #76), CATE lifecycle (PR #80), QoE/novelty/GST/Lorenz (PR #81). Phase 4: search/filter/sort (PR #90). RBAC UI: auth context, role-based button disabling. Performance: in-memory RPC cache, code-split dynamic imports, React.memo, SQL syntax highlighting, Web Worker notebook export. Layer allocation bucket chart. WCAG 2.1 AA accessibility fixes. Live API integration: fixed metrics/bandit port swap, added PauseExperiment/ResumeExperiment RPCs, server-side ListExperiments filters, CATE/GST enum prefix stripping, 37 proto wire-format contract tests. 281 tests pass. |
| Agent-7 | M7 Flags | 🔵 In Progress | agent-7/feat/rbac-integration | Live M5 PromoteToExperiment wiring | — | M1.28–1.30 merged (PR #13). Phases 1–4.5 complete. Phase 4.4: RBAC interceptor. Live M5 wiring: layer_id, auth header forwarding, 10s client timeout, mock contract validation. |

**Legend**: 🟢 Complete | 🔵 In Progress | 🟡 Not Started (unblocked) | ⚪ Waiting (blocked) | 🔴 Blocked (critical path)

## Milestone Tracker

### Phase 0: Schema & Toolchain ✅

| # | Milestone | Owner | Status | PR | Merged | Notes |
|---|-----------|-------|--------|-----|--------|-------|
| P0.1 | Proto schema (17 files + buf config) | Shared | ✅ Done | — | Pre-seeded | In repo at `proto/` |
| P0.2 | PostgreSQL DDL | Shared | ✅ Done | — | Pre-seeded | `sql/migrations/001_schema.sql` |
| P0.3 | Delta Lake tables | Shared | ✅ Done | — | Pre-seeded | `delta/delta_lake_tables.sql` |
| P0.4 | Kafka topic configs | Shared | ✅ Done | — | Pre-seeded | `kafka/topic_configs.sh` |
| P0.5 | Cargo workspace (13 crates) | Shared | ✅ Done | — | Pre-seeded | Working MurmurHash3, t-test, SRM, Thompson, Team Draft |
| P0.6 | Go modules (4 services) | Shared | ✅ Done | — | Pre-seeded | Shells with health endpoints |
| P0.7 | Next.js UI scaffold | Shared | ✅ Done | — | Pre-seeded | ConnectRPC client configured |
| P0.8 | 10,000 hash test vectors | Shared | ✅ Done | — | Pre-seeded | Python reference + Rust validation |
| P0.9 | Docker Compose + CI/CD | Shared | ✅ Done | — | Pre-seeded | 3 workflow files, monitoring stack |
| P0.10 | SDK scaffolding (5 platforms) | Shared | ✅ Done | — | Pre-seeded | Provider abstraction in all SDKs |

### Phase 1: Foundation (Weeks 2–7)

| # | Milestone | Owner | Status | PR | Merged | Unblocks |
|---|-----------|-------|--------|-----|--------|----------|
| **1.1** | **Hash crate: WASM + FFI bindings** | Agent-1 | 🟢 | PR #4 | 2026-03-04 | Agent-7 (CGo bridge), SDKs |
| 1.2 | GetAssignment RPC (static bucketing) | Agent-1 | 🟢 | PR #11 | 2026-03-05 | SDKs, Agent-6 (debug view) |
| 1.3 | Config cache (subscribe to M5 StreamConfigUpdates) | Agent-1 | 🟢 | — | 2026-03-05 | Live config cache with watch channel. |
| 1.4 | Targeting rule evaluation | Agent-1 | 🟢 | PR #21 | 2026-03-05 | Targeting rule evaluation merged. |
| 1.5 | Layer-aware + session-level assignment | Agent-1 | 🟢 | — | — | Session-level bucketing + layer exclusivity tests. |
| **1.6** | **IngestExposure + IngestMetricEvent RPCs** | Agent-2 | 🟢 | PR #1 | 2026-03-04 | Agent-3 (events to compute) |
| 1.7 | IngestRewardEvent + IngestQoEEvent RPCs | Agent-2 | 🟢 | PR #1 | 2026-03-04 | Agent-4 M4b (rewards) |
| 1.8 | Bloom filter dedup (0.1% FPR at 100M/day) | Agent-2 | 🟢 | PR #1 | 2026-03-04 | Rotating filter + Prometheus metrics |
| 1.9 | Go orchestration + SQL query logging | Agent-2 | 🟢 | PR #8 | 2026-03-05 | Agent-3 (query log visibility) |
| **1.10** | **Standard metric computation (MEAN, PROPORTION, COUNT)** | Agent-3 | 🟢 | PR #3 | 2026-03-04 | Agent-4 M4a |
| 1.11 | RATIO metric with delta method inputs | Agent-3 | 🟢 | PR #5 | 2026-03-04 | Delta method inputs for M4a |
| 1.12 | CUPED covariate computation | Agent-3 | 🟢 | PR #9 | 2026-03-05 | Agent-4 M4a (CUPED covariates) |
| 1.13 | Guardrail breach detection → guardrail_alerts topic | Agent-3 | 🟢 | PR #16 | 2026-03-05 | Agent-5 (auto-pause). Breach tracker + Kafka publisher stub. |
| **1.14** | **Welch t-test + SRM check (golden-file validated)** | Agent-4 | 🟢 | PR #2 | 2026-03-05 | Agent-6 (results page) |
| 1.15 | CUPED variance reduction | Agent-4 | 🟢 | PR #14 | 2026-03-05 | Golden-file validated against R. |
| 1.16 | mSPRT sequential testing | Agent-4 | 🟢 | PR #25 | 2026-03-05 | mSPRT + GST sequential testing. |
| 1.17 | Thompson Sampling with Beta-Bernoulli (M4b) | Agent-4 | 🟢 | PR #2 | 2026-03-05 | Agent-1 (SelectArm) |
| 1.18 | LMAX single-threaded policy core (M4b) | Agent-4 | 🟢 | PR #2 | 2026-03-05 | Bandit policy serving |
| 1.19 | RocksDB policy snapshots (M4b) | Agent-4 | 🟢 | PR #2 | 2026-03-05 | Crash recovery |
| **1.20** | **Experiment CRUD + state machine enforcement** | Agent-5 | 🟢 | — | 2026-03-04 | Agent-6 (list/detail), Agent-1 (configs), Agent-3 (experiment list) |
| 1.21 | Layer allocation + bucket reuse | Agent-5 | 🟢 | PR #7, #10 | 2026-03-05 | ADR-009 bucket reuse with cooldown |
| 1.22 | StreamConfigUpdates RPC | Agent-5 | 🟢 | PR #15 | 2026-03-05 | Agent-1 (real-time config cache) |
| 1.23 | Guardrail alert consumer → auto-pause | Agent-5 | 🟢 | PR #18 | 2026-03-05 | ADR-008 auto-pause. Kafka consumer + processor. |
| 1.24 | Metric definition CRUD | Agent-5 | 🟢 | PR #24 | 2026-03-05 | Agent-3 (metric configs) |
| **1.25** | **Experiment list + detail shell (MSW mocked)** | Agent-6 | 🟢 | PR #30 | 2026-03-06 | Stakeholder demo. |
| 1.26 | State indicator component (color-coded lifecycle) | Agent-6 | 🟢 | PR #30 | 2026-03-06 | Color-coded state badges for all 6 states. |
| 1.27 | View SQL page (query log from M3) | Agent-6 | 🟢 | PR #30 | 2026-03-06 | Query log table + notebook export. |
| **1.28** | **Boolean flag CRUD + CGo hash bridge** | Agent-7 | 🟢 | PR #13 | 2026-03-05 | CRUD + EvaluateFlag + CGo bridge. 10K hash vectors match. |
| 1.29 | Percentage rollout (monotonic) | Agent-7 | 🟢 | PR #13 | 2026-03-05 | Monotonic rollout. No user eviction on % increase. |
| 1.30 | PromoteToExperiment → M5 CreateExperiment | Agent-7 | 🟢 | PR #13 | 2026-03-05 | Mocked in PR #13. Agent-5 CRUD now available for live wiring. |

**Bold** = critical path milestones that unblock downstream agents.

**Phase 1 complete**: All 30 milestones merged.

### Phase 2: Analysis & UI (Weeks 6–11)

| # | Milestone | Owner | Status | Unblocks |
|---|-----------|-------|--------|----------|
| 2.1 | GST (O'Brien-Fleming + Pocock) | Agent-4 | 🟢 | — | Implemented as part of M1.16 (PR #25) |
| 2.2 | Bootstrap CI | Agent-4 | 🟢 | Agent-6 (CI charts on results dashboard) | PR #29 |
| 2.3 | Multiple comparison correction (BH-FDR) | Agent-4 | 🟢 | Agent-6 (corrected p-values on results dashboard) | PR #29 |
| 2.4 | Novelty/primacy analysis | Agent-4 | 🟢 | Agent-6 (novelty tab) | PR #38 — Gauss-Newton with LM damping, golden-file validated |
| 2.5 | Interference analysis | Agent-4 | 🟢 | Agent-6 (interference tab) | PR #38 — JSD, Jaccard, Gini, title spillover with BH correction |
| 2.6 | Interleaving analysis (Team Draft scoring) | Agent-4 | 🟢 | Agent-6 (interleaving tab) | PR #38 — Sign test, Bradley-Terry MM, position analysis |
| 2.7 | GetInterleavedList RPC (Team Draft) | Agent-1 | 🟢 | Agent-4 (interleaving analysis) |
| 2.7b | Optimized Interleaving (softmax method) | Agent-1 | 🟢 | Agent-4 (interleaving analysis — sensitivity comparison) |
| 2.7c | Multileave Interleaving (Team Draft N-way) | Agent-1 | 🟢 | Agent-4 (N-way ranking comparison) |
| 2.8 | Results dashboard (treatment effects, CI chart, sequential boundary) | Agent-6 | 🟢 | Stakeholder demo |
| 2.9 | Notebook export (.ipynb from query log) | Agent-6 | 🟢 | — |
| 2.10 | Surrogate metric framework (M3 + M4a) | Agent-3/4 | 🟢 | Agent-3 part: PR #35. Agent-4 part: PR #43. Both merged. |
| 2.11 | SVOD-specific metrics (QoE, lifecycle, content consumption, interleaving scoring, session-level, QoE-engagement correlation) | Agent-3 | 🟢 | Agent-4 (interference, novelty, interleaving analysis M2.6), Agent-6 (QoE dashboard) |

### Phase 3: SVOD-Native + Bandits (Weeks 10–17)

| # | Milestone | Owner | Status | Unblocks |
|---|-----------|-------|--------|----------|
| 3.1 | LinUCB contextual bandit | Agent-4 | 🟢 | Agent-1 (contextual bandit arm selection via SelectArm RPC), Agent-6 (bandit dashboard) | PR #54 merged |
| 3.2 | Content cold-start bandit | Agent-4 | 🟢 | Agent-6 (cold-start widget on bandit dashboard) | PR #62 merged. Cold-start bandit + gRPC RPCs (CreateColdStartBandit, ExportAffinityScores) wired in PR #72. |
| 3.3 | Bandit dashboard (arm allocation, reward curves) | Agent-6 | 🟢 | PR #60 merged — arm allocation chart, reward rates, Thompson Sampling params, reward history |
| 3.4 | Session-level experiment support (full pipeline) | Agent-1/2/3 | 🟢 | — | Agent-2 done (session_id keyed events + `test_session_pipeline_e2e.sh` e2e harness). Agent-3 done (session_level_mean.sql.tmpl + StandardJob orchestration + 11 e2e tests in PR #79). Agent-1 done: enforce `allow_cross_session_variation` flag — `false` hashes on user_id for locked variant, `true` hashes on session_id (existing). 3 new tests. E2e harness validates cross-topic session_id correlation (exposure/metric/QoE). |
| 3.5 | Playback QoE experiment pipeline | Agent-2/3 | 🟢 | — | Agent-2 done (QoE validation + ingestion PR #40 + `test_qoe_pipeline_e2e.sh` e2e harness). Agent-3 done (qoe_metric.sql.tmpl + qoe_engagement_correlation.sql.tmpl + e2e tests PR #79). Pipeline e2e verified. |
| 3.6 | Cumulative holdout support | Agent-5 | 🟢 | M4a periodic lift reports |
| 3.7 | CATE lifecycle segment tab | Agent-6 | 🟢 | PR #80 — Forest plot + Cochran Q heterogeneity indicator per lifecycle segment |
| 3.8 | Phase 3 SVOD visualizations (QoE, novelty curve, GST, Lorenz) | Agent-6 | 🟢 | PR #76 (surrogate/holdout/guardrail), PR #81 (QoE dashboard, novelty decay curve, GST boundary, Lorenz curve). 171 tests. |

### Phase 4: Advanced & Polish (Weeks 16–22)

| # | Milestone | Owner | Status | Unblocks |
|---|-----------|-------|--------|----------|
| 4.1 | CATE heterogeneous treatment effects | Agent-4 | 🔵 | Subgroup analysis + Cochran Q + BH-FDR. 22 tests (18 unit/proptest + 4 golden). |
| 4.2 | Analysis service — all RPCs wired + PG caching | Agent-4 | 🟢 | Agent-6 (results dashboard, interleaving tab, novelty tab), Agent-5 (auto-conclude with fast result lookups) | PR #93 scaffolded, #107 merged. All 5 RPCs wired + PostgreSQL caching (AnalysisStore with sqlx). GetAnalysisResult cache-first. RunAnalysis/novelty/interference write-through. 36 tests. |
| 4.3 | PGO-optimized builds for M1 + M4b | Agent-1/4 | 🔵 | Agent-1: PGO build pipeline (instrument→profile→optimize), panic=abort, SLA validation script, nightly CI benchmarks. Agent-4 part pending. |
| 4.4 | Full RBAC integration | Agent-5 | 🟢 | Agent-6 (role-aware UI controls) | PR #71 merged — ConnectRPC auth interceptor, 4-level role hierarchy, audit trail records real actor |
| 4.4b | RBAC-aware UI controls | Agent-6 | 🟢 | — | Auth context + role-based button disabling + dev role switcher. Mirrors Agent-5 4-level hierarchy. |
| 4.6 | Performance targets (dashboard <1s, SQL <200ms, export <5s) | Agent-6 | 🟢 | — | In-memory RPC cache (30s TTL), code-split dynamic imports (11 tab/chart components), React.memo (7 components), prism-react-renderer SQL highlighting, Web Worker base64 decode for notebook export. 239 tests. |
| 4.7 | Live API integration prep (port fix, pause/resume, filters, contract tests) | Agent-6 | 🟢 | — | Fixed metrics/bandit port mapping swap. Added PauseExperiment/ResumeExperiment RPCs + MSW handlers. Server-side ListExperiments filters (state, type, owner, pagination). CATE/GST enum prefix stripping. 37 proto wire-format contract tests. 281 total tests. Ready to integrate with Agent-5 management service. |
| 4.5 | End-to-end chaos testing passing | All | 🔵 | Production readiness | Agent-1: `chaos_test_assignment.sh` (E2E framework hook) + `chaos_kill_assignment.sh` (standalone kill-9 + determinism verification). Agent-2: chaos scripts + crash-recovery tests merged + E2E chaos framework with pluggable hooks (PR #78). Agent-3: 20 resilience tests (PR #69). Agent-4: chaos_test_analysis.sh (M4a, 6 RPC verification tests) + chaos_test_policy.sh (M4b, cold-start + RocksDB state recovery) + 4 Rust crash recovery integration tests (multi-experiment concurrent restore, Kafka offset verification, high-volume 2200+ rewards, recovery timing <10s SLA). Agent-5: chaos_test_management.sh (PR #96). Agent-7: 13 chaos tests — ChaosStore decorator, atomicity, concurrent CRUD, restart simulation. |

## Pair Integration Schedule

Track integration test results between agent pairs.

| Week | Pair | Status | Notes |
|------|------|--------|-------|
| 3 | Agent-5 ↔ Agent-6 (management API + UI) | 🔵 | Agent-6 live API integration complete — Next.js rewrites proxy (port mapping fixed), ConnectRPC error parsing, enum prefix stripping (incl. CATE/GST), PauseExperiment/ResumeExperiment RPCs, server-side ListExperiments filters, 37 proto wire-format contract tests. Ready for end-to-end pair testing with Agent-5 backend. |
| 3 | Agent-1 ↔ Agent-5 (config streaming) | 🟡 | M5 StreamConfigUpdates ready (PR #15). Agent-1 can subscribe. |
| 4 | Agent-2 ↔ Agent-3 (event pipeline → metrics) | 🟢 | Merged (PR #51): SQL template ↔ M2 Delta Lake schema alignment, PgWriter query_log, notebook export, guardrail alert contract. Extended: 40 M2→M3 contract tests (ExposureEvent, MetricEvent, QoEEvent) — Delta schema alignment, M3 SQL template field coverage (exposure_join, session_level_mean, interleaving_score, qoe_metric), cross-topic user correlation, Kafka key contracts. |
| 4 | Agent-1 ↔ Agent-7 (hash parity via CGo) | 🟢 | CGo bridge parity confirmed — 10K vectors. Justfile target: `test-flags-cgo`. |
| 5 | Agent-3 ↔ Agent-4 (metric summaries → analysis) | 🟢 | ~50 contract tests verify M3 SQL output columns match Delta Lake schemas M4a reads. Covers all 4 output tables + ratio delta method variance components + PERCENTILE/CUSTOM/guardrail/QoE-correlation/surrogate-input templates + StandardJob e2e schema validation (6 experiments) + cross-table metric_id consistency + user-level GROUP BY granularity. |
| 5 | Agent-5 ↔ Agent-3 (guardrail alerts → auto-pause) | 🟢 | M3 Kafka publisher (PR #64) + M5 consumer (PR #18). 3 schema contract tests (field symmetry, bidirectional deser, zero-value). Kafka roundtrip integration test. Agent-2 guardrail E2E harness (`test_guardrail_e2e.sh`, PR #78) validates topic publish/consume. |
| 5 | Agent-2 ↔ Agent-4 (reward events: pipeline → bandit policy) | 🟢 | PR #99: 24 integration tests — 17 protobuf contract (encode/decode parity, context_json parsing, key contract) + 7 Kafka roundtrip (headers, partition determinism, consumer group offsets, ordering). |
| 6 | Agent-1 ↔ Agent-4 (bandit delegation: assignment → SelectArm) | 🔵 | M1 GrpcBanditClient with 10ms timeout + uniform fallback. M4b SelectArm gRPC wired through LMAX core. 3 mock gRPC integration tests pass. Ready for live pairing (set M4B_ADDR env var). |
| 6 | Agent-4 ↔ Agent-6 (analysis results → UI rendering) | 🔵 | All 5 M4a gRPC RPCs wired. Agent-6 can render results dashboard (t-test + SRM + CUPED), interleaving tab (sign test + Bradley-Terry), novelty tab (decay curves), and interference panel. |

## Contract Changes Log

Track any changes to proto schemas, shared crate APIs, or database schemas.

| Date | Agent | Change | Affected Agents | ADR | Status |
|------|-------|--------|-----------------|-----|--------|
| 2026-03-05 | Agent-5 | Added `kafka-go` dependency to Go services module | Agent-3 (shared go.mod) | — | Merged (PR #18) |

## Blockers & Escalations

| Date | Blocker | Raised By | Blocking | Severity | Resolution | Resolved |
|------|---------|-----------|----------|----------|------------|----------|
| — | — | — | — | — | — | — |

## Weekly Checkpoint

### Week 1 — 2026-03-05

**Completed this week:**
- [x] M1.1–1.5 Assignment service complete (Agent-1, PRs #4, #11, #21)
- [x] M1.6–1.9 Event pipeline complete (Agent-2, PRs #1, #8)
- [x] M1.10–1.13 Metric computation + guardrails complete (Agent-3, PRs #3, #5, #9, #16)
- [x] M1.14–1.19 Analysis + bandit + mSPRT complete (Agent-4, PRs #2, #14, #25)
- [x] M1.20–1.24 Management service complete (Agent-5, PRs #7, #10, #15, #18, #24)
- [x] M1.28–1.30 Flag service complete (Agent-7, PR #13)
- [x] M2.11 SVOD-specific metrics (Agent-3, PR #26)
- [x] M2.10 Surrogate metric framework — Agent-3 part (PR #35): model loading, projection, SQL transparency

**In progress:**
- Agent-3: M2.10 surrogate metric framework (Agent-3 part complete in PR #35, Agent-4 part pending)
- Agent-5: Phase 2 complete (PRs #50, #53). M3.6 cumulative holdout support complete
- Agent-6: M1.25–1.27 complete, M2.8–2.9 complete, create experiment form done (PR #49 merged), analysis tabs M2.4–2.6 UI done (PR #56)

**Unblocked this week:**
- Agent-1 unblocked for 1.3 (config cache) by Agent-5 StreamConfigUpdates (PR #15)
- Agent-6 unblocked for 1.25 (experiment UI) by Agent-5 CRUD (M1.20)
- Agent-7 unblocked for live PromoteToExperiment by Agent-5 CRUD (M1.20)

**Risks:**
- None — Agent-6 Phase 1 milestones now complete
