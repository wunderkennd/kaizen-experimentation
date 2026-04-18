# ADR-027: TOST Equivalence Testing

## Status

**Proposed**

## Context

Kaizen's analysis engine (M4a) supports hypothesis tests that detect *differences* — Welch's t-test, SRM chi-squared, mSPRT, GST, and CUPED-adjusted variants. All of these answer the question "is the treatment different from control?" Failure to reject the null hypothesis means "we didn't find evidence of a difference," which is not the same as "there is no difference."

For infrastructure migrations, backend refactors, CDN changes, and codec upgrades, the business question is the opposite: **"can we prove this change has no meaningful impact on user experience?"** A standard t-test cannot answer this. A non-significant p-value could mean the change is truly harmless, or it could mean the experiment was underpowered. Shipping a migration based on a non-significant t-test is scientifically unsound.

The Two One-Sided Tests (TOST) procedure, also known as equivalence testing, solves this by inverting the hypothesis structure. Instead of testing H₀: μ₁ = μ₂ against H₁: μ₁ ≠ μ₂, TOST tests:

- H₁ₐ: μ₁ - μ₂ > -δ (the effect is not too negative)
- H₁ᵦ: μ₁ - μ₂ < +δ (the effect is not too positive)

If both one-sided tests reject at α, the treatment is declared *equivalent* to control within the margin ±δ. This provides affirmative statistical evidence of "no meaningful impact."

This gap was identified in the SFD Product Experimentation Requirements document (Section 3, P1: Equivalence Testing) as a requirement for infrastructure and backend migration experiments.

## Decision

Implement TOST equivalence testing as a first-class analysis method in `experimentation-stats`, with full integration into M4a, M5, and M6.

### 1. Core Implementation (`crates/experimentation-stats/src/tost.rs`)

```rust
pub struct TostConfig {
    /// Equivalence margin (in the metric's natural units).
    /// The treatment is "equivalent" if the effect lies within [-delta, +delta].
    pub delta: f64,
    /// Significance level for each one-sided test (default: 0.05).
    pub alpha: f64,
}

pub struct TostResult {
    /// Point estimate of treatment - control difference.
    pub point_estimate: f64,
    /// Standard error of the difference.
    pub std_error: f64,
    /// Degrees of freedom (Welch-Satterthwaite).
    pub df: f64,
    /// p-value for the lower bound test (H₀: diff ≤ -δ).
    pub p_lower: f64,
    /// p-value for the upper bound test (H₀: diff ≥ +δ).
    pub p_upper: f64,
    /// max(p_lower, p_upper) — the TOST p-value.
    pub p_tost: f64,
    /// The (1 - 2α) confidence interval for the difference.
    /// Equivalence is declared if this CI falls entirely within [-δ, +δ].
    pub ci_lower: f64,
    pub ci_upper: f64,
    /// True if both one-sided tests reject (CI ⊂ [-δ, +δ]).
    pub equivalent: bool,
    /// The configured equivalence margin.
    pub delta: f64,
}

/// Run TOST equivalence test.
pub fn tost_equivalence_test(
    treatment: &MetricSummary,
    control: &MetricSummary,
    config: &TostConfig,
) -> Result<TostResult, StatsError> { ... }
```

The implementation uses Welch's t-test internals (unequal variance, Satterthwaite degrees of freedom) for each one-sided test, consistent with the existing `ttest.rs` implementation.

### 2. CUPED Integration

TOST should compose with CUPED variance reduction. When a `cuped_covariate_metric_id` is configured, the equivalence test operates on CUPED-adjusted means, reducing the required sample size for equivalence conclusions:

```rust
pub fn tost_cuped_equivalence_test(
    treatment: &MetricSummary,
    control: &MetricSummary,
    covariate_treatment: &MetricSummary,
    covariate_control: &MetricSummary,
    config: &TostConfig,
) -> Result<TostResult, StatsError> { ... }
```

### 3. Power Analysis for Equivalence

The required sample size for equivalence testing differs from superiority testing. Provide a power function:

```rust
pub struct TostPowerConfig {
    pub delta: f64,          // Equivalence margin
    pub true_difference: f64, // Expected true difference (often 0 for migrations)
    pub variance: f64,        // Estimated metric variance
    pub alpha: f64,           // Significance level (default 0.05)
    pub power: f64,           // Target power (default 0.80)
}

/// Calculate required sample size per group for TOST.
pub fn tost_sample_size(config: &TostPowerConfig) -> u64 { ... }
```

Note: TOST requires ~2× the sample size of a superiority test for the same effect size, because the equivalence margin δ is typically smaller than the minimum detectable effect in a standard test.

### 4. Proto Extensions

```protobuf
// In analysis/v1/analysis.proto

message EquivalenceTestConfig {
  // Equivalence margin in the metric's natural units.
  // Treatment is "equivalent" if effect ∈ [-delta, +delta].
  double delta = 1;

  // Optional: express delta as a percentage of control mean
  // (e.g., 0.02 = "within 2% of control").
  // If set, overrides absolute delta at analysis time.
  optional double delta_relative = 2;

  // Significance level (default 0.05).
  double alpha = 3;
}

// Add to AnalysisConfig
message AnalysisConfig {
  // ... existing fields ...

  // If set, run TOST equivalence test instead of (or in addition to) superiority test.
  optional EquivalenceTestConfig equivalence_test = N;
}
```

### 5. M5 Integration

When an experiment is configured with `equivalence_test`, M5 validates:
- `delta > 0` (equivalence margin must be positive)
- If `delta_relative` is set, the primary metric must be a MEAN or RATIO type (percentile deltas are not meaningful)
- TOST experiments should display a warning at creation: "Equivalence tests require ~2× the sample size of standard tests. Consider extending the experiment duration."

The experiment conclusion logic changes: instead of "reject H₀ → ship treatment," the decision is "equivalence established → safe to migrate."

### 6. M6 Integration

The results dashboard for equivalence experiments renders differently:
- A confidence interval plot showing the (1-2α) CI relative to the [-δ, +δ] equivalence margin (shaded region)
- Green badge: "Equivalent" when CI falls entirely within the margin
- Yellow badge: "Inconclusive" when CI overlaps the margin boundary
- Red badge: "Not Equivalent" when CI extends beyond the margin
- Power indicator: estimated power at the current sample size given δ

### 7. Validation

- Golden-file tests against R `TOSTER` package (Lakens, 2017) to 6 decimal places
- Proptest invariant: TOST p-value ≥ max of the two individual one-sided p-values
- Proptest invariant: if CI ⊂ [-δ, +δ] then `equivalent == true`
- Proptest invariant: TOST with δ → ∞ always declares equivalence (degenerate case)
- Coverage test: (1-2α) CI covers true parameter at rate ≥ (1-2α) on 10K simulations

## Consequences

### Positive

- Enables statistically rigorous infrastructure migration decisions — "safe to ship" backed by evidence, not absence of evidence.
- Composes with CUPED for faster equivalence conclusions on high-variance metrics.
- Power analysis prevents underpowered equivalence experiments from running.
- The relative delta option (`delta_relative: 0.02` = "within 2%") makes the margin intuitive for non-statisticians.

### Negative

- TOST requires ~2× the sample size of superiority tests, extending experiment duration for migrations.
- The equivalence margin δ requires a business decision ("how much change is acceptable?") that many teams find difficult to specify upfront.
- Adds a new concept to the UI that requires education (most PMs are not familiar with equivalence testing).

### Alternatives Considered

**Non-inferiority testing**: Tests only one direction (treatment is not worse than control by more than δ). Simpler but insufficient for migrations where both positive and negative effects are concerning (a migration that *improves* a metric might indicate a bug, not a feature).

**Bayesian region of practical equivalence (ROPE)**: Computes posterior probability that the effect lies within [-δ, +δ]. More intuitive interpretation but harder to calibrate and doesn't compose cleanly with the existing frequentist pipeline.

**Large sample size + non-significant p-value**: The current workaround. Scientifically unsound — high power reduces but does not eliminate the ambiguity.

## Dependencies

- **Existing**: `ttest.rs` (Welch's t-test internals), `cuped.rs` (variance reduction)
- **Phase 5**: ADR-015 (AVLM) — future extension: sequential equivalence testing via AVLM confidence sequences intersected with equivalence margin
- **No blocking dependencies**: TOST can be implemented independently of all other Phase 5 ADRs

## Implementation

| Component | Owner | Effort |
| --- | --- | --- |
| `tost.rs` + golden files | Agent-4 | ~2 days |
| CUPED-TOST composition | Agent-4 | ~1 day |
| Power analysis function | Agent-4 | ~0.5 days |
| Proto extensions | Agent-4 | ~0.5 days |
| M5 validation + conclusion logic | Agent-5 | ~1 day |
| M6 equivalence results view | Agent-6 | ~2 days |
| **Total** | | **~7 days** |

Recommended sprint: can be added to any sprint as a standalone Issue. No dependencies on other Phase 5 work. Suggested: Sprint 5.1 or 5.2.

## References

- Schuirmann, D.J. (1987). A comparison of the two one-sided tests procedure and the power approach for assessing the equivalence of average bioavailability. *Journal of Pharmacokinetics and Biopharmaceutics*, 15(6), 657-680.
- Lakens, D. (2017). Equivalence tests: A practical primer for t-tests, correlations, and meta-analyses. *Social Psychological and Personality Science*, 8(4), 355-362.
- Lakens, D., Scheel, A.M., & Isager, P.M. (2018). Equivalence testing for psychological research: A tutorial. *Advances in Methods and Practices in Psychological Science*, 1(2), 259-269.
- R `TOSTER` package: https://cran.r-project.org/package=TOSTER
- Walker, E. & Nowacki, A.S. (2011). Understanding equivalence and noninferiority testing. *Journal of General Internal Medicine*, 26(2), 192-196.
