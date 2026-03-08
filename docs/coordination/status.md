# Experimentation Platform — Coordination Status

> **Last updated**: 2026-03-08 by Agent-6 (M2.8–2.9 complete, create experiment form)
>
> This file is the single source of truth for multi-agent execution state.
> Update it each time a milestone merges to `main` or a blocker is identified.

## Active Phase

**Phase 1: Foundation (Weeks 2–7)** — nearing completion. 28 of 30 milestones merged.

## Agent Status

| Agent | Module | Status | Current Branch | Current Milestone | Blocked By | Notes |
|-------|--------|--------|----------------|-------------------|------------|-------|
| Agent-1 | M1 Assignment | 🔵 Phase 2 In Progress | agent-1/feat/bandit-delegation | Bandit delegation (MAB/CONTEXTUAL_BANDIT) | — | M1.1–1.5 + M2.7 complete. Bandit delegation: MAB/CONTEXTUAL_BANDIT experiments use mock uniform arm selection (until M4b SelectArm is live). |
| Agent-2 | M2 Pipeline | 🔵 Phase 4 In Progress | agent-2/feat/phase4-chaos-engineering | Phase 4: Chaos engineering + crash recovery | — | Phase 1 done (PRs #1, #8). Phase 2 done (PR #23). Phase 3 done (PR #40). Phase 4: chaos scripts, crash-recovery tests, synthetic event generator. |
| Agent-3 | M3 Metrics | 🔵 Phase 2 In Progress | agent-3/feat/surrogate-metric-framework | M2.10 Surrogate Metric Framework | — | Phase 1 done. M2.11 done (PR #26). M2.10 in progress (PR #35). |
| Agent-4 | M4a Analysis + M4b Bandit | 🔵 Phase 3 In Progress | agent-4/feat/linucb-contextual-bandit | M3.1 LinUCB Contextual Bandit | — | M1.14–1.19 merged. M2.1–2.6 complete (PRs #25, #29, #38). M2.10 (Agent-4 part) in progress. M3.1 LinUCB PR open. |
| Agent-5 | M5 Management | 🔵 Phase 2 | agent-5/feat/surrogate-crud | Sequential auto-conclude (Phase 2) | — | Surrogate CRUD + sequential auto-conclude. Unblocks Agent-4 (boundary crossing → auto-conclude integration). |
| Agent-6 | M6 UI | 🔵 In Progress | agent-6/feat/results-dashboard | Create experiment form + M2.8–2.9 complete | — | M1.25–1.27 done, M2.8–2.9 done. 87 tests pass. Create experiment form with full field coverage. |
| Agent-7 | M7 Flags | 🔵 In Progress | agent-7/feat/flag-experiment-linkage | Phase 2+3: Flag-experiment linkage + dependency tracking | — | M1.28–1.30 merged (PR #13). PR #36: production wiring. Flag-experiment linkage: PromoteToExperiment records experiment ID, ResolvePromotedExperiment auto-updates flag when experiment concludes. Dependency tracking: query flags by targeting rule. |

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
| 2.8 | Results dashboard (treatment effects, CI chart, sequential boundary) | Agent-6 | 🟢 | Stakeholder demo |
| 2.9 | Notebook export (.ipynb from query log) | Agent-6 | 🟢 | — |
| 2.10 | Surrogate metric framework (M3 + M4a) | Agent-3/4 | 🔵 | Agent-3 part: PR #35 complete. Agent-4 part: surrogate validation in progress. |
| 2.11 | SVOD-specific metrics (QoE, lifecycle, content consumption, interleaving scoring, session-level, QoE-engagement correlation) | Agent-3 | 🟢 | Agent-4 (interference, novelty, interleaving analysis M2.6), Agent-6 (QoE dashboard) |

### Phase 3: SVOD-Native + Bandits (Weeks 10–17)

| # | Milestone | Owner | Status | Unblocks |
|---|-----------|-------|--------|----------|
| 3.1 | LinUCB contextual bandit | Agent-4 | 🔵 | Agent-1 (contextual bandit arm selection via SelectArm RPC), Agent-6 (bandit dashboard) |
| 3.2 | Content cold-start bandit | Agent-4 | ⚪ | — |
| 3.3 | Bandit dashboard (arm allocation, reward curves) | Agent-6 | ⚪ | — |
| 3.4 | Session-level experiment support (full pipeline) | Agent-1/2/3 | 🟡 | — | Agent-2 part done (session_id keyed events). Agent-1/3 parts pending. |
| 3.5 | Playback QoE experiment pipeline | Agent-2/3 | 🟡 | — | Agent-2 part done (QoE validation + ingestion PR #40). Agent-3 part pending (Spark SQL). |
| 3.6 | Cumulative holdout support | Agent-5 | ⚪ | — |

### Phase 4: Advanced & Polish (Weeks 16–22)

| # | Milestone | Owner | Status | Unblocks |
|---|-----------|-------|--------|----------|
| 4.1 | CATE heterogeneous treatment effects | Agent-4 | ⚪ | — |
| 4.2 | Interference detection (content catalog spillover) | Agent-4 | ⚪ | — |
| 4.3 | PGO-optimized builds for M1 + M4b | Agent-1/4 | ⚪ | — |
| 4.4 | Full RBAC integration | Agent-5 | ⚪ | — |
| 4.5 | End-to-end chaos testing passing | All | 🔵 | Production readiness | Agent-2: chaos scripts + crash-recovery tests in PR. Other agents pending. |

## Pair Integration Schedule

Track integration test results between agent pairs.

| Week | Pair | Status | Notes |
|------|------|--------|-------|
| 3 | Agent-5 ↔ Agent-6 (management API + UI) | 🟡 | Agent-5 CRUD ready. Agent-6 can start live integration. |
| 3 | Agent-1 ↔ Agent-5 (config streaming) | 🟡 | M5 StreamConfigUpdates ready (PR #15). Agent-1 can subscribe. |
| 4 | Agent-2 ↔ Agent-3 (event pipeline → metrics) | 🟡 | Agent-2 has synthetic event generator + chaos scripts ready. Agent-3 needs Kafka consumer for live integration. |
| 4 | Agent-1 ↔ Agent-7 (hash parity via CGo) | 🟢 | CGo bridge parity confirmed — 10K vectors. Justfile target: `test-flags-cgo`. |
| 5 | Agent-3 ↔ Agent-4 (metric summaries → analysis) | ⚪ | — |
| 5 | Agent-5 ↔ Agent-3 (guardrail alerts → auto-pause) | 🟡 | Both sides ready (M3 PR #16, M5 PR #18). Needs Kafka for live test. |
| 6 | Agent-1 ↔ Agent-4 (bandit delegation: assignment → SelectArm) | 🟡 | M1 bandit delegation with mock uniform selection ready. Awaiting M4b SelectArm gRPC for live integration. |
| 6 | Agent-4 ↔ Agent-6 (analysis results → UI rendering) | ⚪ | — |

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
- Agent-5: Surrogate model CRUD (Phase 2) — CreateSurrogateModel, ListSurrogateModels, GetSurrogateCalibration, TriggerSurrogateRecalibration
- Agent-6: M1.25–1.27 complete, M2.8–2.9 complete, create experiment form in progress

**Unblocked this week:**
- Agent-1 unblocked for 1.3 (config cache) by Agent-5 StreamConfigUpdates (PR #15)
- Agent-6 unblocked for 1.25 (experiment UI) by Agent-5 CRUD (M1.20)
- Agent-7 unblocked for live PromoteToExperiment by Agent-5 CRUD (M1.20)

**Risks:**
- None — Agent-6 Phase 1 milestones now complete
