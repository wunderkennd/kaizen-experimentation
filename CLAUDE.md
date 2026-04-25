# Kaizen Experimentation Platform

## Project Overview

Full-stack SVOD experimentation platform. 7 modules, 3 languages (Rust/Go/TypeScript), 13 Rust crates, Protobuf schema-first. Phases 0–5 complete (204 PRs, 10 pair integration suites green, 41 Phase 5 PRs across 6 sprints). Current work:

- **Sprint 5.6**: ADR-026 (custom metrics layer) and ADR-027 (TOST equivalence testing) — both Proposed.
- **Infrastructure sprints (I.0 / I.1 / I.2)**: Pulumi + Go IaC on AWS — ECR repos, services, wiring, observability, mock test suites.
- **QoE stream**: EBVS detection as first-class `PlaybackMetrics`, server-side `HeartbeatSessionizer` for QoE aggregation.
- **Palette UI polish**: Standardized search, empty states, filter clearing, CopyButton, accessibility improvements across M6.

## Architecture

| Module | Language | Owner | Port | Purpose |
| --- | --- | --- | --- | --- |
| M1 Assignment | Rust | Agent-1 | 50051 | Variant allocation, interleaving, bandit arm delegation, SDKs |
| M2 Pipeline | Rust+Go | Agent-2 | 50052 (ingest) / 50058 (orch) | Event validation/dedup/Kafka (Rust ingest), orchestration (Go) |
| M3 Metrics | Go | Agent-3 | 50056 | Spark SQL orchestration, metric computation, Delta Lake |
| M4a Analysis | Rust | Agent-4 | 50053 | All statistical computation (experimentation-stats crate) |
| M4b Bandit | Rust | Agent-4 | 50054 | Thompson, LinUCB, Neural (Candle), LMAX single-thread core |
| M5 Management | Rust (ADR-025 executed) + Go variant retained | Agent-5 | 50055 | CRUD, lifecycle, RBAC, guardrails, bucket reuse, portfolio, adaptive-N scheduler |
| M6 UI | TypeScript | Agent-6 | 3000 | Next.js 14, React 18, Recharts, D3, shadcn/ui |
| M7 Flags | Rust (ADR-024 shipped) | Agent-7 | 50057 | Feature flags, percentage rollout, reconciler |

## Cargo Workspace

13 crates. `experimentation-ffi` was deleted when ADR-024 (M7 Rust port) shipped; do not reintroduce it.

```
crates/
  experimentation-core/          # Timestamps, errors, tracing, assert_finite!()
  experimentation-hash/          # MurmurHash3, bucketing
  experimentation-proto/         # tonic-build generated from proto/
  experimentation-stats/         # All statistical methods — incl. avlm, evalue, orl (TC/JIVE),
                                 #   switchback, synthetic_control, adaptive_n, feedback_loop,
                                 #   portfolio, interference, tost (ADR-027 in progress)
  experimentation-bandit/        # Thompson, LinUCB, Neural, cold_start, slate, lp_constraints,
                                 #   reward_composer, mad (e-processes)
  experimentation-interleaving/  # Team Draft, Optimized, Multileave
  experimentation-ingest/        # Event validation, Bloom filter dedup, HeartbeatSessionizer
  experimentation-assignment/    # M1 service binary
  experimentation-analysis/      # M4a service binary
  experimentation-pipeline/      # M2 service binary (Rust half)
  experimentation-policy/        # M4b service binary
  experimentation-flags/         # M7 service binary (ADR-024)
  experimentation-management/    # M5 service binary (ADR-025) — store, state_machine, portfolio,
                                 #   validators, bucket_reuse, kafka, contract_test_support
```

### SDKs (`sdks/`)

| SDK | Language | Purpose |
| --- | --- | --- |
| `sdks/server-go` | Go | Server-side assignment + hash parity (MurmurHash3 pure-Go) |
| `sdks/server-python` | Python | Server-side assignment, batch evaluation |
| `sdks/android` | Kotlin | Mobile client, ConnectRPC |
| `sdks/ios` | Swift (SwiftPM) | Mobile client, ConnectRPC |
| `sdks/web` | TypeScript | Browser client (separate from M6 UI) |

Hash parity across SDKs is enforced by `test-vectors/hash_vectors.json` and verified by `just test-hash`.

## Critical Rules

- **Schema-first**: All interfaces defined in Protobuf. Run `buf lint` and `buf breaking` before committing proto changes.
- **Fail-fast**: Every floating-point path uses `assert_finite!()` from experimentation-core.
- **No statistical computation in Go or TypeScript.** All math lives in experimentation-stats (Rust).
- **TypeScript is UI only.** M6 never performs metric computation, bandit evaluation, or statistical analysis.
- **Golden-file validation**: Every new statistical method requires golden files validated against reference R/Python packages to 4+ decimal places.
- **Proptest invariants**: Every public function in experimentation-stats gets proptest invariants. Nightly CI runs 10K cases.
- **Contract tests**: Cross-module interfaces require wire-format contract tests. The consumer agent writes the test.
- **Conventional commits**: `feat(crate):`, `fix(crate):`, `test(crate):`, `docs:`, `chore:`.
- **Branch naming**: `agent-N/feat/adr-XXX-description`, `agent-N/fix/...`, `agent-N/port/...`. Never use auto-generated worker names as branch names.
- **Work tracking**: All work tracked via GitHub Issues. PRs reference issues with `Closes #N`. Check your assigned issues with `gh issue list --assignee @me`.

## Phase 5 Status — COMPLETE

All 15 Phase 5 ADRs (011–025) shipped across sprints 5.0–5.5 (41 PRs merged, 2026-04-06).

| Cluster | ADRs | Status |
| --- | --- | --- |
| A: Multi-Stakeholder | 011, 012, 013, 014 | Shipped — multi-objective reward, LP constraints, meta-experiments, provider metrics |
| B: Statistical Methods | 015, 018, 020 | Shipped — AVLM (sequential CUPED), e-values + online FDR, adaptive sample size |
| C: Bandit & RL | 016, 017 | Shipped — slate bandits, offline RL with TC/JIVE surrogate fix, doubly-robust OPE |
| D: Quasi-Experimental | 022, 023 | Shipped — switchback, synthetic control |
| E: Platform Operations | 019, 021 | Shipped — portfolio optimization, feedback loop interference |
| F: Language Migration | 024, 025 | 024 shipped (`experimentation-ffi` deleted); 025 executed (`experimentation-management` crate landed) |

See `docs/coordination/phase5-implementation-plan.md` and `docs/coordination/CHANGELOG-phase5.md` for per-sprint detail.

## Active Work (Post-Phase 5)

**New ADRs (Proposed)**:
- **ADR-026** (`docs/adrs/026-custom-metrics-layer.md`) — Custom metrics definition layer (composite / derived / joined metrics beyond the six built-in types). Impact: M5, M3, M4a.
- **ADR-027** (`docs/adrs/027-tost-equivalence-testing.md`) — Two One-Sided Tests for proving equivalence (infra migrations, refactor validation). Impact: M4a, M5, M6. Core impl landed (#443); see `crates/experimentation-stats/src/tost.rs`.

**Infrastructure sprint (Pulumi + Go on AWS)**: `infra/` contains Pulumi stacks (`Pulumi.{dev,staging,prod}.yaml`) and a full Go test suite (`fullstack_test.go`). Sprint I.0 (all 13 modules) and I.1/I.2 (wiring + hardening) merged; ECR repos exist for all 9 Kaizen services.

**QoE stream**: `ebvs_detected` field on `PlaybackMetrics` and `HeartbeatSessionizer` in `experimentation-ingest` deliver server-side QoE aggregation. Specs in `docs/issues/ebvs-detection.md`, `docs/issues/heartbeat-sessionization.md`.

**Palette UI polish**: Ongoing standardization of search, empty states, filter clearing, accessibility, and CopyButton usage in M6. Look for commits prefixed `🎨 Palette:`.

**Current sprint**: Check the active GitHub Milestone:
```bash
gh issue list --milestone "Sprint 5.6" --state open       # ADR-026/027 stream
gh issue list --milestone "Sprint I.2" --state open       # infrastructure stream
```

## Work Tracking

Work is tracked in **GitHub Issues**, not markdown status files.

```
Milestone    =  Sprint (e.g., "Sprint 5.0: Schema & Foundations")
  └── Issue  =  ADR implementation unit (e.g., "ADR-015: AVLM Implementation")
```

### For agents: how to find your work
```bash
# What's assigned to me?
gh issue list --assignee @me --state open

# What's in the current sprint?
gh issue list --milestone "Sprint 5.0: Schema & Foundations"

# What's blocked?
gh issue list --label "blocked"

# Read a task spec
gh issue view <number>
```

### For agents: how to update progress
- Comment on the issue with progress updates
- When creating a PR, include `Closes #<issue-number>` in the PR description
- The issue auto-closes when the PR merges
- If blocked, add the `blocked` label and comment explaining what you're waiting on

### Labels
- `agent-1` through `agent-7` — ownership
- `P0` through `P4` — priority
- `cluster-a` through `cluster-f` — capability cluster
- `blocked` — waiting on another issue/agent
- `contract-test` — cross-module contract test

## Orchestration Model

Phase 5 uses a multi-tool orchestration model. Each tool has a specific role.

| Tool | Role | When |
| --- | --- | --- |
| **Gas Town** | Interactive parallel work — Mayor coordinates polecats, you steer in real time | Daytime active sessions |
| **Multiclaude** | Autonomous overnight grinding — daemon, CI-gated merge queue, self-healing workers | You're away (overnight, weekends) |
| **Jules** | CI-triggered automation — scheduled maintenance, test generation, dependency bumps | Continuous / event-driven (GitHub Actions) |
| **Devin** | Bounded autonomous tasks — test coverage, migrations, repetitive refactoring | Batch dispatch for well-specified work |
| **Gemini CLI** | Quick lookups, second-opinion code review, research | Ad-hoc |
| **Claude Code** (solo) | Single focused task in an isolated worktree | Quick one-off work |

**Daily rhythm**: `just morning` (check overnight results) → `just interactive` (Gas Town) → `just evening <sprint>` (Multiclaude overnight).

**Launching workers from Issues**: Workers read their task spec directly from the GitHub Issue:
```bash
# Multiclaude worker reads Issue #42
gh issue view 42 --json body -q '.body' | multiclaude worker create "$(cat -)"

# Gas Town: tell the Mayor "Pick up Issue #42 for the kaizen rig"
```

**Agent definitions** at `.multiclaude/agents/agent-N-*.md` define *how* agents work (coding standards, contract test obligations). GitHub Issues define *what* they're working on. Infrastructure has its own set: `.multiclaude/agents/infra-{1..5}-*.md` (networking, datastores, streaming, compute, ingress/observability) for the Pulumi IaC track.

## Key File Locations

| What | Where |
| --- | --- |
| This file (agent context) | `CLAUDE.md` (repo root) |
| Design document | `docs/design/design_doc_v7.0.md` |
| ADRs (001–027) | `docs/adrs/` |
| ADR index | `docs/adrs/README.md` |
| Agent definitions (modules) | `.multiclaude/agents/agent-N-*.md` |
| Agent definitions (infra) | `.multiclaude/agents/infra-{1..5}-*.md` |
| Agent onboarding | `docs/onboarding/agent-N-*.md` (incl. `agent-0-coordination.md`) |
| Module runbooks | `docs/runbooks/m4a-analysis.md`, `docs/runbooks/m4b-policy.md` |
| Work tracking | GitHub Issues (Milestones = Sprints, Issues = Tasks) |
| Claude Code settings | `.claude/settings.json` |
| PR triage subagent | `.claude/agents/pr-triage.md` |
| Multiclaude config | `.multiclaude/config.json` |
| Proto schema | `proto/experimentation/` (subdirs: assignment, analysis, bandit, flags, management, metrics, pipeline, common) |
| SQL migrations | `sql/migrations/` |
| Test vectors (hash parity) | `test-vectors/hash_vectors.json` |
| Phase 5 plan & changelog | `docs/coordination/phase5-implementation-plan.md`, `docs/coordination/CHANGELOG-phase5.md` |
| Playbook | `docs/coordination/phase5-playbook.md` |
| Developer guides | `docs/guides/` (git-hygiene, github-issues-workflow, orchestration-workflow, pr-triage-and-cleanup, merge-conflict-resolution, gastown-setup) |
| Issue specs (QoE etc.) | `docs/issues/` |
| Infrastructure (Pulumi) | `infra/` (`main.go`, `Pulumi.{dev,staging,prod}.yaml`, `fullstack_test.go`) |
| SDKs | `sdks/{android,ios,server-go,server-python,web}/` |
| CI workflows | `.github/workflows/` (ci, nightly, nightly-loadtest, weekly-chaos, mobile-sdk, jules-*, claude-*) |
| Justfile (dev commands) | `justfile` (1000+ lines; `just --list` to discover) |

## Testing Commands

```bash
# Rust — per-crate (preferred for agents working on a single crate)
cargo test -p experimentation-stats
cargo test -p experimentation-bandit
cargo test -p experimentation-flags
cargo test -p experimentation-management

# Rust — workspace-wide (run before creating PR)
cargo test --workspace

# Go
go test ./services/metrics/...
go test ./services/management/...     # legacy Go M5; kept until deprecation
go test ./services/orchestration/...  # M2 Go orchestrator

# TypeScript
cd ui && npm test

# Proto validation
buf lint proto/
buf breaking proto/ --against .git#branch=main

# Proptest nightly (CI only — slow)
PROPTEST_CASES=10000 cargo test -p experimentation-stats

# Aggregate (via justfile)
just test            # Rust + Go + TS + hash parity + infra
just test-rust
just test-hash       # cross-SDK MurmurHash3 parity
just test-infra      # Pulumi mock suite
just lint            # proto + rust + go + ts
just fmt-check
```

## Golden-File Validation Targets

| Module | Reference | Precision |
| --- | --- | --- |
| `avlm.rs` (ADR-015) | R `avlm` package | 4 decimal places |
| `orl.rs` TC/JIVE (ADR-017) | Netflix KDD 2024 Table 2 | Reproduce results |
| `evalue.rs` (ADR-018) | Ramdas/Wang monograph examples | 6 decimal places |
| `switchback.rs` (ADR-022) | DoorDash sandwich estimator | 4 decimal places |
| `synthetic_control.rs` (ADR-023) | R `augsynth` package | 4 decimal places |
| `tost.rs` (ADR-027) | R `TOSTER` package (`tsum_TOST`) | 6 decimal places |
| `gst.rs` (existing) | R `gsDesign` + scipy | 4 decimal places |
| `ttest.rs` (existing) | R `t.test()` | 6 decimal places |

## Files That Must NOT Be Tracked in Git

Build artifacts and agent runtime state — never commit:
- `ui/tsconfig.tsbuildinfo`, `.Jules/`, `node_modules/`, `.next/`, `target/`, `dist/`
- `.claude/settings.local.json`, `.claude/worktrees/`, `.claude/teams/`, `.claude/tasks/`
- `.multiclaude/state/`, `.multiclaude/messages/`, `.multiclaude/worktrees/`, `*.pid`, `*.log`

See `.gitignore` and `docs/guides/git-hygiene.md`.
