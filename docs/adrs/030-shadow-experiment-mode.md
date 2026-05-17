# ADR-030: Shadow Mode for Experiments

## Status

Proposed

## Context

Kaizen currently supports five experiment types: `AB`, `INTERLEAVING`, `SESSION_LEVEL`,
`CONTEXTUAL_BANDIT`, and `CUMULATIVE_HOLDOUT`. Every existing type produces treatment
effects that influence the user-facing experience — assignment to a non-control variant
results in that variant's behavior being served.

Several Kaizen use cases require running a candidate variant on production traffic,
computing its metrics through the full M3/M4a pipeline, and analyzing the result,
**without exposing that variant's behavior to users**:

1. Validating a new recommendation/ranking model on real traffic before live A/B exposure.
2. Smoke-testing that a new feature implementation produces metrics consistent with the
   reference implementation under real load distributions.
3. Pre-launch verification that a candidate does not regress guardrail metrics
   relative to a synthetic projection.
4. Pairing with ADR-028 (M4b shadow inference) — running a candidate bandit policy as a
   shadow alongside production before promoting it to serve user traffic.

This pattern is well-established in industry. Netflix's Switchboard/Lightbulb routing
infrastructure
([Netflix Tech Blog, May 2026](https://netflixtechblog.com/state-of-routing-in-model-serving-16e22fe18741))
treats "shadow mode" as a first-class lifecycle stage for ML model promotion, alongside
canary and instant rollback.

The current alternative — running an A/B test where the candidate variant is gated to
0% traffic — does not solve the problem, because it produces no exposures and therefore
no metrics. Running the candidate as a 50/50 A/B exposes users to the unvalidated
variant. Shadow mode closes this gap.

### Why a flag, not a new experiment type

An earlier draft of this ADR proposed `EXPERIMENT_TYPE_SHADOW` as a sixth top-level
type. Two considerations argue for a flag instead:

**Shadow mode is always a shadow of something.** A "shadow neural bandit" needs the
contextual-bandit machinery — M4b's policy core, reward consumption, arm selection. A
"shadow A/B test of a new ranker" needs the AB machinery — variant assignment,
treatment effect analysis, no policy core. Shadow-ness does not replace the experiment
type's mechanics; it modifies their *effect surface* (does the served variant reach
the user, do guardrails auto-pause, does CONCLUDING trigger rollout). A separate type
either silently collapses the underlying type or reinvents a flag inside a sub-field.

**Shadow and live phases belong to a single experiment lifecycle.** A candidate is
typically validated in shadow, then transitioned to live exposure on the same
experiment. Modeling this as two separate experiments breaks the audit trail and
discards an analytically meaningful relationship: shadow-period observations on the
candidate variant are precisely the kind of pre-period data that CUPED can use as a
covariate to reduce variance in the live phase. A unified lifecycle preserves this.
For the bandit case, the shadow → live transition *is* the model promotion event; the
lifecycle and the promotion become the same operation.

A flag with appropriate validation captures both properties cleanly.

## Decision

We add an `is_shadow` flag to `Experiment` and a `SHADOWING` sub-state of `RUNNING` in
the experiment state machine. Shadow mode is a property of an experiment phase, not a
separate experiment type. A single experiment can transition from shadow to live and
(in the rollback case) back to shadow, with all observations and metric history
preserved.

### 1. Flag and validation

```protobuf
message Experiment {
  // existing fields …
  bool is_shadow = N;
}
```

Validation rule, enforced by M5 on `CreateExperiment` and any state transition:

- `is_shadow=true` is permitted for `EXPERIMENT_TYPE_AB` and
  `EXPERIMENT_TYPE_CONTEXTUAL_BANDIT` only.
- Forbidden for `EXPERIMENT_TYPE_INTERLEAVING` (no served variant — interleaving mixes
  results from both algorithms inline; "shadow interleaving" is incoherent).
- Forbidden for `EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT` (the entire point is sustained
  user exposure to a holdout group; shadow contradicts the design).
- Initially forbidden for `EXPERIMENT_TYPE_SESSION_LEVEL`; may be lifted in a future
  ADR if a clear use case emerges.

A `VariantPurpose` enum is added separately to distinguish user-facing from candidate
variants within an experiment. This is independent of `is_shadow` and is also used by
ADR-028 to identify which variants in a `CONTEXTUAL_BANDIT` experiment carry candidate
model URIs:

```protobuf
enum VariantPurpose {
  VARIANT_PURPOSE_UNSPECIFIED = 0;
  VARIANT_PURPOSE_CONTROL = 1;
  VARIANT_PURPOSE_TREATMENT = 2;
  VARIANT_PURPOSE_SHADOW_CANDIDATE = 3;
}

message Variant {
  // existing fields …
  VariantPurpose purpose = N;
  optional string candidate_model_uri = N+1;  // MLflow URI for shadow inference
                                              // (used by ADR-028)
}
```

### 2. Assignment behavior (M1)

When `is_shadow=true`:

- M1 computes the hash-based bucket assignment as it would for a normal experiment of
  that type, but always returns the control variant assignment to the caller.
- The "would-have" variant is emitted as `shadow_variant_id` on the resulting
  `ExposureEvent` and `GetAssignmentResponse`.

When `is_shadow=false`, behavior is unchanged from today.

```protobuf
message ExposureEvent {
  // existing fields …
  optional string shadow_variant_id = N;
}

message GetAssignmentResponse {
  // existing fields …
  optional string shadow_variant_id = N;
}
```

### 3. State machine (M5)

`RUNNING` gains two sub-states: `SHADOWING` and `LIVE`.

```
DRAFT → STARTING → RUNNING { SHADOWING, LIVE } → CONCLUDING → CONCLUDED → ARCHIVED
```

- Experiments created with `is_shadow=true` enter `RUNNING/SHADOWING` after STARTING.
- Experiments created with `is_shadow=false` enter `RUNNING/LIVE` after STARTING (the
  current behavior, preserved).
- An operator can transition `SHADOWING → LIVE` on a RUNNING experiment by setting
  `is_shadow=false`. This is the "promote to production" action.
- An operator can transition `LIVE → SHADOWING` on a RUNNING experiment by setting
  `is_shadow=true`. This is the "rollback to shadow" action — typically used only
  immediately after a regressed promotion.

State transitions:

| From | To | Trigger | Type-specific actions |
|---|---|---|---|
| STARTING | RUNNING/SHADOWING | startup, `is_shadow=true` | For `CONTEXTUAL_BANDIT`: M5 calls `LoadShadowModel` on M4b (per ADR-028). For `AB`: no special action. |
| STARTING | RUNNING/LIVE | startup, `is_shadow=false` | Existing behavior. |
| RUNNING/SHADOWING | RUNNING/LIVE | operator sets `is_shadow=false` | For `CONTEXTUAL_BANDIT`: M5 calls `PromoteShadow` on M4b with `retain_old_production_as_shadow=true`. For `AB`: M1 begins serving non-control variants per assignment. |
| RUNNING/LIVE | RUNNING/SHADOWING | operator sets `is_shadow=true` | For `CONTEXTUAL_BANDIT`: M5 calls `PromoteShadow` again (atomic swap, retaining the rolled-back model as the new shadow). For `AB`: M1 stops serving non-control variants. |
| RUNNING/* | CONCLUDING | normal conclusion | Standard CONCLUDING pipeline; PolicySnapshot runs only if the final phase was LIVE. |

Both transitions within RUNNING are recorded in `audit_trail` with the timestamp and
operator identity, providing a single chronological record of the experiment's
shadow-and-live history.

### 4. Effect-surface rules

The following behaviors are gated on the current `RUNNING` sub-state, not on the
experiment type:

- **Auto-pause from guardrail breach (ADR-008):** Active in `LIVE`, suppressed in
  `SHADOWING`. Guardrail breach events are still logged for diagnostic purposes but
  do not trigger state transitions while shadowing. Configurable; default is suppressed.
- **PolicySnapshot in CONCLUDING:** Runs only if the experiment's final RUNNING phase
  was `LIVE`. An experiment that concludes from `SHADOWING` has nothing to snapshot.
- **CONCLUDING rollout signals:** Emitted only for experiments concluded from `LIVE`.

### 5. Metric computation (M3)

M3 groups per-user metric summaries by `(variant_id, shadow_variant_id, phase)` where
`phase ∈ {SHADOWING, LIVE}` is determined from the `audit_trail` transitions for the
experiment over the metric event's timestamp. Each phase produces its own row stream,
allowing M4a to analyze them separately or jointly.

The `metric_summaries` and `daily_treatment_effects` tables gain:

- `is_shadow BOOLEAN NOT NULL DEFAULT FALSE`
- `shadow_variant_id STRING NULL`
- `phase STRING NOT NULL DEFAULT 'LIVE'` (`SHADOWING` or `LIVE`)

### 6. Statistical analysis (M4a)

M4a runs the standard analysis pipeline on each phase. Results are persisted with
`is_shadow` and `phase` flags. M6 surfaces shadow analysis in a panel that explicitly
states no users were affected during the shadowing phase.

A new analysis variant becomes available for experiments that have transitioned
SHADOWING → LIVE: **shadow-as-pre-period CUPED**. M4a uses shadow-phase observations
on the candidate variant as the pre-experiment covariate for live-phase analysis,
reducing variance on the live treatment effect. This is a strict generalization of
ADR-014's standard CUPED and is gated on a configurable minimum shadow-phase sample
size. Specification of the estimator and its acceptance criteria are deferred to a
follow-up ADR (or an extension to ADR-014).

## Consequences

### Positive

- A single `Experiment` ID covers the candidate's entire validation-and-launch
  lifecycle. The audit trail is continuous; the metric history is continuous; the
  PR description and dashboard link a stakeholder shares is one URL across both
  phases.
- For `CONTEXTUAL_BANDIT`, the shadow → live transition becomes the natural trigger
  for M4b's `PromoteShadow` operation (per ADR-028). The lifecycle event and the
  model promotion event are unified.
- Rollback is a one-flag-flip operation. Setting `is_shadow=true` on a RUNNING
  experiment fires the inverse `PromoteShadow`. The experiment is never lost; the
  rolled-back model is retained as the shadow for diagnosis.
- Shadow-phase observations become available as a pre-period covariate for live-phase
  CUPED, materially improving statistical power on candidates that were shadowed.
  This capability is only possible because both phases share an experiment ID.
- Reuses the entire existing M3/M4a/M6 stack with mostly additive proto, schema, and
  state-machine changes.
- The flag composes cleanly with the existing experiment types it applies to. M4b
  sees `CONTEXTUAL_BANDIT` and uses bandit machinery; the `is_shadow` flag tells it
  to load the policy core as a shadow core (per ADR-028). M1 sees `AB` and runs
  hash-based assignment; the flag tells it to return control regardless. The
  type-specific machinery is untouched.

### Negative

- The state machine grows: two new sub-states of RUNNING and two new transitions
  between them. M5's state-transition validation logic is more complex.
- Auto-pause and PolicySnapshot logic must consult sub-state, not just type. Static
  analysis of "what does CONCLUDING do for a CONTEXTUAL_BANDIT" now depends on the
  experiment's transition history, not just its type. Mitigated by the explicit
  `phase` column in `metric_summaries` and an explicit final-phase column in the
  experiment record.
- Storage cost: shadow exposures roughly double the volume of exposure events for
  any phase configured with multiple shadow variants, since both the served and
  shadow variants are recorded. The bound is per-phase; concluding a shadow phase
  ends the duplication.
- Risk of misinterpretation: stakeholders may mistake shadow-phase analysis results
  for live results. Mitigation in M6: shadow-phase results are visually distinct,
  carry an explicit "no users were exposed during this phase" label, and the phase
  column appears in every result table.
- Rollback (LIVE → SHADOWING) is a powerful operation that, used carelessly, could
  produce a confusing audit trail (live → shadow → live → shadow). M5 should rate-
  limit rollback transitions and require an audit-trail comment on each.

### Neutral

- Bucket reuse (ADR-009) applies unchanged. The same buckets are occupied across both
  phases of the experiment, since it is a single experiment.
- SDKs require no changes to the `getAssignment()` interface — the returned
  `variant_id` is always the variant whose behavior is served. The new
  `shadow_variant_id` field is consumed by `logExposure` to populate the corresponding
  field on `ExposureEvent`.
- The `VariantPurpose` enum is independent of the shadow flag and applies to all
  experiment types. It clarifies variant intent at the variant level; the shadow flag
  controls the effect surface at the experiment-phase level.

## Implementation

### Proto changes

```protobuf
// proto/experimentation/common/v1/experiment.proto
message Experiment {
  // existing fields …
  bool is_shadow = N;
}

enum VariantPurpose {
  VARIANT_PURPOSE_UNSPECIFIED = 0;
  VARIANT_PURPOSE_CONTROL = 1;
  VARIANT_PURPOSE_TREATMENT = 2;
  VARIANT_PURPOSE_SHADOW_CANDIDATE = 3;
}

message Variant {
  // existing fields …
  VariantPurpose purpose = N;
  optional string candidate_model_uri = N+1;
}

// proto/experimentation/common/v1/event.proto
message ExposureEvent {
  // existing fields …
  optional string shadow_variant_id = N;
}

// proto/experimentation/assignment/v1/assignment_service.proto
message GetAssignmentResponse {
  // existing fields …
  optional string shadow_variant_id = N;
}

// proto/experimentation/management/v1/management_service.proto
service ManagementService {
  // existing RPCs …
  rpc TransitionShadowMode(TransitionShadowModeRequest) returns (TransitionShadowModeResponse);
}

message TransitionShadowModeRequest {
  string experiment_id = 1;
  bool is_shadow = 2;        // false = SHADOWING → LIVE; true = LIVE → SHADOWING
  string operator_id = 3;
  string audit_comment = 4;  // required, recorded in audit_trail
}
```

### Delta Lake schema changes

Migration `delta/migrations/M2026XX_shadow_mode_columns.sql` adds to
`metric_summaries` and `daily_treatment_effects`:

- `is_shadow BOOLEAN NOT NULL DEFAULT FALSE`
- `shadow_variant_id STRING NULL`
- `phase STRING NOT NULL DEFAULT 'LIVE'`

### PostgreSQL schema changes

`experiments` table gains `is_shadow BOOLEAN NOT NULL DEFAULT FALSE` and
`final_running_phase STRING NULL` (set by M5 on entry to CONCLUDING).

`audit_trail` records every shadow-mode transition with timestamp, operator,
comment, and resulting sub-state.

### Service-level changes

| Service | Change | Owner |
|---|---|---|
| M1 | When experiment `is_shadow=true`, compute bucket → return control variant; emit `shadow_variant_id` in response and exposure | Agent-1 |
| M2 (ingest) | Persist `shadow_variant_id` from `ExposureEvent` to `exposures` Kafka topic | Agent-2 |
| M3 | Group metric summaries by `(variant_id, shadow_variant_id, phase)`; resolve `phase` from `audit_trail` per metric event timestamp | Agent-3 |
| M4a | Persist analysis results with `is_shadow` and `phase` flags; future: shadow-as-pre-period CUPED | Agent-4 |
| M5 | Add `is_shadow` validation; SHADOWING/LIVE sub-states; `TransitionShadowMode` RPC; effect-surface gating on auto-pause and CONCLUDING actions; trigger M4b promotion calls for CONTEXTUAL_BANDIT transitions | Agent-5 |
| M6 | Phase-aware analysis panels with explicit no-user-impact labeling on shadow phases; transition button on RUNNING experiments | Agent-6 |
| SDKs | Forward `shadow_variant_id` in `logExposure`; no interface change | Agent-1 |

### Acceptance criteria

- An `AB` experiment created with `is_shadow=true` transitions to `RUNNING/SHADOWING`.
  M1 emits exposure events where `variant_id == control_variant.id` always, and
  `shadow_variant_id` is populated per-user via deterministic hash-based bucketing.
- A `CONTEXTUAL_BANDIT` experiment created with `is_shadow=true` triggers M5 to call
  M4b's `LoadShadowModel` on entry to `RUNNING/SHADOWING`.
- `TransitionShadowMode` with `is_shadow=false` on a `RUNNING/SHADOWING`
  `CONTEXTUAL_BANDIT` experiment triggers M4b's `PromoteShadow` and moves the
  experiment to `RUNNING/LIVE`. Subsequent `SelectArm` requests are served by the
  promoted policy.
- `TransitionShadowMode` with `is_shadow=true` on a `RUNNING/LIVE` experiment moves
  it to `RUNNING/SHADOWING` and (for `CONTEXTUAL_BANDIT`) calls `PromoteShadow` again
  to swap back.
- Guardrail breach during `SHADOWING` does not trigger auto-pause; the same breach
  during `LIVE` does, per ADR-008.
- CONCLUDING from `SHADOWING` skips PolicySnapshot and emits no rollout signal.
  CONCLUDING from `LIVE` runs the standard pipeline.
- M3 produces `metric_summaries` rows partitioned by `phase`, with correct phase
  resolution across the SHADOWING → LIVE transition timestamp.
- M6 renders shadow-phase results in a visually distinct panel labeled "No users
  were exposed during this phase."

### Test requirements

- Unit (M1): hash-based bucketing for `is_shadow=true` experiments returns control as
  `variant_id` and the bucketed variant as `shadow_variant_id` across the 10,000 hash
  test vectors. Same vectors with `is_shadow=false` produce the existing behavior.
- Unit (M5): state-machine transitions validate type compatibility and produce
  correct audit-trail entries.
- Integration (M1 ↔ M2 ↔ M3): exposure with `shadow_variant_id` round-trips through
  Kafka into `metric_summaries` with correct `is_shadow` and `phase`.
- Integration (M5 ↔ M4b, deferred to ADR-028): SHADOWING → LIVE transition on a
  `CONTEXTUAL_BANDIT` experiment triggers `PromoteShadow` atomically.
- Property-based (M4a, proptest): per-phase analysis results are statistically
  identical to results computed on equivalent observations in a non-shadow
  experiment — i.e., the shadow flag and phase do not alter computation, only
  partitioning and labeling.
- E2E: create `AB` experiment with `is_shadow=true`, run synthetic traffic, transition
  to LIVE, run more synthetic traffic, conclude. Verify M3 produces correctly
  partitioned metric summaries and M4a produces per-phase analysis.

## Alternatives Considered

### Alternative 1: Top-level `EXPERIMENT_TYPE_SHADOW`

Rejected. Shadow mode is always a shadow of an underlying type with type-specific
mechanics. A top-level type either silently collapses the underlying type or
reinvents a flag inside a sub-field. Forcing a single SHADOW type to handle both AB
and CONTEXTUAL_BANDIT semantics would require M4b to inspect `experiment_type ==
SHADOW AND inner_type == CONTEXTUAL_BANDIT` — strictly worse than the flag.

### Alternative 2: Separate experiments for shadow and live phases

Rejected. Modeling shadow and live as separate experiments breaks the audit trail and
discards the analytical relationship between phases. It also makes rollback an
ad-hoc operation (start a new experiment with the old config) rather than a
first-class transition. The shadow-as-pre-period CUPED capability is impossible
across distinct experiment IDs.

### Alternative 3: Shadow as a feature-flag mode (M7)

Rejected. M7's responsibility is flag evaluation and progressive delivery, not metric
analysis or experiment lifecycle. Shadow mode requires the full M3/M4a pipeline,
which is M5's domain.

### Alternative 4: 0%-allocation A/B tests

Rejected as inadequate. With 0% allocation, no exposures are emitted, so no metrics
are computed. The candidate variant cannot be analyzed at all.

### Alternative 5: Allow shadow on `INTERLEAVING` and `CUMULATIVE_HOLDOUT`

Rejected. Interleaving has no served variant — shadowing is incoherent. Cumulative
holdout's design assumes sustained user exposure to a holdout group; a shadow holdout
is a contradiction. `SESSION_LEVEL` is deferred pending a clear use case.

## References

- ADR-002 (LMAX bandit core) — extended by ADR-028 for the bandit case
- ADR-008 (auto-pause guardrails) — gated on RUNNING sub-state by this ADR
- ADR-009 (bucket reuse) — applies unchanged
- ADR-014 (CUPED variance reduction) — extended in a follow-up ADR for shadow-as-pre-period
- ADR-028 (M4b shadow inference path) — companion ADR; consumes the shadow flag and
  the SHADOWING ↔ LIVE transitions on `CONTEXTUAL_BANDIT` experiments
- Design doc v5.1, Section 3 (Experiment Lifecycle State Machine), Section 7
  (Statistical Analysis Engine), Section 8 (Bandit Policy Service)
- Netflix Tech Blog, "State of Routing in Model Serving" (May 2026):
  <https://netflixtechblog.com/state-of-routing-in-model-serving-16e22fe18741>
