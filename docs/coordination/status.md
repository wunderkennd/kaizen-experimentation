# Experimentation Platform — Coordination Status

> **Last updated**: 2026-03-05 by Agent-5 (comprehensive status sync — all merged PRs reflected)
>
> This file is the single source of truth for multi-agent execution state.
> Update it each time a milestone merges to `main` or a blocker is identified.

## Active Phase

**Phase 1: Foundation (Weeks 2–7)** — nearing completion. 25 of 30 milestones merged.

## Agent Status

| Agent | Module | Status | Current Branch | Current Milestone | Blocked By | Notes |
|-------|--------|--------|----------------|-------------------|------------|-------|
| Agent-1 | M1 Assignment | 🔵 In Progress | agent-1/feat/targeting-rules | Targeting rule evaluation (1.4) | — | M1.1–1.2 merged. PR #21 open for 1.4. M1.3 unblocked by M5 StreamConfigUpdates (PR #15). |
| Agent-2 | M2 Pipeline | 🟢 Phase 1 Complete | — | Phase 2: Operational hardening | — | M1.6–1.9 all merged (PRs #1, #8). All Phase 1 milestones done. |
| Agent-3 | M3 Metrics | 🟢 Phase 1 Complete | — | Phase 2: Surrogate metrics / SVOD (2.10–2.11) | — | M1.10–1.13 all merged (PRs #3, #5, #9, #16). All Phase 1 milestones done. |
| Agent-4 | M4a Analysis + M4b Bandit | 🔵 In Progress | — | mSPRT sequential testing (1.16) | — | M1.14–1.15, 1.17–1.19 merged (PRs #2, #14). Only 1.16 remains in Phase 1. |
| Agent-5 | M5 Management | 🔵 In Progress | — | Metric definition CRUD (1.24) | — | M1.20–1.23 all merged (PRs #7, #10, #15, #18). Next: 1.24. |
| Agent-6 | M6 UI | 🟡 Not Started | — | Experiment list + detail shell (1.25) | — | Unblocked by M1.20. Agent-5 CRUD API available. Can use live backend. |
| Agent-7 | M7 Flags | 🟢 Phase 1 Complete | — | Integration: PromoteToExperiment live wiring | — | M1.28–1.30 merged (PR #13). Agent-5 CRUD now available for real PromoteToExperiment. |

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
| 1.3 | Config cache (subscribe to M5 StreamConfigUpdates) | Agent-1 | 🟡 | — | — | Unblocked by M5 PR #15. Agent-1 can subscribe to live config stream. |
| 1.4 | Targeting rule evaluation | Agent-1 | 🔵 | PR #21 | — | In review. |
| 1.5 | Layer-aware + session-level assignment | Agent-1 | 🟡 | — | — | — |
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
| 1.16 | mSPRT sequential testing | Agent-4 | 🟡 | — | — | Unblocked. Last remaining Phase 1 milestone for Agent-4. |
| 1.17 | Thompson Sampling with Beta-Bernoulli (M4b) | Agent-4 | 🟢 | PR #2 | 2026-03-05 | Agent-1 (SelectArm) |
| 1.18 | LMAX single-threaded policy core (M4b) | Agent-4 | 🟢 | PR #2 | 2026-03-05 | Bandit policy serving |
| 1.19 | RocksDB policy snapshots (M4b) | Agent-4 | 🟢 | PR #2 | 2026-03-05 | Crash recovery |
| **1.20** | **Experiment CRUD + state machine enforcement** | Agent-5 | 🟢 | — | 2026-03-04 | Agent-6 (list/detail), Agent-1 (configs), Agent-3 (experiment list) |
| 1.21 | Layer allocation + bucket reuse | Agent-5 | 🟢 | PR #7, #10 | 2026-03-05 | ADR-009 bucket reuse with cooldown |
| 1.22 | StreamConfigUpdates RPC | Agent-5 | 🟢 | PR #15 | 2026-03-05 | Agent-1 (real-time config cache) |
| 1.23 | Guardrail alert consumer → auto-pause | Agent-5 | 🟢 | PR #18 | 2026-03-05 | ADR-008 auto-pause. Kafka consumer + processor. |
| 1.24 | Metric definition CRUD | Agent-5 | 🟡 | — | — | Agent-3 (metric configs) |
| **1.25** | **Experiment list + detail shell (MSW mocked)** | Agent-6 | 🟡 | — | — | Stakeholder demo. Unblocked by M1.20. Ready to start. |
| 1.26 | State indicator component (color-coded lifecycle) | Agent-6 | 🟡 | — | — | Unblocked. Agent-5 CRUD API available for live integration. |
| 1.27 | View SQL page (query log from M3) | Agent-6 | ⚪ | — | — | Needs Agent-2 query log (1.9 merged) |
| **1.28** | **Boolean flag CRUD + CGo hash bridge** | Agent-7 | 🟢 | PR #13 | 2026-03-05 | CRUD + EvaluateFlag + CGo bridge. 10K hash vectors match. |
| 1.29 | Percentage rollout (monotonic) | Agent-7 | 🟢 | PR #13 | 2026-03-05 | Monotonic rollout. No user eviction on % increase. |
| 1.30 | PromoteToExperiment → M5 CreateExperiment | Agent-7 | 🟢 | PR #13 | 2026-03-05 | Mocked in PR #13. Agent-5 CRUD now available for live wiring. |

**Bold** = critical path milestones that unblock downstream agents.

**Phase 1 remaining**: 1.3 (Agent-1), 1.4 (Agent-1, in review), 1.5 (Agent-1), 1.16 (Agent-4), 1.24 (Agent-5), 1.25–1.27 (Agent-6)

### Phase 2: Analysis & UI (Weeks 6–11)

| # | Milestone | Owner | Status | Unblocks |
|---|-----------|-------|--------|----------|
| 2.1 | GST (O'Brien-Fleming + Pocock) | Agent-4 | ⚪ | — |
| 2.2 | Bootstrap CI | Agent-4 | ⚪ | — |
| 2.3 | Multiple comparison correction (BH-FDR) | Agent-4 | ⚪ | — |
| 2.4 | Novelty/primacy analysis | Agent-4 | ⚪ | Agent-6 (novelty tab) |
| 2.5 | Interference analysis | Agent-4 | ⚪ | Agent-6 (interference tab) |
| 2.6 | Interleaving analysis (Team Draft scoring) | Agent-4 | ⚪ | Agent-6 (interleaving tab) |
| 2.7 | GetInterleavedList RPC (Team Draft) | Agent-1 | ⚪ | — |
| 2.8 | Results dashboard (treatment effects, CI chart, sequential boundary) | Agent-6 | ⚪ | Stakeholder demo |
| 2.9 | Notebook export (.ipynb from query log) | Agent-6 | ⚪ | — |
| 2.10 | Surrogate metric framework (M3 + M4a) | Agent-3/4 | ⚪ | — |
| 2.11 | SVOD-specific metrics (QoE, lifecycle, content consumption) | Agent-3 | 🔵 | Agent-4 (interference, novelty), Agent-6 (QoE dashboard) |

### Phase 3: SVOD-Native + Bandits (Weeks 10–17)

| # | Milestone | Owner | Status | Unblocks |
|---|-----------|-------|--------|----------|
| 3.1 | LinUCB contextual bandit | Agent-4 | ⚪ | — |
| 3.2 | Content cold-start bandit | Agent-4 | ⚪ | — |
| 3.3 | Bandit dashboard (arm allocation, reward curves) | Agent-6 | ⚪ | — |
| 3.4 | Session-level experiment support (full pipeline) | Agent-1/2/3 | ⚪ | — |
| 3.5 | Playback QoE experiment pipeline | Agent-2/3 | ⚪ | — |
| 3.6 | Cumulative holdout support | Agent-5 | ⚪ | — |

### Phase 4: Advanced & Polish (Weeks 16–22)

| # | Milestone | Owner | Status | Unblocks |
|---|-----------|-------|--------|----------|
| 4.1 | CATE heterogeneous treatment effects | Agent-4 | ⚪ | — |
| 4.2 | Interference detection (content catalog spillover) | Agent-4 | ⚪ | — |
| 4.3 | PGO-optimized builds for M1 + M4b | Agent-1/4 | ⚪ | — |
| 4.4 | Full RBAC integration | Agent-5 | ⚪ | — |
| 4.5 | End-to-end chaos testing passing | All | ⚪ | Production readiness |

## Pair Integration Schedule

Track integration test results between agent pairs.

| Week | Pair | Status | Notes |
|------|------|--------|-------|
| 3 | Agent-5 ↔ Agent-6 (management API + UI) | 🟡 | Agent-5 CRUD ready. Agent-6 can start live integration. |
| 3 | Agent-1 ↔ Agent-5 (config streaming) | 🟡 | M5 StreamConfigUpdates ready (PR #15). Agent-1 can subscribe. |
| 4 | Agent-2 ↔ Agent-3 (event pipeline → metrics) | ⚪ | — |
| 4 | Agent-1 ↔ Agent-7 (hash parity via CGo) | 🟢 | CGo bridge parity confirmed — 10K vectors. Justfile target: `test-flags-cgo`. |
| 5 | Agent-3 ↔ Agent-4 (metric summaries → analysis) | ⚪ | — |
| 5 | Agent-5 ↔ Agent-3 (guardrail alerts → auto-pause) | 🟡 | Both sides ready (M3 PR #16, M5 PR #18). Needs Kafka for live test. |
| 6 | Agent-1 ↔ Agent-4 (bandit delegation: assignment → SelectArm) | ⚪ | — |
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
- [x] M1.1 Hash WASM + FFI (Agent-1, PR #4)
- [x] M1.2 GetAssignment RPC (Agent-1, PR #11)
- [x] M1.6–1.9 Event pipeline complete (Agent-2, PRs #1, #8)
- [x] M1.10–1.13 Metric computation + guardrails complete (Agent-3, PRs #3, #5, #9, #16)
- [x] M1.14–1.15, 1.17–1.19 Analysis + bandit core (Agent-4, PRs #2, #14)
- [x] M1.20–1.23 Management service core (Agent-5, PRs #7, #10, #15, #18)
- [x] M1.28–1.30 Flag service complete (Agent-7, PR #13)

**In progress:**
- Agent-1: targeting rule evaluation (1.4, PR #21 open)
- Agent-4: mSPRT sequential testing (1.16, unblocked)
- Agent-5: metric definition CRUD (1.24, ready to start)

**Unblocked this week:**
- Agent-1 unblocked for 1.3 (config cache) by Agent-5 StreamConfigUpdates (PR #15)
- Agent-6 unblocked for 1.25 (experiment UI) by Agent-5 CRUD (M1.20)
- Agent-7 unblocked for live PromoteToExperiment by Agent-5 CRUD (M1.20)

**Risks:**
- Agent-6 has not started Phase 1 work — may become critical path for stakeholder demo
