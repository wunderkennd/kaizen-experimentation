# Kaizen — SVOD Experimentation Platform

A full-stack experimentation system purpose-built for streaming platforms. Supports A/B testing, interleaving, multi-armed bandits (Thompson Sampling, LinUCB, Neural Contextual), feature flags, sequential testing (mSPRT, GST), CUPED variance reduction, surrogate metrics, novelty detection, content interference analysis, lifecycle segmentation, and session-level experiments.

**Status**: Phases 0–4 complete (163 PRs, 10 pair integration suites green). Phase 5 in progress — 15 proposed ADRs across multi-stakeholder optimization, statistical methodology, bandit/RL advances, quasi-experimental designs, platform operations, and language migration.

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
│  :50056 (Go)             │    │  :50055 (Go)                 │
│  Spark SQL, Delta Lake,  │    │  CRUD, lifecycle, RBAC,      │
│  surrogates, providers   │    │  guardrails, bucket reuse    │
└──────────────────────────┘    └──────────────────────────────┘
                                            │
┌──────────────────────────┐    ┌──────────────────────────────┐
│  M7 Feature Flags        │    │  M6 Decision Support UI      │
│  :50057 (Go→Rust)        │    │  :3000 (TypeScript)          │
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
git clone https://github.com/your-org/kaizen.git && cd kaizen
docker compose up -d
cargo build --workspace && cargo test --workspace
go build ./... && go test ./...
cd ui && npm install && npm test && cd ..
buf lint proto/
```

## Phase 5: Architecture Evolution

Phase 5 implements 15 proposed ADRs driven by a 2024–2026 experimentation research gap analysis:

| Cluster | ADRs | Capability |
| --- | --- | --- |
| A: Multi-Stakeholder | 011–014 | Multi-objective bandits, LP constraint layer, meta-experiments, provider-side metrics |
| B: Statistical Methods | 015, 018, 020 | AVLM (sequential CUPED), e-value framework + online FDR, adaptive sample size |
| C: Bandit & RL | 016, 017 | Slate-level bandits, offline RL for long-term causal estimation |
| D: Quasi-Experimental | 022, 023 | Switchback experiments, synthetic control methods |
| E: Platform Operations | 019, 021 | Portfolio optimization, feedback loop interference detection |
| F: Language Migration | 024, 025 | M7 Go→Rust (unconditional), M5 Go→Rust (conditional) |

### Work Tracking

Work is tracked via **GitHub Milestones and Issues**, not in-repo files:

```
Milestone    =  Sprint (e.g., "Sprint 5.0: Schema & Foundations")
  └── Issue  =  ADR task (e.g., "ADR-015: AVLM Implementation")
```

```bash
# View current sprint
gh issue list --milestone "Sprint 5.0: Schema & Foundations"

# View by agent
gh issue list --label "agent-4" --state open

# View blocked work
gh issue list --label "blocked"
```

### Development Orchestration

Phase 5 uses a multi-tool orchestration model:

| Tool | Role | When |
| --- | --- | --- |
| Gas Town | Interactive parallel work — Mayor + polecats | Daytime active sessions |
| Multiclaude | Autonomous grinding — daemon + CI-gated merge queue | Overnight / weekends |
| Jules | CI-triggered automation — maintenance, tests, deps | Continuous (GitHub Actions) |
| Devin | Bounded autonomous tasks — migrations, test coverage | Batch dispatch |
| Gemini CLI | Second-opinion review, research | Ad-hoc |

```bash
just morning              # Check overnight results, pull main
just interactive          # Start Gas Town Mayor session
just evening 0            # Launch Sprint 5.0 Multiclaude workers overnight
just status               # Unified view across all tools
just pr-triage            # AI-assisted PR cleanup
```

See `docs/guides/orchestration-workflow.md` for the full guide.

## Project Structure

```
kaizen/
├── CLAUDE.md                          # Agent context
├── AGENTS.md                          # Jules agent context
├── README.md                          # This file
├── CONTRIBUTING.md                    # Contribution guide
├── Cargo.toml                         # Workspace root
├── justfile                           # Task runner
│
├── .claude/
│   ├── settings.json                  # Project-level Claude Code settings
│   └── agents/
│       └── pr-triage.md               # PR triage subagent
│
├── .multiclaude/
│   ├── config.json                    # Multiclaude repo config
│   └── agents/                        # 7 agent definitions
│
├── .github/
│   ├── workflows/                     # CI/CD + Jules automation
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
├── docs/
│   ├── design/design_doc_v7.0.md
│   ├── adrs/                          # ADRs 001–010, 014–026
│   ├── coordination/                  # Sprint plan, playbook
│   └── guides/                        # Developer guides
│
├── docker-compose.yml
└── docker-compose.monitoring.yml
```

## Documentation

| Document | Description |
| --- | --- |
| [Design Document v7.0](docs/design/design_doc_v7.0.md) | Complete system reference + Phase 5 architecture plan |
| [ADR Index](docs/adrs/README.md) | 25 architecture decision records |
| [Phase 5 Plan](docs/coordination/phase5-implementation-plan.md) | 6 sprints, agent assignments, milestones |
| [Orchestration Workflow](docs/guides/orchestration-workflow.md) | Multi-tool daily workflow guide |
| [Gas Town Setup](docs/guides/gastown-setup.md) | Gas Town installation and configuration |
| [GitHub Issues Workflow](docs/guides/github-issues-workflow.md) | Work tracking with Milestones, Issues, labels |
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
