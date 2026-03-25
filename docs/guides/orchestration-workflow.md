# Multi-Tool Orchestration Workflow

This guide describes the daily development rhythm for Phase 5 using five orchestration tools, each with a distinct role.

## Tool Roles

| Tool | Strength | Kaizen Role | Compute |
| --- | --- | --- | --- |
| Gas Town | Interactive steering, Mayor coordinates polecats | Primary ADR implementation, design decisions, debugging | Local (Claude tokens) |
| Multiclaude | Autonomous daemon, CI-gated merge, self-healing workers | Overnight grinding on well-specified tasks | Local (Claude tokens) |
| Jules | Async cloud VMs, GitHub-native, scriptable CLI | Scheduled maintenance, test generation, dependency bumps | Google Cloud (Jules tokens) |
| Devin | Full autonomy on bounded tasks, sandboxed environment | Test coverage, migrations, golden-file generation | Cognition Cloud (Devin ACUs) |
| Gemini CLI | Fast, cheap, works alongside other tools | Research, second-opinion code review | Google API |

The key principle: Claude/Gas Town for thinking, Jules for maintaining, Devin for grinding, Gemini for checking.

## Daily Rhythm

### Morning — Check Overnight Results

```bash
just morning
```

This command:
1. Checks Multiclaude worker status
2. Lists open PRs from all sources
3. Shows current sprint Issues (open, closed, blocked)
4. Pulls latest main

Review and merge green PRs. Close stale PRs with `just pr-triage`.

```bash
# Quick sprint status
gh issue list --milestone "Sprint 5.0: Schema & Foundations" \
  --json number,title,state,assignees \
  --jq '.[] | "\(.state)\t#\(.number)\t\(.title)"'
```

### Daytime — Interactive Development (Gas Town)

```bash
just interactive
```

Attaches to the Gas Town Mayor. Tell the Mayor which Issues to work on:

```
"Pick up Issue #42 (ADR-015 AVLM) and sling it to a polecat"
"What's the status of Issue #38 (M7 port)?"
"Issues #42 and #45 have a contract test dependency — coordinate them"
```

While Gas Town runs, dispatch bounded tasks to other tools:

```bash
# Jules: test coverage (runs in Google Cloud)
jules remote new --repo your-org/kaizen \
  --session "Write unit tests for crates/experimentation-stats/src/avlm.rs. Target 80% coverage."

# Gemini: second opinion
gemini -p "Review this implementation for correctness: $(cat crates/experimentation-stats/src/avlm.rs)"
```

### Evening — Handoff to Autonomous Mode

```bash
just evening <sprint_number>
```

Stops Gas Town, pulls main, launches Multiclaude workers. Each worker reads its task from a GitHub Issue:

```bash
# Launch worker from Issue #42
just work-on 42
```

Detach tmux and workers continue overnight. When a PR merges with `Closes #42`, the Issue auto-closes.

## Sprint Execution

```bash
just autonomous-sprint 0    # Sprint 5.0
just autonomous-sprint 1    # Sprint 5.1
# ... through 5
```

Each sprint launcher creates Multiclaude workers whose task descriptions are derived from the GitHub Issues in that sprint's Milestone.

## Jules GitHub Actions

Set up once, runs automatically:

```yaml
# .github/workflows/jules-weekly-maintenance.yml
name: Weekly Maintenance
on:
  schedule:
    - cron: '0 3 * * 1'
jobs:
  maintenance:
    runs-on: ubuntu-latest
    steps:
      - uses: google-labs-code/jules-invoke@v1
        with:
          prompt: |
            Read CLAUDE.md for project context.
            1. Run cargo outdated and update patch-level dependencies
            2. Run cargo clippy --workspace and fix new warnings
            3. Run go vet ./... and fix new issues
            4. Only open a PR if all tests pass
          jules_api_key: ${{ secrets.JULES_API_KEY }}
```

## Crash Recovery

```bash
just pr-triage              # Clean up orphaned PRs
git worktree prune          # Clean up orphaned worktrees
multiclaude start           # Restart daemon
cd ~/gt && gt up && gt doctor --fix   # Restart Gas Town

# Issues persist — check where things left off
gh issue list --milestone "Sprint 5.0: Schema & Foundations" --state open

# Resume work
just autonomous-sprint <current>  # or: just interactive
```

Unlike status files, no state is lost on crash. Issue comments and acceptance criteria checkboxes persist in GitHub regardless of what happens to your local machine.

## Cost Management

| Tool | Cost Model | Phase 5 Estimate |
| --- | --- | --- |
| Gas Town + Claude Code | Claude Max ($200/mo) | Daytime sessions |
| Multiclaude | Same Claude Max | Overnight (shared budget) |
| Jules | Google AI Ultra ($125/mo) or free tier | Weekly maintenance + ad-hoc |
| Devin | $20/mo Core + $2.25/ACU | ~10–20 ACUs/sprint |
| Gemini CLI | Free tier or Google AI sub | Negligible |

Budget ~$300–400/month total.
