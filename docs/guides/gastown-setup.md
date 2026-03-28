# Gas Town Setup for Kaizen

This guide covers installing Gas Town and configuring it for the Kaizen experimentation platform.

## Prerequisites

```bash
# Gas Town CLI + Beads (issue tracker)
brew install gastown
# or from source:
go install github.com/steveyegge/gastown/cmd/gt@latest
go install github.com/steveyegge/beads/cmd/bd@latest

# Verify installation
gt version
bd version

# Also required
which tmux git gh    # all three must be present
gh auth status       # must be authenticated with GitHub
```

If `gt` is not found, ensure `$GOPATH/bin` (usually `~/go/bin`) is in your PATH:
```bash
export PATH="$PATH:$HOME/go/bin"   # add to ~/.zshrc or ~/.bashrc
```

## Create the Workspace

```bash
# Create your Gas Town HQ
gt install ~/gt --git
cd ~/gt
```

This creates:
```
~/gt/
├── CLAUDE.md        # Gas Town's identity anchor (populated by gt prime)
├── mayor/           # Mayor config and state
├── .beads/          # Town-level issue tracking
└── (empty rigs/)
```

Note: Gas Town creates its own `CLAUDE.md` at `~/gt/CLAUDE.md` for the Mayor's context. This is separate from the repo-level `CLAUDE.md` at the Kaizen project root. Polecats working on Kaizen read the repo-level one since they operate in git worktrees of the Kaizen repo.

## Add Kaizen as a Rig

```bash
gt rig add kaizen https://github.com/your-org/kaizen.git
```

This clones and sets up:
```
~/gt/kaizen/
├── .beads/          # Project issue tracking
├── mayor/rig/       # Mayor's clone (canonical)
├── refinery/rig/    # Merge queue processor
├── witness/         # Worker monitor
└── polecats/        # Worker clones (created on demand via gt sling)
```

## Create Your Crew Workspace

```bash
gt crew add kenneth --rig kaizen
```

This gives you a personal workspace within the rig for hands-on work.

## Start Services

```bash
cd ~/gt
gt up          # Start daemon, Mayor, witnesses, refineries
gt doctor      # Health check
gt doctor --fix   # Auto-repair any issues
gt status      # Workspace overview
```

## Verify Everything Works

```bash
# Attach to the Mayor (your primary interface)
gt mayor attach

# Inside the Mayor session, try:
#   "What rigs do I have?"
#   "Show me the status of kaizen"
#   "List open beads"
```

Detach from the Mayor with `Ctrl-b d` (tmux detach).

## Daily Commands

```bash
# Primary interface
gt mayor attach              # Enter Mayor session
gt crew attach kenneth --rig kaizen   # Enter your personal workspace

# Work management
gt convoy create "Sprint 5.0 AVLM" <bead-ids>  # Group work items
gt sling <bead-id> kaizen    # Assign to polecat (spawns automatically)
gt convoy list               # See active work

# Agent management
gt agents                    # List active agent sessions
gt status                    # Full workspace overview

# Lifecycle
gt up                        # Start everything
gt down                      # Stop everything gracefully
```

## Using Gas Town with Multiclaude

The two tools don't conflict — they operate on different worktrees and track state in different directories. The discipline is: don't run both simultaneously against the same branches.

**Typical handoff**:
```bash
# Evening: stop Gas Town, start Multiclaude
cd ~/gt && gt down
just autonomous-sprint <N>

# Morning: stop Multiclaude workers, start Gas Town
just autonomous-stop
git checkout main && git pull origin main
cd ~/gt && gt up
gt mayor attach
```

Both tools' agents read the same `CLAUDE.md` and per-agent status files, so context transfers naturally through the git repo.

## Troubleshooting

| Problem | Solution |
| --- | --- |
| `gt: command not found` | Add `~/go/bin` to PATH |
| `bd: command not found` | `go install github.com/steveyegge/beads/cmd/bd@latest` |
| Mayor won't attach | `gt up` first, then `gt mayor attach` |
| Polecats not spawning | Check `gt doctor`, ensure tmux is running |
| Stale worktrees after crash | `gt doctor --fix` or manually `git worktree prune` |
| Dolt server port conflict | `gt install ~/gt --force` to re-detect ports |

## Uninstalling

```bash
# Stop services
cd ~/gt && gt down

# Remove workspace (CAUTION: deletes all Gas Town work state)
rm -rf ~/gt

# Remove binaries
rm $(which gt) $(which bd)
```
