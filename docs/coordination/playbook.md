# Coordinator Playbook — Advancing Agents Between Milestones

This document describes the operational workflow you follow each time an agent
completes a milestone and the system needs to advance.

## The Cycle

Every milestone completion triggers a three-step cycle:

```
1. MERGE    — Review PR, merge to main, update status.md
2. RESOLVE  — Identify which agents are now unblocked by this merge
3. ADVANCE  — Give each ready agent its continuation prompt
```

This cycle runs as often as PRs land — potentially multiple times per day
during active development.

---

## Step 1: Merge

When an agent opens a PR with a completed milestone:

1. **Review the PR** — check that tests pass, acceptance criteria are met,
   and the agent hasn't modified directories outside its ownership boundary.

2. **Merge to main** (squash merge preferred).

3. **Update `docs/coordination/status.md`**:
   - Mark the completed milestone as 🟢 in the milestone tracker
   - Fill in the PR link and merge date
   - Update the agent's row in the Agent Status table (current milestone → next milestone)
   - Check if any previously-blocked agents are now unblocked and update their status

Example diff after Agent-2 delivers event ingestion:

```diff
 | Agent-2 | M2 Pipeline | 🟢 Complete | — | ~~Event validation + Kafka publisher~~ | — | Milestone 1.6 merged |
-| Agent-3 | M3 Metrics | ⚪ Waiting | — | Standard metric computation job | Agent-2 (events on Kafka) | Use synthetic events until M2 delivers |
+| Agent-3 | M3 Metrics | 🟡 Not Started | — | Standard metric computation job | — | Unblocked by Agent-2 merge |
```

4. **Commit and push the status update** directly to `main`.

---

## Step 2: Resolve

Read the "What this unblocks" section from the agent's PR (or completion
message). Cross-reference against the milestone tracker to identify:

- **Directly unblocked agents** — agents whose "Blocked By" dependency is now
  satisfied. These can be advanced immediately.
- **Partially unblocked agents** — agents that needed multiple dependencies
  and now have one fewer. Update their "Blocked By" column but don't advance
  them yet.
- **The completing agent itself** — check if it has a next milestone that's
  ready (no new blockers). If so, it can continue.

### Decision Matrix

| Situation | Action |
|-----------|--------|
| Agent finishes milestone, has another unblocked milestone | Advance with continuation prompt |
| Agent finishes milestone, next milestone is blocked | Park the agent — note in status.md, resume when unblocked |
| Agent finishes milestone, unblocks another agent | Advance the newly-unblocked agent with its continuation prompt |
| Agent finishes all milestones in current phase | Assign Phase 2 milestones or have it assist a blocked agent |

---

## Step 3: Advance

For each agent that should continue, provide a **continuation prompt** in its
Claude Code session (or start a new session with the agent's system prompt if
the previous session has ended).

### Continuation Prompt Template

Use this template. Fill in the bracketed fields:

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: [milestone name] (PR #[number])
- **Branch cleaned up**: your old branch has been merged; start fresh from main

### What's New on Main

The following agent work has also merged since your last rebase:
- [Agent-X]: [what they delivered and why it matters to you]
- [Agent-Y]: [what they delivered and why it matters to you]

(If nothing new is relevant, say: "No new dependencies have landed that affect your work.")

### Rebase Before Starting

```bash
cd ../agent-[N]
git fetch origin
git rebase origin/main
git checkout -b agent-[N]/[type]/[next-milestone-description]
```

### Your Next Milestone

**[Milestone name from status.md]**

[Paste the acceptance criteria from the onboarding guide, or summarize them.
Include any context that's changed since the onboarding guide was written —
e.g., if an upstream agent delivered something slightly different than expected.]

### Dependencies

- [List current dependency status — what's available, what's still mocked]

### When Done

1. Run [relevant just command]
2. Open a PR with a clear description of what was delivered
3. Include a "What this unblocks" section listing downstream agents
4. Update `docs/coordination/status.md` in your PR
```

---

## Example: Full Cycle After Agent-2 Completes Event Ingestion

### Agent-2's completion message:

```
Milestone 1.6 complete: IngestExposure + IngestMetricEvent RPCs

What this unblocks:
- Agent-3 M3 — metric computation requires events on Kafka
- Agent-4 M4b — bandit policy requires reward events
```

### Your actions:

1. **Merge** Agent-2's PR to main.

2. **Update status.md**:
   - Milestone 1.6: 🟡 → 🟢, fill PR link and date
   - Agent-2 row: current milestone → "IngestRewardEvent + IngestQoEEvent RPCs" (milestone 1.7)
   - Agent-3 row: ⚪ Waiting → 🟡 Not Started, clear "Blocked By"
   - Agent-4 row: update note — "M4b reward events now available on Kafka"

3. **Advance three agents**:

   **Agent-2** (continuing to next milestone):
   ```
   ## Milestone Complete — Advancing to Next Task

   Your previous milestone has been merged to main:
   - **Merged**: IngestExposure + IngestMetricEvent RPCs (PR #12)

   ### What's New on Main
   No new dependencies have landed that affect your work.

   ### Rebase Before Starting
   cd ../agent-2
   git fetch origin
   git rebase origin/main
   git checkout -b agent-2/feat/reward-qoe-events

   ### Your Next Milestone
   **IngestRewardEvent + IngestQoEEvent RPCs** (milestone 1.7)

   - Implement IngestRewardEvent RPC: validate reward event → publish to
     `reward_events` Kafka topic
   - Implement IngestQoEEvent / IngestQoEEventBatch RPCs: validate → publish
     to `qoe_events` topic
   - Same validation and dedup patterns as exposure/metric events
   - Acceptance: valid reward → on Kafka, invalid → gRPC INVALID_ARGUMENT

   ### When Done
   1. Run `just test-rust`
   2. Open PR — "What this unblocks: Agent-4 M4b (reward stream for bandit learning)"
   3. Update status.md
   ```

   **Agent-3** (newly unblocked):
   ```
   ## Milestone Complete — Advancing to Next Task

   You were previously waiting on Agent-2 to deliver events on Kafka.
   That work has now merged to main.

   ### What's New on Main
   - Agent-2: IngestExposure + IngestMetricEvent RPCs merged. Events are now
     flowing to `exposures` and `metric_events` Kafka topics. You can consume
     these directly instead of using synthetic data.

   ### Rebase Before Starting
   cd ../agent-3
   git fetch origin
   git rebase origin/main
   git checkout -b agent-3/feat/standard-metric-computation

   ### Your Next Milestone
   **Standard metric computation (MEAN, PROPORTION, COUNT)** (milestone 1.10)

   [acceptance criteria from onboarding guide]

   ### Dependencies
   - Agent-2 events: ✅ Available on Kafka
   - Agent-5 experiment configs: Still mocked — use local JSON config with
     seed data experiments
   ```

   **Agent-4** (partially unblocked — note update only):
   ```
   ## Status Update

   Agent-2 has delivered reward events on Kafka (PR #12 merged to main).
   Your M4b track now has live reward data available. If you're still working
   on the M4a track (golden-file t-test validation), continue that — but when
   you reach M4b integration, rebase to pick up the reward event schema.
   ```

---

## Handling Common Situations

### Agent finishes faster than expected

If an agent completes all its Phase 1 milestones while others are still in
progress, give it one of these tasks:

1. **Write integration tests** for the pair integrations in the schedule
2. **Improve test coverage** on their module
3. **Start Phase 2 milestones** if their dependencies are met
4. **Help a blocked agent** by building the mock/synthetic data generator
   that the blocked agent needs

### Agent is stuck or producing low-quality work

If a PR doesn't meet acceptance criteria:

1. Request changes with specific feedback
2. If the agent is in a Claude Code session, paste the feedback directly:
   ```
   Your PR for [milestone] needs revisions:
   - [specific issue 1]
   - [specific issue 2]
   Please fix these and update the PR. Do not move to the next milestone.
   ```

### Two agents have a contract disagreement

If Agent-A's output doesn't match what Agent-B expected:

1. Check the proto schema — it's the source of truth
2. If the proto is ambiguous, have both agents' prompts reference the same
   clarifying decision
3. Document the resolution in a new ADR
4. Update both agents' continuation prompts with the clarified contract

### Session expires or context is lost

If you need to restart an agent's Claude Code session:

1. Start a new session with the agent's system prompt from
   `docs/coordination/prompts/agent-[N]-*.md`
2. The agent will read `status.md` to determine its current milestone
3. Add a brief context message:
   ```
   You're resuming work. Your worktree is at ../agent-[N].
   Your last PR ([branch name]) [was merged / is still open / needs revisions].
   Continue from where you left off.
   ```

---

## Cadence

During active development, expect to run this cycle 2–3 times per day:

| Time | Action |
|------|--------|
| Morning | Review overnight PRs, merge what's ready, advance agents |
| Midday | Check for newly-completed milestones, resolve blockers |
| End of day | Status check — update status.md, note any risks for tomorrow |

As agents become more autonomous and the critical path dependencies are
resolved (typically by Week 4–5), the cycle frequency decreases.
