# GitHub Issues Workflow

Work tracking for Phase 5 uses GitHub Milestones, Issues, and Labels. This replaces the per-agent status markdown files used in Phases 0–4.

## Why Issues Instead of Status Files

Status markdown files (`docs/coordination/status/agent-N-status.md`) worked in Phases 0–4 but caused friction at scale:

| Problem with Status Files | How Issues Solve It |
| --- | --- |
| Merge conflicts on every rebase (7 agents × frequent updates) | Issues live in GitHub's database, not in the repo — zero merge conflicts |
| Stale state when workers crash before committing | Issue comments persist even if the worker dies mid-task |
| No queryability (grep-based filtering only) | `gh issue list` with filters by milestone, label, assignee, state |
| No dependency tracking between tasks | Issues reference each other; `blocked` label + comments explain the blocker |
| No integration with PRs | `Closes #N` in PR description auto-closes the issue on merge |
| Invisible to GitHub's UI, projects, and automation | Full GitHub Projects board support, webhook integration |

## Structure

```
Milestone (Sprint)
├── Issue (ADR task)           — assigned to agent, labeled with priority + cluster
│   ├── Acceptance criteria    — checkboxes in Issue body
│   ├── Agent comments         — progress updates
│   └── Linked PR              — "Closes #N" auto-closes on merge
└── Issue (ADR task)
    └── ...
```

## Milestones = Sprints

Each Phase 5 sprint is a GitHub Milestone with a due date:

| Milestone | Due Date | Focus |
| --- | --- | --- |
| Sprint 5.0: Schema & Foundations | Week 3 | Proto extensions, AVLM, TC/JIVE, M7 port scaffold, e-values |
| Sprint 5.1: Measurement Foundations | Week 6 | Provider metrics, ModelRetrainingEvent, M7 port complete |
| Sprint 5.2: Statistical Core | Week 9 | AVLM integration, multi-objective reward, adaptive N, feedback loops |
| Sprint 5.3: Constraints & New Experiment Types | Week 12 | LP constraints, switchback, synthetic control, e-LOND |
| Sprint 5.4: Slate Bandits & Meta-Experiments | Week 15 | Slate bandits, meta-experiments, portfolio dashboard |
| Sprint 5.5: Advanced & Integration | Week 18 | ORL P2, MLRATE, MAD, full integration tests |

Create milestones:
```bash
gh api repos/OWNER/REPO/milestones \
  -f title="Sprint 5.0: Schema & Foundations" \
  -f due_on="2026-04-15T00:00:00Z"
```

Or use the bootstrap script: `./scripts/create-phase5-issues.sh owner/repo`

## Issues = Tasks

Each Issue represents one implementation unit — typically one ADR or one phase of a multi-phase ADR. The Issue body serves as the task specification that agents read before starting work.

### Issue Body Format

```markdown
## Summary
[What this task does and why]

## Specification
Read `docs/adrs/NNN-*.md`

## Acceptance Criteria
- [ ] Criterion 1
- [ ] Criterion 2
- [ ] Tests pass: `cargo test -p <crate>`
- [ ] Golden-file validated against [reference] to [N] decimal places

## Agent
Agent-N (Module Name)

## ADR
ADR-NNN [Phase N if multi-phase]

## Blocks / Blocked By
Blocks: #45, #48
Blocked by: #40 (proto schema must land first)
```

### Creating Issues

```bash
# From the command line
gh issue create \
  --milestone "Sprint 5.0: Schema & Foundations" \
  --title "ADR-015: AVLM Implementation (Phase 1)" \
  --label "P0,agent-4,cluster-b" \
  --body "$(cat docs/issue-bodies/adr-015-avlm.md)"

# Or interactively
gh issue create
```

## Labels

### Agent Ownership
`agent-1` through `agent-7` — who owns this Issue.

### Priority
`P0` (highest) through `P4` (lowest) — matches the ADR README implementation sequence.

### Cluster
`cluster-a` through `cluster-f` — which capability cluster.

### Status
`blocked` — waiting on another Issue or external dependency. Must include a comment explaining the blocker.

### Type
`contract-test` — cross-module contract test work.

Create all labels:
```bash
for label in agent-{1..7} P{0..4} cluster-{a..f} blocked contract-test; do
  gh label create "$label" 2>/dev/null || true
done
```

## Agent Workflow

### Starting Work

```bash
# Find your assigned issues
gh issue list --assignee @me --state open

# Read the task spec
gh issue view 42

# Create your branch
git checkout -b agent-4/feat/adr-015-avlm main

# Comment that you're starting
gh issue comment 42 --body "Starting implementation. Branch: agent-4/feat/adr-015-avlm"
```

### Updating Progress

```bash
# Post progress update as a comment
gh issue comment 42 --body "AvlmSequentialTest struct implemented. Working on golden-file tests next."

# If blocked, add label and explain
gh issue edit 42 --add-label "blocked"
gh issue comment 42 --body "Blocked by #40 (proto schema). Need SequentialMethod::AVLM enum value."
```

### Completing Work

```bash
# Create PR that references the issue
gh pr create \
  --title "feat(experimentation-stats): implement AVLM confidence sequences" \
  --body "Implements ADR-015 Phase 1 AVLM.

Closes #42"

# When the PR merges, Issue #42 auto-closes
```

### If You Crash Mid-Task

The Issue persists with all your comments. The next worker (or you, restarted) can:
```bash
# Read what was done before the crash
gh issue view 42 --comments

# Pick up where the previous worker left off
```

This is the main advantage over status files — no state is lost on crash.

## Querying for Status

These replace reading 7 markdown status files:

```bash
# Sprint overview (replaces reading all status files)
gh issue list --milestone "Sprint 5.0: Schema & Foundations" \
  --json number,title,state,assignees,labels \
  --jq '.[] | "\(.state)\t\(.assignees | map(.login) | join(","))\t#\(.number)\t\(.title)"'

# What's blocked?
gh issue list --label "blocked" --state open

# Agent-4's open work
gh issue list --label "agent-4" --state open

# P0 items not yet done
gh issue list --label "P0" --state open

# Recently closed (what shipped)
gh issue list --milestone "Sprint 5.0: Schema & Foundations" --state closed

# Cross-module contract tests
gh issue list --label "contract-test" --state open
```

## Launching Workers from Issues

### Multiclaude

```bash
# Read issue body, pass as worker task
TASK=$(gh issue view 42 --json body -q '.body')
multiclaude worker create "$TASK"
```

### Gas Town

Tell the Mayor:
```
"Pick up Issue #42 from the kaizen rig. Read the acceptance criteria and implement it."
```

The Mayor runs `gh issue view 42` to read the spec, then slings the work to a polecat.

### Jules

```bash
# Dispatch Issue to Jules as a remote task
TASK=$(gh issue view 42 --json title,body -q '"\(.title)\n\n\(.body)"')
jules remote new --repo your-org/kaizen --session "$TASK"
```

### Devin

Copy the Issue body into Devin's task prompt via the web UI or Slack integration.

## Justfile Integration

```just
# Show current sprint status
sprint-status milestone="Sprint 5.0: Schema & Foundations":
    gh issue list --milestone "{{milestone}}" \
      --json number,title,state,assignees,labels \
      --jq '.[] | "\(.state)\t\(.assignees | map(.login) | join(","))\t#\(.number)\t\(.title)"'

# Show blocked issues
blocked:
    gh issue list --label "blocked" --state open

# Launch a Multiclaude worker from an Issue number
work-on issue:
    #!/usr/bin/env bash
    TASK=$(gh issue view {{issue}} --json body -q '.body')
    echo "=== Launching worker for Issue #{{issue}} ==="
    multiclaude worker create "$TASK"
```

## Migration from Status Files

If transitioning from the markdown status file model:

1. Create milestones and issues using the bootstrap script
2. Delete `docs/coordination/status/` directory
3. Remove status file references from `.gitattributes` (the `theirs-status` merge driver)
4. Update `.multiclaude/agents/*.md` — remove "Write status to docs/coordination/status/" instructions, replace with "Comment progress on the GitHub Issue and include `Closes #N` in your PR"
5. Update CLAUDE.md, CONTRIBUTING.md, and playbook references
