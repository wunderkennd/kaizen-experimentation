# Git Strategy for .claude/ and .multiclaude/

## Rationale

Agent definitions and project settings are **shared team knowledge** — they define how agents behave, what tools they can use, and what coding standards they follow. These should be version-controlled so that every developer (and every spawned agent) gets the same configuration.

Runtime state — worktrees, team sessions, task queues, daemon PIDs, message mailboxes — is ephemeral per-machine and per-session. Committing it would cause constant merge conflicts and serve no purpose.

## What to Commit

```
.claude/
  settings.json              # Project-level: Agent Teams flag, pre-approved permissions
  agents/                    # Project-scoped custom subagents (if any defined here)

.multiclaude/
  agents/                    # The 7 Kaizen agent definitions
    agent-1-assignment.md
    agent-2-pipeline.md
    agent-3-metrics.md
    agent-4-analysis-bandit.md
    agent-5-management.md
    agent-6-ui.md
    agent-7-flags.md
  config.json                # Repo-level Multiclaude settings (mode, merge strategy)
```

## What to Gitignore

```
.claude/
  settings.local.json        # User-specific overrides (model preference, API key path)
  worktrees/                 # Claude Code --worktree creates these; ephemeral per-machine
  teams/                     # Agent Teams session state (config.json, messages/)
  tasks/                     # Shared task list state for Agent Teams
  credentials/               # Auth tokens — never commit
  statsig/                   # Feature flag evaluation cache
  cache/                     # General cache
  *.log                      # Session logs

.multiclaude/
  state/                     # Daemon state (worker registry, health status)
  messages/                  # Inter-agent message mailbox (JSON files routed by daemon)
  locks/                     # File locks for task claiming
  worktrees/                 # Per-worker git worktrees
  *.pid                      # Daemon PID files
  *.log                      # Daemon and worker logs
```

## Setup Steps

1. **Append to `.gitignore`**: Copy the rules from `gitignore-additions.txt` into your repo's `.gitignore`.

2. **Commit `.claude/settings.json`**:
   ```bash
   git add .claude/settings.json
   git commit -m "chore: add project-level Claude Code settings (Agent Teams enabled)"
   ```

3. **Move Multiclaude agent definitions into the repo**:
   ```bash
   mkdir -p .multiclaude/agents/
   # Copy the 7 agent definition files from coordination/multiclaude-agents/
   cp coordination/multiclaude-agents/*.md .multiclaude/agents/
   git add .multiclaude/agents/
   git commit -m "chore: add Multiclaude agent definitions for Phase 5"
   ```

4. **Verify nothing ephemeral is tracked**:
   ```bash
   # After running multiclaude start or an Agent Teams session:
   git status
   # Should show NO changes in .claude/worktrees/, .claude/teams/,
   # .multiclaude/state/, .multiclaude/messages/, etc.
   ```

## Why settings.json but NOT settings.local.json

`.claude/settings.json` is the **project-level** config that applies to all contributors. It sets the Agent Teams flag and pre-approves safe commands (cargo test, go test, buf lint, git operations) so agents don't prompt for permission on every action.

`.claude/settings.local.json` is the **user-level** override. It might contain a developer's preferred model, custom API endpoint, or personal permissions. This varies per person and should never be committed.

If a developer needs to override a project setting locally:
```bash
# .claude/settings.local.json (gitignored)
{
  "model": "claude-sonnet-4-6",
  "env": {
    "SOME_PERSONAL_OVERRIDE": "value"
  }
}
```

## Why Agent Definitions ARE Code

The `.multiclaude/agents/*.md` files are functionally equivalent to CI workflow definitions or Dockerfiles — they specify how automated workers behave. Changes to agent definitions should go through the same review process as code changes:

- ADR responsibility updates → update the agent's `.md` file in the same PR.
- New coding standard → update all affected agent definitions.
- New contract test obligation → add to the relevant agent's "Contract Tests to Write" section.

The agent definitions reference ADRs, crate paths, proto schemas, and test commands. If any of those change, the definitions should be updated atomically in the same commit to stay consistent.
