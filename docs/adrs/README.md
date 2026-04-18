# Architecture Decision Records

This directory contains the architectural decisions that shaped the experimentation platform. Each ADR documents a significant technical choice, the alternatives considered, and the consequences. Settled decisions (Accepted) should not be relitigated without strong new evidence. Proposed decisions are open for review.

## Decision Index

| ADR | Decision | Status | Impact |
|-----|----------|--------|--------|
| [001](001-language-selection.md) | Rust for hot paths, Go for orchestration, TypeScript for UI only | Accepted | All modules |
| [002](002-lmax-bandit-core.md) | LMAX-inspired single-threaded core for bandit policy | Accepted | M4b |
| [003](003-rocksdb-policy-state.md) | RocksDB for bandit policy crash-only state | Accepted | M4b |
| [004](004-gst-alongside-msprt.md) | Group Sequential Tests alongside mSPRT | Accepted | M4a, M5, M6 |
| [005](005-component-state-machine.md) | Transitional states (STARTING, CONCLUDING) in experiment lifecycle | Accepted | M1, M4a, M5, M6 |
| [006](006-cargo-workspace.md) | Cargo workspace with 13 crates across 4 layers | Accepted | All Rust |
| [007](007-sdk-provider-abstraction.md) | SDK provider abstraction with fallback chain | Accepted | SDKs, M1 |
| [008](008-auto-pause-guardrails.md) | Auto-pause as default guardrail behavior | Accepted | M3, M5 |
| [009](009-bucket-reuse.md) | Automated bucket reuse with 24h cooldown | Accepted | M1, M5 |
| [010](010-connectrpc.md) | ConnectRPC for Go, tonic for Rust, shared proto contracts | Accepted | All modules |
| [011](011-multi-objective-bandit-reward.md) | Multi-objective reward composition for bandit policies | **Proposed** | M4b, M5, M6 |
| [012](012-constrained-arm-selection-lp.md) | Constrained arm selection via LP post-processing layer | **Proposed** | M4b, M1, M5, M6 |
| [013](013-meta-experiments-objective-functions.md) | Meta-experiments on objective function parameters | **Proposed** | M1, M4a, M4b, M5, M6 |
| [014](014-provider-side-metrics.md) | Provider-side metrics as first-class experiment measures | **Proposed** | M3, M4a, M5, M6 |
| [015](015-anytime-valid-regression-adjustment.md) | Anytime-valid regression adjustment (sequential CUPED) | **Proposed** | M4a, M3, M5, M6 |
| [016](016-slate-bandit-optimization.md) | Slate-level bandit optimization and off-policy evaluation | **Proposed** | M4b, M4a, M1 |
| [017](017-offline-rl-long-term-effects.md) | Offline RL for long-term causal effect estimation | **Proposed** | M4a, M3 |
| [018](018-e-value-framework-online-fdr.md) | E-value framework and online FDR control | **Proposed** | M4a, M4b, M5 |
| [019](019-portfolio-experiment-optimization.md) | Portfolio-level experiment program optimization | **Proposed** | M5, M4a, M6 |
| [020](020-adaptive-sample-size-recalculation.md) | Adaptive sample size recalculation via promising-zone designs | **Proposed** | M4a, M5, M6 |
| [021](021-feedback-loop-interference.md) | Feedback loop interference detection and mitigation | **Proposed** | M4a, M2, M3, M5, M6 |
| [022](022-switchback-experiment-designs.md) | Switchback experiment designs for interference-prone treatments | **Proposed** | M1, M4a, M5, M6 |
| [023](023-synthetic-control-methods.md) | Synthetic control methods for quasi-experimental evaluation | **Proposed** | M4a, M3, M5, M6 |
| [024](024-m7-rust-port.md) | Port M7 Feature Flag Service from Go to Rust | **Proposed** | M7, CI, SDKs |
| [025](025-m5-conditional-rust-port.md) | Conditional port of M5 Management Service from Go to Rust | **Proposed (conditional)** | M5 |

## Proposed ADR Clusters

The 15 proposed ADRs (011–025) group into six clusters. Clusters A–E correspond to the 2024–2026 Gap Analysis capability themes. Cluster F addresses language boundary simplification motivated by the expanding Go→Rust computation surface in Clusters B and E.

### Cluster A: Multi-Stakeholder Optimization (011–014)

Enables Kaizen to balance subscriber engagement, content provider fairness, and platform economics.

```
ADR-014 (Provider Metrics)          ADR-011 (Multi-Objective Reward)
    │  measurement foundation            │  reward composition
    │                                    │
    ├──────────────┐    ┌────────────────┤
    │              ▼    ▼                │
    │        ADR-012 (LP Constraints)    │
    │              │                     │
    │              ▼                     │
    │        ADR-013 (Meta-Experiments)  │
    │              │                     │
    └──────────────┘                     │
```

**Implementation order**: ADR-014 → ADR-011 (parallel) → ADR-012 → ADR-013

### Cluster B: Statistical Methodology (015, 018, 020)

Closes the core inference gaps — sequential variance reduction, e-value framework, adaptive power.

```
ADR-015 (AVLM / Sequential CUPED)
    │  regression-adjusted CIs
    │
    ├──────────────────────────────────────┐
    ▼                                      ▼
ADR-020 (Adaptive Sample Size)      ADR-018 (E-Values / Online FDR)
    uses AVLM variance estimate          parallel inference track
```

**Implementation order**: ADR-015 first (#1 ROI item) → ADR-018 and ADR-020 in parallel

### Cluster C: Bandit & RL Advances (016, 017)

Extends bandits to combinatorial slate actions and corrects the surrogate paradigm for continual treatments.

```
ADR-016 (Slate Bandits)            ADR-017 (Offline RL / Surrogates)
    extends ADR-002 LMAX               corrects surrogate calibration
    complements ADR-011/012            standalone, foundational
```

**Implementation order**: ADR-017 first (corrects a theoretical error in existing surrogates) → ADR-016

### Cluster D: Quasi-Experimental Designs (022, 023)

Enables experimentation on interventions that cannot be user-level randomized.

```
ADR-022 (Switchback)               ADR-023 (Synthetic Control)
    temporal randomization              observational counterfactual
    new experiment type                 new experiment type
```

**Implementation order**: Independent; either can go first. ADR-022 is more operationally complex (requires M1 assignment changes); ADR-023 is analysis-only.

### Cluster E: Platform Operations (019, 021)

Portfolio-level optimization and feedback loop detection.

```
ADR-019 (Portfolio Optimization)
    │  depends on ADR-017 (annualization), ADR-018 (FDR)
    │
ADR-021 (Feedback Loop Interference)
    │  extends existing interference detection
    │  standalone
```

**Implementation order**: ADR-021 first (standalone, immediate value) → ADR-019 (needs 017/018)

### Cluster F: Language Migration (024, 025)

Eliminates the Go→Rust FFI boundary and simplifies the build toolchain. ADR-024 is unconditional; ADR-025 triggers only when the statistical computation surface in M5 exceeds what Go→M4a RPCs can efficiently support.

```
ADR-024 (M7 Rust Port)
    │  unconditional — eliminates experimentation-ffi crate
    │  deletes CGo bridge, cbindgen, 10K-vector parity test
    │
ADR-025 (M5 Conditional Rust Port)
    │  trigger: >= 3 of {ADR-015 P2, ADR-018, ADR-019, ADR-020, ADR-021}
    │  enables direct experimentation-stats import (no RPC for power/e-value/etc)
    │
    Post-migration language surface:
      Rust: M1, M2-ingest, M4a, M4b, M7, (M5 if triggered)
      Go:   M2-orch, M3, (M5 if not triggered)
      TS:   M6
```

**Implementation order**: ADR-024 in Sprint 5.0–5.1 (standalone, ~3 weeks). ADR-025 evaluated at end of Sprint 5.5; execution is a separate Sprint 5.6 if triggered.

## Recommended Global Implementation Sequence

Considering dependencies, ROI, complexity, and language migration:

| Priority | ADR(s) | Rationale |
|----------|--------|-----------|
| **P0** | 015 (AVLM) | #1 ROI gap: unifies CUPED + sequential monitoring |
| **P0** | 017 (ORL Phase 1: TC/JIVE) | Fixes a theoretical error in production surrogates |
| **P0** | 024 (M7 Rust port) | Eliminates FFI crate; standalone; ~3 weeks |
| **P1** | 014 (Provider Metrics) | Measurement foundation for multi-stakeholder cluster |
| **P1** | 018 (E-Values Phase 1) | Parallel e-value computation; no breaking changes |
| **P1** | 021 (Feedback Loops) | Standalone detection, immediate diagnostic value |
| **P2** | 011 (Multi-Objective Reward) | Enables multi-stakeholder bandit optimization |
| **P2** | 020 (Adaptive Sample Size) | Layers on 015; reduces wasted traffic |
| **P2** | 023 (Synthetic Control) | Analysis-only; no assignment changes |
| **P3** | 012 (LP Constraints) | Depends on 011 + 014 |
| **P3** | 022 (Switchback) | New experiment type; significant M1 changes |
| **P3** | 016 (Slate Bandits) | Large scope; extends M4b core |
| **P4** | 013 (Meta-Experiments) | Depends on 011 + 012 |
| **P4** | 019 (Portfolio) | Depends on 017 + 018; needs historical data |
| **P4** | 017 Phase 2 (Full ORL/MDP) | Most complex; requires trajectory data |
| **P4** | 018 Phase 3 (MAD) | Requires M4b changes for bandit randomization mixing |
| **P4** | 015 Phases 2–3 (MLRATE, in-experiment covariates) | Requires M3 model training infrastructure |
| **Conditional** | 025 (M5 Rust port) | Trigger: ≥ 3 of {015 P2, 018, 019, 020, 021} committed |

## Cross-Cutting Dependencies on Existing ADRs

| Proposed ADR | Depends On (Existing) | Relationship |
|-------------|----------------------|--------------|
| 011 | ADR-002 (LMAX core) | Extends policy core reward computation |
| 012 | ADR-002, ADR-003 | LP layer runs on LMAX thread; state in RocksDB |
| 013 | ADR-005 (state machine) | New EXPERIMENT_TYPE_META lifecycle |
| 015 | ADR-004 (GST/mSPRT) | Subsumes both into unified AVLM framework |
| 016 | ADR-002 (LMAX core) | Extends to slate-level policy state |
| 018 | ADR-004 (GST/mSPRT) | Partially supersedes p-value sequential tests |
| 020 | ADR-004, ADR-015 | Layers on GST spending; uses AVLM variance |
| 024 | ADR-001 (language selection), ADR-010 (ConnectRPC) | Narrows Go surface; tonic-web replaces connect-go for M7 |
| 025 | ADR-001, ADR-010, ADR-024 | Further narrows Go surface; depends on 024 completing FFI elimination first |

## Cross-Dependencies Between Proposed ADRs

| ADR | Depends On (Proposed) | Nature |
|-----|----------------------|--------|
| 012 | 011, 014 | LP layer uses multi-objective reward; provider metrics as constraint inputs |
| 013 | 011, 012 | Meta-experiments test objective parameterizations from 011; LP constraints from 012 |
| 019 | 017, 018 | Portfolio uses annualization from ORL; FDR control from e-values |
| 020 | 015 | Adaptive N uses AVLM regression-adjusted variance for conditional power |
| 025 | 024 | M5 port assumes FFI crate already eliminated by M7 port |

## Agent Ownership

Each proposed ADR has a primary owning agent and supporting agents. Full responsibilities are documented in the Multiclaude agent definitions at `.multiclaude/agents/`.

| ADR | Primary Agent | Supporting Agents |
|-----|--------------|-------------------|
| 011 | Agent-4 (M4b) | Agent-5 (validation), Agent-6 (UI) |
| 012 | Agent-4 (M4b) | Agent-1 (serving integration), Agent-5 (validation), Agent-6 (UI) |
| 013 | Agent-5 (M5) | Agent-1 (routing), Agent-4 (analysis + policy isolation), Agent-6 (UI) |
| 014 | Agent-3 (M3) | Agent-4 (guardrail beta-correction), Agent-5 (validation), Agent-6 (UI) |
| 015 | Agent-4 (M4a) | Agent-3 (MLRATE P2), Agent-5 (config), Agent-6 (UI) |
| 016 | Agent-4 (M4b) | Agent-1 (GetSlateAssignment RPC), Agent-6 (UI) |
| 017 | Agent-4 (M4a) | Agent-3 (user_trajectories table) |
| 018 | Agent-4 (M4a) | Agent-5 (OnlineFdrController), Agent-6 (UI) |
| 019 | Agent-5 (M5) | Agent-4 (power analysis), Agent-6 (UI) |
| 020 | Agent-4 + Agent-5 | Agent-6 (UI) |
| 021 | Agent-4 (M4a) | Agent-2 (ingestion), Agent-3 (contamination SQL), Agent-5 (alerting), Agent-6 (UI) |
| 022 | Agent-4 (M4a) + Agent-1 (M1) | Agent-3 (block aggregation), Agent-5 (lifecycle), Agent-6 (UI) |
| 023 | Agent-4 (M4a) | Agent-3 (panel data), Agent-5 (lifecycle), Agent-6 (UI) |
| 024 | Agent-7 (M7) | Agent-6 (wire-format tests), Agent-1 (SDK fallback verification) |
| 025 | Agent-5 (M5) | Agent-4 (stats crate API surface) |

## Related

- [CI/CD Pipeline Design](cicd-pipeline-design.md) — build pipeline architecture for the multi-language workspace
- [Design Document v7.0](../design/design_doc_v7.0.md) — post-implementation reference and Phase 5 architecture plan
- [2024–2026 Experimentation Gap Analysis](../research/gap_analysis.md) — research report that motivated ADRs 011–023
- [Phase 5 Implementation Plan](../coordination/phase5-implementation-plan.md) — sprint plan, milestones, and agent assignments
- [Phase 5 Playbook](../coordination/phase5-playbook.md) — hybrid Multiclaude + Agent Teams operational guide
- [Sprint Prompt Templates](../coordination/sprint-prompts.md) — pre-written worker create commands per sprint
