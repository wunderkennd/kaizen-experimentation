# ADR-018: E-Value Framework and Online FDR Control

**Status**: Accepted and Implemented
- **Date**: 2026-03-19
- **Author**: Agent-4 (Analysis)
- **Supersedes**: Partially supersedes BH-FDR correction in `experimentation-stats` for concurrent experiment programs

## Context

Kaizen applies Benjamini-Hochberg (BH) FDR correction across metrics within an experiment and across lifecycle segments (CATE analysis). This is appropriate for within-experiment multiple testing. However, BH assumes a specific dependence structure (PRDS — positive regression dependence on a subset) that is routinely violated when running concurrent experiments on shared user populations.

Consider: Kaizen runs 50 concurrent experiments. User X is in experiments A, B, and C simultaneously (different layers). Experiment A's treatment improves watch time, which makes Experiment B's engagement metric look better for users who happen to be in both treatments. The p-values from A and B are positively dependent in ways that violate PRDS — they're dependent through shared users and shared outcome metrics, not through a simple positive regression structure.

The **e-value framework**, consolidated in a 390-page monograph by Ramdas and Wang (Foundations and Trends in Statistics, 2025), provides a strictly superior alternative for experimentation platforms:

1. **Safe multiplication**: E-values from independent or dependent tests can be safely multiplied. If experiment A produces e-value e_A = 3 and experiment B produces e_B = 4, the combined evidence is e_A × e_B = 12. This is impossible with p-values under dependence.

2. **Intrinsic anytime-validity**: E-values are valid at any stopping time by construction — no alpha-spending or boundary adjustments needed. They are the natural building block for sequential testing.

3. **Optional continuation**: If an experiment is inconclusive (e-value near 1), running a follow-up experiment and multiplying e-values is guaranteed to control type I error. With p-values, combining evidence from sequential experiments requires pre-registration and careful adjustment.

4. **Online FDR control under arbitrary dependence**: e-LOND (Xu and Ramdas, AISTATS 2024) provides FDR control across a *stream* of experiments without requiring independence or specific dependence structures between test statistics. This is the correct framework for an experimentation platform running dozens of concurrent tests.

Additional recent advances:
- **E-BH with conditional calibration** (Lee and Ren, 2024): Applies BH directly to e-values, controlling FDR under arbitrary dependence with improved power.
- **Online closed testing with e-values** (Fischer et al., 2024): Provides true discovery proportion bounds — stronger than FDR control — enabling post-hoc selection of rejection sets.
- **MAD/MADCovar** (Liang and Bojinov, HBS 2024; Molitor and Gold, 2025): Enables valid inference from bandit experiments by mixing the bandit algorithm with a Bernoulli design, producing e-processes that maintain anytime validity under adaptive allocation.

## Decision

Implement e-values as a parallel inference track alongside p-values in `experimentation-stats`, then adopt e-value-based online FDR control for cross-experiment multiple testing.

### Phase 1: E-Value Computation (Within-Experiment)

For each metric result, compute both a p-value (existing) and an e-value:

```rust
/// E-value for a two-sample comparison.
/// Uses the universal inference framework: e = likelihood ratio
/// under the alternative vs. the null.
pub struct EValueResult {
    /// The e-value. Values > 1 are evidence against the null.
    /// Values > 1/α reject at level α.
    pub e_value: f64,
    /// Log e-value (for numerical stability in products).
    pub log_e_value: f64,
    /// The implied significance level: 1/e_value.
    /// Comparable to a p-value but with different semantics.
    pub implied_level: f64,
}

/// Compute e-value for a mean comparison using the GROW martingale.
/// Compatible with sequential monitoring (anytime-valid by construction).
pub fn e_value_grow(
    control: &[f64],
    treatment: &[f64],
    mixing_variance: f64,  // ρ: controls power profile
) -> EValueResult;

/// Compute e-value for regression-adjusted comparison (pairs with ADR-015 AVLM).
pub fn e_value_avlm(
    control: &[f64],
    treatment: &[f64],
    covariate_control: &[f64],
    covariate_treatment: &[f64],
    mixing_variance: f64,
) -> EValueResult;
```

E-values are stored alongside p-values in `metric_results`:

```sql
ALTER TABLE metric_results ADD COLUMN e_value DOUBLE PRECISION;
ALTER TABLE metric_results ADD COLUMN log_e_value DOUBLE PRECISION;
```

### Phase 2: Online FDR Control (Cross-Experiment)

Implement e-LOND for controlling FDR across the stream of experiments the platform runs over time:

```rust
/// e-LOND: Online FDR control with e-values under arbitrary dependence.
/// Xu and Ramdas (AISTATS 2024).
///
/// Maintains a running state across all experiments. Each time an
/// experiment concludes, its e-value is submitted to the e-LOND
/// procedure, which decides whether to reject while maintaining
/// FDR ≤ α across all decisions ever made.
pub struct OnlineFdrController {
    /// Target FDR level (default 0.05).
    alpha: f64,
    /// Number of hypotheses tested so far.
    num_tested: u64,
    /// Number of rejections so far.
    num_rejected: u64,
    /// Alpha wealth: the remaining budget for future rejections.
    /// Starts at alpha and is replenished by rejections.
    alpha_wealth: f64,
    /// Wealth allocation strategy.
    strategy: FdrStrategy,
}

pub enum FdrStrategy {
    /// e-LOND: allocate α_i = α_wealth * γ_i where γ_i is a
    /// non-negative sequence summing to 1 (default: geometric decay).
    ELond { gamma_decay: f64 },
    /// E-BH: batch mode, apply BH to e-values directly.
    /// Used for within-experiment metric correction.
    EBh,
}

impl OnlineFdrController {
    /// Submit a new experiment's e-value and get a reject/don't-reject decision.
    pub fn test(&mut self, e_value: f64) -> FdrDecision;

    /// Current estimated FDR.
    pub fn current_fdr(&self) -> f64;
}
```

The `OnlineFdrController` is a platform-level singleton managed by M5. Each time an experiment transitions to CONCLUDED, M5 submits the primary metric's e-value to the controller. The controller's decision is stored in the experiment record and displayed in M6.

### Phase 3: E-Processes for Bandit Experiments (MAD)

For bandit experiments, standard e-values are invalid because the data is adaptively collected. Implement the Mixture Adaptive Design (MAD) from Liang and Bojinov (2024):

```rust
/// MAD: Mixture Adaptive Design for anytime-valid bandit inference.
/// Mixes the bandit algorithm with a Bernoulli randomization design.
///
/// At each step:
///   with probability (1 - ε): follow bandit policy
///   with probability ε: randomize uniformly
///
/// The ε-fraction of uniformly randomized observations forms a
/// valid basis for e-process computation.
pub struct MadEProcess {
    /// Mixing probability (fraction of uniform randomization).
    /// Default 0.1 (10% of observations are uniformly randomized).
    epsilon: f64,
    /// Running e-process value (product of per-observation e-values
    /// from the uniformly randomized subset).
    e_process: f64,
}
```

MAD requires modifying M4b's arm selection to mix in uniform randomization at rate ε. This is configured via:

```protobuf
message BanditConfig {
  // ... existing fields ...

  // NEW: Fraction of observations to uniformize for valid inference.
  // Default 0.0 (no MAD; use IPW-adjusted inference instead).
  // When > 0, M4b mixes uniform selection at this rate.
  double mad_randomization_fraction = 15;
}
```

When `mad_randomization_fraction > 0`, the SelectArm response includes a flag indicating whether this observation was from the bandit or the uniform component. M4a uses only the uniform-component observations for e-process computation, while all observations feed the bandit's learning.

### Within-Experiment vs. Cross-Experiment Correction

| Scope | Current Method | Proposed Method | Rationale |
|-------|---------------|-----------------|-----------|
| Across metrics within one experiment | BH-FDR (p-values) | E-BH (e-values) | Metrics within an experiment have known dependence structure; E-BH handles it without PRDS assumption |
| Across lifecycle segments (CATE) | BH-FDR (p-values) | E-BH (e-values) | Segments are dependent (overlapping user populations) |
| Across experiments over time | None (each experiment analyzed independently) | e-LOND (e-values) | **New capability**: controls FDR across the entire experimentation program under arbitrary dependence |
| Bandit experiments | IPW-adjusted CI | MAD e-process + IPW | MAD provides anytime-valid inference; IPW provides de-biased point estimates |
| Guardrail metrics | BH-FDR (alpha-side) | Bonferroni on beta-side (ADR-014) + e-values | Guardrails require power correction, not alpha correction |

### Backward Compatibility

P-values remain the primary reported statistic. E-values are computed alongside and stored in the database. The `OnlineFdrController` is opt-in at the platform level — teams that want cross-experiment FDR control enable it; others see no change. Within-experiment correction transitions from BH to E-BH, which is strictly more powerful under dependence and identical under independence.

## Consequences

### Positive

- Cross-experiment FDR control — a capability Kaizen currently lacks entirely — becomes possible under arbitrary dependence between experiments.
- E-values compose naturally: evidence from a pilot experiment and a follow-up experiment can be safely combined by multiplication, enabling incremental evidence accumulation.
- MAD e-processes provide the first valid sequential inference method for Kaizen's bandit experiments that doesn't require the strong assumptions of IPW (bounded propensity scores, correct logging).
- Anytime-validity is intrinsic to e-values — no separate alpha-spending or boundary-crossing logic needed. This simplifies the sequential testing implementation (though mSPRT/GST are retained for backward compatibility).

### Negative

- E-values are less intuitive than p-values. "The e-value is 15, meaning you can reject at any level ≥ 1/15 ≈ 0.067" is harder to explain to product managers than "p = 0.03."
- MAD's uniform randomization fraction (ε) reduces the bandit's effective learning rate. At ε = 0.10, 10% of observations are wasted (from the bandit's perspective) on uniform randomization. This is the statistical price of valid inference from adaptive data.
- The `OnlineFdrController` is stateful at the platform level — it must persist across deployments and cannot be recomputed from individual experiment results alone. M5 must store the controller state in PostgreSQL.
- Two parallel inference tracks (p-values and e-values) increase cognitive load and storage. Recommendation: report e-values prominently in M6 for teams using online FDR; report p-values as the primary statistic for teams using within-experiment correction only.

### Risks

- E-value power depends on the mixing distribution choice (parameter ρ). Misspecified ρ can result in low power. Mitigation: use adaptive mixing distributions that learn ρ from accumulating data (Ramdas and Wang, 2025, Chapter 8).
- The `OnlineFdrController` is a single point of coordination across all experiments. If it becomes corrupted or its state is lost, cross-experiment FDR guarantees are voided. Mitigation: snapshot the controller state in PostgreSQL after every experiment conclusion; provide a rebuild procedure from historical experiment e-values.
- MAD's uniform randomization conflicts with ADR-012's LP constraint layer — uniformly randomized observations may violate provider exposure constraints. Mitigation: apply LP constraints even to MAD's uniform component, accepting that the resulting observations are "constrained-uniform" rather than fully uniform. The e-process validity holds as long as the randomization probability is known and positive for all arms.

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| BH-FDR across experiments (status quo for within-experiment) | Familiar | Requires PRDS; invalid for concurrent experiments on shared users; no cross-experiment control | The core limitation this ADR addresses |
| Bonferroni across experiments | Valid under arbitrary dependence | Extremely conservative; with 50 concurrent experiments, per-experiment alpha = 0.001 | Too conservative for practical use |
| LORD/SAFFRON online FDR (p-value based) | Established online FDR methods | Require independence or PRDS between test statistics — violated in practice | Same dependence problem as BH |
| Ignore cross-experiment FDR | Simplest | Inflated false discovery rate scales with experiment volume; at 50 concurrent experiments with α=0.05, expected ~2.5 false discoveries | Unacceptable at scale |

## References

- Ramdas and Wang: "Hypothesis Testing with E-values" (Foundations and Trends in Statistics, 2025, 390pp monograph)
- Xu and Ramdas: "e-LOND: online FDR control under arbitrary dependence" (AISTATS 2024)
- Lee and Ren: "E-BH with conditional calibration" (2024)
- Fischer et al.: "Online closed testing with e-values" (2024)
- Liang and Bojinov: "Mixture Adaptive Design for bandit inference" (HBS, 2024)
- Molitor and Gold: "MADCovar: covariate-adjusted MAD" (2025)
- van den Akker, Werker, Zhou: "Valid Post-Contextual Bandit Inference" (May 2025)
- ADR-004 (GST/mSPRT — e-values as an alternative sequential framework)
- ADR-014 (Guardrail beta-correction — complementary to e-value-based guardrails)
- ADR-015 (AVLM — e-value variant of anytime-valid regression adjustment)
