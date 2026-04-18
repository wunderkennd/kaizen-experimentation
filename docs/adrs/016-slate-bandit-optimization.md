# ADR-016: Slate-Level Bandit Optimization and Off-Policy Evaluation

- **Status**: Proposed
- **Date**: 2026-03-19
- **Author**: Agent-4 (Bandit) / Agent-1 (Assignment)
- **Supersedes**: None (extends ADR-002 LMAX core; complements existing interleaving in M1)

## Context

Kaizen's bandits operate at the single-arm level: Thompson Sampling, LinUCB, and the neural contextual bandit each select one arm per request. But SVOD homepage recommendation is fundamentally a *slate* problem — the platform presents an ordered list of 10–50 items, and the value of the slate depends on cross-item interactions (diversity, complementarity, position effects, redundancy avoidance). Selecting items independently and assembling them into a list ignores these interactions.

Kaizen's interleaving module (Team Draft, Optimized, Multileave) addresses a related problem — comparing two ranking algorithms — but it is not a bandit: it does not learn or adapt. The cold-start bandit selects a single content placement per request. Neither addresses the combinatorial optimization of composing a full slate where items interact.

The combinatorial action space is the core challenge. A 50-item slate drawn from a 7,000-title catalog has ~10^130 possible compositions — far beyond exhaustive enumeration. Three lines of 2024–2025 research make this tractable:

1. **LIPS (Latent IPS)** — Kiyohara, Nomura, and Saito (WWW 2024): defines importance weights in a learned low-dimensional abstraction space rather than the full combinatorial space, enabling unbiased off-policy evaluation of slate policies. Optimizes the abstraction itself to minimize MSE.

2. **Slot-wise factorized contextual slate bandits** — Goyal et al. (UAI 2025): decomposes the slate into independent slot-level decisions with shared context, achieving sub-millisecond inference and O(K × L) regret (K items, L slots) rather than O(K^L) for the full combinatorial problem.

3. **GeMS (Generative Model for Slate recommendation)** — Deffayet et al. (WSDM 2023, heavily cited through 2025): encodes slates in a continuous latent space via a VAE, enabling an RL agent to optimize entire slates as holistic units rather than assembling items independently.

Netflix's incrementality-based bandit policy (Data Council 2025) adds an SVOD-specific insight: recommend titles that bring the maximum *causal increase* in engagement, not just titles with the highest absolute engagement probability. Their budget-constrained RL framework models finite user browsing time as an MDP constraint, achieving ~8% play rate increase and ~15% effective slate size improvement over myopic approaches.

## Decision

Introduce slate-level bandit optimization as a new bandit algorithm class in M4b, with a corresponding off-policy evaluation capability in M4a.

### Slate Bandit Algorithm: Slot-Wise Factorized Thompson Sampling

Adopt the slot-wise factorization approach as the primary slate bandit algorithm, extended with Thompson Sampling for exploration:

```protobuf
enum BanditAlgorithm {
  // ... existing values ...
  BANDIT_ALGORITHM_SLATE_FACTORIZED_TS = 5;   // NEW: slot-wise factorized Thompson
  BANDIT_ALGORITHM_SLATE_GENERATIVE = 6;       // NEW: GeMS-style VAE slate generation
}

message SlateConfig {
  // Number of slots in the slate (e.g., 10 for a homepage row).
  int32 num_slots = 1;
  // Maximum items per slot (for candidate pre-filtering).
  int32 candidates_per_slot = 2;
  // Whether to enforce no-duplicate items across slots.
  bool enforce_item_uniqueness = 3;
  // Position bias model: how slot position affects engagement.
  PositionBiasModel position_bias = 4;
  // Cross-item interaction model (for non-factorized approaches).
  SlateInteractionModel interaction_model = 5;
}

enum PositionBiasModel {
  POSITION_BIAS_NONE = 0;
  POSITION_BIAS_CASCADE = 1;         // User scans top-to-bottom, stops at first click
  POSITION_BIAS_GEOMETRIC_DECAY = 2; // Examination probability decays geometrically
  POSITION_BIAS_LEARNED = 3;         // Position bias learned from data
}

enum SlateInteractionModel {
  SLATE_INTERACTION_NONE = 0;            // Independent slots (factorized)
  SLATE_INTERACTION_DIVERSITY_PENALTY = 1; // Penalize redundant items
  SLATE_INTERACTION_FULL_CONTEXT = 2;    // GeMS-style holistic slate encoding
}
```

**Factorized approach (default)**: Each slot maintains its own Thompson Sampling posterior, but slots share a global context vector (user features + items already selected in higher-priority slots). The sequential selection process:

```
for slot_i in 0..num_slots:
    context_i = user_context ⊕ [items selected in slots 0..i-1]
    sample θ_i ~ Posterior_i(context_i)
    select item_i = argmax_j θ_i(j) over remaining candidates
    update context for next slot
```

This runs in O(L × K) where L = slots and K = candidates per slot, achieving sub-millisecond inference for typical SVOD configurations (L=10, K=100).

**Generative approach (optional, behind feature flag)**: For experiments requiring full cross-item modeling, implement GeMS-style VAE encoding. The encoder maps a candidate set to a latent slate representation; the decoder generates a complete slate. The RL agent optimizes in the latent space. This runs on the Candle neural network backend (ADR-006, `gpu` feature flag).

### LMAX Integration

Slate bandits run on the same LMAX dedicated thread (ADR-002). The policy state expands from one posterior per arm to one posterior per (slot, candidate) pair:

```rust
enum PolicyState {
    // Existing
    SingleArm(HashMap<ArmId, ArmPosterior>),
    // New
    Slate {
        slot_posteriors: Vec<HashMap<ItemId, ArmPosterior>>,  // per-slot
        position_bias: PositionBiasParams,
        interaction_penalty: Option<DiversityPenaltyParams>,
    },
}
```

RocksDB snapshots include the full slate policy state. Crash recovery replays reward events with per-slot credit assignment.

### Reward Attribution

Slate bandits require credit assignment: when a user engages with item 3 in a 10-item slate, how much credit does each slot's decision receive?

Three attribution models (configured via `SlateConfig`):

1. **Clicked-slot only**: Only the slot containing the engaged item receives reward. Simple but ignores the contribution of other items to the user's browsing context.
2. **Position-weighted**: All slots receive reward weighted by their position bias coefficient. Accounts for the fact that higher-ranked items influenced the user's decision to keep browsing.
3. **Counterfactual**: Estimate each slot's contribution via a leave-one-out counterfactual: "would the user have engaged if this slot contained a random item?" Requires the position bias model for estimation.

### Off-Policy Evaluation: LIPS Estimator

For offline evaluation of candidate slate policies before deployment, implement the LIPS (Latent IPS) estimator in `experimentation-stats`:

```rust
/// LIPS: Latent Importance-weighted Propensity Scoring for slates.
/// Kiyohara, Nomura, Saito (WWW 2024).
///
/// Maps slates to a learned low-dimensional abstraction space,
/// then computes importance weights in the abstraction space
/// rather than the full combinatorial space.
pub struct LipsEstimator {
    /// Abstraction function: slate -> latent representation
    abstraction: LearnedAbstraction,
    /// Logging policy's latent propensities
    logging_propensities: HashMap<LatentKey, f64>,
}

impl LipsEstimator {
    /// Estimate the value of a target policy using logged data.
    pub fn evaluate(
        &self,
        target_policy: &dyn SlatePolicy,
        logged_data: &[SlateInteraction],
    ) -> OpeResult;
}
```

LIPS is used during the STARTING phase to estimate whether a new slate policy is likely to improve over the current production policy before exposing users to it. M5 can require a positive LIPS estimate as a validation gate for slate bandit experiments.

### Assignment Service Integration (M1)

M1's `GetAssignment` RPC is not suitable for slate requests — it returns a single variant, not an ordered list. A new RPC on the Assignment Service:

```protobuf
// On AssignmentService:
rpc GetSlateAssignment(GetSlateAssignmentRequest) returns (GetSlateAssignmentResponse);

message GetSlateAssignmentRequest {
  string experiment_id = 1;
  string user_id = 2;
  // Candidate items for slate construction.
  repeated string candidate_item_ids = 3;
  map<string, double> context_features = 4;
}

message GetSlateAssignmentResponse {
  // Ordered slate: item IDs in display order.
  repeated string slate_item_ids = 1;
  // Per-slot assignment probabilities (for IPW analysis).
  repeated double slot_probabilities = 2;
  // Overall slate probability (product of slot probabilities
  // under factorized model; latent probability under GeMS).
  double slate_probability = 3;
}
```

M1 forwards the request to M4b, which runs the slate bandit on the LMAX core and returns the composed slate.

### IPW for Slate Experiments

M4a's IPW analysis (Hájek estimator) extends to slates using the factorized probability:

```
P(slate) = Π_i P(item_i | slot_i, context_i, items_{<i})
```

Under the factorized model, this product is exact. Under the generative model, `slate_probability` is the VAE's latent probability. Both are logged in the exposure event for downstream analysis.

For doubly-robust estimation (variance reduction over pure IPW), M4a combines the LIPS abstraction with a direct method (reward prediction model) to produce a DR-LIPS estimate.

## Consequences

### Positive

- Kaizen can optimize entire recommendation slates rather than individual items, capturing cross-item diversity, complementarity, and position effects.
- Factorized approach maintains sub-millisecond inference compatible with the 15ms SelectArm SLA, while providing a principled way to account for position bias and item interactions.
- LIPS OPE enables safe offline evaluation of slate policies before deployment — a critical capability when the combinatorial action space makes online exploration expensive.
- Natural integration with ADR-012 (LP constraints): the LP layer can enforce per-slot constraints (e.g., "slot 1 must be an original title") after the slate bandit produces raw probabilities.

### Negative

- Policy state scales as O(L × K) per experiment (L slots × K candidates), compared to O(K) for single-arm bandits. For L=10, K=1000: ~10× more state. RocksDB snapshot sizes increase proportionally.
- Reward attribution is an unsolved problem in general. The three attribution models are approximations; no model perfectly captures how a user's engagement with one item was influenced by the presence of other items in the slate.
- GeMS-style generative approach requires GPU inference for the VAE, limiting it to the `gpu` feature flag and adding infrastructure complexity.
- New RPC (`GetSlateAssignment`) breaks the existing SDK abstraction where `getVariant()` returns a string. SDKs need a new `getSlate()` method.

### Risks

- Exploration in combinatorial spaces is inherently expensive. Even with factorization, a 10-slot slate with 100 candidates per slot has 100^10 possible slates. The warmup period (ADR-002, default 1,000 observations) may be insufficient for convergence. Recommended warmup for slate bandits: 10,000+ observations.
- Position bias model misspecification can introduce systematic bias in reward attribution. Mitigation: learn position bias from randomized position experiments (short-duration, logged data) rather than assuming a parametric form.

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| Single-arm bandit + post-hoc re-ranking (status quo) | Simple; existing infrastructure | Ignores cross-item interactions; re-ranking breaks IPW | The core gap this ADR addresses |
| Full combinatorial TS over all possible slates | Theoretically optimal | Intractable: K^L arms with L=10, K=100 | Computationally impossible |
| Interleaving only (no slate learning) | Already implemented | Compares algorithms but doesn't learn/adapt; no bandit loop | Interleaving is evaluation, not optimization |
| List-wise learning-to-rank | Mature field; many off-the-shelf models | Not a bandit — no exploration; requires retraining on new data; doesn't integrate with the experimentation framework | Doesn't address the exploration-exploitation tradeoff |

## References

- Kiyohara, Nomura, Saito: "Off-Policy Evaluation of Slate Bandit Policies via Optimizing Abstraction" (LIPS, WWW 2024)
- Goyal et al.: slot-wise factorized contextual slate bandits (UAI 2025)
- Deffayet et al.: "GeMS: Generative Model for Slate Recommendation" (WSDM 2023)
- Netflix incrementality-based bandit policy (Data Council 2025)
- Netflix budget-constrained RL for time-constrained recommendations (2022–2025)
- Saito et al.: long-term OPE for slate policies (WWW 2024)
- ADR-002 (LMAX single-threaded policy core)
- ADR-011 (Multi-objective reward — slate reward composition)
- ADR-012 (LP constraints — per-slot constraints)
