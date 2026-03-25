# PR Triage and Cleanup

This guide covers managing PR accumulation — after crashes, sprint transitions, or when workers create more PRs than you can review in real time.

## When to Triage

- After a system crash or forced restart (orphaned worker PRs)
- At sprint boundaries (closing incomplete work from the previous sprint)
- When `gh pr list` shows > 10 open PRs
- After switching orchestration tools (e.g., Gas Town → Multiclaude handoff)

## Quick Triage with the Subagent

The fastest path is the PR triage subagent at `.claude/agents/pr-triage.md`:

```bash
claude -p "Use the pr-triage agent to clean up open PRs"
```

Or via the justfile:
```bash
just pr-triage
```

The subagent inventories all open PRs, categorizes them into MERGE / CLOSE / REVIEW buckets, presents a summary, and waits for your confirmation before acting.

## Manual Triage

### Step 1: Inventory

```bash
# List all open PRs with CI status
gh pr list --limit 100 --json number,title,headRefName,createdAt,additions,deletions,statusCheckRollup
```

### Step 2: Categorize

**MERGE** — CI green, branch name follows `agent-N/feat/adr-XXX-*` pattern, diff looks complete:
```bash
gh pr diff <number>        # Quick review
gh pr merge <number> --squash --delete-branch
```

**CLOSE** — CI failed, no CI run (worker crashed), stale, or worker-named branch with unclear scope:
```bash
gh pr close <number> --comment "Closed: worker interrupted. Will re-dispatch." --delete-branch
```

**REVIEW** — CI green but touches critical paths (proto/, experimentation-stats LMAX core, large diffs):
```bash
gh pr diff <number>        # Detailed review required
```

### Step 3: Batch Cleanup Script

For crash recovery with many orphaned PRs:

```bash
#!/usr/bin/env bash
set -euo pipefail

git checkout main && git pull origin main

prs=$(gh pr list --limit 100 --json number,title,headRefName,statusCheckRollup)

echo "--- CI GREEN ---"
echo "$prs" | jq -r '.[] | select(
  .statusCheckRollup != null and
  (.statusCheckRollup | length > 0) and
  ([.statusCheckRollup[] | select(.conclusion == "SUCCESS")] | length > 0)
) | "  #\(.number)  \(.headRefName)"'

echo ""
echo "--- CI RED / NO RUN ---"
echo "$prs" | jq -r '.[] | select(
  .statusCheckRollup == null or
  (.statusCheckRollup | length == 0) or
  ([.statusCheckRollup[] | select(.conclusion == "SUCCESS")] | length == 0)
) | "  #\(.number)  \(.headRefName)"'

echo ""
read -p "Close all RED/NO-CI PRs and delete branches? (y/n) " confirm
if [ "$confirm" = "y" ]; then
  echo "$prs" | jq -r '.[] | select(
    .statusCheckRollup == null or
    (.statusCheckRollup | length == 0) or
    ([.statusCheckRollup[] | select(.conclusion == "SUCCESS")] | length == 0)
  ) | "\(.number) \(.headRefName)"' | while read -r num branch; do
    echo "  Closing #$num ($branch)"
    gh pr close "$num" --comment "Closed: worker interrupted by system restart." --delete-branch
  done
fi

# Clean up orphaned worktrees
git worktree prune
git fetch --prune
```

### Step 4: Post-Cleanup

```bash
# Verify the queue is clean
gh pr list --limit 10

# Restart orchestration
multiclaude start                   # or: cd ~/gt && gt up
just status                         # Unified status check

# Re-dispatch closed work if needed
just autonomous-sprint <current>    # or tell the Gas Town Mayor
```

## Decision Rules

| Signal | Action |
| --- | --- |
| CI green + feature branch name | Likely MERGE |
| CI green + worker-named branch | REVIEW (check diff completeness) |
| CI failed | CLOSE (unless flaky test — check logs) |
| No CI run | CLOSE (worker crashed before push) |
| Touches `proto/experimentation/` | Always REVIEW |
| Deletes `experimentation-ffi` or `services/flags/` | Always REVIEW (ADR-024 cutover) |
| Two PRs modify same file | REVIEW both, note the conflict |
| No commits in > 7 days | CLOSE (stale) |

## Preventing Accumulation

- Review PRs daily during `just morning`
- Keep Multiclaude in multiplayer mode (human review required)
- Use feature branch names so PRs are self-documenting
- Set up GitHub notifications for PR creation from the `multiclaude` label
- Limit concurrent Multiclaude workers to 4–5 (more creates review bottleneck)
