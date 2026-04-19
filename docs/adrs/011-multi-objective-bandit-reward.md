# ADR-011: Multi-Objective Bandit Reward Composition

**Status**: Accepted and Implemented
**Date**: 2026-03-24
**Deciders**: Agent-4 (M4b Bandit Policy)
**Cluster**: A — Multi-Stakeholder Optimization

---

## Context

The Kaizen platform's bandit policy service (M4b) optimized a single scalar reward signal — typically a user engagement metric like watch-time or play-start rate. SVOD recommendation is a three-sided market involving subscribers, content providers, and the platform itself. These stakeholders have competing objectives:

- **Subscribers** want personalized, engaging content.
- **Content providers** want fair exposure across their catalog, not winner-take-all concentration.
- **The platform** wants long-term retention, catalog utilization, and content diversity.

Optimizing a single engagement metric creates pathological outcomes: catalog concentration on a few popular titles, provider inequity, filter bubbles, and reduced content diversity — all of which harm long-term platform health even as short-term engagement metrics improve.

The platform needed the ability to compose bandit rewards from multiple stakeholder objectives simultaneously, with configurable strategies for handling the inherent trade-offs between competing goals.

---

## Decision

Extend the LMAX single-threaded policy core (Thread 3, ADR-002) with a `RewardComposer` that transforms multi-metric reward vectors into scalar rewards for posterior updates. Three composition strategies are provided, selectable per experiment via `BanditConfig.composition_method`.

### Composition Strategies

**Weighted Scalarization** (`REWARD_COMPOSITION_WEIGHTED_SUM`):
```
reward = Σ wᵢ × normalized(rᵢ)
```
Weights `wᵢ` must sum to 1.0 (±1e-6). Produces optimal arms on convex regions of the Pareto frontier. Simplest to reason about; appropriate when objectives are roughly aligned.

**Epsilon-Constraint (Lagrangian)** (`REWARD_COMPOSITION_EPSILON_CONSTRAINT`):
```
reward = normalized(r_primary) + Σ_{secondary} credit(rᵢ, floorᵢ)
```
Maximizes a designated primary objective while treating secondaries as soft constraints with floor thresholds. Credit is awarded when a secondary exceeds its floor, penalty when below. Appropriate when one objective dominates but others must not degrade below acceptable levels.

**Tchebycheff** (`REWARD_COMPOSITION_TCHEBYCHEFF`):
```
reward = −max_i { wᵢ × max(0, idealᵢ − normalized(rᵢ)) }
```
Minimizes the maximum weighted deviation from an ideal point (running maximum per metric). Reaches Pareto-optimal solutions on non-convex frontiers where weighted scalarization cannot. Requires a warmup period (~100 observations) to establish stable ideal point estimates.

### Metric Normalization

Raw metric values arrive on different scales (watch-time in minutes, diversity scores in [0,1], provider Gini in [0,1]). A `MetricNormalizer` maintains per-metric running mean and variance via exponential moving average (EMA, α = 0.01):

```
normalized(r) = (r − μ_ema) / σ_ema
```

Normalization clamps to ±10σ to prevent outlier-driven instability. The normalizer also tracks a running-maximum ideal point per metric for Tchebycheff.

### LMAX Core Integration

All composition runs on the existing dedicated LMAX Thread 3. The reward update path extends from:
```
reward = scalar → posterior.update(reward)
```
to:
```
metric_values = {metric_1: r_1, ..., metric_k: r_k}
  → normalizer.normalize(metric_values)
  → composer.compose(normalized_values)
  → sigmoid(composed_reward)
  → posterior.update(mapped_reward)
```

The `sigmoid()` mapping converts the composed real-valued reward to (0, 1) for Beta-Bernoulli posterior compatibility. Composer and normalizer state are persisted in RocksDB alongside posterior parameters via the existing snapshot mechanism, surviving crashes and restarts.

The `sigmoid()` mapping is required only for Beta-Bernoulli posteriors, which expect rewards in (0, 1). Future Thompson Sampling variants using Gaussian posteriors would not require this mapping; the composer's output could be passed directly. The mapping is conditional on the policy's posterior type, not a universal requirement.

### State Growth

For a typical multi-objective experiment with 3 objectives and 10 arms, the composer adds ~200 bytes of state per metric per experiment. At 100 concurrent experiments, total additional RocksDB snapshot overhead is ~60KB — negligible relative to existing posterior state.

---

## Consequences

### Benefits

1. **Three-sided market optimization**: Bandits can now balance subscriber engagement, provider fairness, and platform health simultaneously.
2. **Zero-lock integration**: Composition runs on the existing LMAX single-threaded core with no new synchronization primitives.
3. **Crash-safe**: Normalizer and composer state persists through RocksDB snapshots alongside policy posteriors.
4. **Backward-compatible**: Single-objective experiments are unaffected. Multi-objective only activates when `BanditConfig.reward_objectives` is non-empty.
5. **Pareto coverage**: Tchebycheff composition reaches non-convex Pareto-optimal solutions that weighted scalarization cannot.

### Trade-offs

1. **Normalization warmup**: EMA normalization requires ~50–100 observations before mean/variance estimates stabilize. During warmup, composed rewards may be noisy.
2. **Tchebycheff ideal instability**: The running-maximum ideal point can be distorted by outliers early in the experiment. Clamping at ±10σ mitigates but does not eliminate this.
3. **IPW interaction**: When combined with ADR-012 LP constraints, the *adjusted* arm probabilities (not raw) must be used for IPW analysis. The composer itself does not affect assignment probabilities — only the reward signal.

---

## Implementation Details

### Default Behavior

When `BanditConfig.reward_objectives` is empty, the existing scalar reward path is used and `composition_method` is ignored. Single-objective experiments are not affected by this ADR — they continue to use the pre-existing `Posterior::update(scalar_reward)` flow without modification.

### Proto Schema

```protobuf
// bandit.proto additions (PR #196)

enum RewardCompositionMethod {
  REWARD_COMPOSITION_UNSPECIFIED = 0;
  REWARD_COMPOSITION_WEIGHTED_SUM = 1;
  REWARD_COMPOSITION_EPSILON_CONSTRAINT = 2;
  REWARD_COMPOSITION_TCHEBYCHEFF = 3;
}

message RewardObjective {
  string metric_id = 1;
  double weight = 2;
  optional double floor = 3;       // epsilon-constraint floor threshold
  bool is_primary = 4;             // epsilon-constraint primary designator
}

// BanditConfig extensions
message BanditConfig {
  // ... existing fields 1-7 ...
  repeated RewardObjective reward_objectives = 8;
  RewardCompositionMethod composition_method = 9;
}
```

### Crate Layout

```
crates/experimentation-bandit/src/
  reward_composer.rs    — RewardComposer, MetricNormalizer (serialize/deserialize)
  multi_objective.rs    — MetricStats (EMA), RewardObjective, CompositionMethod, sigmoid()
  lib.rs                — pub mod reward_composer; pub mod multi_objective;
```

### Public API

```rust
// MetricNormalizer — per-metric EMA running statistics
pub struct MetricNormalizer { /* keyed by metric name, serialize/deserialize */ }
impl MetricNormalizer {
    pub fn normalize(&mut self, metric: &str, value: f64) -> f64;
    pub fn ideal(&self, metric: &str) -> Option<f64>;  // Tchebycheff ideal point
}

// RewardComposer — strategy dispatch
pub struct RewardComposer { /* objectives, method, normalizer */ }
impl RewardComposer {
    pub fn compose(&mut self, metric_values: &HashMap<String, f64>) -> f64;
}

// ThompsonSamplingPolicy extensions
impl ThompsonSamplingPolicy {
    pub fn new_multi_objective(objectives: Vec<RewardObjective>, method: CompositionMethod) -> Self;
    pub fn update_multi_objective(&mut self, arm: &str, metrics: &HashMap<String, f64>);
}
```

### PolicyCore Integration

```rust
// crates/experimentation-policy/src/core.rs

impl PolicyCore {
    // Registers an experiment with multi-objective composition
    fn register_multi_objective_experiment(&mut self, exp_id: &str, config: &BanditConfig);

    // Extended reward handler — composes metric vector before posterior update
    fn handle_reward_update(&mut self, update: RewardUpdate) {
        if let Some(metrics) = &update.metric_values {
            let composer = self.reward_composers.get_mut(&update.experiment_id);
            let scalar = composer.compose(metrics);
            let mapped = sigmoid(scalar);
            self.policies.get_mut(&update.experiment_id)
                .update(update.arm_id, mapped);
        } else {
            // Single-objective fallback
            self.policies.get_mut(&update.experiment_id)
                .update(update.arm_id, update.reward);
        }
    }

    // Snapshot includes composer state
    fn write_snapshot(&self) -> SnapshotEnvelope {
        SnapshotEnvelope {
            policy_state: self.serialize_policies(),
            reward_composer_state: self.serialize_composers(),  // NEW
        }
    }
}
```

---

## Validation

### Unit Tests (18)

- MetricStats EMA convergence to known mean/variance
- Normalization direction (positive value → positive normalized when > mean)
- WeightedSum output matches manual computation
- EpsilonConstraint penalty when below floor, credit when above
- Tchebycheff balance: equal deviations produce equal penalty
- Serialize/deserialize roundtrip for normalizer and composer
- Weight validation: reject weights not summing to 1.0

### Proptest Invariants (4)

1. WeightedSum output is always finite for finite inputs
2. EpsilonConstraint output is always finite for finite inputs
3. Tchebycheff output is always finite for finite inputs
4. Tchebycheff output is non-positive after warmup (≥100 observations)

### Convergence Test

2-arm Thompson Sampling bandit with 2-metric weighted-sum reward (engagement w=0.6, quality w=0.4). Arm "high" (μ_eng=0.8, μ_qual=0.7) vs arm "low" (μ_eng=0.3, μ_qual=0.4). After 1000 rounds with Gaussian noise, arm "high" selected >60% of the time.

### Crash Recovery Test

50 multi-objective reward events → kill -9 → restart → normalizer restored with ≥45 observations (within Kafka replay tolerance).

---

## Dependencies

- **ADR-002** (LMAX core): Composer runs on Thread 3.
- **ADR-003** (RocksDB snapshots): Composer state persisted alongside posteriors.
- **ADR-014** (Provider Metrics): Provider-side metrics become available as reward objectives.
- **Enables ADR-012**: LP constraints operate on arm probabilities downstream of composed rewards.
- **Enables ADR-013**: Meta-experiments test different composition configurations.

---

## Merged PRs

| PR | Description |
|----|-------------|
| #196 | Proto schema: `RewardObjective`, `RewardCompositionMethod`, `BanditConfig` fields 8–9 |
| #221 | Multi-objective reward composition: `reward_composer.rs`, `multi_objective.rs`, PolicyCore integration |
| #228 | Reconciliation fixes + convergence test |

---

## References

- Qassimi et al.: MOC-MAB (Scientific Reports, 2025) — multi-objective contextual bandits
- Spotify calibrated bandits (RecSys 2025) — multi-stakeholder recommendation bandits
- Jannach & Abdollahpouri: Multi-stakeholder RecSys survey (Frontiers, 2023)
