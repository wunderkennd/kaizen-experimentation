# Sprint 5.1 + 5.6 Multi-Agent Coordination

How the next two open sprints are split across agents and the three orchestration tools (Claude sub-agents, Multiclaude, Gas Town).

## Sprint 5.1: Measurement Foundations (4 open Issues)

| Issue | Title | Lead Agent(s) | Cluster |
|---|---|---|---|
| [#422](https://github.com/wunderkennd/kaizen-experimentation/issues/422) | ADR-027 TOST — Core Implementation | Agent-4 | B |
| [#423](https://github.com/wunderkennd/kaizen-experimentation/issues/423) | ADR-027 TOST — M5 Validation + M6 Equivalence Results View | Agent-5, Agent-6 | B |
| [#424](https://github.com/wunderkennd/kaizen-experimentation/issues/424) | Heartbeat Sessionization | Agent-2 | A |
| [#425](https://github.com/wunderkennd/kaizen-experimentation/issues/425) | EBVS Detection | Agent-2, Agent-3, Agent-4, Agent-6 | (cross-cluster) |

**Critical path**: #422 (TOST core) blocks #423 (M5/M6). #424 (sessionizer) provides the integration point for #425's EBVS classification logic, so #424 should land before #425's classification work — though the proto change in #425 (Agent-4) can land in parallel.

## Sprint 5.6: Metric Definition Layer (6 open Issues)

| Issue | Title | Lead Agent(s) | Phase |
|---|---|---|---|
| [#432](https://github.com/wunderkennd/kaizen-experimentation/issues/432) | Phase 1: structured proto types + M3 templates | Agent-3, Agent-4 | 1 |
| [#433](https://github.com/wunderkennd/kaizen-experimentation/issues/433) | Phase 1: M5 validation | Agent-5 | 1 |
| [#434](https://github.com/wunderkennd/kaizen-experimentation/issues/434) | Phase 1: M6 UI | Agent-6 | 1 |
| [#435](https://github.com/wunderkennd/kaizen-experimentation/issues/435) | Phase 2: MetricQL parser/compiler | Agent-3 | 2 |
| [#436](https://github.com/wunderkennd/kaizen-experimentation/issues/436) | Phase 2: M5 validation + M6 editor | Agent-5, Agent-6 | 2 |
| [#437](https://github.com/wunderkennd/kaizen-experimentation/issues/437) | Phase 3: migration + deprecation | Agent-3, Agent-5, Agent-6 | 3 |

**Critical path**: #432 (Phase 1 proto) blocks #433 + #434 (M5/M6 for new types). Phase 2 (#435 → #436) blocks Phase 3 (#437).

---

## Tool routing

Match the work shape to the right orchestration tool. All three read GitHub Issues as the source of truth — no separate task list.

### Claude sub-agents (in-session, parallel)
Best for: **multi-file mechanical edits** that benefit from tight feedback during an active session.
- Spawn via the `Agent` tool with `subagent_type: general-purpose`.
- Each gets its own worktree; merge back via PR.
- Already used in this repo for: ADR consolidation, status sync, format backfills.
- Rule of thumb: if it would take you ≤30 min to do alone, dispatch a sub-agent.

### Multiclaude (autonomous, overnight)
Best for: **single-Issue implementations** with clear acceptance criteria, runnable while you're away.
- Launch from a labeled Issue:
  ```bash
  just work-on 422            # Single Issue
  just autonomous-sprint 5.1  # All open Sprint 5.1 Issues, one worker each
  ```
- Workers consume from `gh issue list --label "agent-N,sprint-5.X" --state open`.
- The current `.multiclaude/config.json` is **infra-defaults** (`branch_prefix: "infra-"`, `ci.required_checks` for `cd infra`). For product agent work (Sprint 5.1 / 5.6), the per-task prompt from `just work-on` overrides naming via `agent-N/feat/adr-XXX-description`. The CI checks in config.json are infra-only and **do not gate product PRs** — those are gated by the `.github/workflows/ci.yml` workflow on push.
- Status files land at `docs/coordination/status/<agent>-status.md` — see [`status/README.md`](status/README.md).

### Gas Town (interactive, daytime)
Best for: **complex deals you want to steer in real time** — e.g., ambiguous design decisions, cross-agent handoffs, debug sessions.
- Launch via the justfile:
  ```bash
  just interactive            # gt up + gt mayor attach
  ```
- Inside the Mayor session, dispatch a polecat onto an Issue:
  ```
  Pick up Issue #422 for the kaizen rig
  ```
- The Mayor coordinates polecats; you steer through tmux. Detach with `Ctrl-b d` when done; `just interactive-stop` shuts the rig.

### Daily rhythm
```
morning  → just morning           # Triage overnight Multiclaude runs + open PRs
work     → just interactive       # Gas Town for the day
evening  → just evening 5.1       # Hand off to Multiclaude for overnight
```

---

## Per-agent quick start

### Agent-2 (Pipeline)
```bash
gh issue list --label "agent-2,sprint-5.1" --state open
# → #424 Heartbeat, #425 EBVS classification
```
Read [`.multiclaude/agents/agent-2-pipeline.md`](../../.multiclaude/agents/agent-2-pipeline.md) for context. Sprint 5.1 is your only post-Phase-5 add-on.

### Agent-3 (Metrics)
```bash
gh issue list --label "agent-3" --state open
# → #425 EBVS SQL, #432 Phase 1 templates, #435 MetricQL, #437 migration
```
Heaviest Sprint 5.6 load. Phase 1 → Phase 2 → Phase 3 is sequential.

### Agent-4 (Stats/Bandit)
```bash
gh issue list --label "agent-4" --state open
# → #422 TOST core (P0 for Sprint 5.1), #425 EBVS proto, #432 Phase 1 proto
```
TOST core (#422) is on the Sprint 5.1 critical path — start here.

### Agent-5 (Management)
```bash
gh issue list --label "agent-5" --state open
# → #423 TOST validation, #433 Phase 1 validation, #436 MetricQL validation, #437 deprecation
```
Blocked on Agent-4 for #423 (TOST core), and on Agent-3/Agent-4 for #433 (Phase 1 proto).

### Agent-6 (UI)
```bash
gh issue list --label "agent-6" --state open
# → #423 TOST UI, #425 EBVS dashboard, #434 Phase 1 form, #436 MetricQL editor, #437 UI removal
```
Heaviest UI load across both sprints. Phase 1 form (#434) and TOST results (#423) are the largest items.

### Agent-1, Agent-7
No new work in Sprint 5.1 or 5.6.

---

## Verification commands

```bash
# All open Sprint 5.1 Issues
gh issue list --label sprint-5.1 --state open

# All open Sprint 5.6 Issues
gh issue list --label sprint-5.6 --state open

# All blocked Issues across both sprints
gh issue list --label "blocked" --state open

# Multiclaude worker status
multiclaude status

# Gas Town rig status
gt status
```
