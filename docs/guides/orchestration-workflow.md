# Multi-Tool Orchestration Workflow

This guide describes the daily development rhythm for Phase 5 using five orchestration tools, each with a distinct role.

## Tool Roles

| Tool | Strength | Kaizen Role | Compute |
| --- | --- | --- | --- |
| Gas Town | Interactive steering, Mayor coordinates polecats in real time | Primary ADR implementation, design decisions, debugging | Local (your machine, Claude tokens) |
| Multiclaude | Autonomous daemon, CI-gated merge, self-healing workers | Overnight grinding on well-specified tasks | Local (your machine, Claude tokens) |
| Jules | Async cloud VMs, GitHub-native, scriptable CLI, GitHub Actions | Scheduled maintenance, test generation, dependency bumps | Google Cloud (Jules tokens) |
| Devin | Full autonomy on bounded tasks, sandboxed environment | Test coverage, migrations, golden-file generation, repetitive refactoring | Cognition Cloud (Devin ACUs) |
| Gemini CLI | Fast, cheap, works alongside other tools | Research, second-opinion code review, ad-hoc lookups | Google API |

The key principle: Claude/Gas Town for thinking, Jules for maintaining, Devin for grinding, Gemini for checking. Each runs on its own compute — they genuinely parallelize without competing for the same token budget.

## Daily Rhythm

### Morning — Check Overnight Results

```bash
just morning
```

This command:
1. Checks Multiclaude worker status (what merged overnight)
2. Lists open PRs from all sources (Multiclaude, Jules, Devin)
3. Summarizes agent status files
4. Pulls latest main

Review and merge green PRs. Close any stale or broken PRs from crashed workers. Use the pr-triage subagent for batch cleanup:

```bash
claude -p "Use the pr-triage agent to clean up open PRs"
```

### Daytime — Interactive Development (Gas Town)

```bash
just interactive
```

This attaches you to the Gas Town Mayor. The Mayor is your primary interface for all interactive work. Tell it what you want to accomplish — it spawns polecats, coordinates work, and reports results.

Example Mayor interactions:
```
"Create a convoy for ADR-015 AVLM and sling it to a polecat"
"What's the status of the M7 port?"
"Agent-4 and Agent-1 need to coordinate on the slate proto — set up a convoy"
"Show me open beads for the kaizen rig"
```

While Gas Town runs, dispatch bounded tasks to other tools in parallel:

```bash
# Jules: test coverage for a specific crate (runs in Google Cloud)
jules remote new --repo your-org/kaizen \
  --session "Write unit tests for crates/experimentation-stats/src/avlm.rs. Target 80% coverage."

# Devin: golden-file generation (runs in Cognition sandbox)
# Submit via Devin web UI or Slack:
# "Generate golden-file tests in crates/experimentation-stats/tests/golden/
#  for the switchback HAC estimator. Reference: DoorDash sandwich estimator.
#  Precision: 4 decimal places."

# Gemini: quick second opinion without consuming Claude context
gemini -p "Review this confidence sequence implementation for correctness: $(cat crates/experimentation-stats/src/avlm.rs)"
```

### Evening — Handoff to Autonomous Mode

```bash
just evening <sprint_number>
```

This command:
1. Stops Gas Town gracefully
2. Pulls latest main
3. Launches Multiclaude workers for the specified sprint
4. Optionally dispatch Jules tasks for overnight cloud execution

```bash
# Additional Jules tasks for overnight
jules remote new --repo your-org/kaizen \
  --session "Write proptest invariants for crates/experimentation-stats/src/evalue.rs"
```

Detach tmux (`Ctrl-b d`) and the workers continue overnight.

## Sprint Execution

Each sprint runs for ~3 weeks. Launch all workers with one command:

```bash
just autonomous-sprint 0    # Sprint 5.0: Schema & Foundations
just autonomous-sprint 1    # Sprint 5.1: Measurement Foundations
just autonomous-sprint 2    # Sprint 5.2: Statistical Core
just autonomous-sprint 3    # Sprint 5.3: Constraints & New Experiment Types
just autonomous-sprint 4    # Sprint 5.4: Slate Bandits & Meta-Experiments
just autonomous-sprint 5    # Sprint 5.5: Advanced & Integration
```

See `docs/coordination/sprint-prompts.md` for the full task descriptions embedded in each sprint launcher.

## Jules GitHub Actions (Continuous)

Set up once, runs automatically:

```yaml
# .github/workflows/jules-weekly-maintenance.yml
name: Weekly Maintenance
on:
  schedule:
    - cron: '0 3 * * 1'   # Monday 3 AM
jobs:
  maintenance:
    runs-on: ubuntu-latest
    steps:
      - uses: google-labs-code/jules-invoke@v1
        with:
          prompt: |
            Read CLAUDE.md for project context.
            1. Run cargo outdated and update patch-level dependencies
            2. Run buf breaking proto/ --against origin/main
            3. Run cargo clippy --workspace and fix new warnings
            4. Run go vet ./... and fix new issues
            5. Only open a PR if all tests pass
          jules_api_key: ${{ secrets.JULES_API_KEY }}
```

```yaml
# .github/workflows/jules-test-coverage.yml
name: Test Coverage Boost
on:
  workflow_dispatch:
    inputs:
      crate:
        description: 'Target crate'
        required: true
jobs:
  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: google-labs-code/jules-invoke@v1
        with:
          prompt: |
            Write additional unit tests for crates/${{ inputs.crate }}/.
            Focus on untested public functions and edge cases.
            Target >= 80% line coverage.
            Run cargo test -p ${{ inputs.crate }} before opening PR.
            Do not modify source code — tests only.
          jules_api_key: ${{ secrets.JULES_API_KEY }}
```

Trigger the coverage workflow manually:
```bash
gh workflow run jules-test-coverage.yml -f crate=experimentation-stats
```

## Devin Best Practices

Devin works best on tasks with clear inputs and verifiable outputs. Good Devin tasks for Kaizen:

- Port a specific Go function to Rust with wire-format contract tests
- Generate golden-file tests from reference R/Python package output
- Write SQL migration scaffolds from ADR specifications
- Bump test coverage for a specific module to a target percentage
- Generate docstrings for all public APIs in a crate

Avoid using Devin for architectural decisions, ambiguous requirements, or work that spans multiple modules without clear boundaries.

## Crash Recovery

If your machine restarts unexpectedly:

```bash
# 1. Triage orphaned PRs
just pr-triage

# 2. Clean up orphaned worktrees
git worktree prune

# 3. Restart orchestration
multiclaude start
cd ~/gt && gt up && gt doctor --fix

# 4. Check where agents left off
just status

# 5. Resume the current sprint
just autonomous-sprint <current_sprint>
# or
just interactive
```

See `docs/guides/pr-triage-and-cleanup.md` for detailed crash recovery procedures.

## Cost Management

| Tool | Cost Model | Phase 5 Estimate |
| --- | --- | --- |
| Gas Town + Claude Code | Claude Max subscription ($200/mo) | Daytime sessions |
| Multiclaude | Same Claude Max subscription | Overnight (shared budget) |
| Jules | Google AI Ultra ($125/mo) or free tier (15 tasks/day) | Weekly maintenance + ad-hoc |
| Devin | $20/mo Core + $2.25/ACU | ~10–20 ACUs/sprint for bounded tasks |
| Gemini CLI | Free tier or Google AI subscription | Negligible |

Budget ~$300–400/month total across all tools for active Phase 5 development.
