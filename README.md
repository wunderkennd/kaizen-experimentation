# Kaizen — SVOD Experimentation Platform

A full-stack experimentation system purpose-built for streaming platforms. Supports A/B testing, interleaving, multi-armed bandits (Thompson Sampling, LinUCB, Neural Contextual), feature flags, sequential testing (mSPRT, GST), CUPED variance reduction, surrogate metrics, novelty detection, content interference analysis, lifecycle segmentation, and session-level experiments.

**Status**: Phases 0–5 complete (204 PRs, 10 pair integration suites green; all 15 Phase 5 ADRs shipped 2026-04-06). Active streams: ADR-026 custom metrics + ADR-027 TOST equivalence (sprint 5.6), Pulumi/AWS infrastructure sprints, the QoE stream, and the harness modernization (H0–H7) — orchestration consolidated onto GitHub-native primitives; see [`docs/coordination/harness-modernization-proposal.md`](docs/coordination/harness-modernization-proposal.md).

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                        Client SDKs                           │
│    Web (TS)  ·  iOS (Swift)  ·  Android (Kotlin)            │
│    Server-Go  ·  Server-Python                               │
└──────────────┬───────────────────────────────────────────────┘
               │ JSON HTTP (ConnectRPC)
               ▼
┌──────────────────────────┐    ┌──────────────────────────────┐
│  M1 Assignment (Rust)    │───▶│  M4b Bandit Policy (Rust)    │
│  :50051                  │    │  :50054                      │
│  Hash bucketing,         │    │  Thompson, LinUCB, Neural,   │
│  interleaving, slate     │    │  cold-start, LMAX core       │
└──────────────────────────┘    └──────────────────────────────┘
               │                            │
               ▼                            ▼
┌──────────────────────────┐    ┌──────────────────────────────┐
│  M2 Event Pipeline       │    │  M4a Statistical Analysis    │
│  :50052 (Rust)           │    │  :50053 (Rust)               │
│  :50058 (Go orch)        │    │  Frequentist, Bayesian,      │
│  Validation, dedup,      │    │  sequential, CUPED, CATE,    │
│  Kafka publish           │    │  IPW, interference, novelty  │
└──────────┬───────────────┘    └──────────────────────────────┘
           │ Kafka                          ▲
           ▼                                │
┌──────────────────────────┐    ┌──────────────────────────────┐
│  M3 Metric Computation   │───▶│  M5 Experiment Management    │
│  :50056 (Go)             │    │  :50055 (Go; ADR-025 → Rust) │
│  Spark SQL, Delta Lake,  │    │  CRUD, lifecycle, RBAC,      │
│  surrogates, providers   │    │  guardrails, bucket reuse    │
└──────────────────────────┘    └──────────────────────────────┘
                                            │
┌──────────────────────────┐    ┌──────────────────────────────┐
│  M7 Feature Flags        │    │  M6 Decision Support UI      │
│  :50057 (Rust)           │    │  :3000 (TypeScript)          │
│  Flags, rollout,         │    │  Next.js 14, React 18,       │
│  promote to experiment   │    │  Recharts, D3, shadcn/ui     │
└──────────────────────────┘    └──────────────────────────────┘
```

## Tech Stack

| Layer | Technology |
| --- | --- |
| Hot-path services | Rust (tonic gRPC, tonic-web JSON HTTP) |
| Orchestration services | Go (ConnectRPC) |
| UI | TypeScript (Next.js 14, React 18) |
| Schema | Protobuf (buf v2, 17 .proto files, 8 packages) |
| Streaming | Apache Kafka (MSK/Confluent, 7 topics) |
| Lakehouse | Delta Lake on S3/GCS |
| Database | PostgreSQL 16 |
| Policy Store | RocksDB (embedded, crash-only) |
| Observability | Prometheus, Grafana, Jaeger (OpenTelemetry) |

## Getting Started

### Prerequisites

- Rust 1.80+ with `cargo`
- Go 1.22+
- Node.js 20+ with `npm`
- Docker and Docker Compose
- `buf` CLI v2 (proto toolchain)
- PostgreSQL 16
- `tmux` (for multi-agent orchestration)

### Development Setup

```bash
git clone https://github.com/wunderkennd/kaizen-experimentation.git && cd kaizen-experimentation
docker compose up -d
cargo build --workspace && cargo test --workspace
go build ./... && go test ./...
cd ui && npm install && npm test && cd ..
buf lint proto/
```

## Phase 5: Architecture Evolution — SHIPPED

Phase 5 shipped all 15 ADRs (011–025) across sprints 5.0–5.5 (41 PRs, complete 2026-04-06), driven by a 2024–2026 experimentation research gap analysis:

| Cluster | ADRs | Capability |
| --- | --- | --- |
| A: Multi-Stakeholder | 011–014 | Multi-objective bandits, LP constraint layer, meta-experiments, provider-side metrics |
| B: Statistical Methods | 015, 018, 020 | AVLM (sequential CUPED), e-value framework + online FDR, adaptive sample size |
| C: Bandit & RL | 016, 017 | Slate-level bandits, offline RL for long-term causal estimation |
| D: Quasi-Experimental | 022, 023 | Switchback experiments, synthetic control methods |
| E: Platform Operations | 019, 021 | Portfolio optimization, feedback loop interference detection |
| F: Language Migration | 024, 025 | M7 Go→Rust (shipped), M5 Go→Rust (Phase 2/4 landed; RBAC + stats integration pending) |

Post-Phase-5 ADRs **026–030** (custom metrics layer, TOST equivalence testing, M4b shadow inference, cross-modal score calibration, shadow experiment mode) form the active product stream — see `CLAUDE.md` §Active Work for per-ADR status.

### Work Tracking

Work is tracked via **GitHub Issues** on a native work graph (harness phase H2; Milestones were retired 2026-07-05):

```
Iteration (Project #5)  =  Sprint — `sprint-*` labels carry it for machines
  └── Issue             =  one dispatchable unit
                           blockers  = native "blocked by" dependency edges
                           goals     = native sub-issue trees with progress bars
```

Readiness is computed from the graph, not from issue-body text: open ∧ unclaimed ∧ no open closing PR ∧ no open blocking edge (`scripts/orchestration/ready.sh`, one GraphQL query per sprint cohort).

```bash
just morning                                    # iteration status + per-cohort ready counts

gh issue list --label sprint-5.6 --state open   # current product sprint
gh issue list --label "agent-4" --state open    # by agent
gh issue list --label "blocked"                 # blocked work
```

### Development Orchestration

A multi-tool executor portfolio rides on GitHub-native coordination (the harness modernization, phases H0–H7): a **claim protocol** prevents duplicate dispatch (H1), the **dependency-edge work graph** computes readiness (H2), and **merging is platform-owned** — required status checks via the native ruleset [`.github/rulesets/main.json`](.github/rulesets/main.json) (PR title, review gate, PR-size gate, schema, rust, go, typescript, hash-parity), green non-risk PRs auto-merge, and human review is reserved for `breaking`/`contract-test`/proto-touching changes (H3/H6). The delivery lifecycle from idea to dispatch is codified with templates and advisory lints (H7).

| Tool | Role | When |
| --- | --- | --- |
| Gas Town | Interactive parallel work — Mayor + polecats | Daytime active sessions |
| Multiclaude | Autonomous overnight grinding — local daemon | Overnight / weekends — retirement path decided (H4): graduated cutover to a GitHub-native evening dispatcher |
| Jules | CI-triggered automation — maintenance, tests, deps | Continuous (GitHub Actions) |
| Devin | Bounded autonomous tasks + automatic PR review | Batch dispatch |
| Gemini CLI | Second-opinion review, research | Ad-hoc |
| Claude Code (solo / web / `@claude`) | Focused tasks, harness work, privileged operations via workflow vehicles | One-off work; H4's reference executor |

```bash
just morning              # Check overnight results, pull main
just interactive          # Start Gas Town Mayor session
just evening 5.6          # Launch sprint-5.6 Multiclaude workers overnight
just status               # Unified view across all tools
just pr-triage            # AI-assisted PR cleanup
```

The governance workflows are `workflow_call` reusables (`_review-gate.yml`, `_pr-title.yml`, `_automerge.yml`, `_pr-size.yml`) so sibling Kaizen repos run identical ~20-line callers, with per-repo rulesets stamped by Pulumi (`infra/github-governance/`, org-migration-ready for wunderkind-ventures).

See `docs/guides/orchestration-workflow.md` for the full guide and `docs/coordination/harness-modernization-proposal.md` for phase status.

## Project Structure

```
kaizen/
├── CLAUDE.md                          # Agent context (the front door — every tool reads it)
├── README.md                          # This file
├── CONTRIBUTING.md                    # Contribution guide (PR lifecycle, size policy, graduated review)
├── Cargo.toml                         # Workspace root
├── justfile                           # Task runner (1000+ lines; `just --list`)
│
├── .claude/
│   ├── settings.json                  # Project-level Claude Code settings
│   └── agents/
│       └── pr-triage.md               # PR triage subagent
│
├── .multiclaude/
│   ├── config.json                    # Multiclaude repo config
│   └── agents/                        # 12 definitions — views of docs/agents/registry/
│
├── .github/
│   ├── workflows/                     # CI/CD + governance reusables (_review-gate, _pr-title,
│   │                                  #   _automerge, _pr-size) + Jules/Claude automation
│   ├── rulesets/                      # Branch protection as data (main.json — required checks)
│   └── ISSUE_TEMPLATE/                # Issue templates for ADR work
│
├── crates/                            # Rust workspace (13 crates)
├── services/                          # Go services
├── ui/                                # M6 (Next.js 14)
├── proto/experimentation/             # Protobuf schema
├── sdks/                              # Client SDKs (5 platforms)
├── sql/migrations/                    # PostgreSQL DDL
├── delta/                             # Delta Lake table schemas
├── test-vectors/                      # Hash parity vectors (10K)
│
├── scripts/
│   ├── orchestration/                 # H1 dispatch layer: claims, native _ready, adapters
│   ├── check_okf.py · check_docs.py   # Registry + delivery-lifecycle conformance lints
│   └── generate_governance_onboarding.py  # Fleet governance file generator
│
├── infra/                             # Pulumi IaC (all 13 modules) + github-governance/ fleet stack
│
├── docs/
│   ├── design/design_doc_v7.0.md
│   ├── adrs/                          # ADRs 001–030
│   ├── agents/registry/               # Canonical agent identity (OKF v0.1 bundle)
│   ├── coordination/                  # Phase plans, playbook, harness-modernization-proposal.md
│   ├── guides/                        # Developer guides (incl. delivery-lifecycle, plan-review)
│   ├── templates/                     # PRD / RFC / UX-spec templates
│   ├── superpowers/                   # Locked plans + specs (plan template v2)
│   └── runbooks/                      # Module, operator, and ecosystem-governance runbooks
│
├── docker-compose.yml
└── docker-compose.monitoring.yml
```

## Documentation

| Document | Description |
| --- | --- |
| [Design Document v7.0](docs/design/design_doc_v7.0.md) | Complete system reference + Phase 5 architecture plan |
| [ADR Index](docs/adrs/README.md) | 30 architecture decision records |
| [Harness Modernization Proposal](docs/coordination/harness-modernization-proposal.md) | H0–H7: claim protocol, native work graph, platform merge path, executor consolidation, fleet governance, delivery codification |
| [Delivery Lifecycle](docs/guides/delivery-lifecycle.md) | Idea → PRD → RFC/ADR → spec → locked plan → plan-review → `prime-issue` → dispatch |
| [Projects & Goals](docs/guides/projects-and-goals.md) | Iterations-as-sprints, Goal sub-issue trees, native dependency edges |
| [Ecosystem Governance Runbook](docs/runbooks/ecosystem-governance.md) | Fleet onboarding, ruleset apply (POST vs PUT), wunderkind-ventures org migration |
| [Phase 5 Plan](docs/coordination/phase5-implementation-plan.md) | 6 sprints, agent assignments (shipped 2026-04-06) |
| [Orchestration Workflow](docs/guides/orchestration-workflow.md) | Multi-tool daily workflow guide |
| [Gas Town Setup](docs/guides/gastown-setup.md) | Gas Town installation and configuration |
| [GitHub Issues Workflow](docs/guides/github-issues-workflow.md) | Work tracking with Issues, labels, and the claim protocol |
| [PR Triage & Cleanup](docs/guides/pr-triage-and-cleanup.md) | Crash recovery, batch PR cleanup |
| [Merge Conflict Resolution](docs/guides/merge-conflict-resolution.md) | Per-file-type resolution strategies |
| [Git Hygiene](docs/guides/git-hygiene.md) | What to track vs. gitignore |

## Verified Performance

| Service | Metric | Target | Achieved |
| --- | --- | --- | --- |
| M1 Assignment | GetAssignment p99 | < 5ms | ✅ at 50K rps |
| M4b Bandit | SelectArm p99 | < 15ms at 10K rps | ✅ |
| M7 Flags | EvaluateFlag p99 | < 10ms at 20K rps | ✅ (< 5ms post-Rust-port) |
| All stateless | Crash recovery | < 2 seconds | ✅ |
| Hash parity | Rust ↔ WASM ↔ CGo ↔ Python ↔ TS | 10K vectors | ✅ |

## License

Proprietary. See LICENSE.
