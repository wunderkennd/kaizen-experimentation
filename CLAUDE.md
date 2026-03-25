# Kaizen Experimentation Platform

## Project Overview

Full-stack SVOD experimentation platform. 7 modules, 3 languages (Rust/Go/TypeScript), 13 Rust crates, Protobuf schema-first. Phases 0–4 complete (163 PRs, 10 pair integration suites green). Phase 5 in progress: 15 ADRs (011–025) across 6 capability clusters.

## Architecture

| Module | Language | Owner | Port | Purpose |
| --- | --- | --- | --- | --- |
| M1 Assignment | Rust | Agent-1 | 50051 | Variant allocation, interleaving, bandit arm delegation, SDKs |
| M2 Pipeline | Rust+Go | Agent-2 | 50052/50058 | Event validation/dedup/Kafka (Rust), orchestration (Go) |
| M3 Metrics | Go | Agent-3 | 50056 | Spark SQL orchestration, metric computation, Delta Lake |
| M4a Analysis | Rust | Agent-4 | 50053 | All statistical computation (experimentation-stats crate) |
| M4b Bandit | Rust | Agent-4 | 50054 | Thompson, LinUCB, Neural (Candle), LMAX single-thread core |
| M5 Management | Go (conditional Rust port, ADR-025) | Agent-5 | 50055 | CRUD, lifecycle, RBAC, guardrails, bucket reuse |
| M6 UI | TypeScript | Agent-6 | 3000 | Next.js 14, React 18, Recharts, D3, shadcn/ui |
| M7 Flags | Go → Rust (ADR-024) | Agent-7 | 50057 | Feature flags, percentage rollout, reconciler |

## Cargo Workspace

```
crates/
  experimentation-core/          # Timestamps, errors, tracing, assert_finite!()
  experimentation-hash/          # MurmurHash3, bucketing
  experimentation-proto/         # tonic-build generated from proto/
  experimentation-stats/         # All statistical methods (Phase 5: +9 new modules)
  experimentation-bandit/        # Thompson, LinUCB, Neural, cold-start (Phase 5: +slate, +multi-objective, +LP)
  experimentation-interleaving/  # Team Draft, Optimized, Multileave
  experimentation-ingest/        # Event validation, Bloom filter dedup
  experimentation-ffi/           # CGo bindings — DELETED after ADR-024 completes
  experimentation-assignment/    # M1 service binary
  experimentation-analysis/      # M4a service binary
  experimentation-pipeline/      # M2 service binary
  experimentation-policy/        # M4b service binary
  experimentation-flags/         # M7 service binary (NEW, ADR-024)
```

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

## Phase 5 Status

Phase 5 implements 15 ADRs across 6 clusters:

| Cluster | ADRs | Core Gap |
| --- | --- | --- |
| A: Multi-Stakeholder | 011, 012, 013, 014 | Multi-objective bandits, LP constraints, meta-experiments, provider metrics |
| B: Statistical Methods | 015, 018, 020 | AVLM (sequential CUPED), e-values + online FDR, adaptive sample size |
| C: Bandit & RL | 016, 017 | Slate bandits, offline RL / surrogate calibration fix |
| D: Quasi-Experimental | 022, 023 | Switchback experiments, synthetic control methods |
| E: Platform Operations | 019, 021 | Portfolio optimization, feedback loop interference |
| F: Language Migration | 024, 025 | M7 Rust port (unconditional), M5 Rust port (conditional) |

**Current sprint**: Check the active GitHub Milestone:
```bash
gh issue list --milestone "Sprint 5.0: Schema & Foundations" --state open
```

**P0 items** (highest priority):
- ADR-015 (AVLM) — #1 ROI: unifies CUPED + mSPRT
- ADR-017 Phase 1 (TC/JIVE) — corrects theoretical error in surrogate calibration
- ADR-024 (M7 Rust port) — eliminates FFI crate

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

**Agent definitions** at `.multiclaude/agents/agent-N-*.md` define *how* agents work (coding standards, contract test obligations). GitHub Issues define *what* they're working on.

## Key File Locations

| What | Where |
| --- | --- |
| This file (agent context) | `CLAUDE.md` (repo root) |
| Design document | `docs/design/design_doc_v7.0.md` |
| ADRs (001–025) | `docs/adrs/` |
| ADR index | `docs/adrs/README.md` |
| Agent definitions | `.multiclaude/agents/agent-N-*.md` |
| Work tracking | GitHub Issues (Milestones = Sprints, Issues = Tasks) |
| Claude Code settings | `.claude/settings.json` |
| PR triage subagent | `.claude/agents/pr-triage.md` |
| Multiclaude config | `.multiclaude/config.json` |
| Proto schema | `proto/experimentation/` |
| SQL migrations | `sql/migrations/` |
| Test vectors | `test-vectors/hash_vectors.json` |
| Sprint plan (narrative) | `docs/coordination/phase5-implementation-plan.md` |
| Playbook | `docs/coordination/phase5-playbook.md` |
| Developer guides | `docs/guides/` |

## Testing Commands

```bash
# Rust — per-crate (preferred for agents working on a single crate)
cargo test -p experimentation-stats
cargo test -p experimentation-bandit
cargo test -p experimentation-flags

# Rust — workspace-wide (run before creating PR)
cargo test --workspace

# Go
go test ./services/metrics/...
go test ./services/management/...

# TypeScript
cd ui && npm test

# Proto validation
buf lint proto/
buf breaking proto/ --against .git#branch=main

# Proptest nightly (CI only — slow)
PROPTEST_CASES=10000 cargo test -p experimentation-stats
```

## Golden-File Validation Targets

| Module | Reference | Precision |
| --- | --- | --- |
| `avlm.rs` (ADR-015) | R `avlm` package | 4 decimal places |
| `orl.rs` TC/JIVE (ADR-017) | Netflix KDD 2024 Table 2 | Reproduce results |
| `evalue.rs` (ADR-018) | Ramdas/Wang monograph examples | 6 decimal places |
| `switchback.rs` (ADR-022) | DoorDash sandwich estimator | 4 decimal places |
| `synthetic_control.rs` (ADR-023) | R `augsynth` package | 4 decimal places |
| `gst.rs` (existing) | R `gsDesign` + scipy | 4 decimal places |
| `ttest.rs` (existing) | R `t.test()` | 6 decimal places |

## Files That Must NOT Be Tracked in Git

Build artifacts and agent runtime state — never commit:
- `ui/tsconfig.tsbuildinfo`, `.Jules/`, `node_modules/`, `.next/`, `target/`, `dist/`
- `.claude/settings.local.json`, `.claude/worktrees/`, `.claude/teams/`, `.claude/tasks/`
- `.multiclaude/state/`, `.multiclaude/messages/`, `.multiclaude/worktrees/`, `*.pid`, `*.log`

See `.gitignore` and `docs/guides/git-hygiene.md`.
