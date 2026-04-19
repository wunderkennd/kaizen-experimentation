# ADR-012: Constrained Arm Selection via Linear Programming

**Status**: Accepted (In Progress)
**Date**: 2026-03-24
**Deciders**: Agent-4 (M4b Bandit Policy)
**Cluster**: A — Multi-Stakeholder Optimization

---

## Context

After ADR-011 introduced multi-objective reward composition, the bandit policy produces arm selection probabilities **p** that maximize composed reward. However, these raw probabilities are unconstrained — they can produce outcomes that violate hard business requirements:

- A content provider contract guarantees ≥5% impression share. Thompson Sampling may allocate 0% to arms serving that provider's content if engagement is low.
- A regulatory requirement mandates minimum representation of local-language content. The bandit has no awareness of this constraint.
- A fairness policy requires no single arm to exceed 40% of total traffic. An arm with high reward may receive 80%+.

These are *hard constraints* that cannot be expressed as soft reward objectives (ADR-011) — violating a provider contract has legal consequences regardless of how much engagement improves. The platform needed a deterministic post-processing layer between the bandit's raw probabilities and the actual selection distribution that enforces constraint satisfaction while minimizing distortion to the bandit's learned preferences.

---

## Decision

Implement an `LpConstraintSolver` that runs on the LMAX Thread 3 after arm probability computation and before arm selection. The solver finds the selection distribution **q** closest to the bandit's raw distribution **p** (in KL-divergence) that satisfies all configured constraints.

### Optimization Problem

```
minimize   KL(q ‖ p) = Σᵢ qᵢ · log(qᵢ / pᵢ)
subject to:
  Σᵢ qᵢ = 1                              (probability simplex)
  qᵢ ≥ 0                    ∀i            (non-negativity)
  qᵢ ≥ floor_i              ∀i ∈ F        (per-arm floor constraints)
  qᵢ ≤ ceil_i               ∀i ∈ C        (per-arm ceiling constraints)
  Σⱼ aⱼᵢ · qᵢ ≥ bⱼ         ∀j ∈ G        (general linear constraints)
```

### Constraint Types

**Per-arm constraints** (`ArmConstraint`): Floor and/or ceiling on individual arm selection probabilities. Use case: provider minimum impression guarantee, maximum arm concentration limit.

**Global linear constraints** (`GlobalConstraint`): Arbitrary linear inequalities over the probability vector. Use case: "arms tagged as local-language content must collectively receive ≥20% traffic" — a single linear constraint `Σ_{i ∈ local} qᵢ ≥ 0.20`.

### Solver Algorithms

**Simple per-arm constraints only** (no general linear): O(K log K) water-filling algorithm. Sort arms by `pᵢ`, iteratively redistribute probability mass from unconstrained arms to those below their floor. Guaranteed to find the KL-minimizing solution.

**General linear constraints**: Interior-point method with warm start from the previous solution. Target: <50μs for K ≤ 100 arms and ≤ 10 linear constraints. Warm starting from the previous time step's solution amortizes convergence cost — the constraint polytope shifts slowly as population-level statistics update.

### Population-Level Enforcement

Per-arm constraints express *selection probability* requirements, but business constraints are typically about *population-level impression counts*. The solver maintains running impression counts per arm with EMA decay (α = 0.01) on the LMAX thread. These running counts feed into the constraint satisfaction check: if arm `i`'s running impression share is already above its floor, the constraint is slack and does not distort **q**.

### Feasibility Handling

If the constraint polytope is empty (conflicting constraints make simultaneous satisfaction impossible), the solver returns `CONSTRAINT_INFEASIBLE` and falls back to the raw probabilities **p**. This is logged as a warning and surfaced to the experiment owner via M6. Common cause: floor constraints that sum to >1.0.

### IPW Validity

The *adjusted* probabilities **q** (not the raw **p**) are logged as `assignment_probability` in exposure events. This is critical: downstream IPW-adjusted analysis in M4a must use the actual selection probabilities to produce unbiased treatment effect estimates. If **p** were logged instead, the IPW estimator would be biased because the logged probability would not match the true selection mechanism.

---

## Consequences

### Benefits

1. **Hard constraint satisfaction**: Provider contracts, regulatory requirements, and fairness policies are enforced deterministically, not approximately via reward shaping.
2. **Minimal distortion**: KL-divergence minimization preserves the bandit's learned preferences as much as possible while satisfying constraints.
3. **Zero-lock integration**: Solver runs on the existing LMAX Thread 3 after probability computation, before selection. No new threads or synchronization.
4. **IPW-correct**: Adjusted probabilities logged for valid downstream causal inference.
5. **Warm-started convergence**: Interior-point solver warm starts from previous solution, amortizing cost over time.

### Trade-offs

1. **Regret increase**: Constraint satisfaction necessarily increases bandit regret relative to the unconstrained optimum. The KL-minimization ensures this increase is minimal.
2. **Infeasibility edge case**: Contradictory constraints produce fallback to raw probabilities. Experiment creators must validate constraint consistency.
3. **Interior-point complexity**: The general linear solver is more complex than the water-filling algorithm. The <50μs target may require careful tuning for experiments with many arms and constraints.
4. **EMA decay sensitivity**: The α = 0.01 decay rate determines how quickly population-level counts respond to allocation changes. Too fast → noisy constraint checking. Too slow → delayed constraint satisfaction.

---

## Implementation Details

### Proto Schema

```protobuf
// bandit.proto additions (PR #196)

message ArmConstraint {
  string arm_id = 1;
  optional double min_fraction = 2;   // floor: qᵢ ≥ min_fraction
  optional double max_fraction = 3;   // ceiling: qᵢ ≤ max_fraction
}

message GlobalConstraint {
  string label = 1;                              // human-readable name
  map<string, double> coefficients = 2;          // arm_id → coefficient aⱼᵢ
  double rhs = 3;                                // right-hand side bⱼ
}

// BanditConfig extensions
message BanditConfig {
  // ... existing fields 1-9 ...
  repeated ArmConstraint arm_constraints = 10;
  repeated GlobalConstraint global_constraints = 11;
}
```

### Planned Crate Layout

```
crates/experimentation-bandit/src/
  lp_constraint.rs    — LpConstraintSolver, water-filling, interior-point
  lib.rs              — pub mod lp_constraint;
```

### Planned Public API

```rust
pub struct LpConstraintSolver {
    arm_constraints: Vec<ArmConstraint>,
    global_constraints: Vec<GlobalConstraint>,
    running_counts: HashMap<String, f64>,  // EMA impression counts
    previous_q: Option<Vec<f64>>,          // warm start
}

pub enum ConstraintResult {
    Feasible(Vec<f64>),        // adjusted probabilities q
    Infeasible,                // constraint polytope empty
}

impl LpConstraintSolver {
    pub fn new(arm_constraints: Vec<ArmConstraint>, global_constraints: Vec<GlobalConstraint>) -> Self;
    pub fn solve(&mut self, raw_probabilities: &[f64], arm_ids: &[String]) -> ConstraintResult;
    pub fn update_counts(&mut self, selected_arm: &str);  // EMA update after selection
}
```

### LMAX Core Integration (Planned)

```rust
// In PolicyCore::handle_select_arm() on Thread 3:

let raw_p = self.policy.arm_probabilities(&context);

let final_p = if let Some(solver) = self.constraint_solvers.get_mut(&experiment_id) {
    match solver.solve(&raw_p, &arm_ids) {
        ConstraintResult::Feasible(q) => q,
        ConstraintResult::Infeasible => {
            warn!("Constraint infeasible for {}, using raw probabilities", experiment_id);
            raw_p
        }
    }
} else {
    raw_p  // no constraints configured
};

let selected = sample_from(&final_p, &mut rng);
solver.update_counts(&selected);

// Log final_p (not raw_p) as assignment_probability
exposure.assignment_probability = final_p[selected_index];
```

### Performance Targets

| Scenario | Target | Algorithm |
|----------|--------|-----------|
| Per-arm only, K ≤ 20 | <5μs | Water-filling O(K log K) |
| Per-arm only, K ≤ 100 | <15μs | Water-filling O(K log K) |
| General linear, K ≤ 100, ≤ 10 constraints | <50μs | Interior-point (warm-started) |

### State Growth

Running counts add ~40 bytes per arm per experiment. At 100 concurrent experiments with 10 arms each, total additional state is ~40KB.

---

## Validation (Planned)

### Proptest Invariants

1. **Simplex**: `Σ qᵢ = 1.0` (±1e-9) for all feasible outputs
2. **Non-negativity**: `qᵢ ≥ 0` for all i
3. **Floor satisfaction**: `qᵢ ≥ floor_i - ε` for all constrained arms when feasible
4. **Ceiling satisfaction**: `qᵢ ≤ ceil_i + ε` for all constrained arms when feasible
5. **KL minimality**: No single-coordinate perturbation of **q** reduces KL(q‖p) while maintaining feasibility (first-order optimality check)

### Golden-File Tests

- Water-filling on 5-arm example with 2 floor constraints: verify against analytically computed solution
- Interior-point on 10-arm example with 3 linear constraints: verify against Python `scipy.optimize.minimize` reference

### Integration Tests

- M4b ↔ M1 contract: `SelectArm` response includes LP-adjusted probabilities
- IPW roundtrip: adjusted probabilities logged in exposure events, consumed by M4a for unbiased estimation

---

## Dependencies

- **ADR-002** (LMAX core): Solver runs on Thread 3.
- **ADR-003** (RocksDB): Running counts and previous solution persisted in snapshots.
- **ADR-011** (Multi-objective reward): LP operates on probabilities produced after reward composition. The two layers are sequential: compose reward → update posterior → compute raw **p** → LP adjust to **q**.
- **ADR-014** (Provider metrics): Provider impression share metrics feed constraint definitions.
- **Enables ADR-013**: Meta-experiments can compare different constraint configurations.

---

## Current Status

- Proto schema landed in PR #196 (`ArmConstraint`, `GlobalConstraint`, `BanditConfig` fields 10–11).
- Implementation in progress (Sprint 5.2/5.3). Agent-4 status: LP constraint solver on LMAX core.
- M6 UI: `ConstraintStatusTable` component implemented (PR from Agent-6, work/fancy-koala) showing constraint name, current value, limit, SATISFIED/VIOLATED badge.
- Contract test defined: M4b ↔ M1 LP constraint adjusted probabilities.

---

## Rejected Alternatives

| Alternative | Reason Rejected |
|-------------|----------------|
| Reward shaping (add penalty for constraint violation to reward) | Soft — does not guarantee satisfaction. A large penalty distorts learning; a small penalty allows violations. |
| Constrained Thompson Sampling (reject samples violating constraints) | Rejection sampling is inefficient when constraints are tight; can require thousands of rejections per selection. |
| Action masking (remove arms violating constraints from selection set) | Too coarse — completely eliminates arms rather than reducing their probability. Cannot express "at least 5%" type constraints. |
| Simplex projection (Euclidean distance instead of KL) | KL-divergence better preserves the information structure of probability distributions. Euclidean projection can produce unintuitive results (e.g., zero-probability arms receiving mass). |

---

## References

- LinkedIn BanditLP (KDD 2025) — LP post-processing for constrained bandit allocation
- Chen et al.: Interpolating fairness constraints in bandit optimization (NeurIPS 2024)
- Boyd & Vandenberghe: Convex Optimization (2004) — KL-projection onto polyhedra, interior-point methods
