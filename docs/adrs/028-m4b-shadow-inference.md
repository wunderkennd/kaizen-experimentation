# ADR-028: M4b Shadow Inference Path for Bandit Policy Promotion

## Status

Proposed

## Context

M4b's Bandit Policy Service serves real-time arm selections via the LMAX-inspired
single-threaded core (ADR-002), with crash-only persistence to RocksDB (ADR-003) and
neural bandit inference via Candle (ADR-011). Phase 4 introduces the neural contextual
bandit, which raises a model promotion problem the existing architecture does not solve.

The current promotion model for a new bandit policy version (new Thompson hyperparameters,
new LinUCB regularizer, new neural architecture, retrained model weights) has two unsafe
options:

1. **Hard cutover.** Swap the production model atomically. The new model has no
   production observations until it is already serving live traffic. If it regresses,
   the only recovery is a rollback after damage is done.
2. **A/B with both arms live.** Run the candidate as a treatment arm in a
   `CONTEXTUAL_BANDIT` experiment. This exposes users to a potentially regressed model
   during the validation window — the very thing we want to avoid.

Neither approach permits validating the candidate's behavior on production traffic
distributions before any user is exposed to it. Industry practice for ML model serving
fills this gap with a "shadow inference" stage: the candidate model receives the same
inputs as production, produces a parallel output, and that output is logged but never
acted upon. Netflix's Switchboard explicitly enumerates shadow mode as a first-class
lifecycle stage for this reason
([Netflix Tech Blog, May 2026](https://netflixtechblog.com/state-of-routing-in-model-serving-16e22fe18741)).

ADR-027 establishes shadow mode as a property of an experiment phase: a flag
`is_shadow` on `Experiment`, with `SHADOWING` and `LIVE` sub-states of `RUNNING`, and
state transitions between them recorded in the audit trail. This ADR specifies the
M4b-side mechanism that gives that flag operational meaning for `CONTEXTUAL_BANDIT`
experiments.

We need a shadow inference path in M4b with two non-negotiable constraints:

1. **The LMAX guarantee for production must be preserved.** The production policy core
   is the latency-critical path (p99 < 15ms at 10K rps). Adding shadow inference cannot
   introduce contention or backpressure on the production core under any operating
   condition.
2. **Crash-only recovery must hold for both production and shadow state.** No "save on
   shutdown" code paths; both cores recover identically from snapshot + Kafka replay.

## Decision

We add a **dedicated shadow policy core** to M4b for any `CONTEXTUAL_BANDIT` experiment
in `RUNNING/SHADOWING`. The shadow core runs on a separate dedicated thread, with
state isolated in a separate RocksDB column family. The architecture mirrors the
production LMAX core, with three critical isolation properties:

1. **Fire-and-forget delivery from gRPC handler to shadow core.** The tokio gRPC handler
   sends `SelectArm` requests to the shadow core via `try_send` on a bounded channel.
   On a full channel, the request is dropped. Production responses are never blocked
   on shadow channel availability.
2. **Independent Kafka consumer for shadow rewards.** The shadow core has its own
   consumer group on `reward_events` with its own committed offset. Production reward
   processing is unaffected by shadow consumer lag.
3. **Separate RocksDB column families.** Production state lives in column family
   `production`; shadow state lives in column family `shadow`. Both persist on every
   reward update as a side effect of normal operation, per ADR-003.

The shadow core's arm selections are emitted to a new Kafka topic `shadow_arm_events`,
consumed by M4a for offline comparison against logged production arm selections.

### Lifecycle integration with ADR-027

The shadow core's lifecycle is **driven by M5's experiment state machine**, not by
direct operator RPCs. This is the key change from earlier drafts: shadow inference is
a consequence of an experiment being in `RUNNING/SHADOWING`, and promotion is a
consequence of the `SHADOWING → LIVE` transition. The two stay aligned by
construction.

The triggers and actions are:

| Lifecycle event | M4b action |
|---|---|
| `CONTEXTUAL_BANDIT` experiment enters `RUNNING/SHADOWING` | M5 calls `LoadShadowModel`, supplying the experiment ID, candidate `model_uri` from the variant marked `purpose=SHADOW_CANDIDATE`, and bandit algorithm. Shadow core begins inference. |
| `CONTEXTUAL_BANDIT` experiment transitions `SHADOWING → LIVE` | M5 calls `PromoteShadow` with `retain_old_production_as_shadow=true`. Atomic state swap; the candidate now serves user traffic; the previously-production model is retained as shadow for rollback. |
| `CONTEXTUAL_BANDIT` experiment transitions `LIVE → SHADOWING` (rollback) | M5 calls `PromoteShadow` again. The retained model from the previous swap returns to production; the regressed model becomes shadow for diagnosis. |
| `CONTEXTUAL_BANDIT` experiment enters `CONCLUDING` | M5 calls `UnloadShadow` if a shadow is active. Shadow column family is checkpointed and cleared. |

`LoadShadowModel`, `PromoteShadow`, and `UnloadShadow` are M5-internal RPCs on M4b —
operators do not call them directly. The operator-facing surface is
`TransitionShadowMode` on M5 (per ADR-027). M4b additionally exposes `GetShadowDiff`
as a read-only RPC for M4a and M6 to query offline comparison results.

```protobuf
// proto/experimentation/bandit/v1/bandit_service.proto
service BanditPolicyService {
  // existing RPCs …

  // NEW — called by M5's state-machine actions
  rpc LoadShadowModel(LoadShadowModelRequest) returns (LoadShadowModelResponse);
  rpc PromoteShadow(PromoteShadowRequest) returns (PromoteShadowResponse);
  rpc UnloadShadow(UnloadShadowRequest) returns (UnloadShadowResponse);

  // NEW — read-only, called by M4a and M6
  rpc GetShadowDiff(GetShadowDiffRequest) returns (GetShadowDiffResponse);
}

message LoadShadowModelRequest {
  string experiment_id = 1;
  string model_uri = 2;        // MLflow URI, e.g. models:/neural_bandit/15
  BanditAlgorithm algorithm = 3;
}

message PromoteShadowRequest {
  string experiment_id = 1;
  bool retain_old_production_as_shadow = 2;  // M5 sets this true to enable rollback
}

message UnloadShadowRequest {
  string experiment_id = 1;
}
```

### Threading model (extension of ADR-002)

```
Thread 1 (tokio): gRPC server receives SelectArm requests
  ├─ sends (context, oneshot_tx) into production_channel  ← blocking send (existing)
  └─ if shadow active for experiment:
      try_send (context) into shadow_channel              ← NEW, drop on full

Thread 2a (tokio): Production Kafka consumer  (existing)
  └─ sends RewardEvent into production_reward_channel

Thread 2b (tokio): Shadow Kafka consumer     ← NEW, runs only when ≥1 shadow active
  └─ sends RewardEvent into shadow_reward_channel

Thread 3 (dedicated): Production policy core event loop  (existing, unchanged)
  loop {
    select! {
      req = production_channel.recv() => {
        let arm = production_policy.select_arm(req.context);
        req.response_tx.send(arm);
      }
      reward = production_reward_channel.recv() => {
        production_policy.update(reward);
        production_policy.snapshot_to_rocksdb(cf="production");
      }
    }
  }

Thread 4 (dedicated): Shadow policy core event loop  ← NEW
  loop {
    select! {
      req = shadow_channel.recv() => {
        let arm = shadow_policy.select_arm(req.context);
        emit_shadow_arm_event(req.context, arm);  // → shadow_arm_events Kafka topic
      }
      reward = shadow_reward_channel.recv() => {
        shadow_policy.update(reward);
        shadow_policy.snapshot_to_rocksdb(cf="shadow");
      }
    }
  }
```

When no experiment is in `RUNNING/SHADOWING`, Thread 4 and Thread 2b are idle (or
not spawned). Activation is per-experiment via `LoadShadowModel`.

### Promotion atomicity

The `SHADOWING → LIVE` transition on a `CONTEXTUAL_BANDIT` experiment requires that
no in-flight `SelectArm` request observes a partially-swapped state. The
implementation uses a quiesce-swap-resume sequence coordinated via a single command
sent to the production core:

1. M5 calls `PromoteShadow`.
2. M4b sends a `Promote { experiment_id, retain }` command into the production
   core's command channel.
3. The production core processes the command between two consecutive `SelectArm`
   responses. It atomically:
   - Swaps the in-memory policy reference for the experiment.
   - Renames the RocksDB column family handles (production ↔ shadow).
   - If `retain=true`, leaves the (now-shadow) state intact; if `retain=false`,
     marks the shadow column family for deletion.
4. The next `SelectArm` request observes the swapped state.
5. M4b acknowledges to M5; M5 commits the state-machine transition to
   `RUNNING/LIVE`.

The swap occurs on the production core thread to guarantee serialization with
ongoing arm selections. The shadow core is signaled in parallel and quiesces its
own command processing.

### M4a comparison metrics

M4a's offline diff analysis computes:

- **Arm-selection agreement rate:** Fraction of contexts where both policies select
  the same arm.
- **Counterfactual expected reward:** For each arm selection, the IPW-adjusted
  estimate of expected reward under each policy, using logged assignment
  probabilities. Builds on the existing IPW machinery in `experimentation-stats`.
- **Reward delta distribution:** Per-context expected reward delta (shadow minus
  production), reported as mean, median, and worst decile.
- **Exploration calibration:** Empirical exploration rate of each policy compared to
  the configured `min_exploration_fraction`.
- **Shadow drop rate:** Fraction of `SelectArm` requests where shadow inference was
  skipped due to channel saturation. Diff metrics are computed on the sampled
  subset; this rate is reported alongside.

## Consequences

### Positive

- The shadow core's lifecycle is governed by M5's state machine. Operators never
  call M4b directly — they use `TransitionShadowMode` on the experiment, and M4b
  follows. This eliminates the class of bugs where shadow state and experiment
  state drift apart.
- Rollback is one operator action: `TransitionShadowMode(is_shadow=true)` on a
  `RUNNING/LIVE` experiment fires the inverse `PromoteShadow`. The previously-
  retained model returns to production; the regressed model becomes shadow for
  diagnosis. The retained-shadow design from the earlier draft is preserved; what
  changes is that this is no longer a separate operator workflow — it falls out of
  the lifecycle.
- The LMAX guarantee is preserved. Production core has zero new dependencies on
  shadow state; shadow lag cannot affect production latency.
- Reuses existing RocksDB infrastructure via column families — no new persistence
  layer.
- The shadow core is enabled per-experiment, not globally. Experiments in `LIVE`
  with no shadow incur zero overhead.

### Negative

- Increased memory footprint per M4b instance: roughly 2× for any experiment with
  an active shadow. For Thompson Sampling/LinUCB this is small (kilobytes); for
  neural bandits with Candle MLPs this can be tens of megabytes. M4b node sizing
  must account for active-shadow worst case (validated under chaos test).
- Increased CPU on M4b: the shadow core consumes a dedicated thread per experiment
  with active shadow. Scheduling for the M4b dedicated NVMe node (per the AWS
  deployment plan) must reserve cores for shadow work.
- Increased Kafka topic count: `shadow_arm_events` is an additional topic.
  Partitioning matches `reward_events` for parallel consumption.
- Promotion atomicity is a careful operation. The swap must occur between
  consecutive events in the LMAX core to avoid any in-flight reward being applied
  to the wrong state. Implementation requires the quiesce-swap-resume sequence
  described above and is the subject of dedicated property-based tests.
- More state to crash-recover. RocksDB recovery time roughly doubles when both
  column families are populated. The crash recovery SLA in Section 8.2 (< 10
  seconds) must be re-validated under chaos test.
- Coupling between M5 and M4b grows: M5 must call M4b's lifecycle RPCs as part of
  state transitions, and both must agree on what state the experiment is actually
  in. Mitigated by making `TransitionShadowMode` on M5 idempotent and reconcile-
  driven: M5 is the source of truth, and M4b reconciles on startup by querying M5
  for all `CONTEXTUAL_BANDIT` experiments in `RUNNING/SHADOWING` and ensuring it
  has a shadow core for each.

### Neutral

- Dropping shadow `SelectArm` requests on channel-full (rather than queueing
  unboundedly) is a deliberate trade-off favoring production isolation over shadow
  data completeness. M4a's diff analysis reports the drop rate so operators know
  the diff is computed on a sample, not the full request stream.
- Shadow rewards are consumed from a dedicated consumer group, meaning shadow lag
  does not affect production lag and vice versa. The two cores can be looking at
  different reward windows during transient lag, which M4a's diff analysis must
  account for via timestamp alignment.

## Implementation

### Crate-level changes

| Crate | Change |
|---|---|
| `experimentation-bandit` | Add `ShadowPolicy` wrapper around existing `Policy` trait; no algorithm changes |
| `experimentation-policy` (M4b binary) | Add shadow core thread, shadow channels, shadow Kafka consumer, shadow column family handling, four new RPC handlers, M5 reconciliation on startup |
| `experimentation-stats` | Add `compute_shadow_diff()` operating on production vs shadow arm-event logs; reuses IPW machinery |
| `experimentation-analysis` (M4a binary) | Schedule shadow diff jobs alongside existing analysis jobs; persist `shadow_diff_results` table |
| `experimentation-proto` | New RPCs and messages above |

### Storage changes

- **RocksDB:** Schema becomes `cf=production` (existing default migrated) +
  `cf=shadow`. On startup, both column families are restored if present. Migration
  script: `crates/experimentation-policy/src/rocksdb/migrate_v2.rs` performs the
  one-time migration of existing RocksDB data into the `production` column family.
- **Kafka:** New topic `shadow_arm_events`, partition count matching `reward_events`,
  retention 7 days (sufficient for offline diff analysis windows).
- **PostgreSQL:** New table `shadow_diff_results`:
  ```sql
  CREATE TABLE shadow_diff_results (
    experiment_id TEXT NOT NULL,
    computed_at TIMESTAMPTZ NOT NULL,
    agreement_rate DOUBLE PRECISION NOT NULL,
    mean_reward_delta DOUBLE PRECISION NOT NULL,
    median_reward_delta DOUBLE PRECISION NOT NULL,
    worst_decile_reward_delta DOUBLE PRECISION NOT NULL,
    shadow_drop_rate DOUBLE PRECISION NOT NULL,
    n_observations BIGINT NOT NULL,
    PRIMARY KEY (experiment_id, computed_at)
  );
  ```

### Reconciliation on startup

On M4b startup (cold or post-crash):

1. Restore both `production` and `shadow` RocksDB column families from snapshot.
2. Resume both Kafka consumers from their respective committed offsets.
3. Query M5 for all `CONTEXTUAL_BANDIT` experiments in `RUNNING/SHADOWING`.
4. For each, verify the shadow core has state for that experiment. If missing
   (e.g., due to crash before snapshot), call the equivalent of `LoadShadowModel`
   self-internally to re-initialize. If extra (shadow state for an experiment no
   longer in SHADOWING), log a warning and clean up — M5 is the source of truth.

### Acceptance criteria

- **Production isolation under load:** With shadow core active and shadow channel
  saturated to drop rate > 50%, production `SelectArm` p99 latency at 10K rps stays
  within 5% of the no-shadow baseline. Validated by criterion benchmark with
  synthetic load.
- **Crash recovery:** Kill -9 with both column families populated. On restart, both
  cores restore from snapshot, resume from committed Kafka offsets, and reconcile
  with M5 within 10 seconds total.
- **Promotion atomicity:** `PromoteShadow` performs the swap such that for any
  sequence of `SelectArm` requests issued during the swap window, each request is
  served by exactly one consistent policy state (either pre-swap or post-swap,
  never a mix). Validated by a deterministic test that issues 1000 concurrent
  requests during a triggered swap and asserts each response references a valid
  pre-swap-or-post-swap policy snapshot.
- **Lifecycle integration:** End-to-end test starting from M5 `CreateExperiment`
  with `is_shadow=true` and `experiment_type=CONTEXTUAL_BANDIT`. Verify M4b loads
  shadow on RUNNING entry, swaps on `TransitionShadowMode(is_shadow=false)`, swaps
  back on `TransitionShadowMode(is_shadow=true)`, and unloads on CONCLUDING.
- **Shadow drop rate under steady state:** With production traffic at 10K rps and a
  warm shadow core running an equivalent algorithm, shadow drop rate < 1% over a
  10-minute window.
- **Diff analysis correctness:** On a synthetic dataset where production and shadow
  policies are constructed to produce known agreement rates and reward deltas,
  M4a's `compute_shadow_diff` recovers the known values within 0.1 absolute
  tolerance.

### Test requirements

- Unit (`experimentation-policy`): `try_send` drop semantics, column family
  isolation, promotion swap atomicity (single-threaded simulation).
- Integration: full M4b binary with shadow core, synthetic Kafka reward stream,
  end-to-end load + chaos kill -9 + verify recovery and reconciliation.
- Property-based (proptest): for any sequence of shadow-only events, production
  state remains bit-identical. For any sequence of production-only events, shadow
  state remains bit-identical.
- Property-based (proptest): for any interleaving of `LoadShadowModel`,
  `PromoteShadow`, `UnloadShadow` calls and `SelectArm`/`ReportReward` events,
  the production core's state evolution is consistent with a serial schedule of
  the same operations.
- Benchmark (criterion): `SelectArm` p99 latency under (a) no shadow, (b) shadow
  active + low channel pressure, (c) shadow active + saturated channel.
- E2E with M5 ↔ M4b: full lifecycle test from `CreateExperiment` through
  CONCLUDING, exercising both `SHADOWING → LIVE` and `LIVE → SHADOWING` transitions.

## Alternatives Considered

### Alternative 1: Operator-facing shadow RPCs on M4b

Earlier draft. M4b exposed `LoadShadowModel`, `PromoteShadow`, etc. as
operator-facing RPCs, with the experiment lifecycle and the shadow lifecycle as
parallel concerns the operator coordinated manually. Rejected in favor of
lifecycle-driven shadow management (per ADR-027) — drift between experiment state
and shadow state was a real and recurring failure mode. The RPCs still exist but
are M5-internal.

### Alternative 2: Single core runs both production and shadow inference

Rejected. The LMAX core's value is single-thread ownership of state without
contention. Running both inferences on a single thread doubles per-request CPU
work, making the production p99 SLA trivially harder to meet. Worse, it couples
shadow correctness to production latency budgets.

### Alternative 3: Shadow core runs in a separate process

Considered. Process-level isolation would offer the strongest crash boundary
(production crash does not take down shadow and vice versa). Rejected because:
(a) shadow rewards must be derived from the same Kafka topic as production
rewards, so event-ordering coordination across processes is fragile; (b) RocksDB
column families already provide write isolation within a single process while
keeping snapshot recovery atomic; (c) the operational complexity of running two
M4b processes per experiment outweighs the failure-isolation benefit at our
scale. Re-evaluate if M4b ever exceeds the per-process memory footprint of a
multi-experiment, multi-shadow deployment.

### Alternative 4: Shadow inference computed offline from logs

Rejected. Replaying logged contexts through a candidate model offline produces a
historical evaluation, not a live evaluation. The point of shadow mode is to
validate the candidate against the same real-time context distribution and feature
freshness as production — which only an online shadow path provides.

### Alternative 5: Shadow inference at the SDK / caller side

Rejected. Pushing shadow inference to callers would require every consuming
service to load the candidate model, breaking Kaizen's centralization of bandit
policy serving. The whole point of M4b is that callers do not own model state.

### Alternative 6: Tokio task instead of dedicated thread for shadow core

Considered. A tokio task is lighter-weight than a dedicated thread. Rejected to
remain consistent with ADR-002's reasoning: state-mutating cores own a dedicated
OS thread to prevent the runtime scheduler from preempting state mutation under
load. The principle applies to shadow state for the same reason it applies to
production state.

## References

- ADR-002 (LMAX bandit core) — extended by this ADR
- ADR-003 (RocksDB policy state) — extended via column families
- ADR-011 (Candle ML framework) — applicable to shadow neural bandit inference
- ADR-027 (Shadow mode for experiments) — companion ADR; defines the experiment-level
  flag, sub-states, and transitions that drive this ADR's lifecycle integration
- Design doc v5.1, Section 8 (M4b Bandit Policy Service), Section 2.3 (LMAX threading)
- Netflix Tech Blog, "State of Routing in Model Serving" (May 2026):
  <https://netflixtechblog.com/state-of-routing-in-model-serving-16e22fe18741>
