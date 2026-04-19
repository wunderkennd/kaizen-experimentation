# ADR-013: Meta-Experiments on Objective Functions

**Status**: Accepted (Planned — Sprint 5.4)
**Date**: 2026-03-24
**Deciders**: Agent-1 (M1 Assignment), Agent-4 (M4b Bandit Policy), Agent-5 (M5 Management)
**Cluster**: A — Multi-Stakeholder Optimization

---

## Context

ADR-011 (Multi-Objective Reward) and ADR-012 (LP Constraints) give the platform the ability to configure bandit reward composition and constraint parameters. But choosing the *right* configuration is itself an optimization problem:

- Should the engagement weight be 0.7 or 0.5?
- Is Tchebycheff better than weighted scalarization for this content catalog?
- Does a 5% provider floor constraint improve long-term retention or hurt it?
- Does adding a diversity objective reduce churn more than adding a fairness constraint?

These are causal questions about the objective function itself — they cannot be answered by observing a single bandit's behavior. They require *randomizing users over different objective parameterizations* while holding the bandit algorithm constant, then comparing business outcomes (retention, LTV, satisfaction) across parameterization variants.

This is a meta-experiment: an A/B test where the treatment arms are not content recommendations but the reward function configurations that drive the bandit's behavior.

---

## Decision

Introduce `EXPERIMENT_TYPE_META` as a new experiment type with dedicated lifecycle validation, isolated policy state per variant, and two-level IPW analysis.

### Design

A meta-experiment has 2+ variants. Each variant specifies a complete `MetaVariantObjective` configuration: reward objectives, composition method, and optional LP constraints. All variants use the same bandit algorithm (e.g., Thompson Sampling). The experiment randomizes users across variants using standard hash-based bucketing (same as A/B tests). Each variant runs its own independent bandit instance.

```
Meta-Experiment: "Engagement vs. Balance"
├── Variant A: WeightedSum(engagement=0.7, diversity=0.3)
│   └── Independent Thompson Sampling bandit (isolated state)
├── Variant B: WeightedSum(engagement=0.5, diversity=0.5)
│   └── Independent Thompson Sampling bandit (isolated state)
└── Variant C: Tchebycheff(engagement=0.6, diversity=0.4)
    └── Independent Thompson Sampling bandit (isolated state)

Primary metric: 30-day retention (NOT a bandit reward metric)
```

The primary outcome metric must be a business outcome (retention, LTV, satisfaction survey score) that is *not* one of the bandit's reward objectives. This prevents circular reasoning — the meta-experiment evaluates which reward configuration produces the best downstream business result, not which configuration maximizes its own reward.

### M1 Assignment Flow

```
User request → M1 GetAssignment(experiment_id=meta-exp, user_id=U)
  1. Hash U into meta-experiment variant (standard bucketing)
  2. Look up variant's MetaVariantObjective config
  3. Call M4b SelectArm with (experiment_id, variant_id, user_context)
  4. M4b routes to the variant-specific policy instance
  5. Return: variant assignment + arm selection + probabilities
```

The user sees a single recommendation. They are unaware they are in a meta-experiment — the only difference between variants is which reward function drives the bandit's learning.

### M4b Policy Isolation

Each (experiment_id, variant_id) pair gets its own independent policy state on the LMAX Thread 3. Variants do not share posteriors, normalizer state, or composer state. This ensures that each variant's bandit learns independently under its assigned reward configuration.

Policy state key changes from:
```
HashMap<ExperimentId, PolicyState>
```
to:
```
HashMap<(ExperimentId, VariantId), PolicyState>
```

For non-meta experiments, `VariantId` is a sentinel value (empty string), preserving backward compatibility.

### M4a Two-Level IPW Analysis

Meta-experiments have two levels of randomization:
1. User → meta-variant (hash-based, known probability from traffic allocation)
2. User → arm (bandit-selected, logged probability from SelectArm)

Treatment effect estimation for the primary metric uses two-level IPW:
```
weight = 1 / (P(variant) × P(arm | variant))
```

M4a's `RunAnalysis` for META experiments computes:
- **Cross-variant business outcome comparison**: treatment effects on the primary metric (retention, LTV) between meta-variants. This is the key output.
- **Per-variant bandit efficiency**: convergence speed, cumulative regret, arm concentration.
- **Per-variant ecosystem health**: provider fairness metrics (from ADR-014) under each configuration.
- **Pareto frontier visualization**: for 3+ variants, scatter plot of (engagement, diversity) with dominated region shading.

---

## Consequences

### Benefits

1. **Causal answers about objective design**: Eliminates guesswork in reward function configuration. Data-driven selection of composition strategies and weights.
2. **Isolated learning**: Each variant's bandit learns independently, preventing cross-contamination of policy state.
3. **Reuses existing infrastructure**: Hash-based bucketing (M1), standard analysis (M4a), existing UI results rendering (M6). The meta layer is thin.
4. **Prevents circular metrics**: Requiring the primary metric to differ from reward objectives ensures the meta-experiment measures downstream impact, not tautological reward maximization.

### Trade-offs

1. **Traffic splitting**: A meta-experiment with 3 variants and 10 bandit arms per variant means each variant-arm combination sees 1/30th of total traffic. Bandit learning within each variant is slower than a single unconstrained bandit.
2. **Duration**: Meta-experiments must run long enough for both bandit convergence *and* business outcome measurement. Typical duration: 4–8 weeks (vs. 1–2 weeks for standard A/B tests).
3. **Complexity for experiment owners**: Configuring a meta-experiment requires understanding reward composition (ADR-011) and optionally LP constraints (ADR-012). The M6 creation wizard should guide this.
4. **Policy state footprint**: K variants × N arms × posterior state. For a meta-experiment with 4 variants and 20 arms, this is 4× the state of a single bandit experiment.

---

## Implementation Details

### Proto Schema

```protobuf
// experiment.proto additions (PR #196)

enum ExperimentType {
  // ... existing types 0-8 ...
  EXPERIMENT_TYPE_META = 9;
}

message MetaExperimentConfig {
  string base_algorithm = 1;                        // e.g., "THOMPSON_SAMPLING"
  repeated MetaVariantObjective variant_objectives = 2;
}

message MetaVariantObjective {
  string variant_id = 1;
  repeated RewardObjective reward_objectives = 2;
  RewardCompositionMethod composition_method = 3;
  repeated ArmConstraint arm_constraints = 4;       // optional, per-variant LP
  repeated GlobalConstraint global_constraints = 5;  // optional, per-variant LP
  string payload_json = 6;                           // additional config as JSON
}

// Experiment message gains:
message Experiment {
  // ... existing fields 1-26 ...
  MetaExperimentConfig meta_config = 30;
}
```

### M5 STARTING Validation

When `experiment_type == META`, `StartExperiment` validates:

1. `meta_config.base_algorithm` is set and maps to a known algorithm
2. `meta_config.variant_objectives` has one entry per variant defined on the experiment
3. Each variant's `reward_objectives` weights sum to 1.0 (±1e-6)
4. Each variant's `metric_ids` resolve to existing metric definitions
5. All referenced metric_ids have `aggregation_level = USER`
6. The experiment's `primary_metric_id` is NOT present in any variant's `reward_objectives` (warn, not block — in case the user intentionally wants this)
7. If LP constraints are specified per variant, constraint consistency is validated (floors don't sum to >1.0)

### M1 Assignment Changes

`GetAssignment` for META experiments:
1. Hash user to variant using standard bucketing (same as A/B)
2. Extract the variant's `MetaVariantObjective` from cached config
3. Call M4b `SelectArm` with `experiment_id`, `variant_id`, and user context
4. Return `AssignmentResponse` with variant_id, arm_id, and assignment_probability

The variant_id is included in the exposure event for downstream analysis.

### M4b Policy State Changes

```rust
// Policy state keyed by (experiment, variant) tuple
type PolicyKey = (String, String);  // (experiment_id, variant_id)

struct PolicyCore {
    policies: HashMap<PolicyKey, Box<dyn BanditPolicy>>,
    reward_composers: HashMap<PolicyKey, RewardComposer>,
    constraint_solvers: HashMap<PolicyKey, LpConstraintSolver>,
}
```

On `SelectArm(experiment_id, variant_id, context)`:
- Look up `(experiment_id, variant_id)` in the policy map
- If not found and this is a META experiment, lazily initialize from the variant's config
- Run the variant's independent policy + composer + LP solver pipeline
- Return arm selection with probabilities

### M6 UI — Meta-Experiment Results Page

- **Objective comparison table**: Side-by-side variant configs (composition method, weights, constraints)
- **Business outcome comparison**: Primary metric treatment effects between variants (forest plot)
- **Ecosystem health comparison**: Provider fairness metrics per variant (from ADR-014)
- **Bandit efficiency per variant**: Convergence curves, cumulative regret, arm concentration
- **Pareto frontier visualization** (3+ variants): D3 scatter plot with engagement vs. diversity axes, dominated region shading, variant labels

### M6 UI — Create Experiment Form

The meta-experiment creation wizard:
1. Select `EXPERIMENT_TYPE_META`
2. Choose base algorithm
3. For each variant: configure reward objectives, composition method, optional LP constraints
4. Select primary metric (with validation that it's not a reward metric)
5. Review: side-by-side comparison of variant configurations

---

## Validation (Planned)

### Unit Tests

- M5: STARTING validation rejects missing base_algorithm, mismatched variant count, non-resolving metric_ids
- M5: STARTING validation warns when primary_metric overlaps reward objectives
- M4b: Isolated policy state — reward updates for variant A do not affect variant B posteriors
- M4a: Two-level IPW weight computation matches manual calculation
- M1: Hash bucketing assigns user to variant, then delegates to M4b with correct variant_id

### Contract Tests

- M1 ↔ M4b: Meta-experiment variant-specific policy routing (SelectArm with variant_id)
- M5 ↔ M6: Meta-experiment config rendering (variant objectives displayed correctly)
- M5 ↔ M1: StreamConfigUpdates includes META experiment configs with variant objectives

### Integration Tests

- End-to-end: Create META experiment → start → assign users → feed variant-specific rewards → run analysis → cross-variant business outcome comparison

---

## Dependencies

- **ADR-011** (Multi-objective reward): Meta-variants configure reward composition. ADR-011 must be implemented before meta-experiments can use multi-objective rewards.
- **ADR-012** (LP constraints): Meta-variants can optionally include LP constraints. ADR-012 should be implemented for full functionality, but meta-experiments work without it (constraints are optional per variant).
- **ADR-014** (Provider metrics): Ecosystem health comparison uses provider-side metrics.
- **Proto PR #196**: `EXPERIMENT_TYPE_META`, `MetaExperimentConfig`, `MetaVariantObjective` landed.

---

## Current Status

- Proto schema landed in PR #196
- Implementation planned for Sprint 5.4
- Agent-1: M1 routing logic for META experiments (planned)
- Agent-4: M4b isolated policy state per (experiment, variant) (planned)
- Agent-5: M5 STARTING validation for `MetaExperimentConfig` (planned)
- Agent-6: M6 meta-experiment results page (planned)

---

## Rejected Alternatives

| Alternative | Reason Rejected |
|-------------|----------------|
| Manually run separate bandit experiments with different configs | No within-experiment statistical comparison. User populations differ across experiments. Selection bias. |
| Use the portfolio system (ADR-019) to compare historical experiments | Confounded by time, population shifts, and concurrent platform changes. Not causal. |
| Contextual bandit with reward config as context feature | Conflates the meta-level question (which config is better?) with the object-level question (which arm is better?). Policy cannot disentangle the two. |
| Multi-armed bandit over reward configs (bandit of bandits) | Interesting theoretically but operationally complex. Requires convergence at two levels simultaneously. Meta-experiments with A/B randomization are simpler and produce standard causal estimates. |

---

## References

- Netflix incrementality bandits (Data Council 2025) — testing bandit configurations
- Spotify calibrated bandits (RecSys 2025) — multi-stakeholder objective tuning
- Lattimore & Szepesvári: Bandit Algorithms (2020) — Chapter 36: meta-learning and algorithm selection
