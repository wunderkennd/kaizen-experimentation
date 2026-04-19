# Worker Status Files

Multiclaude autonomous workers write per-agent status files here, matching the pattern from `.multiclaude/config.json`:

```
docs/coordination/status/
  infra-1-status.md   ← infra worker 1
  infra-2-status.md   ← infra worker 2
  ...
  agent-1-status.md   ← (planned) product worker 1
  agent-4-status.md   ← (planned) product worker 4
  ...
```

Each file is the worker's heartbeat — its current Issue, branch, blockers, and next step. The supervisor reads these every `worker.health_check_interval_seconds` (120s as of writing) to detect stuck workers.

## When reviewing status files

- `Last updated > 30 min ago` and worker still attached → likely stuck. Run `multiclaude worker nudge <agent>`.
- `blocked` in the status text → check the linked Issue for the blocker.
- `Open question:` lines → the worker is asking the supervisor (you) for input. Reply via Issue comment.

## Why this dir is committed (with this README) rather than ignored

The directory must exist for `multiclaude start` to write into it on first launch. Status files themselves are gitignored via `.multiclaude/state/` rules; only this README is tracked.
