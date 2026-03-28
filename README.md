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
| ML Models | MLflow on S3 |
| Database | PostgreSQL 16 |
| Policy Store | RocksDB (embedded, crash-only) |
| Feature Store | Redis Cluster |
| Observability | Prometheus, Grafana, Jaeger (OpenTelemetry) |

## SVOD-Specific Capabilities

| Capability | Module(s) | Description |
| --- | --- | --- |
| Interleaving | M1, M4a | Team Draft, Optimized, Multileave — 10–100× more sensitive than A/B |
| Surrogate Metrics | M3, M4a | MLflow-calibrated projection of 90-day churn from 7-day signals |
| Novelty Detection | M4a | Gauss-Newton exponential decay fitting — prevents shipping based on fading lift |
| Content Interference | M4a | JSD, Jaccard, Gini on consumption distributions with BH correction |
| Lifecycle Segmentation | M4a | CATE + Cochran Q across TRIAL/NEW/ESTABLISHED/MATURE/AT_RISK/WINBACK |
| Session-Level Experiments | M1, M4a | HC1 sandwich estimator for clustered standard errors |
| Playback QoE | M2, M3, M4a | TTFF, rebuffer, bitrate, resolution switches with engagement correlation |
| Cold-Start Bandit | M4b | Contextual bandit for new content targeting with affinity score export |
| Cumulative Holdout | M1, M4a | Fail-closed holdout assignment measuring total algorithmic lift |

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
# Clone
git clone https://github.com/your-org/kaizen.git
cd kaizen

# Start infrastructure
docker compose up -d   # Kafka, PostgreSQL, Redis, Prometheus, Grafana, Jaeger

# Rust services
cargo build --workspace
cargo test --workspace

# Go services
go build ./...
go test ./...

# UI
cd ui && npm install && npm test && cd ..

# Proto validation
buf lint proto/
```

### Running Services

```bash
# Use justfile for common tasks
just run-assignment      # M1 on :50051
just run-pipeline        # M2 on :50052
just run-metrics         # M3 on :50056
just run-analysis        # M4a on :50053
just run-policy          # M4b on :50054
just run-management      # M5 on :50055
just run-ui              # M6 on :3000
just run-flags           # M7 on :50057
```

## Phase 5: Architecture Evolution

Phase 5 implements 15 proposed ADRs driven by a 2024–2026 experimentation research gap analysis (50+ papers, Netflix/Spotify/Meta/Etsy/LinkedIn):

| Cluster | ADRs | Capability |
| --- | --- | --- |
| A: Multi-Stakeholder | 011–014 | Multi-objective bandits, LP constraint layer, meta-experiments, provider-side metrics |
| B: Statistical Methods | 015, 018, 020 | AVLM (sequential CUPED), e-value framework + online FDR, adaptive sample size |
| C: Bandit & RL | 016, 017 | Slate-level bandits, offline RL for long-term causal estimation |
| D: Quasi-Experimental | 022, 023 | Switchback experiments, synthetic control methods |
| E: Platform Operations | 019, 021 | Portfolio optimization, feedback loop interference detection |
| F: Language Migration | 024, 025 | M7 Go→Rust (unconditional), M5 Go→Rust (conditional) |

### Development Orchestration

Phase 5 uses a multi-tool orchestration model where each tool has a distinct role:

| Tool | Role | When |
| --- | --- | --- |
| Gas Town | Interactive parallel work — Mayor coordinates polecats, you steer | Daytime active sessions |
| Multiclaude | Autonomous overnight grinding — daemon, CI-gated merge queue | You're away |
| Jules | CI-triggered automation — maintenance, tests, dependency bumps | Continuous (GitHub Actions) |
| Devin | Bounded autonomous tasks — migrations, test coverage, golden files | Batch dispatch |
| Gemini CLI | Quick lookups, second-opinion review, research | Ad-hoc |

```bash
# Daily workflow (justfile commands)
just morning              # Check overnight Multiclaude results, pull main
just interactive          # Start Gas Town Mayor session for the day
just evening 0            # Stop Gas Town, launch Sprint 5.0 Multiclaude workers

# Other modes
just solo adr-015-debug   # Single Claude Code session in isolated worktree
just autonomous-sprint 2  # Launch Sprint 5.2 Multiclaude workers
just status               # Unified view across all tools
just pr-triage            # AI-assisted PR cleanup
```

See `docs/guides/orchestration-workflow.md` for the full multi-tool guide and `docs/guides/gastown-setup.md` for Gas Town configuration.

## Project Structure

```
kaizen/
├── CLAUDE.md                          # Agent context (read by all sessions)
├── AGENTS.md                          # Jules agent context
├── README.md                          # This file
├── CONTRIBUTING.md                    # Contribution guide
├── Cargo.toml                         # Workspace root
├── justfile                           # Task runner (includes orchestration recipes)
│
├── .claude/
│   ├── settings.json                  # Project-level Claude Code settings
│   └── agents/
│       └── pr-triage.md               # PR triage subagent
│
├── .multiclaude/
│   ├── config.json                    # Multiclaude repo config
│   └── agents/                        # 7 agent definitions (committed)
│
├── crates/                            # Rust workspace (13 crates)
├── services/                          # Go services (M3, M5, M2-orch, M7-legacy)
├── ui/                                # M6 (Next.js 14)
├── proto/experimentation/             # Protobuf schema (17 files, 8 packages)
├── sdks/                              # Client SDKs (5 platforms)
├── sql/migrations/                    # PostgreSQL DDL
├── delta/                             # Delta Lake table schemas
├── test-vectors/                      # Hash parity vectors (10K)
│
├── docs/
│   ├── design/
│   │   └── design_doc_v7.0.md
│   ├── adrs/                          # 25 ADRs (001–025)
│   ├── coordination/                  # Sprint plans, playbook, status files
│   └── guides/                        # Developer guides
│       ├── orchestration-workflow.md
│       ├── gastown-setup.md
│       ├── pr-triage-and-cleanup.md
│       ├── merge-conflict-resolution.md
│       └── git-hygiene.md
│
├── .github/workflows/                 # CI/CD + Jules automation
│   ├── ci.yml
│   ├── jules-weekly-maintenance.yml
│   └── jules-test-coverage.yml
│
├── docker-compose.yml
└── docker-compose.monitoring.yml
```

## Documentation

| Document | Description |
| --- | --- |
| [Design Document v7.0](docs/design/design_doc_v7.0.md) | Complete system reference + Phase 5 architecture plan |
| [ADR Index](docs/adrs/README.md) | 25 architecture decision records (10 accepted, 15 proposed) |
| [Phase 5 Implementation Plan](docs/coordination/phase5-implementation-plan.md) | 6 sprints, agent assignments, milestones |
| [Phase 5 Playbook](docs/coordination/phase5-playbook.md) | Gas Town + Multiclaude operational guide |
| [Sprint Prompts](docs/coordination/sprint-prompts.md) | Pre-written worker/task commands per sprint |
| [Orchestration Workflow](docs/guides/orchestration-workflow.md) | Multi-tool daily workflow (Gas Town + Multiclaude + Jules + Devin + Gemini) |
| [Gas Town Setup](docs/guides/gastown-setup.md) | Gas Town installation and Kaizen rig configuration |
| [PR Triage & Cleanup](docs/guides/pr-triage-and-cleanup.md) | Handling PR accumulation, crash recovery, batch cleanup |
| [Merge Conflict Resolution](docs/guides/merge-conflict-resolution.md) | Strategies for status files, generated files, multi-branch rebases |
| [Git Hygiene](docs/guides/git-hygiene.md) | What to track, what to gitignore, .gitattributes rules |

## Verified Performance

| Service | Metric | Target | Achieved |
| --- | --- | --- | --- |
| M1 Assignment | GetAssignment p99 | < 5ms | ✅ at 50K rps |
| M1 Assignment | GetInterleavedList p99 | < 15ms | ✅ |
| M4b Bandit | SelectArm p99 | < 15ms at 10K rps | ✅ |
| M4b Bandit | Crash recovery | < 10 seconds | ✅ |
| M7 Flags | EvaluateFlag p99 | < 10ms at 20K rps | ✅ (< 5ms post-Rust-port) |
| All stateless | Crash recovery | < 2 seconds | ✅ |
| Hash parity | Rust ↔ WASM ↔ CGo ↔ Python ↔ TS | 10K vectors | ✅ |

## License

Proprietary. See LICENSE.
