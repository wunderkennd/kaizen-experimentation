# Experimentation Platform — Coordination Status

> **Last updated**: 2026-04-06 — Sprints 5.2/5.3 complete, Sprint 5.4 launched
>
> This file is the single source of truth for multi-agent execution state.
> Update it each time a milestone merges to `main` or a blocker is identified.

## Active Phase

**Phase 5: Advanced Capabilities** — in progress. 15 ADRs across 6 clusters. See [Phase 5 ADR Tracker](#phase-5-adr-tracker) below.

**Phase 1: Foundation (Weeks 2–7)** — **complete**. All 30 milestones merged.

## Agent Status

> **Per-agent details have moved to `docs/coordination/status/agent-N.md`** to eliminate
> merge conflicts. Each agent updates only their own file. See [status/README.md](status/README.md).

| Agent | Module | Status | Details |
|-------|--------|--------|---------|
| Agent-1 | M1 Assignment | 🔵 Polish | [agent-1.md](status/agent-1.md) |
| Agent-2 | M2 Pipeline | 🔵 Polish | [agent-2.md](status/agent-2.md) |
| Agent-3 | M3 Metrics | 🟢 All Phases Complete | [agent-3.md](status/agent-3.md) |
| Agent-4 | M4a Analysis + M4b Bandit | 🟢 All Phases Complete | [agent-4.md](status/agent-4.md) |
| Agent-5 | M5 Management | 🟢 All Phases Complete | [agent-5.md](status/agent-5.md) |
| Agent-6 | M6 UI | 🟢 All Phases Complete | [agent-6.md](status/agent-6.md) |
| Agent-7 | M7 Flags | 🟢 All Phases Complete | [agent-7.md](status/agent-7.md) |

**Legend**: 🟢 Complete | 🔵 In Progress | 🟡 Not Started (unblocked) | ⚪ Waiting (blocked) | 🔴 Blocked (critical path)

---

## Phase 5 ADR Tracker

> Updated: 2026-04-06. 15 ADRs across 6 clusters. PRs counted: **35 submitted, 35 merged, 0 pending, 0 red**.
> Sprints 5.0–5.4 complete. Sprint 5.5 in progress.

### ADR Implementation Status

| ADR | Title | Cluster | Agent(s) | PRs | CI | Status |
|-----|-------|---------|----------|-----|----|--------|
| ADR-011 | Multi-objective reward composition | A | Agent-4 | #221, #228 | Green | MERGED |
| ADR-012 | LP constraints (bandit arm/global) | A | Agent-4 | #245 | Green | MERGED |
| ADR-013 | Meta-experiments | A | Agent-1, Agent-5 | #249, #259 | Green | MERGED (M5 validation + M1 assignment) |
| ADR-014 | Provider-side metrics | A | Agent-3, Agent-4, Agent-5, Agent-6 | #208, #211, #212 | Green | MERGED |
| ADR-015 | AVLM (sequential CUPED) | B | Agent-4, Agent-6 | #199, #223, #226 | Green | MERGED |
| ADR-016 | Slate bandits | C | Agent-1, Agent-4 | #253, #327, #329 | Green | MERGED |
| ADR-017 | Offline RL / TC+JIVE surrogate calibration | C | Agent-4 | #198 | Green | Ph 1 MERGED / Ph 2 pending |
| ADR-018 | E-values + online FDR (e-LOND) | B | Agent-4, Agent-5 | #200, #231, #267 | Green | Ph 1+2 MERGED / Ph 3 pending |
| ADR-019 | Portfolio optimization | E | Agent-5, Agent-6 | #250, #261, #328, #330 | Green | MERGED |
| ADR-020 | Adaptive sample size recalculation | B | Agent-4, Agent-5, Agent-6 | #223, #227, #228 | Green | MERGED |
| ADR-021 | Feedback loop interference detection | E | Agent-2, Agent-3, Agent-4, Agent-6 | #209, #222, #223 | Green | MERGED |
| ADR-022 | Switchback experiments | D | Agent-1, Agent-4 | #229, #252, #258, #259 | Green | MERGED |
| ADR-023 | Synthetic control methods | D | Agent-4 | #243, #252, #258 | Green | MERGED |
| ADR-024 | M7 Rust port (unconditional) | F | Agent-7 | #197, #215, #220 | Green | MERGED (all 4 phases) |
| ADR-025 | M5 Rust port (conditional) | F | Agent-5 | — | — | NOT STARTED |

### Cluster Summary

| Cluster | ADRs | Fully Merged | In Progress | Not Started |
|---------|------|-------------|-------------|-------------|
| A: Multi-Stakeholder | 011, 012, 013, 014 | 011, 012, 013, 014 | — | — |
| B: Statistical Methods | 015, 018, 020 | 015, 018 (Ph 1+2), 020 | 018 (Ph 3) | — |
| C: Bandit & RL | 016, 017 | 016, 017 (Ph 1) | 017 (Ph 2) | — |
| D: Quasi-Experimental | 022, 023 | 022, 023 | — | — |
| E: Platform Operations | 019, 021 | 019, 021 | — | — |
| F: Language Migration | 024, 025 | 024 | — | 025 |

### Phase 5 PR Index

| PR | ADR(s) | Description | Status |
|----|--------|-------------|--------|
| #196 | 011–023 | Proto schema extensions (Phase 5 unblock) | Merged |
| #197 | 024 | ADR-024 Phase 1 — M7 Rust scaffold, CRUD, EvaluateFlag | Merged |
| #198 | 017 | ADR-017 Ph 1 — TC/JIVE K-fold IV surrogate calibration | Merged |
| #199 | 015 | ADR-015 Ph 1 — AVLM sequential test (experimentation-stats) | Merged |
| #200 | 018 | ADR-018 Ph 1 — E-value computation (GROW + AVLM) | Merged |
| #208 | 014 | ADR-014 — /portfolio/provider-health M6 UI page | Merged |
| #209 | 021 | ADR-021 — ModelRetrainingEvent ingestion (M2) | Merged |
| #211 | 014 | ADR-014 — Provider-side SQL metrics (M3) | Merged |
| #212 | 014 | ADR-014 — Guardrail Bonferroni + M5 metric validation | Merged |
| #215 | 024 | ADR-024 Phases 2–4 — M7 Rust port complete, FFI crate deleted | Merged |
| #220 | 024 | Fix — remove duplicate experimentation-flags workspace entry | Merged |
| #221 | 011 | ADR-011 — Multi-objective reward composition (M4b) | Merged |
| #222 | 021 | ADR-021 — Feedback loop interference detection (stats) | Merged |
| #223 | 015, 020, 021 | ADR-015/020/021 — AVLM plot, adaptive-N badge, feedback loop tab (M6) | Merged |
| #226 | 015 | ADR-015 — Wire AVLM into M4a RunAnalysis | Merged |
| #227 | 020 | ADR-020 — Adaptive sample size stats + M5 scheduler | Merged |
| #228 | 011, 020 | Fix — ADR-011 reconcile + ADR-020 clippy/test fixes | Merged |

| #229 | 022 | ADR-022 — M1 switchback temporal assignment | Merged |
| #231 | 018 | ADR-018 Ph 2 — e-LOND OnlineFdrController (M5) | Merged |
| #243 | 023 | ADR-023 — Synthetic control methods (experimentation-stats) | Merged |
| #245 | 012 | ADR-012 — LP constraint post-processing layer (experimentation-bandit) | Merged |
| #249 | 013 | ADR-013 — META experiment type (M5 validation + M1 two-level assignment) | Merged |
| #250 | 019 | ADR-019 — GetPortfolioAllocation gRPC (M5) | Merged |
| #252 | 022, 023 | ADR-022/023 — Switchback + quasi-experiment UI tabs (M6) | Merged |
| #253 | 016 | ADR-016 — Slate bandit UI components (M6) | Merged |
| #258 | 022, 023, 018 | ADR-022/023/018 — Wire switchback, SCM, e-values into M4a RunAnalysis | Merged |
| #259 | 013, 022, 023 | STARTING validation for META, SWITCHBACK, QUASI types (M5) | Merged |
| #261 | 019 | ADR-019 — Portfolio power analysis (experimentation-stats, M4a) | Merged |
| #266 | 015, 020 | ADR-015/020 — AvlmSequencePlot + AdaptiveNZoneBadge (M6) | Merged |
| #267 | 018 | ADR-018 — E-value gauge + online FDR budget bar (M6) | Merged |

**Totals**: 30 PRs submitted · 30 merged · 0 open · 0 red

### Pending Work (no PR yet)

| ADR | Gap | Owner | Depends On |
|-----|-----|-------|------------|
| ADR-015 Ph 2 | MLRATE cross-fitting | Agent-3 | — |
| ADR-017 Ph 2 | Doubly-robust offline RL policy evaluation | Agent-4 | — |
| ADR-018 Ph 3 | MAD e-processes | Agent-4 | — |
| ADR-025 | M5 Rust port (conditional — awaiting go/no-go, 3/5 triggers met) | Agent-5 | ADR-024 |

---

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
| 4.1 | CATE heterogeneous treatment effects | Agent-4 | 🟢 | Subgroup analysis + Cochran Q + BH-FDR. 28 stats tests (18 unit/proptest + 4 golden) + 3 integration tests wiring CATE into RunAnalysis RPC via lifecycle_segment. |
| 4.2 | Analysis service — all RPCs wired + PG caching | Agent-4 | 🟢 | Agent-6 (results dashboard, interleaving tab, novelty tab), Agent-5 (auto-conclude with fast result lookups) | PR #93 scaffolded, #107 merged. All 5 RPCs wired + PostgreSQL caching (AnalysisStore with sqlx). GetAnalysisResult cache-first. RunAnalysis/novelty/interference write-through. 36 tests. |
| 4.3 | PGO-optimized builds for M1 + M4a/M4b | Agent-1/4 | 🟢 | Agent-1 (PR #116, merged), Agent-4 (PR #133, merged) |
| 4.4 | Full RBAC integration | Agent-5 | 🟢 | Agent-6 (role-aware UI controls) | PR #71 merged — ConnectRPC auth interceptor, 4-level role hierarchy, audit trail records real actor |
| 4.4b | RBAC-aware UI controls | Agent-6 | 🟢 | — | Auth context + role-based button disabling + dev role switcher. Mirrors Agent-5 4-level hierarchy. |
| 4.6 | Performance targets (dashboard <1s, SQL <200ms, export <5s) | Agent-6 | 🟢 | — | In-memory RPC cache (30s TTL), code-split dynamic imports (11 tab/chart components), React.memo (7 components), prism-react-renderer SQL highlighting, Web Worker base64 decode for notebook export. 239 tests. |
| 4.7 | Live API integration prep (port fix, pause/resume, filters, contract tests) | Agent-6 | 🟢 | — | Fixed metrics/bandit port mapping swap. Added PauseExperiment/ResumeExperiment RPCs + MSW handlers. Server-side ListExperiments filters (state, type, owner, pagination). CATE/GST enum prefix stripping. 37 proto wire-format contract tests. 281 total tests. Ready to integrate with Agent-5 management service. |
| 4.8 | Error boundary + chaos resilience | Agent-6 | 🟢 | — | React ErrorBoundary wrapping page content, RetryableError component on all pages/tabs, 404 vs 500 distinction (no-data vs service-down), 8 MSW chaos tests. 317 total tests. |
| 4.5 | End-to-end chaos testing passing | All | 🔵 | Production readiness | Agent-1: `chaos_test_assignment.sh` (E2E framework hook) + `chaos_kill_assignment.sh` (standalone kill-9 + determinism verification). Agent-2: chaos scripts + crash-recovery tests merged + E2E chaos framework with pluggable hooks (PR #78) + full pipeline E2E test `test_full_pipeline_e2e.sh` (PR #124) — 7 phases, ~24 tests across M1/M2/M3/M4a. Agent-3: 20 resilience tests (PR #69). Agent-4: `chaos_kill_policy.sh` (M4b, 8-phase kill-9 under load — create experiment, feed rewards, baseline capture, sustained SelectArm load, SIGKILL, RocksDB recovery <10s SLA, arm distribution comparison, full RPC verification) + `chaos_test_analysis.sh` (M4a, 7 RPC verification tests for all 5 wired RPCs + input validation) + `chaos_test_policy.sh` (M4b framework hook) + 4 Rust crash recovery integration tests + Justfile recipes (`just chaos`, `just chaos-policy`, `just chaos-analysis`) + weekly-chaos.yml wiring (M4a standalone + multi-service E2E framework). Agent-5: chaos_test_management.sh (PR #96). Agent-7: 13 chaos tests — ChaosStore decorator, atomicity, concurrent CRUD, restart simulation. |

## Pair Integration Schedule

Track integration test results between agent pairs.

| Week | Pair | Status | Notes |
|------|------|--------|-------|
| 3 | Agent-5 ↔ Agent-6 (management API + UI) | 🟢 | Agent-5: 11 wire-format contract tests (m5m6_contract_test.go, PR #126) — camelCase fields, enum prefixed strings, proto3 zero-value omission, response envelope, RFC 3339 timestamps, ConnectRPC error format, RBAC 403. Agent-6: 37 proto wire-format contract tests (PR #130) — port mapping fix, PauseExperiment/ResumeExperiment RPCs, server-side ListExperiments filters, CATE/GST enum prefix stripping. Ready for live end-to-end pair testing. |
| 3 | Agent-1 ↔ Agent-5 (config streaming) | 🟢 | 10 contract tests (m1m5_contract_test.go): required fields, holdout flag, version monotonicity, deletion on conclude, variant contract (payload_json roundtrip), state=RUNNING only, hash_salt stability, enum non-UNSPECIFIED, snapshot completeness, non-running exclusion. Validates M1's experiment_from_proto() + variant_from_proto() consumer contract. |
| 4 | Agent-2 ↔ Agent-3 (event pipeline → metrics) | 🟢 | Merged (PR #51): SQL template ↔ M2 Delta Lake schema alignment, PgWriter query_log, notebook export, guardrail alert contract. Extended: 40 M2→M3 contract tests (ExposureEvent, MetricEvent, QoEEvent) — Delta schema alignment, M3 SQL template field coverage (exposure_join, session_level_mean, interleaving_score, qoe_metric), cross-topic user correlation, Kafka key contracts. |
| 4 | Agent-1 ↔ Agent-7 (hash parity via CGo) | 🟢 | CGo bridge parity confirmed — 10K vectors. Justfile target: `test-flags-cgo`. |
| 5 | Agent-3 ↔ Agent-4 (metric summaries → analysis) | 🟢 | ~50 contract tests (PR #127) verify M3 SQL output columns match Delta Lake schemas M4a reads. Covers all 4 output tables + ratio delta method variance components + PERCENTILE/CUSTOM/guardrail/QoE-correlation/surrogate-input templates + StandardJob e2e schema validation (6 experiments) + cross-table metric_id consistency + user-level GROUP BY granularity. |
| 5 | Agent-5 ↔ Agent-3 (guardrail alerts → auto-pause) | 🟢 | M3 Kafka publisher (PR #64) + M5 consumer (PR #18). 3 schema contract tests (field symmetry, bidirectional deser, zero-value). Kafka roundtrip integration test. Agent-2 guardrail E2E harness (`test_guardrail_e2e.sh`, PR #78) validates topic publish/consume. |
| 5 | Agent-2 ↔ Agent-4 (reward events: pipeline → bandit policy) | 🟢 | PR #99: 24 integration tests — 17 protobuf contract (encode/decode parity, context_json parsing, key contract) + 7 Kafka roundtrip (headers, partition determinism, consumer group offsets, ordering). |
| 6 | Agent-3 ↔ Agent-5 (experiment/metric/surrogate definitions) | 🟢 | 22 tests, 49 subtests (PR #139): M3 reads experiment definitions, metric definitions, and surrogate models from M5's PostgreSQL tables. Validates wire-format roundtrip for all definition types M3 consumes. |
| 6 | Agent-1 ↔ Agent-4 (bandit delegation: assignment → SelectArm) | 🟢 | 10 contract tests (`m1m4b_contract_test.rs`): Thompson roundtrip, deterministic user seeding, LinUCB context feature serialization, NOT_FOUND error code, cold-start full lifecycle (Create → SelectArm → ExportAffinity), affinity score finiteness, concurrent SelectArm (20 parallel calls), user distribution across arms, default window_days. Uses real Thompson Sampling + LinUCB from `experimentation-bandit` (not mocks). |
| 6 | Agent-7 ↔ Agent-6 (flag management UI integration) | 🟢 | Agent-7: Flag types, API layer (listFlags/getFlag/createFlag/updateFlag/promoteToExperiment), MSW handlers + seed data (4 flags), nav link. Agent-6 UI: /flags list page, /flags/[id] detail page with promote-to-experiment, /flags/new create form. next.config.js rewrite for flags service port 50057. 376 total UI tests passing. Wire-format contract tests validate FLAG_TYPE_ prefix stripping, proto3 zero-value omission, pagination, EvaluateFlag/EvaluateFlags response shape, PromoteToExperiment → Experiment adapter. |
| 6 | Agent-4 ↔ Agent-6 (analysis results → UI rendering) | 🟢 | Agent-4: 12 Rust wire-format contract tests (`m4a_m6_contract_test.rs`) — AnalysisResult field presence, MetricResult 14-field contract (finite + CI containment + p-value range), SRM map fields + mismatch detection, SegmentResult lifecycle enum values (TRIAL=1, ESTABLISHED=3, MATURE=4), proto3 zero-value omission for optional sub-messages, InterleavingAnalysisResult (win rates + sign test + Bradley-Terry + position analysis), NoveltyAnalysisResult (decay params + stabilization), InterferenceAnalysisResult (JSD + Jaccard + Gini + coverage + spillover titles), NOT_FOUND for all 5 RPCs, INVALID_ARGUMENT for all 5 RPCs, Cochran Q heterogeneity detection. Agent-6: 27 TS wire-format contract tests (`m4a-wire-format.test.ts`) — proto3 zero-value omission, int64 string→number coercion (SRM counts + segment sampleSize), LIFECYCLE_SEGMENT_/SEQUENTIAL_METHOD_ prefix stripping, SurrogateProjection adapter (modelId→metricId, variantId→surrogateMetricId), cochranQPValue + segmentResults added to UI types, 5 missing RPCs cataloged, 2 UI-only fields remaining (dailyEffects, Lorenz curves). `adaptAnalysisResult()` pipeline transforms proto3 JSON to typed UI objects. Error boundary distinguishes 404 (no data) from 500 (service down) with retry. 344 total tests. |

### Cross-Cutting Fixes (Devin AI + Manual Review)

| PR | Description | Status | Reviewed By |
|----|-------------|--------|-------------|
| #153 | DocMost documentation site | Merged 2026-03-13 | — |
| #155 | DocMost populate script review fixes | Merged 2026-03-13 | — |
| #158 | Integration and user experience documentation | Merged 2026-03-14 | — |
| #160 | Platform improvements: port collisions, assert_finite, status split, contract tests, CI workflows, SDK consolidation | Merged 2026-03-14 | — |
| #161 | 12 bugs + optimizations from early PR review (#1–#79): Thompson MC propensity, Pause/Resume TOCTOU, holdout fail-closed, negative nanos guard, config cache layer auto-registration, allocator overlap detection, LinUCB Frobenius norm, golden file significance check | Merged 2026-03-15 | Agent-2 (6/6 checklist items passed) |
| #162 | Post-review cleanup: doc port mismatches, iOS SDK dedup, Python SDK drift, Rust API `#[doc(hidden)]` | Merged 2026-03-15 | — |
| #163 | Mobile SDK CI builds: guard UniFFI imports (iOS `#if canImport`, Android conditional source set) | Open | — |

## Contract Changes Log

Track any changes to proto schemas, shared crate APIs, or database schemas.

| Date | Agent | Change | Affected Agents | ADR | Status |
|------|-------|--------|-----------------|-----|--------|
| 2026-03-05 | Agent-5 | Added `kafka-go` dependency to Go services module | Agent-3 (shared go.mod) | — | Merged (PR #18) |
| 2026-03-15 | Devin | Thompson Sampling `select_arm` now uses MC simulation (1000 draws) for correct IPW propensity scores | Agent-1 (SelectArm latency), Agent-4 (M4b policy) | — | Merged (PR #161) |
| 2026-03-15 | Devin | Holdout assignment fail-closed: failed holdout now blocks layer instead of leaking users to treatment | Agent-1 (M1 assignment) | — | Merged (PR #161) |
| 2026-03-15 | Devin | Allocator overlap detection: `ErrOverlappingRanges` sentinel on occupied range overlap | Agent-5 (M5 allocation) | — | Merged (PR #161) |

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

### Week 2 — 2026-03-12

**Completed this week:**
- [x] Agent-4: PGO-optimized builds for M4a/M4b (PR #133)
- [x] Agent-3: M3↔M4a PG cache contract tests (PR #134)
- [x] Agent-5: Agent-1 ↔ Agent-5 config streaming contract tests (PR #135)
- [x] Agent-4: GST scipy boundary validation (PR #136)
- [x] Agent-6: Session-level analysis panel (PR #137)
- [x] Agent-1: 50K rps load test with dynamic VU scaling (PR #138)
- [x] Agent-3: M3↔M5 wire-format contracts — 22 tests, 49 subtests (PR #139)
- [x] Agent-4: Bootstrap BCa/percentile coverage validation (PR #140)
- [x] Status sync (PR #141)

### Week 3 — 2026-03-15

**Completed this week:**
- [x] Agent-1: M1-M4b live bandit contract tests — 10 tests (PR #142)
- [x] Agent-6: Phase 4 — performance, live API, error resilience, M4a pair testing (PR #143)
- [x] Agent-1: SDK LocalProvider hash-based variant assignment (PR #144)
- [x] Agent-4: CATE lifecycle segment wired into RunAnalysis RPC (PR #145)
- [x] Agent-1: Agent-1 ↔ Agent-4 live bandit delegation contract tests (PR #146)
- [x] Agent-6: Proto-to-UI type alignment adapters (PR #147)
- [x] Agent-3: Prometheus observability — 7 metrics on :50056 (PR #148)
- [x] Agent-3: Grafana dashboard panels + Prometheus alert rules (PR #149)
- [x] Agent-1: SDK RemoteProviders with JSON HTTP API (PR #150)
- [x] Agent-4: Agent-4 ↔ Agent-6 wire-format contract tests (PR #151)
- [x] Agent-1: All phases complete (PR #152)
- [x] Devin: DocMost documentation site (PRs #153, #155, #158)
- [x] Agent-6: Metric definition browser — /metrics page (PR #154)
- [x] Agent-4: Bayesian, IPW, clustered SE, neural bandit (PR #156)
- [x] Agent-5: MetricType type_filter for ListMetricDefinitions (PR #157)
- [x] Agent-4: Migrate neural bandit from tch-rs to Candle (PR #159)
- [x] Devin: Platform improvements — port collisions, assert_finite, status split, CI, SDKs (PR #160)
- [x] Devin: 12 bugs + optimizations from early PR review, human review checklist passed 6/6 (PR #161)
- [x] Agent-1: Post-review cleanup — doc ports, iOS SDK, Python drift (PR #162)

- [x] Agent-2: PGO build + k6 load test + 6 Grafana panels + 2 alert rules (PR #179)

**In progress:**
- Agent-1: Mobile SDK CI builds — guard UniFFI imports (PR #163, open)
- Agent-6: IPW-adjusted results integration (branch `agent-6/feat/ipw-results`)

### Week 4 — 2026-03-16

**Completed this week:**
- [x] Agent-6: Experiment creation wizard — 5-step type-aware flow (PR #169)
- [x] Agent-6: Real-time monitoring page /monitoring (PR #176)
- [x] Agent-6: Experiment comparison view /compare (PR #177)
- [x] Agent-6: Audit log viewer /audit (PR #178)
- [x] Agent-6: IPW-adjusted results integration — IpwToggle, IpwDetailsPanel, wire-format contract tests

**Platform status:**
- 6 of 7 agents at 🟢 All Phases Complete
- Agent-1 in polish mode (PR #163 pending)
- Agent-6 in polish mode (wizard, monitoring, comparison, audit, IPW)
- All 10 pair integrations at 🟢
- Cross-cutting Devin review addressed 12 bugs across 6 crates/services
- Agent-6 at 416 tests (40 wire-format contract, 37 wizard, 11 metric browser)
