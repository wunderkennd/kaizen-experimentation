# Phase 5 Coordinator Playbook

## Overview

Phase 5 uses a hybrid orchestration model:
- **Multiclaude** for persistent sprint-level work (workers own worktrees, CI gates merges)
- **Agent Teams** for ephemeral cross-agent collaboration (contract test debugging, design sessions)

Your role as coordinator: review PRs, kick off sprints, resolve cross-agent conflicts, and evaluate ADR-025 trigger.


## 1. Initial Setup

### Prerequisites
```bash
# Multiclaude
go install github.com/dlorenc/multiclaude/cmd/multiclaude@latest

# Required tools
which tmux gh git   # all three required
gh auth status      # must be authenticated

# Agent Teams (for ad-hoc sessions)
# Add to .claude/settings.json:
# { "env": { "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1" } }
```

### Repository Initialization
```bash
# Start Multiclaude daemon
multiclaude start

# Initialize repo (multiplayer mode — you review PRs)
multiclaude repo init https://github.com/your-org/kaizen

# Copy agent definitions
cp .multiclaude/agents/agent-*.md  # from this coordination package

# Verify CLAUDE.md is at repo root
cat CLAUDE.md | head -5   # should show "Kaizen Experimentation Platform"
```

### Verify Agent Definitions
```bash
ls .multiclaude/agents/
# Should show:
# agent-1-assignment.md
# agent-2-pipeline.md
# agent-3-metrics.md
# agent-4-analysis-bandit.md
# agent-5-management.md
# agent-6-ui.md
# agent-7-flags.md
```


## 2. Sprint Execution

### Starting a Sprint

Each sprint runs for ~3 weeks. Create workers for all milestones in the sprint:

```bash
# Sprint 5.0 Example
# ==================

# P0: AVLM implementation
multiclaude worker create \
  "Implement AVLM (ADR-015) in experimentation-stats/src/avlm.rs. \
   Read docs/adrs/015-anytime-valid-regression-adjustment.md. \
   Implement AvlmSequentialTest with O(1) update. \
   Golden-file validation against R avlm package to 4 decimal places. \
   Add proptest invariant: CS covers true parameter at rate >= (1-alpha). \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

# P0: TC/JIVE surrogate calibration fix
multiclaude worker create \
  "Implement TC/JIVE de-biased surrogate calibration (ADR-017 Phase 1) \
   in experimentation-stats/src/orl.rs. \
   Read docs/adrs/017-offline-rl-long-term-effects.md. \
   Cross-fold IV estimation replacing R²-based calibration. \
   Update SurrogateModelConfig proto with JIVE fields. \
   Write status to docs/coordination/status/agent-4-status.md." \
  --agent agent-4-analysis-bandit

# P0: M7 Rust port scaffold
multiclaude worker create \
  "Port M7 Feature Flag Service from Go to Rust (ADR-024). \
   Read docs/adrs/024-m7-rust-port.md. \
   Sprint 5.0 scope: scaffold crates/experimentation-flags/, \
   tonic-web service, sqlx PostgreSQL, flag CRUD RPCs. \
   Wire-format contract tests against Go M7. \
   Write status to docs/coordination/status/agent-7-status.md." \
  --agent agent-7-flags

# Proto schema: land all Phase 5 extensions
multiclaude worker create \
  "Land Phase 5 proto schema extensions across proto/experimentation/. \
   Read design_doc_v7.0.md Section 3.6 for full list. \
   New experiment types (META=9, SWITCHBACK=10, QUASI=11). \
   Bandit extensions (RewardObjective, SlateConfig, etc). \
   Metric extensions (MetricStakeholder, MetricAggregationLevel). \
   Analysis extensions (SEQUENTIAL_METHOD_AVLM). \
   Run buf lint and buf breaking before PR. \
   This blocks all other Phase 5 work — priority." \
  --agent agent-4-analysis-bandit
```

### Monitoring During Sprint

```bash
# Check all worker status
multiclaude status

# Attach to tmux session to watch workers
tmux attach -t mc-kaizen

# Navigate between workers: Ctrl-b + arrow keys
# Detach: Ctrl-b d

# Check agent status files (merged to main by workers)
cat docs/coordination/status/agent-4-status.md
cat docs/coordination/status/agent-7-status.md
```

### Reviewing PRs (Multiplayer Mode)

Workers create PRs automatically. Your review process:

1. **Check CI status**: green CI is the minimum bar. Tests must pass.
2. **Review the diff**: focus on architectural decisions, not syntax.
3. **Check status file**: worker should have updated their agent status file.
4. **Check contract tests**: cross-module PRs should include contract tests.
5. **Approve → merge queue ships it**.

```bash
# List open PRs from workers
gh pr list --label multiclaude

# Review a specific PR
gh pr view <number>
gh pr diff <number>

# Approve and let merge queue handle it
gh pr review <number> --approve
```

### Handling Stuck Workers

If a worker is stuck (no progress for > 30 minutes):

```bash
# Check what the worker is doing
tmux attach -t mc-kaizen
# Navigate to the stuck worker's window

# Option 1: Supervisor nudges automatically (every 2 min)
# Just wait — the health check loop handles this

# Option 2: Manual intervention
# Type directly into the worker's tmux window:
# "You seem stuck on X. Try Y approach instead."

# Option 3: Kill and respawn
multiclaude worker kill <worker-name>
multiclaude worker create "<refined task description>" --agent <agent>
```


## 3. Agent Teams Sessions (Ad-Hoc Collaboration)

Use Agent Teams when two or more agents need to collaborate interactively — contract test debugging, proto schema design, interface negotiation.

### Example: Contract Test Debugging

```bash
# In a separate terminal (not the Multiclaude tmux session)
export CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1
claude

# Then in the Claude session:
> Create an agent team to debug the M4b ↔ M1 slate assignment contract test failure.
> Spawn 2 teammates:
> - Agent-4-Debug: Owns M4b side. Check experimentation-bandit slate policy output format.
> - Agent-1-Debug: Owns M1 side. Check GetSlateAssignment request/response serialization.
> Have them message each other about the exact JSON they're producing vs. expecting.
> The test is at tests/contract/m1_m4b_slate_test.rs.
```

### Example: Proto Schema Review

```bash
> Create a 3-person agent team to review the Phase 5 proto extensions before landing.
> Teammate 1 (Proto Author): Walk through all changes in proto/experimentation/.
> Teammate 2 (Consumer): Check that M6 TypeScript codegen works with the new types.
> Teammate 3 (Reviewer): Verify backward compatibility, buf breaking, naming conventions.
```

### When to Use Agent Teams vs. Multiclaude

| Situation | Use |
| --- | --- |
| Implementing an ADR milestone | Multiclaude worker |
| Debugging a cross-module contract test | Agent Teams (2–3 agents) |
| Designing a shared proto interface | Agent Teams (2–4 agents) |
| Reviewing a large PR interactively | Agent Teams (2 agents: author + reviewer) |
| Running the full sprint | Multiclaude |
| Quick pair-programming on a shared file | Agent Teams (2 agents) |


## 4. Sprint Transition

### End-of-Sprint Checklist

1. **All PRs merged or deferred**: check `gh pr list --label multiclaude`
2. **Status files current**: verify each agent-N-status.md has been updated
3. **Contract tests passing**: `cargo test --workspace` + `go test ./...` + `cd ui && npm test`
4. **Proto schema clean**: `buf lint` + `buf breaking`
5. **No dependency deadlocks**: check for circular waits in status files

### Starting Next Sprint

```bash
# Shut down current workers
multiclaude worker kill --all

# Pull latest main (includes all merged PRs from this sprint)
git pull origin main

# Create workers for next sprint milestones
# (Use sprint plan from docs/coordination/phase5-implementation-plan.md)
multiclaude worker create "..." --agent agent-N-*
```


## 5. ADR-025 Trigger Evaluation (End of Sprint 5.5)

At the end of Sprint 5.5, evaluate whether M5 should be ported to Rust:

```
Count completed ADRs from {015 P2, 018, 019, 020, 021}:
  - ADR-015 P2 (MLRATE): [complete? y/n]
  - ADR-018 (E-Values/FDR): [complete? y/n]
  - ADR-019 (Portfolio): [complete? y/n]
  - ADR-020 (Adaptive N): [complete? y/n]
  - ADR-021 (Feedback Loops): [complete? y/n]

If >= 3 complete: TRIGGER — plan M5 Rust port as Sprint 5.6
If < 3 complete: DEFER — keep Go M5, use M4a RPCs for statistical computation
```


## 6. Cost Management

### Token Budget per Sprint
- 4–5 concurrent workers: ~$80–160 per sprint
- Supervisor overhead: ~$5 per sprint
- Ad-hoc Agent Teams sessions: ~$10–20 per sprint
- **Total per sprint**: ~$100–180
- **Phase 5 total (6 sprints)**: ~$600–1,100

### Cost Reduction Tactics
- Kill idle workers promptly (they consume tokens even when waiting)
- Use Sonnet for simple tasks (status file updates, minor fixes)
- Reserve Opus for complex algorithmic work (AVLM, ORL, LP solver)
- Limit Agent Teams sessions to genuine collaboration needs (not solo work)


## 7. Troubleshooting

| Problem | Solution |
| --- | --- |
| Worker creates PR but CI fails | Merge queue spawns fix-it worker automatically. If it persists, review the test failure and refine the worker's task description. |
| Two workers edit the same file | Multiclaude worktrees prevent this for most files. If it happens (e.g., both edit `lib.rs` to add `pub mod`), resolve the merge conflict manually or let the merge queue's worktree refresh handle rebase. |
| Worker goes off-track | Type directly into its tmux window with a correction. Or kill and respawn with a refined prompt. |
| Agent Teams session loses teammates | Known limitation — `/resume` doesn't restore teammates. Spawn fresh. |
| Multiclaude daemon dies | `multiclaude start` restarts it. Workers in tmux survive daemon restart. |
| Proto schema change breaks downstream workers | Workers should wait for the proto PR to merge before starting dependent work. The daemon's worktree refresh loop rebases workers after merge. |
