# Per-Agent Status Files

This directory contains per-agent status files to reduce merge conflicts.
Each agent updates only their own file, eliminating the contention that occurred
when all agents shared a single `status.md`.

## Convention

- Each agent owns `agent-N.md` where N is their agent number (1-7).
- Update your file after each PR merges to `main`.
- The parent `status.md` retains shared tables (milestone tracker, pair integration schedule,
  contract changes log, blockers) that change infrequently.
- **Do not** update other agents' files — this is what caused the merge conflicts.

## Files

| File | Agent | Module |
|------|-------|--------|
| [agent-1.md](agent-1.md) | Agent-1 | M1 Assignment |
| [agent-2.md](agent-2.md) | Agent-2 | M2 Pipeline |
| [agent-3.md](agent-3.md) | Agent-3 | M3 Metrics |
| [agent-4.md](agent-4.md) | Agent-4 | M4a Analysis + M4b Bandit |
| [agent-5.md](agent-5.md) | Agent-5 | M5 Management |
| [agent-6.md](agent-6.md) | Agent-6 | M6 UI |
| [agent-7.md](agent-7.md) | Agent-7 | M7 Flags |
