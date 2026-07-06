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
| M5 Management | Go (production); Rust port at ADR-025 Phase 2/4 (RBAC + Phase-3 stats pending, #590) | Agent-5 | 50055 | CRUD, lifecycle, RBAC, guardrails, bucket reuse, portfolio, adaptive-N scheduler |
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
- **Right-sized PRs**: the PR-size gate (`PR size / check`, required) warns at 400 changed lines / 10 files and fails at 900 / 25 — lockfiles, generated trees, and markdown exempt. Genuinely atomic oversize diffs take the `oversize-approved` label plus a justifying comment. Workers slice-and-propose instead of shipping omnibuses.
- **Branch naming**: see [Branch-naming convention](#branch-naming) below — canonical pattern is `agent-N/feat/adr-XXX-description`, with documented alternates for infra, palette, and chore work. Validate before pushing with `just check-branch-name`; a CI advisory check (`.github/workflows/branch-naming.yml`) also flags violations on PR open.
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
| F: Language Migration | 024, 025 | 024 shipped (`experimentation-ffi` deleted); 025 Phase 2/4 implemented (`experimentation-management` crate landed) — Phase 1 RBAC interceptor + Phase 3 statistical integration pending (#590) |

See `docs/coordination/phase5-implementation-plan.md` and `docs/coordination/CHANGELOG-phase5.md` for per-sprint detail.

## Active Work (Post-Phase 5)

**New ADRs (026–030 Proposed; 031 Accepted)**:
- **ADR-026** (`docs/adrs/026-custom-metrics-layer.md`) — Custom metrics definition layer (composite / derived / joined metrics beyond the six built-in types). Impact: M5, M3, M4a. **Phase 1 implemented** (Rust M5 + M6 UI + M3 topo-order scheduling — FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT; #552, #555, #475 — M3 dependency ordering via Kahn's algorithm with `metric_computation_status` table). **Phase 2 #435 implemented**: M3 MetricQL parser/compiler in `services/metrics/internal/metricql/` (lexer + recursive-descent parser + AST + semantic analyzer + DFS cycle detector + Spark SQL codegen; proto field `metricql_expression`; migration 013; integrated with #475 topo-order via @metric_ref operand extraction; symmetric upstream-failure gate). #436 (M5 expression validation + M6 expression editor) and Phase 3 (CUSTOM deprecation, #437) remain Proposed.
- **ADR-027** (`docs/adrs/027-tost-equivalence-testing.md`) — Two One-Sided Tests for proving equivalence (infra migrations, refactor validation). Impact: M4a, M5, M6. Core impl landed (#443); see `crates/experimentation-stats/src/tost.rs`.
- **ADR-028** (`docs/adrs/028-m4b-shadow-inference.md`) — M4b shadow inference path for bandit policy promotion (dedicated shadow core, column-family isolation). Impact: M4b, M4a, M5, M6.
- **ADR-029** (`docs/adrs/029-cross-modal-score-calibration.md`) — Cross-modal score calibration for heterogeneous slates (unified NEV scale across video, manga, commerce). Introduces a new `experimentation-calibration` Rust crate owned by Agent-4 and opens cluster **G — Personalization Orchestration**. Impact: M4a, M4b, M5, Personalization service.
- **ADR-030** (`docs/adrs/030-shadow-experiment-mode.md`) — Shadow mode flag on experiments — run candidate variants on production traffic without user exposure. Impact: M1, M4a, M4b, M5, M6.
- **ADR-031** (`docs/adrs/031-connectrpc-rust-assignment-pilot.md`) — **Accepted** (2026-06-23, #634): ConnectRPC (Rust) pilot on M1 Assignment via the Tower-based `connectrpc` runtime — a scoped revisit of ADR-010's "Connect for Go, tonic for Rust" split; fleet-wide adoption gated on the pilot's success criteria. Impact: M1, SDKs.

**Infrastructure sprint (Pulumi + Go on AWS)**: `infra/` contains Pulumi stacks (`Pulumi.{dev,staging,prod}.yaml`) and a full Go test suite (`fullstack_test.go`). Sprint I.0 (all 13 modules) and I.1/I.2 (wiring + hardening) merged; ECR repos exist for all 9 Kaizen services.

**QoE stream**: `ebvs_detected` field on `PlaybackMetrics` and `HeartbeatSessionizer` in `experimentation-ingest` deliver server-side QoE aggregation. Specs in `docs/issues/ebvs-detection.md`, `docs/issues/heartbeat-sessionization.md`.

**Palette UI polish**: Ongoing standardization of search, empty states, filter clearing, accessibility, and CopyButton usage in M6. Look for commits prefixed `🎨 Palette:`.

**Current sprint**: sprints live on Project #5's Iteration field (humans/roadmap) and `sprint-*` labels (machines) — Milestones are closed (H2 #693):
```bash
just morning                                    # iteration + per-cohort ready counts
gh issue list --label sprint-5.6 --state open   # ADR-026/027 stream
gh issue list --label sprint-I.3 --state open   # infrastructure stream
```

## Work Tracking

Work is tracked in **GitHub Issues**, not markdown status files.

```
Iteration (Project #5) = Sprint — `sprint-N` labels carry it for machines
  └── Issue  =  ADR implementation unit; blockers = native dependency edges (H2)
```

### For agents: how to find your work
```bash
# What's assigned to me?
gh issue list --assignee @me --state open

# What's in the current sprint?
gh issue list --label sprint-5.6 --state open

# What's blocked?
gh issue list --label "blocked"

# Read a task spec
gh issue view <number>
```

### For agents: how to update progress
- **Claim before starting** (H1 protocol): comment `claim: executor=<tool> worker=<id> expires=<ISO8601>` on the issue; release with a `claim-released:` comment on completion or handoff. Expired claims are re-dispatchable. See `scripts/orchestration/README.md`.
- Comment on the issue with progress updates
- When creating a PR, include `Closes #<issue-number>` in the PR description (use `Refs #<n>` instead when the issue has post-merge steps)
- The issue auto-closes when the PR merges
- If blocked, add the `blocked` label and comment explaining what you're waiting on — and wire the real edge natively (issue *Relationships* → "Blocked by", or `gh api .../dependencies/blocked_by`); `_ready` reads the native edges, not body text

### Labels
- `agent-1` through `agent-7` — ownership
- `P0` through `P4` — priority
- `cluster-a` through `cluster-g` — capability cluster (cluster-g = ADR-029 Personalization Orchestration)
- `blocked` — waiting on another issue/agent
- `contract-test` — cross-module contract test

## Branch-naming

Branch names are validated against the allowlist in [`.github/branch-naming.yml`](.github/branch-naming.yml). Six pattern families are recognized (four canonical + two tolerated automation families); everything else is flagged by the CI advisory check. Branch naming is **advisory hygiene** — agent ownership is now carried by **PR metadata** (see below), not the ref name.

| Pattern | When to use | Examples |
| --- | --- | --- |
| `agent-N/<verb>/adr-XXX-<slug>` | **Canonical** — agent-owned implementation work tied to an ADR | `agent-3/feat/adr-026-phase-2-metricql`, `agent-5/design/adr-026-phase-2-m5-m6`, `agent-3/fix/composite-cycle-depth` |
| `infra-N/<verb>/<slug>` | Pulumi / GCP / AWS infra work owned by `infra-N` agents | `infra-2/feat/gcp-sql-private-access`, `infra-5/fix/cloud-armor-policy` |
| `palette[/-]<slug>[-<digits>]` | Design-system / M6 polish dispatched by the palette tooling | `palette/refine-breadcrumb-18322858338088205282`, `palette-standardize-sort-headers-16091075850831504436` |
| `chore/<slug>` | Repo-wide hygiene without a single agent owner (justfile, docs, CI, tooling) | `chore/prime-issue-recipe`, `chore/branch-name-enforcement` |
| `claude/<slug>`, `work/<slug>` | **Tolerated, not encouraged** — harness-generated ref names that can't be renamed after launch (Claude Code web/remote sessions, multiclaude workers). Recognized so the advisory check doesn't flag the unfixable. | `claude/repo-status-next-steps-mze2fh`, `work/swift-eagle` |

**Allowed verbs** (for `agent-N/<verb>/...`): `feat`, `fix`, `port`, `design`, `chore`, `refactor`, `docs`, `test`. Verb choice mirrors the conventional-commit prefix used in the eventual PR title.

**Prefer a canonical family when you control the branch name** — a legible `agent-N/...` name is still the ideal. But agent ownership no longer *depends* on the branch: it rides on **PR metadata** — the Conventional-Commit PR title enforced by [`.github/workflows/pr-title.yml`](.github/workflows/pr-title.yml), plus the `agent-N` label copied from the linked issue by [`.github/workflows/pr-label-inheritance.yml`](.github/workflows/pr-label-inheritance.yml). Harness-generated names (`claude/...`, `work/...`) that can't be renamed are therefore *tolerated* (recognized above), not flagged — they no longer bypass attribution. A name that matches no family at all (e.g., `plan-development-strategy`) is still flagged by the advisory check.

### How to validate

```
just check-branch-name          # exits 0 on match, 1 with suggestions on no match
```

The CI workflow [`.github/workflows/branch-naming.yml`](.github/workflows/branch-naming.yml) runs the same check on PR open / branch rename and posts an advisory comment if no pattern matches. **Currently advisory only** — does not block merge. With attribution now carried by PR metadata, this check stays **advisory** and enforcement lives on the PR side: **"PR title check / check"** (`.github/workflows/pr-title.yml`), the **Review gate** (`Review gate / gate`), and the **PR-size gate** (`PR size / check`) are required status checks via the native ruleset `.github/rulesets/main.json` (H3/#681; fleet-wide stamping via `infra/github-governance/` — H6); routine green PRs then auto-merge (`automerge.yml`), with human review reserved for `breaking`/`contract-test`/proto-touching PRs. The governance workflows are `workflow_call` reusables (`_review-gate.yml`, `_pr-title.yml`, `_automerge.yml`, `_pr-size.yml`) so sibling Kaizen repos run identical callers — see `docs/runbooks/ecosystem-governance.md`.

### Adding a new pattern family

If a legitimate new branch-name convention emerges (e.g., a future `bench-N/...` for benchmarking work), add the regex to `.github/branch-naming.yml::allowed_patterns` in the same PR that creates the first branch using it. Both `just check-branch-name` and the CI check read from the same file.

## Orchestration Model

Phase 5 uses a multi-tool orchestration model. Each tool has a specific role.

| Tool | Role | When |
| --- | --- | --- |
| **Gas Town** | Interactive parallel work — Mayor coordinates polecats, you steer in real time | Daytime active sessions |
| **Multiclaude** | Autonomous overnight grinding — local daemon + workers (the merge path itself is now owned by the platform gates, not the daemon) | You're away (overnight, weekends) — **retirement path decided**, see H4 note below |
| **Jules** | CI-triggered automation — scheduled maintenance, test generation, dependency bumps | Continuous / event-driven (GitHub Actions) |
| **Devin** | Bounded autonomous tasks — test coverage, migrations, repetitive refactoring; automatic PR review | Batch dispatch for well-specified work |
| **Gemini CLI** | Quick lookups, second-opinion code review, research | Ad-hoc |
| **Claude Code** (solo / web / `@claude`) | Focused tasks in isolated worktrees; remote/web sessions drive harness work and run privileged operations via **workflow vehicles** (not-for-merge PRs carrying `pull_request`-triggered workflows) | One-off work; H4's reference executor |

**Harness modernization (H0–H7)** — the orchestration layer is being consolidated onto GitHub-native primitives; status lives in `docs/coordination/harness-modernization-proposal.md`. Shipped so far: **H1** claim protocol + executor-agnostic dispatch (`scripts/orchestration/`), **H2** native work graph (dependency edges + GraphQL `_ready`; Milestones retired 2026-07-05; final parser deletions calendar-gated on #694), **H3/H6** platform merge path (ruleset-required checks + `automerge.yml` + PR-size gate, fleet-reusable workflows + Pulumi ruleset stamping), **H7** delivery-practice codification (lifecycle map, templates, plan-review, advisory doc-lints). **H4 (amended 2026-07-05)**: multiclaude's coordination plane is fully absorbed; the daemon retires behind a GitHub-native **evening dispatcher** on graduated evidence (shadow → limited-live → full sprint). Probe #713 confirmed the dispatcher is buildable with the default `GITHUB_TOKEN` (`workflow_dispatch` launch works; worker workflows must be registered on `main`; comment-based triggering is dead).

**Daily rhythm**: `just morning` (check overnight results) → `just interactive` (Gas Town) → `just evening <sprint>` (Multiclaude overnight, until the H4 dispatcher graduates).

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
| ADRs (001–031) | `docs/adrs/` |
| ADR index | `docs/adrs/README.md` |
| **Agent registry (canonical identity)** | `docs/agents/registry/` — OKF v0.1 bundle; validate with `just check-registry` |
| Agent definitions (modules) | `.multiclaude/agents/agent-N-*.md` (view of the registry; generated under #682) |
| Agent definitions (infra) | `.multiclaude/agents/infra-{1..5}-*.md` (view of the registry; generated under #682) |
| Agent onboarding | `docs/onboarding/agent-N-*.md` (incl. `agent-0-coordination.md`) |
| Module runbooks | `docs/runbooks/m4a-analysis.md`, `docs/runbooks/m4b-policy.md` |
| Infra runbook (adding a Cloud Run service) | `docs/runbooks/gcp-compute-services.md` (registry pattern at `infra/pkg/gcp/services/`, established by #542 / PR #546) |
| Ecosystem governance runbook (H6) | `docs/runbooks/ecosystem-governance.md` (fleet onboarding, ruleset apply, wunderkind-ventures org migration) |
| Operator runbook (creating M5 custom metrics) | `docs/runbooks/m5-metric-definitions.md` (Tier 1 types FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT; #434) |
| Operator runbook (ADR-026 Phase 3 migration) | `docs/runbooks/adr-026-phase-3-migration.md` (scan + translate + shadow + apply workflow for legacy CUSTOM metrics; #437) |
| Work tracking | GitHub Issues (Project #5 Iterations = Sprints; `sprint-N` labels for machines; native dependency edges = blockers) |
| **Harness modernization proposal (H0–H7)** | `docs/coordination/harness-modernization-proposal.md` — phase status annotations, prior-art revisions, open questions |
| Dispatch layer (H1) | `scripts/orchestration/` — `claims.sh` (claim protocol), `ready.sh` (native `_ready` + drift mode), `dispatch.sh` + `dispatch.d/` adapters; offline tests in `orchestration-tests.yml` |
| Plan-review procedure (H7) | `docs/guides/plan-review.md` — run before blessing any locked plan; worked example: #680 v1→v2 |
| Docs lint (H7) | `scripts/check_docs.py` (`just check-docs`; advisory via `docs-conformance.yml`, strict mode `DOCS_LINT_STRICT=1`) |
| Claude Code settings | `.claude/settings.json` |
| PR triage subagent | `.claude/agents/pr-triage.md` |
| Multiclaude config | `.multiclaude/config.json` |
| Proto schema | `proto/experimentation/` (subdirs: assignment, analysis, bandit, flags, management, metrics, pipeline, common) |
| SQL migrations | `sql/migrations/` |
| Test vectors (hash parity) | `test-vectors/hash_vectors.json` |
| Phase 5 plan & changelog | `docs/coordination/phase5-implementation-plan.md`, `docs/coordination/CHANGELOG-phase5.md` |
| Playbook | `docs/coordination/phase5-playbook.md` |
| Developer guides | `docs/guides/` (git-hygiene, github-issues-workflow, orchestration-workflow, projects-and-goals, plan-review, pr-triage-and-cleanup, merge-conflict-resolution, gastown-setup, palette) |
| **Delivery lifecycle (H7)** | `docs/guides/delivery-lifecycle.md` — idea → PRD → RFC/ADR → spec → plan-review → `prime-issue` → dispatch; plan quality bar; Lock convention |
| Document templates | `docs/templates/` (PRD, RFC, UX spec) + `docs/superpowers/templates/` (locked plan v2) |
| Issue specs (QoE etc.) | `docs/issues/` |
| Infrastructure (Pulumi) | `infra/` (`main.go`, `Pulumi.{dev,staging,prod}.yaml`, `fullstack_test.go`) |
| GitHub governance (rulesets, H6) | `.github/rulesets/main.json` (this repo) · `infra/github-governance/` (fleet stamping, org-ready) |
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

# Harness conformance (advisory lints, promoted to required only after a clean window)
just check-registry     # OKF agent-registry conformance
just check-docs         # delivery-lifecycle doc lints (ADRs, plans, PRDs, templates)
just check-branch-name  # branch-name allowlist
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
- `.agents/`, `.claude/skills/` — restored from `skills-lock.json` via `just install-skills`
- `.claude/agents/` third-party library (The Agency) — restored via `just install-agents`; only repo-authored agents (e.g. `pr-triage.md`) are tracked

See `.gitignore` and `docs/guides/git-hygiene.md`.

## Agent Skills (portable across devices)

Project-required agent skills are pinned in `skills-lock.json` (committed). The skill content itself lives in `.agents/skills/` and `.claude/skills/` and is **not committed** — it's regenerated on demand:

```bash
just install-skills          # restore from lockfile (runs as part of `just setup`)
just update-skills-check     # see what new versions are available upstream
just update-skills           # pull latest, then commit the updated skills-lock.json
```

Skills cover: Pulumi-to-Terraform migration (`pulumi-terraform-to-pulumi`), Playwright UI testing (`webapp-testing`), ADR drafting (`documentation-and-adrs`), plus Matt Pocock's engineering productivity set (`tdd`, `diagnose`, `triage`, `to-issues`, `to-prd`, `grill-me`, etc.). Add new shared skills with `npx skills add <owner/repo@skill>` (project scope — omit `-g`).
