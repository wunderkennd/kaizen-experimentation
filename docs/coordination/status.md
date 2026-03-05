# Experimentation Platform — Coordination Status

> **Last updated**: 2026-03-05 by Agent-5 (M1.24 metric definition CRUD PR #24)
>
> This file is the single source of truth for multi-agent execution state.
> Update it each time a milestone merges to `main` or a blocker is identified.

## Active Phase

**Phase 1: Foundation (Weeks 2–7)**

## Agent Status

| Agent | Module | Status | Current Branch | Current Milestone | Blocked By | Notes |
|-------|--------|--------|----------------|-------------------|------------|-------|
| Agent-1 | M1 Assignment | 🔵 In Progress | agent-1/feat/get-assignment-rpc | Config cache (1.3) | — | M1.1 (PR #4), M1.2 complete. Next: config cache. |
| Agent-2 | M2 Pipeline | 🔵 In Progress | agent-2/feat/go-orchestration-querylog | Go orchestration + SQL query logging (1.9) | — | M1.6+1.7+1.8 merged (PR #1). PR #8 open for M1.9. |
| Agent-3 | M3 Metrics | 🔵 In Progress | agent-3/feat/cuped-covariate | CUPED covariate computation (1.12) | — | M1.10 (PR #3), M1.11 (PR #5) merged. PR #9 open for M1.12. |
| Agent-4 | M4a Analysis + M4b Bandit | 🔵 In Progress | agent-4/feat/golden-validation-lmax-core | t-test + SRM + Thompson + LMAX + RocksDB (1.14–1.19) | — | PR #2 open. CI unblocked by PR #6 merge. |
| Agent-5 | M5 Management | 🔵 In Progress | agent-5/feat/metric-definition-crud | Metric definition CRUD (1.24) | — | M1.20–1.23 merged. PR #24 open for 1.24. All Phase 1 milestones addressed. |
| Agent-6 | M6 UI | 🟡 Not Started | — | Experiment list + detail shell (1.25) | — | Unblocked by M1.20 merge. Can start with MSW mocks. |
| Agent-7 | M7 Flags | 🔵 In Progress | agent-7/feat/flag-service | PR open: CRUD + eval + CGo bridge + audit (1.28–1.30) | — | Phase 1+2 complete. 10K hash vectors pass. CGo bridge parity confirmed. |

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
| 1.2 | GetAssignment RPC (static bucketing) | Agent-1 | 🟢 | — | 2026-03-04 | SDKs, Agent-6 (debug view) |
| 1.3 | Config cache (subscribe to M5 StreamConfigUpdates) | Agent-1 | ⚪ | — | — | — |
| 1.4 | Targeting rule evaluation | Agent-1 | 🟡 | — | — | — |
| 1.5 | Layer-aware + session-level assignment | Agent-1 | 🟡 | — | — | — |
| **1.6** | **IngestExposure + IngestMetricEvent RPCs** | Agent-2 | 🟢 | PR #1 | 2026-03-04 | Agent-3 (events to compute) |
| 1.7 | IngestRewardEvent + IngestQoEEvent RPCs | Agent-2 | 🟢 | PR #1 | 2026-03-04 | Agent-4 M4b (rewards) |
| 1.8 | Bloom filter dedup (0.1% FPR at 100M/day) | Agent-2 | 🟢 | PR #1 | 2026-03-04 | Rotating filter + Prometheus metrics |
| 1.9 | Go orchestration + SQL query logging | Agent-2 | 🔵 | PR #8 | — | In review |
| **1.10** | **Standard metric computation (MEAN, PROPORTION, COUNT)** | Agent-3 | 🟢 | PR #3 | 2026-03-04 | Agent-4 M4a |
| 1.11 | RATIO metric with delta method inputs | Agent-3 | 🟢 | PR #5 | 2026-03-04 | Delta method inputs for M4a |
| 1.12 | CUPED covariate computation | Agent-3 | 🔵 | PR #9 | — | In review |
| 1.13 | Guardrail breach detection → guardrail_alerts topic | Agent-3 | ⚪ | — | — | Agent-5 (auto-pause) |
| **1.14** | **Welch t-test + SRM check (golden-file validated)** | Agent-4 | 🔵 | PR #2 | — | Agent-6 (results page). In review. |
| 1.15 | CUPED variance reduction | Agent-4 | ⚪ | — | — | — |
| 1.16 | mSPRT sequential testing | Agent-4 | ⚪ | — | — | — |
| 1.17 | Thompson Sampling with Beta-Bernoulli (M4b) | Agent-4 | 🔵 | PR #2 | — | Agent-1 (SelectArm). In review. |
| 1.18 | LMAX single-threaded policy core (M4b) | Agent-4 | 🔵 | PR #2 | — | In review. |
| 1.19 | RocksDB policy snapshots (M4b) | Agent-4 | 🔵 | PR #2 | — | In review. |
| **1.20** | **Experiment CRUD + state machine enforcement** | Agent-5 | 🟢 | agent-5/feat/experiment-crud-handlers | 2026-03-04 | Agent-6 (list/detail), Agent-1 (configs), Agent-3 (experiment list) |
| 1.21 | Layer allocation + bucket reuse | Agent-5 | 🟢 | PR #7, #10 | 2026-03-04 | Merged |
| 1.22 | StreamConfigUpdates RPC | Agent-5 | 🟢 | PR #15 | 2026-03-04 | Merged. Unblocks Agent-1 config cache. |
| 1.23 | Guardrail alert consumer → auto-pause | Agent-5 | 🟢 | PR #18 | 2026-03-05 | Merged. Kafka consumer + processor per ADR-008. |
| 1.24 | Metric definition CRUD | Agent-5 | 🔵 | PR #24 | — | Create/Get/List RPCs. 14 unit + 12 integration tests. |
| **1.25** | **Experiment list + detail shell (MSW mocked)** | Agent-6 | 🟡 | — | — | Stakeholder demo. Unblocked by M1.20. Ready to start. |
| 1.26 | State indicator component (color-coded lifecycle) | Agent-6 | ⚪ | — | — | — |
| 1.27 | View SQL page (query log from M3) | Agent-6 | ⚪ | — | — | — |
| **1.28** | **Boolean flag CRUD + CGo hash bridge** | Agent-7 | 🟢 | PR #13 | — | CRUD + EvaluateFlag + CGo bridge. 10K hash vectors match. |
| 1.29 | Percentage rollout (monotonic) | Agent-7 | 🟢 | PR #13 | — | Monotonic rollout via hash bucketing. Tests confirm no user eviction. |
| 1.30 | PromoteToExperiment → M5 CreateExperiment | Agent-7 | 🟢 | PR #13 | — | Mocked — awaiting Agent-5 CRUD API. Audit trail included. |

**Bold** = critical path milestones that unblock downstream agents.

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
| 2.11 | SVOD-specific metrics (QoE, lifecycle, content consumption) | Agent-3 | ⚪ | — |

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
| 3 | Agent-5 ↔ Agent-6 (management API + UI) | ⚪ | — |
| 3 | Agent-1 ↔ Agent-5 (config streaming) | ⚪ | — |
| 4 | Agent-2 ↔ Agent-3 (event pipeline → metrics) | ⚪ | — |
| 4 | Agent-1 ↔ Agent-7 (hash parity via CGo) | ⚪ | — |
| 5 | Agent-3 ↔ Agent-4 (metric summaries → analysis) | ⚪ | — |
| 5 | Agent-5 ↔ Agent-3 (guardrail alerts → auto-pause) | ⚪ | — |
| 6 | Agent-1 ↔ Agent-4 (bandit delegation: assignment → SelectArm) | ⚪ | — |
| 6 | Agent-4 ↔ Agent-6 (analysis results → UI rendering) | ⚪ | — |

## Contract Changes Log

Track any changes to proto schemas, shared crate APIs, or database schemas.

| Date | Agent | Change | Affected Agents | ADR | Status |
|------|-------|--------|-----------------|-----|--------|
| — | — | — | — | — | — |

## Blockers & Escalations

| Date | Blocker | Raised By | Blocking | Severity | Resolution | Resolved |
|------|---------|-----------|----------|----------|------------|----------|
| — | — | — | — | — | — | — |

## Weekly Checkpoint Template

Copy this for each weekly status update:

```
### Week N — YYYY-MM-DD

**Completed this week:**
- [ ] Milestone X.Y merged (Agent-N)

**In progress:**
- Agent-N: working on milestone X.Y, ETA [date]

**Blocked:**
- Agent-N blocked on [dependency], workaround: [description]

**Unblocked this week:**
- Agent-N unblocked by milestone X.Y merge

**Integration tests:**
- Agent-X ↔ Agent-Y: [pass/fail/not run]

**Risks:**
- [any timeline concerns]

**Decisions made:**
- [any ADRs created or contracts changed]
```
