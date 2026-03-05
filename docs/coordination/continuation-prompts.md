# Continuation Prompt Templates

Copy the relevant template below, fill in the bracketed fields, and paste it
into the agent's Claude Code session to advance it to the next milestone.

---

## Agent-1: Assignment Service

### After Milestone 1.1 (Hash WASM + FFI) → Milestone 1.2 (GetAssignment RPC)

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: Hash crate WASM + FFI bindings (PR #[number])

### What's New on Main
[List any other agent merges, or: "No new dependencies affect your work."]

### Rebase Before Starting
cd ../agent-1
git fetch origin
git rebase origin/main
git checkout -b agent-1/feat/get-assignment-rpc

### Your Next Milestone
**GetAssignment RPC — static user bucketing** (milestone 1.2)

Implement the core assignment flow:
1. Receive GetAssignment request with experiment_id, user_id, attributes
2. Look up experiment config (from local JSON config — M5 not yet available)
3. Hash user_id + salt → bucket → layer allocation check → variant mapping
4. Return variant assignment with payload

Acceptance criteria:
- Given a RUNNING experiment with 50/50 split, assignments are deterministic
  and balanced (chi-squared test on 10K users, p > 0.01)
- User not in any allocation range → return empty assignment (not an error)
- Missing experiment_id → gRPC NOT_FOUND
- Assignment latency p99 < 5ms (benchmark with criterion)

### Dependencies
- M5 config stream: Still mocked — use local JSON config at dev/config.json
- M4b SelectArm: Still mocked — uniform random for bandit types

### When Done
1. Run `just test-rust && just test-hash`
2. Open PR — "What this unblocks: SDKs (assignment API contract now stable), Agent-6 (debug view)"
3. Update status.md
```

### After Milestone 1.2 → Milestone 1.3 (Config Cache) — NOW UNBLOCKED

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: GetAssignment RPC (PR #11)

### What's New on Main
Agent-5 has delivered StreamConfigUpdates RPC (PR #15, merged 2026-03-05).
This is the server-streaming endpoint you need to subscribe to for real-time
experiment config updates. It replaces your local JSON config at dev/config.json.

The endpoint is on the AssignmentService proto (assignment/v1/assignment_service.proto):
- StreamConfigUpdates(StreamConfigUpdatesRequest) → stream ConfigUpdate
- Request: { last_known_version: int64 }
- Response stream: { experiment: Experiment, is_deletion: bool, version: int64 }
- On connect: sends full snapshot of all RUNNING experiments
- On mutation: sends delta updates (upsert for start/pause/resume, deletion for conclude/archive)
- Server runs on management service at port 50055

### Rebase Before Starting
git fetch origin
git checkout main && git pull
git checkout -b agent-1/feat/config-cache

### Your Next Milestone
**Config cache (subscribe to M5 StreamConfigUpdates)** (milestone 1.3)

Replace the static dev/config.json with a live config cache:
- Connect to M5's StreamConfigUpdates RPC on startup
- Receive full snapshot → build in-memory config map (Arc<RwLock<HashMap<experiment_id, Config>>>)
- Receive delta updates → upsert or delete from the map
- GetAssignment reads from this cache instead of the static file
- On disconnect: reconnect with exponential backoff; serve stale cache during reconnect
- Fallback: if M5 is unreachable at startup, fall back to dev/config.json (dev mode)

Acceptance criteria:
- On connect: all RUNNING experiments available for assignment within 1 second
- Start experiment in M5 → config appears in cache, new assignments served
- Conclude experiment in M5 → config removed, no new assignments
- M5 goes down → cache still serves from last known state
- Reconnect after M5 restart → fresh snapshot replaces stale cache

### Dependencies
- M5 StreamConfigUpdates: ✅ Available (PR #15 merged)
- ConnectRPC client for Rust: use tonic or reqwest-based gRPC client

### When Done
1. Run `just test-rust`
2. Open PR — "What this unblocks: live experiment config without static JSON"
3. Update status.md
```

### After Milestone 1.2 → Milestone 1.4 (Targeting Rules)

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: GetAssignment RPC (PR #[number])

### What's New on Main
[Check if Agent-5 has delivered StreamConfigUpdates — if so, note it]

### Rebase Before Starting
cd ../agent-1
git fetch origin
git rebase origin/main
git checkout -b agent-1/feat/targeting-rules

### Your Next Milestone
**Targeting rule evaluation** (milestone 1.4)

Implement predicate evaluation against user attributes:
- Parse TargetingRule JSON predicate tree from experiment config
- Evaluate groups of predicates (AND within group, OR across groups)
- Support operators: EQUALS, NOT_EQUALS, IN, NOT_IN, CONTAINS, GT, LT, GTE, LTE
- If user doesn't match targeting rule → skip experiment (no assignment)

Acceptance criteria:
- Rule `{"groups": [{"predicates": [{"attribute_key": "country", "operator": "IN", "values": ["US", "UK"]}]}]}` → US users assigned, FR users excluded
- Empty rule (no predicates) → all users match
- Missing attribute key → user does not match (safe default)

### When Done
1. Run `just test-rust`
2. Open PR
3. Update status.md
```

---

## Agent-2: Event Pipeline

### After Milestone 1.6 (Exposure + Metric RPCs) → Milestone 1.7 (Reward + QoE RPCs)

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: IngestExposure + IngestMetricEvent RPCs (PR #[number])

### What's New on Main
[List any other merges, or: "No new dependencies affect your work."]

### Rebase Before Starting
cd ../agent-2
git fetch origin
git rebase origin/main
git checkout -b agent-2/feat/reward-qoe-events

### Your Next Milestone
**IngestRewardEvent + IngestQoEEvent RPCs** (milestone 1.7)

- IngestRewardEvent: validate reward event (experiment_id, user_id, arm_id, reward_value) → publish to `reward_events` topic
- IngestQoEEvent / IngestQoEEventBatch: validate QoE metrics (rebuffer_ratio, time_to_first_frame, resolution_switches) → publish to `qoe_events` topic
- Same validation, dedup, and idempotent producer patterns as exposure/metric events

Acceptance criteria:
- Valid reward event → on `reward_events` topic
- QoE event with all required fields → on `qoe_events` topic
- Missing arm_id on reward → gRPC INVALID_ARGUMENT
- Bloom filter dedup works across all event types

### When Done
1. Run `just test-rust`
2. Open PR — "What this unblocks: Agent-4 M4b (reward stream for bandit learning)"
3. Update status.md
```

### After Milestone 1.7 → Milestone 1.8 (Bloom Filter Tuning)

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: IngestRewardEvent + IngestQoEEvent RPCs (PR #[number])

### Rebase Before Starting
cd ../agent-2
git fetch origin
git rebase origin/main
git checkout -b agent-2/feat/bloom-filter-tuning

### Your Next Milestone
**Bloom filter dedup optimization** (milestone 1.8)

- Size the Bloom filter for 100M events/day at 0.1% FPR
- Implement filter rotation (new filter per hour, retain previous for overlap)
- Expose filter stats as Prometheus metrics: size, estimated FPR, item count
- Load test: sustain 100K events/sec with dedup active

### When Done
1. Run `just test-rust`
2. Open PR
3. Update status.md
```

---

## Agent-3: Metric Computation

### After Milestone 1.10 (Standard Metrics) → Milestone 1.11 (RATIO + Delta Method)

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: Standard metric computation — MEAN, PROPORTION, COUNT (PR #[number])

### What's New on Main
[Check for Agent-5 metric definition CRUD, Agent-2 QoE events, etc.]

### Rebase Before Starting
cd ../agent-3
git fetch origin
git rebase origin/main
git checkout -b agent-3/feat/ratio-delta-method

### Your Next Milestone
**RATIO metric with delta method inputs** (milestone 1.11)

- Compute RATIO metrics (numerator / denominator per user)
- Provide delta method inputs to M4a: numerator variance, denominator variance, covariance
- Store in metric_summaries with metric_type = RATIO

Acceptance criteria:
- Given revenue_per_session (revenue / sessions), correct ratio computed per user
- Delta method inputs match manual computation on test dataset
- All SQL logged to query_log

### When Done
1. Run `just test-go`
2. Open PR — "What this unblocks: Agent-4 M4a (delta method analysis for ratio metrics)"
3. Update status.md
```

---

## Agent-4: Analysis + Bandit

### After Milestone 1.14 (t-test + SRM golden files) → Milestone 1.15 (CUPED)

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: Welch t-test + SRM golden-file validation (PR #[number])

### What's New on Main
[Check if Agent-3 has delivered metric_summaries with CUPED covariates]

### Rebase Before Starting
cd ../agent-4
git fetch origin
git rebase origin/main
git checkout -b agent-4/feat/cuped-variance-reduction

### Your Next Milestone
**CUPED variance reduction** (milestone 1.15)

- Implement CUPED adjustment: Y_adj = Y - theta * X, where X is pre-experiment covariate
- Theta = Cov(Y, X) / Var(X)
- Accept covariate data from M3's metric_summaries (pre_experiment_value column)
- Fall back to standard analysis if covariates are unavailable

Acceptance criteria:
- On synthetic data with known effect and high-variance covariate, CUPED reduces
  CI width by >30% compared to unadjusted
- Results match R implementation to 6 decimal places (new golden files)
- assert_finite!() on all intermediate computations
- Graceful fallback when covariate is missing (log warning, return unadjusted result)

### When Done
1. Run `just test-rust`
2. Open PR
3. Update status.md
```

### After Milestone 1.17 (Thompson Sampling) → Milestone 1.18 (LMAX Core)

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: Thompson Sampling Beta-Bernoulli integration (PR #[number])

### Rebase Before Starting
cd ../agent-4
git fetch origin
git rebase origin/main
git checkout -b agent-4/feat/lmax-policy-core

### Your Next Milestone
**LMAX single-threaded policy core** (milestone 1.18)

Implement the LMAX-inspired threading model for M4b (see ADR-002):
- Single tokio task owns all policy state (HashMap<experiment_id, BanditState>)
- Receives commands via bounded mpsc channel: SelectArm, ReportReward, UpdatePolicy
- Never shares mutable state across threads — all mutation is serial
- SelectArm responses sent back via oneshot channel
- Benchmark: p99 < 15ms at 10K SelectArm requests/sec

See `docs/design/lmax_threading.mermaid` for the architecture diagram.

### When Done
1. Run `just test-rust && just bench-crate experimentation-policy`
2. Open PR — "What this unblocks: Agent-1 (SelectArm RPC integration for bandit experiments)"
3. Update status.md
```

---

## Agent-5: Management Service

### After Milestone 1.20 (CRUD + State Machine) → Milestone 1.21 (Layer Allocation)

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: Experiment CRUD + state machine enforcement (PR #[number])

### What's New on Main
[List any other merges]

### Rebase Before Starting
cd ../agent-5
git fetch origin
git rebase origin/main
git checkout -b agent-5/feat/layer-allocation

### Your Next Milestone
**Layer allocation + bucket reuse** (milestone 1.21)

Implement the bucket allocation system (see ADR-009):
- On StartExperiment: allocate contiguous bucket range within the experiment's layer
- Respect existing allocations (no overlap with RUNNING experiments in same layer)
- On ConcludeExperiment: mark allocation as released, set reusable_after timestamp
- Bucket reuse cooldown: released buckets not reusable for 24 hours (configurable)
- Concurrent StartExperiment calls on the same layer must not produce overlapping allocations (use SELECT FOR UPDATE)

Acceptance criteria:
- Start 2 experiments in same layer with 50% each → non-overlapping bucket ranges
- Start 3rd experiment requesting 50% → rejected (insufficient capacity)
- Conclude experiment, wait cooldown, start new → reuses released buckets
- Concurrent start attempts → both succeed with non-overlapping ranges (no data race)

### When Done
1. Run `just test-go`
2. Open PR
3. Update status.md
```

### After Milestone 1.21 → Milestone 1.22 (StreamConfigUpdates) ✅ COMPLETED

> PR #15 merged 2026-03-05. Implemented PG LISTEN/NOTIFY fan-out + ConnectRPC
> server-streaming handler on the AssignmentService proto.

### After Milestone 1.22 → Milestone 1.23 (Guardrail Alert Consumer) ✅ COMPLETED

> PR #18 merged 2026-03-05. Kafka consumer for `guardrail_alerts` topic with
> auto-pause per ADR-008. Processor checks `guardrail_action` field.

### After Milestone 1.23 → Milestone 1.24 (Metric Definition CRUD)

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: Guardrail alert consumer → auto-pause (PR #18)
- **Merged**: StreamConfigUpdates RPC (PR #15)
- **Merged**: Layer allocation + bucket reuse (PR #7, #10)

### What's New on Main
- Agent-3: Guardrail breach detection merged (PR #16). Their Kafka publisher
  is a stub but the alert schema is stable.
- Agent-4: CUPED variance reduction merged (PR #14). Golden-file validated.
- Agent-7: Flag service complete (PR #13). PromoteToExperiment mocked — needs
  live wiring to your CreateExperiment API.

### Rebase Before Starting
git fetch origin
git checkout main && git pull
git checkout -b agent-5/feat/metric-definition-crud

### Your Next Milestone
**Metric definition CRUD** (milestone 1.24)

Implement ConnectRPC handlers for metric definitions that Agent-3 consumes:
- CreateMetricDefinition: validate and store metric specs in `metric_definitions` table
- GetMetricDefinition: retrieve by metric_id
- ListMetricDefinitions: paginated list with page_size/page_token

The `MetricDefinition` proto (common/v1/metric.proto) has 14 fields:
- Core: metric_id, name, description, type (MEAN/PROPORTION/RATIO/COUNT/PERCENTILE/CUSTOM)
- Source: source_event_type, numerator/denominator_event_type (RATIO), percentile, custom_sql
- Behavior: lower_is_better, is_qoe_metric, cuped_covariate_metric_id
- Planning: minimum_detectable_effect, surrogate_target_metric_id

The `metric_definitions` table already exists in `sql/migrations/001_schema.sql`.

Acceptance criteria:
- CreateMetricDefinition → stores in DB, audit trail entry
- GetMetricDefinition with valid metric_id → returns full definition
- GetMetricDefinition with unknown metric_id → gRPC NOT_FOUND
- ListMetricDefinitions → paginated results
- Type-specific validation: RATIO requires numerator + denominator event types,
  PERCENTILE requires percentile value in (0,1), CUSTOM requires non-empty custom_sql
- MEAN/PROPORTION/COUNT require non-empty source_event_type

### Dependencies
- No external dependencies — self-contained CRUD against PostgreSQL

### When Done
1. Run `just test-go`
2. Open PR — "What this unblocks: Agent-3 (metric configs from DB instead of JSON)"
3. Update status.md
```

---

## Agent-6: Decision Support UI

### After Milestone 1.25 (Experiment List + Detail) → Milestone 1.26 (State Indicator)

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: Experiment list + detail shell with MSW mocks (PR #[number])

### What's New on Main
[Check if Agent-5 CRUD APIs have merged — if so, instruct to swap MSW for real APIs]

### Rebase Before Starting
cd ../agent-6
git fetch origin
git rebase origin/main
git checkout -b agent-6/feat/state-indicator-live-api

### Your Next Milestone
**State indicator + live API integration** (milestone 1.26)

[If Agent-5 CRUD is merged]:
- Swap MSW mocks for real ConnectRPC calls to Agent-5's management service
- Keep MSW as fallback for CI (tests should run without a live backend)
- Verify experiment list and detail pages render correctly with live data

[If Agent-5 CRUD is NOT yet merged]:
- Refine the state indicator component with animations
- Add STARTING and CONCLUDING pulse animations
- Build the variant editing form (for DRAFT experiments)
- Continue using MSW mocks

### When Done
1. Run `just test-ts`
2. Open PR with screenshots
3. Update status.md
```

---

## Agent-7: Feature Flags

### After Milestone 1.28 (Boolean Flag CRUD + CGo Bridge) → Milestone 1.29 (Percentage Rollout)

```
## Milestone Complete — Advancing to Next Task

Your previous milestone has been merged to main:
- **Merged**: Boolean flag CRUD + CGo hash bridge (PR #[number])

### What's New on Main
[Check for Agent-5 CreateExperiment API availability]

### Rebase Before Starting
cd ../agent-7
git fetch origin
git rebase origin/main
git checkout -b agent-7/feat/percentage-rollout

### Your Next Milestone
**Percentage rollout with monotonic guarantee** (milestone 1.29)

- Implement percentage rollout using CGo hash bridge: hash(user_id, flag_salt) % 10000 < rollout_pct * 10000
- String, numeric, and JSON flag types (beyond boolean)
- Monotonic rollout: prove that increasing rollout_percentage never removes existing users
- Multi-variant flags: support multiple string variants with percentage weights

Acceptance criteria:
- Rollout from 10% → 20%: all users in original 10% still receive treatment
- 1M user simulation: actual rollout percentage within 0.5% of target
- String flag with 3 variants at 30/30/40: correct distribution

### When Done
1. Run `just test-go`
2. Open PR
3. Update status.md
```

---

## Unblocking Prompts

Use these when an agent was parked (waiting on a dependency) and you need to
wake it up after the dependency merges.

### Generic Unblocking Template

```
## You Are Now Unblocked

You were previously waiting on [Agent-X] to deliver [dependency].
That work has now merged to main (PR #[number]).

### What This Means For You
[Explain what's now available — e.g., "Events are now flowing to the
`exposures` and `metric_events` Kafka topics. You can consume these
directly instead of using synthetic data."]

### Rebase to Pick Up the Changes
cd ../agent-[N]
git fetch origin
git rebase origin/main

### Proceed With Your Milestone
[Restate the milestone and acceptance criteria, noting what's changed]

### Updated Dependencies
- [Dependency X]: ✅ Now available
- [Dependency Y]: Still mocked — [workaround]
```

---

## Session Restart Prompt

Use when a Claude Code session has expired and you need to resume an agent:

```
You are Agent-[N], responsible for [module name]. Your system prompt is at
docs/coordination/prompts/agent-[N]-[name].md — read it now for full context.

You're resuming work. Your worktree is at ../agent-[N].
Your current branch is `agent-[N]/[type]/[description]`.

[Choose one]:
- Your last PR was merged. Advance to the next milestone — check status.md.
- Your last PR is still open and needs revisions: [list specific issues].
- You were in the middle of [milestone]. Continue from where you left off.

Read docs/coordination/status.md for current project state before proceeding.
```
