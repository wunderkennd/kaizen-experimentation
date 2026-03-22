# Agent Teams vs. Multiclaude: Kaizen Phase 5 Evaluation

**Date**: March 2026
**Decision context**: Kaizen Phase 5 requires 7 agents working across 15 ADRs, 7 modules, 3 languages (Rust/Go/TypeScript), 13+ Rust crates, and 6 sprints over 18 weeks. Agents must own separate modules, communicate about cross-cutting proto changes, and converge on contract tests at integration boundaries.


## 1. Architecture Comparison

### Claude Code Agent Teams (Anthropic, native)

**Model**: Ephemeral team sessions. One lead spawns teammates via natural language. Teammates get their own context windows (1M tokens each), communicate via peer-to-peer mailbox, and self-claim tasks from a shared task list with file-lock-based concurrency control.

**Lifecycle**: Team exists for one session. No session resumption with in-process teammates — if the lead crashes, teammates are lost. You restart fresh with a new spawn prompt.

**Communication**: Teammates message each other directly and the lead. The lead synthesizes findings. Messages are stored at `~/.claude/teams/{team-name}/`. Automatic task dependency tracking — when a blocking task completes, downstream tasks auto-unblock.

**Isolation**: All teammates share the same working directory by default. No built-in git worktree isolation.

**Custom agents**: No persistent agent definitions. Teammates are defined conversationally in the spawn prompt. Each spawn is bespoke.

### Multiclaude (Dan Lorenc, open-source)

**Model**: Persistent daemon with four 2-minute loops (health check, message routing, wake/nudge, worktree refresh). Supervisor coordinates workers. Workers are single-task: one branch, one PR, self-destruct on completion.

**Lifecycle**: Daemon persists across sessions. Workers survive terminal disconnects (tmux). You can `detach` and they keep working. Supervisor monitors health and respawns failed workers.

**Communication**: JSON files on disk. The daemon routes messages by typing them into recipients' tmux windows. Low-tech but robust. Supervisor nudges stuck agents periodically.

**Isolation**: Every worker gets its own git worktree automatically. Zero file collision risk. Workers commit to their own branches and create PRs.

**Custom agents**: Defined in markdown files at `~/.multiclaude/repos/<repo>/agents/`. Persistent, version-controllable, shareable. Define role, focus areas, and behavior in markdown.

**Merge strategy**: CI-gated merge queue. In singleplayer mode, green CI = auto-merge. In multiplayer mode, human reviewers approve first. If CI goes red, the merge queue spawns a fix-it worker automatically.


## 2. Kaizen-Specific Requirements Matrix

| Requirement | Agent Teams | Multiclaude | Winner |
| --- | --- | --- | --- |
| **7 agents, module-isolated** | Spawn 7 teammates from one lead. All share working directory unless you manually add worktrees. | Each worker gets its own worktree automatically. Supervisor tracks all workers. Custom agent definitions persist across sessions. | **Multiclaude** |
| **Per-agent status files** | Teammates can write to `docs/coordination/status/agent-N-status.md` and read others'. But files are in the same working directory — concurrent writes could conflict. | Each worker is in its own worktree but shares git history. Status file writes happen on separate branches; merge queue handles convergence. | **Multiclaude** |
| **Proto schema changes (Sprint 5.0)** | One teammate can own proto changes. Others wait for task to unblock. Works well — the shared task list models this dependency. | Proto worker creates a PR. Merge queue merges it. Other workers' worktrees get rebased automatically by the daemon's worktree refresh loop. | **Multiclaude** (worktree refresh handles rebase automatically) |
| **Cross-agent contract tests** | Teammates can message each other: "I changed the MetricStakeholder proto, update your golden files." Direct peer communication is the strength. | Workers create PRs independently. Contract test failures appear as CI failures. Merge queue blocks the PR. Worker fixes or supervisor escalates. | **Agent Teams** (direct communication is faster than CI-mediated feedback) |
| **Long-running sprints (3 weeks)** | Sessions are ephemeral. A 3-week sprint cannot be one session. You'd run multiple team sessions per sprint, each covering a subset of milestones. Session resumption does not restore teammates. | Daemon persists. You can run Multiclaude for days. Workers self-destruct on task completion; supervisor spawns new ones for next tasks. Survives terminal disconnects via tmux. | **Multiclaude** (persistence is essential for multi-week sprints) |
| **Rust + Go + TypeScript codebase** | All teammates share the same environment. `cargo build`, `go build`, and `npm test` all run in the same directory. Concurrent builds could conflict. | Each worktree is independent. Worker building `crates/experimentation-stats` doesn't interfere with worker building `services/metrics/` (Go). | **Multiclaude** (worktree isolation prevents build conflicts) |
| **Human review before merge** | You interact with teammates directly. Review happens in-session. No PR-based workflow. | Multiplayer mode: workers create PRs, human reviewers approve, merge queue merges on approval + green CI. Native GitHub PR integration. | **Multiclaude** (PR-based review is more auditable and scalable) |
| **Cost / token efficiency** | ~5x token consumption (7 teammates × 1M context each). Each teammate starts fresh — no shared history. CLAUDE.md provides project context. | Each worker is a separate Claude Code process. Similar token cost per worker. But workers self-destruct on task completion, freeing tokens. Supervisor uses fewer tokens (coordination only). | **Roughly equal**, but Multiclaude reclaims tokens faster via worker self-destruct |
| **Observability** | In-process mode: all output in one terminal. Split-pane (tmux): one pane per teammate. Lead synthesizes status. | Each worker has its own tmux window. Supervisor provides status overview. `multiclaude status` shows all workers. | **Multiclaude** (purpose-built monitoring) |
| **Setup complexity** | One environment variable. Describe team in natural language. Zero installation beyond Claude Code. | Requires Go installation, tmux, gh CLI (authenticated). `multiclaude start`, `multiclaude repo init`. More moving parts. | **Agent Teams** (near-zero setup) |
| **Error recovery** | If a teammate hangs, the lead can nudge it. If the lead crashes, all teammates are lost. No automatic retry. | Daemon health-checks every 2 minutes. Dead workers are resurrected. If resurrection fails, cleanup is automatic. Merge queue spawns fix-it workers for CI failures. | **Multiclaude** (self-healing) |
| **ADR-specific work (statistical algorithms)** | Teammate implements AVLM in `avlm.rs`. Can message other teammates about interface decisions. Runs tests in shared directory. | Worker implements AVLM in its worktree. Creates PR. CI runs `cargo test`. If golden files need updating, worker handles it on its branch. | **Roughly equal** for isolated algorithmic work |
| **M7 Rust port (ADR-024)** | One teammate owns the entire port. Can coordinate with the M6 teammate about wire-format changes. | One worker owns the port. Creates PR with all changes. Reviewer agent checks wire-format compatibility. Merge queue blocks until CI green. | **Multiclaude** (port spans many files; worktree isolation prevents accidental interference) |


## 3. Kaizen-Specific Challenges

### Challenge 1: Proto Schema as Shared Contract

Sprint 5.0 requires all 7 agents to align on proto schema extensions. In Phases 0–4, this was solved by pre-seeding the schema before agents began work.

**Agent Teams approach**: Assign one "Proto Lead" teammate to make all schema changes. Other teammates wait (task dependency). Once the Proto Lead marks the task complete, downstream tasks auto-unblock. Teammates then work against the committed schema. Risk: if the Proto Lead makes a mistake, all downstream work is affected within the same working directory.

**Multiclaude approach**: Proto worker creates a branch, makes all schema changes, creates a PR. CI runs `buf breaking` + `buf lint`. On merge, the daemon's worktree refresh loop rebases all other workers to include the new schema. Each worker then works against the committed schema in their own worktree. Risk: rebase conflicts if workers started proto-dependent work before the schema PR merged.

**Verdict**: Multiclaude's CI-gated approach is safer. The schema is validated before any worker depends on it. Agent Teams' shared-directory model means a bad schema change silently breaks everyone.

### Challenge 2: experimentation-stats Crate Shared by Multiple ADRs

ADRs 015, 017, 018, 020, 021, 022, 023 all add modules to `experimentation-stats`. In Agent Teams, multiple teammates editing the same crate could create file conflicts. In Multiclaude, each worker has its own worktree, so they can all add new `.rs` files independently — but they'll need to coordinate on `mod.rs` / `lib.rs` declarations.

**Mitigation for both**: Structure the crate so each ADR adds a new file (e.g., `avlm.rs`, `evalue.rs`, `switchback.rs`) and only touches `lib.rs` to add `pub mod` declarations. Keep the `mod` declarations as the sole merge point.

### Challenge 3: Kaizen's 13-Crate Cargo Workspace

`cargo build` in the workspace root compiles all crates. Two agents editing different crates will trigger overlapping builds. In a shared directory (Agent Teams), this means lock file contention on `Cargo.lock` and `target/` directory conflicts.

**Agent Teams mitigation**: Each teammate runs `cargo build -p experimentation-stats` (single-crate build) rather than workspace-wide builds.

**Multiclaude**: Non-issue. Each worktree has its own `target/` directory.

### Challenge 4: Persistent Agent Identity Across Sprints

Kaizen's agents have persistent identities (Agent-1 through Agent-7) that span all 6 sprints. Agent-1 always owns M1. Agent-4 always owns M4a + M4b.

**Agent Teams**: No persistent identity. Each team session spawns fresh teammates from a prompt. You can make the prompt identical across sessions ("You are Agent-4, you own M4a and M4b, read your onboarding at docs/coordination/prompts/agent-4.md"), but the teammate doesn't *remember* previous sessions. CLAUDE.md + onboarding docs provide context, not continuity.

**Multiclaude**: Custom agent definitions in markdown persist at `~/.multiclaude/repos/<repo>/agents/`. You define `agent-4-stats.md` once, and it's reused across worker spawns. The supervisor knows the agent's role. But workers are still ephemeral — they self-destruct on task completion. The *definition* persists; the *instance* doesn't.

**Verdict**: Neither provides true persistent agent identity across multi-week sprints. Both rely on written context (CLAUDE.md, onboarding docs, agent definitions) to reconstruct the agent's perspective on each spawn.


## 4. Recommendation

**Use Multiclaude for Kaizen Phase 5, with Agent Teams as a fallback for ad-hoc collaboration.**

The deciding factors:

1. **Worktree isolation is non-negotiable for a 13-crate Rust workspace.** Concurrent `cargo build` in a shared directory will cause lock file contention, `target/` directory conflicts, and intermittent build failures. Multiclaude gives every worker its own worktree automatically. Agent Teams requires manual worktree setup per teammate.

2. **Persistence matters for 18-week sprints.** Multiclaude's daemon survives terminal disconnects, health-checks workers, and respawns failures. Agent Teams sessions are ephemeral — a 3-week sprint would require multiple session restarts with manual teammate re-spawning.

3. **CI-gated merge queue is the right ratchet for Kaizen.** The "CI passes → merge" model matches Kaizen's existing validation strategy (golden-file tests, proptest invariants, contract tests, `buf breaking`). Every ADR implementation lands as a PR that must pass the full test suite before merging. Agent Teams has no merge queue — you review in-session.

4. **Custom agent definitions in markdown align with Kaizen's existing per-agent onboarding docs.** You already have `docs/coordination/prompts/agent-{N}-*.md`. These translate directly to Multiclaude agent definitions.

5. **Multiplayer mode fits your review workflow.** You review PRs before they merge. Workers create PRs, you approve, merge queue ships. This is the "you become the reviewer who merges PRs and kicks off the next sprint session" model from the implementation plan.

**Where Agent Teams still wins**: Intra-sprint ad-hoc coordination. If Agent-4 and Agent-1 need to debug a contract test failure together, spawn a 2-person Agent Team for a focused collaborative session. This is Agent Teams' sweet spot — short, interactive problem-solving where direct peer messaging is faster than PR-mediated feedback.

### Proposed Hybrid Model

```
Multiclaude (persistent, PR-based)          Agent Teams (ephemeral, collaborative)
─────────────────────────────────           ──────────────────────────────────────
Sprint-level orchestration                  Contract test debugging sessions
Worker per ADR milestone                    Cross-agent design discussions
CI-gated merge queue                        Proto schema review sessions
Per-agent markdown definitions              Quick pair-programming on shared interfaces
Worktree-isolated builds
Status tracked via PRs + supervisor
```

### Setup for Kaizen

```bash
# Install Multiclaude
go install github.com/dlorenc/multiclaude/cmd/multiclaude@latest

# Initialize for Kaizen repo
multiclaude start
multiclaude repo init https://github.com/your-org/kaizen

# Create persistent agent definitions
mkdir -p .multiclaude/agents/

# Create one markdown file per Kaizen agent
# (adapt from existing docs/coordination/prompts/agent-N-*.md)
```

Agent definition example (`.multiclaude/agents/agent-4-stats.md`):

```markdown
# Agent-4: Statistical Analysis & Bandit Policy

You own M4a (Statistical Analysis Engine) and M4b (Bandit Policy Service).
All work happens in Rust, in the `experimentation-stats` and
`experimentation-bandit` crates.

## Phase 5 Responsibilities
- ADR-015: AVLM implementation in `avlm.rs`
- ADR-017: TC/JIVE in `orl.rs`, ORL estimator
- ADR-018: E-values in `evalue.rs`, MAD in `mad.rs`
- ADR-011: Multi-objective reward on LMAX core
- ADR-012: LP constraint solver on LMAX core
- ADR-016: Slate bandit policy state
- ADR-020: Adaptive sample size in `adaptive_n.rs`
- ADR-021: Feedback loop detection in `feedback_loop.rs`
- ADR-022: Switchback HAC in `switchback.rs`
- ADR-023: Synthetic control in `synthetic_control.rs`

## Rules
- Run `cargo test -p experimentation-stats` before creating PR.
- Golden-file tests required for every new method. Validate against
  reference R packages to 4 decimal places.
- Add proptest invariants for every public function.
- Write status to `docs/coordination/status/agent-4-status.md`.
- Read other agents' status files before starting dependent work.
- All floating-point paths must use `assert_finite!()`.
```

Sprint execution:

```bash
# Sprint 5.0: Create workers for P0 items
multiclaude worker create "Implement AVLM (ADR-015) in experimentation-stats. See docs/adrs/015-anytime-valid-regression-adjustment.md" --agent agent-4-stats
multiclaude worker create "Implement TC/JIVE (ADR-017 Phase 1). See docs/adrs/017-offline-rl-long-term-effects.md" --agent agent-4-stats
multiclaude worker create "Port M7 to Rust (ADR-024). See docs/adrs/024-m7-rust-port.md" --agent agent-7-flags
multiclaude worker create "Land Phase 5 proto schema extensions. See design_doc_v7.0.md Section 3.6" --agent agent-proto

# Detach and let them work
tmux detach  # Ctrl-b d

# Check in later
multiclaude status
```

### Cost Estimate

Per sprint (3 weeks), running 4–5 workers concurrently:
- Each worker: ~$15–30 in API tokens (Opus 4.6, complex multi-file tasks)
- Supervisor: ~$5 per sprint (coordination overhead)
- Total per sprint: ~$80–160
- Phase 5 total (6 sprints): ~$500–1,000

On Claude Max ($200/month), you get extended usage limits. Running 3–4 workers concurrently is sustainable on a single Max subscription. For 7 concurrent workers, you may need multiple accounts or API access with usage-based billing.


## 5. Migration Path from Phase 1–4 Coordination Model

| Phase 1–4 Pattern | Phase 5 Multiclaude Equivalent |
| --- | --- |
| Per-agent status files (`docs/coordination/status/agent-N.md`) | Workers write status files in their worktree; merged to main via PR. Other workers read from main. |
| Continuation prompt templates (`docs/coordination/continuation-prompts.md`) | Custom agent markdown definitions at `.multiclaude/agents/`. Loaded automatically on worker spawn. |
| Coordinator playbook (merge → resolve → advance) | Supervisor + merge queue automate the merge→resolve cycle. You advance by creating new workers for the next sprint's milestones. |
| Branch naming (`agent-N/<type>/<description>`) | Workers auto-create branches. Configure naming convention in agent definition. |
| Pair integration (Agent-X ↔ Agent-Y contract tests) | Both workers create PRs touching shared contracts. CI runs contract tests. Merge queue blocks incompatible changes. For interactive debugging, spawn an Agent Teams session. |
