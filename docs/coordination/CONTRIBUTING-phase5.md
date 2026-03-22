# Contributing — Phase 5 Addendum

This addendum covers the Phase 5 coordination model. For general contributing guidelines (commit conventions, code style, testing requirements), see the main CONTRIBUTING.md.


## Branching Convention

Workers create branches automatically via Multiclaude. The naming convention:

```
agent-N/feat/adr-XXX-description     # New feature work
agent-N/fix/adr-XXX-description      # Bug fixes
agent-N/test/adr-XXX-contract-name   # Contract test additions
agent-N/port/m7-rust                  # Language migration (ADR-024)
agent-N/proto/phase5-extensions       # Proto schema changes
```

All branches are created from `main` and target `main` for merge.


## PR Convention

Every PR from a Multiclaude worker should include:

1. **Title**: `feat(crate): ADR-XXX brief description`
2. **Body**: What changed, why, and which ADR section it implements.
3. **Status file update**: The worker's `docs/coordination/status/agent-N-status.md` must be updated in the same PR (or a follow-up commit before merge).
4. **Tests**: Unit tests for new code. Golden-file tests for statistical methods. Contract tests for cross-module interfaces.
5. **Label**: `multiclaude` (auto-applied by workers).

### Proto Schema PRs

Proto PRs have special requirements:
- Must pass `buf lint proto/` and `buf breaking proto/ --against .git#branch=main`.
- Must include generated code updates (tonic-build for Rust, connect-go for Go).
- Should land *before* any implementation PRs that depend on the new types.
- Review by the coordinator is mandatory (not auto-merged).


## Merge Process

### Standard PRs (Multiclaude multiplayer mode)
1. Worker creates PR with passing CI.
2. Coordinator reviews the diff.
3. Coordinator approves.
4. Merge queue merges to main.
5. Daemon's worktree refresh loop rebases all other workers.

### Proto Schema PRs
1. Worker creates PR with `buf lint` + `buf breaking` passing.
2. Coordinator reviews for naming conventions, backward compatibility, and completeness.
3. Coordinator may request an Agent Teams session for interactive review with 2–3 agents.
4. On approval, merge queue ships. All downstream workers get the schema via rebase.

### Cross-Agent Contract Test PRs
1. The *consumer* agent writes the contract test.
2. The *producer* agent's code must pass the test.
3. If the test fails against the producer's current code, the consumer files an issue (or messages the producer directly in an Agent Teams session).
4. Both agents' PRs should reference each other in their descriptions.


## Status File Protocol

Each agent maintains `docs/coordination/status/agent-N-status.md`:

- **Write your own only.** Never edit another agent's status file.
- **Read others as needed.** Check dependencies before starting blocked work.
- **Update on every PR.** The status file should reflect current sprint progress.
- **Format**:

```markdown
# Agent-N Status — Phase 5

**Module**: [module name]
**Last updated**: 2026-03-20

## Current Sprint
Sprint: 5.X
Focus: [ADR number and milestone]
Branch: agent-N/feat/adr-XXX-description

## In Progress
- [ ] Milestone description (ADR-XXX)
  - Blocked by: [none | Agent-M: description]
  - ETA: [date]

## Completed (Phase 5)
- [x] Milestone description (ADR-XXX) — PR #NNN, merged 2026-XX-XX

## Blocked
- Waiting on Agent-M: [description]

## Next Up
- [milestone description] — depends on: [list]
```


## Agent Teams Sessions

For ad-hoc collaboration, use Claude Code Agent Teams:

```bash
export CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1
claude
```

### When to Use
- Debugging contract test failures between two modules.
- Designing a shared proto interface.
- Pair-programming on a shared file (e.g., `experimentation-stats/src/lib.rs` module declarations).
- Interactive PR review for complex changes.

### When NOT to Use
- Solo implementation work (use Multiclaude worker).
- Long-running tasks (sessions are ephemeral).
- Work that touches files in multiple worktrees (use separate Multiclaude workers).

### Session Naming Convention
```
team-name: kaizen-{sprint}-{topic}
Example: kaizen-5.2-m4b-lp-contract-test
```


## Dependency Resolution

When your work depends on another agent's output:

1. Check their status file: `cat docs/coordination/status/agent-M-status.md`
2. If the dependency is "In Progress" — wait or work on non-dependent milestones.
3. If the dependency is "Completed" — pull main (worktree refresh should have rebased you already).
4. If the dependency is "Blocked" — check *their* blocker. If it's you, prioritize.
5. If unclear — spawn an Agent Teams session with the other agent to resolve.

**Never create circular dependencies.** If Agent-4 depends on Agent-1 and Agent-1 depends on Agent-4, escalate to the coordinator.
