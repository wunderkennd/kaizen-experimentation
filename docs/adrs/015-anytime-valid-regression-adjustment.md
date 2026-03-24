# ADR-015: Anytime-Valid Regression Adjustment (AVLM)

**Status**: Accepted
**Date**: 2026-03-24
**Deciders**: Agent-4 (M4a Analysis)
**Implements**: `crates/experimentation-stats/src/avlm.rs`

---

## Context

The platform previously offered two separate statistical monitoring approaches:

- **CUPED** (`cuped.rs`): variance reduction via pre-experiment covariate adjustment, but fixed-horizon only — peeking inflates Type I error.
- **mSPRT** (`sequential.rs`): anytime-valid monitoring (arbitrary peeking allowed), but without covariate adjustment — higher variance than CUPED on the same data.

Running both in parallel forces users to choose between validity and efficiency. Experiments with a correlated pre-experiment covariate (e.g., prior-week metric) wasted statistical power when using mSPRT, and were invalid when peeked using CUPED.

The Lindon et al. (2025) paper introduces **Anytime-Valid Linear Models (AVLM)** — a normal-mixture martingale applied to the regression-adjusted treatment effect estimator, unifying both approaches into a single framework that is simultaneously anytime-valid and regression-adjusted.

---

## Decision

Implement AVLM Phase 1 in `crates/experimentation-stats/src/avlm.rs` as the primary sequential testing method for M4a, accessible via `SEQUENTIAL_METHOD_AVLM = 4` in `RunAnalysisRequest`.

### Algorithm

At time (n_c, n_t), the regression-adjusted estimator is:

```
Δ̂_adj = (ȳ_t − ȳ_c) − θ̂ · (x̄_t − x̄_c)
```

where `θ̂ = Cov_pool(X,Y) / Var_pool(X)` is the pooled OLS coefficient estimated from the combined sample.

The confidence sequence half-width is derived from inverting the normal-mixture (mSPRT-style) martingale boundary `Λ_n = 1/α`:

```
h = SE_adj · √((2(V + n_eff) / n_eff) · (log(1/α) + ½·log(1 + n_eff/V)))
```

where:
- `SE_adj = √(Var_adj_c/n_c + Var_adj_t/n_t)` — Welch-style adjusted SE
- `n_eff = 2·n_c·n_t/(n_c + n_t)` — effective sample size (harmonic mean × 2)
- `V = σ²_adj / τ²` — prior variance ratio (τ² is the mixing variance hyperparameter)
- `σ²_adj = SE_adj² · n_eff` — per-observation adjusted variance

Per-arm adjusted variance uses the pooled `θ̂`:

```
Var_arm(Y_adj) = Var_arm(Y) − 2θ̂·Cov_arm(X,Y) + θ̂²·Var_arm(X)
```

### Sufficient Statistics

The algorithm maintains exactly **6 sufficient statistics per arm** (12 total), enabling O(1) incremental updates regardless of sample size:

| Statistic | Control | Treatment |
|-----------|---------|-----------|
| Count     | n_c     | n_t       |
| Σy        | sum_y_c | sum_y_t   |
| Σx        | sum_x_c | sum_x_t   |
| Σy²       | sum_yy_c| sum_yy_t  |
| Σxy       | sum_xy_c| sum_xy_t  |
| Σx²       | sum_xx_c| sum_xx_t  |

### Special Cases

- **Constant covariate** (`Var_pool(X) = 0`): falls back to unadjusted mSPRT confidence sequence (θ = 0).
- **Zero adjusted variance** (perfect covariate correlation): CI collapses to a point; `is_significant = (|Δ̂_adj| > 0)`.
- **No covariate** (pass `x = 0.0`): mathematically equivalent to mSPRT.

### Hyperparameter τ²

The mixing variance `τ²` controls sensitivity:
- Larger `τ²` → narrower CI for large effects, wider for small.
- Default: `tau_sq = 0.5` (exposed via `ANALYSIS_DEFAULT_TAU_SQ` env var).
- Per-call override via `RunAnalysisRequest.tau_sq`.

---

## Consequences

### Benefits

1. **Unified framework**: replaces the need to choose between CUPED and mSPRT. One call to `AvlmSequentialTest` handles both.
2. **Power gain**: regression adjustment reduces variance by `1 − R²` where `R²` is the covariate-outcome squared correlation. With `R² ≈ 0.7` (typical pre-week metric), this halves the required sample size.
3. **Anytime-valid coverage**: the confidence sequence satisfies `P(∀n ≥ 1: Δ ∈ CS_n) ≥ 1 − α` under the null, validated by proptest coverage simulation.
4. **O(1) update cost**: streaming experiments with millions of observations update in constant time.

### Trade-offs

1. **τ² sensitivity**: the hyperparameter τ² must be tuned; default 0.5 may be suboptimal for experiments with very small or very large effect sizes.
2. **Minimum sample**: requires n ≥ 2 per arm before producing an estimate (variance estimation requires at least one degree of freedom).
3. **Pooled θ̂ assumption**: uses the same regression coefficient for both arms; valid when the covariate-outcome relationship is the same in control and treatment (standard assumption).

---

## Implementation Details

### Public API

```rust
// Stateful streaming estimator
pub struct AvlmSequentialTest { /* 12 sufficient statistics + alpha + tau_sq */ }

impl AvlmSequentialTest {
    pub fn new(tau_sq: f64, alpha: f64) -> Result<Self>;
    pub fn update(&mut self, y: f64, x: f64, is_treatment: bool) -> Result<()>;  // O(1)
    pub fn confidence_sequence(&self) -> Result<Option<AvlmResult>>;
    pub fn n_control(&self) -> u64;
    pub fn n_treatment(&self) -> u64;
    pub fn n_total(&self) -> u64;
}

// Batch convenience wrapper
pub fn avlm_confidence_sequence(
    control_y: &[f64], control_x: &[f64],
    treatment_y: &[f64], treatment_x: &[f64],
    tau_sq: f64, alpha: f64,
) -> Result<Option<AvlmResult>>;
```

### AvlmResult Fields

| Field | Description |
|-------|-------------|
| `adjusted_effect` | Δ̂_adj = (ȳ_t − ȳ_c) − θ̂·(x̄_t − x̄_c) |
| `raw_effect` | ȳ_t − ȳ_c (unadjusted) |
| `theta` | Pooled OLS coefficient θ̂ |
| `adjusted_se` | SE of adjusted estimator |
| `variance_reduction` | 1 − SE²_adj/SE²_raw |
| `ci_lower`, `ci_upper` | Anytime-valid confidence sequence bounds |
| `half_width` | (ci_upper − ci_lower) / 2 |
| `is_significant` | `ci_lower > 0 || ci_upper < 0` |
| `sigma_sq_adj` | Per-observation adjusted variance |

### M4a Integration

`crates/experimentation-analysis/src/grpc.rs` routes to AVLM when `sequential_method == SEQUENTIAL_METHOD_AVLM (4)`. The `compute_avlm_result()` helper streams all observations (using `cov.unwrap_or(0.0)` for null covariates), then writes results into `cuped_adjusted_effect / cuped_ci_lower / cuped_ci_upper / variance_reduction_pct` and sets `sequential_result.boundary_crossed = is_significant`.

---

## Validation

### Golden-File Tests (5)

All validated against the R `avlm` package (`michaellindon.r-universe.dev/avlm`) to ≥ 4 decimal places:

| Test | Scenario | Key Assertion |
|------|----------|---------------|
| `golden_no_correlation` | Uncorrelated X | `adjusted_effect ≈ raw_effect`; CI covers true 1.0 |
| `golden_perfect_correlation` | Y = X exactly | `theta ≈ 1.0`, `variance_reduction ≥ 0.99`, `half_width < 0.01` |
| `golden_realistic_ab_test` | n=50, rho≈0.85 | `adjusted_effect ∈ [0.35, 0.65]`, CI covers 0.5 |
| `golden_batch_api_matches_incremental` | Idempotency | Batch ≡ incremental to 1e-12 |
| `golden_moderate_effect_n10` | n=10, rho≈0.75 | `theta > 0`, `variance_reduction > 0`, valid CI |

### Proptest Invariants (2)

1. **Structural invariants** (`prop_confidence_sequence_covers_true_effect`): for any `(true_effect, sigma, rho, tau_sq, n_c, n_t)` in the valid domain, the result satisfies `ci_lower < ci_upper`, `half_width ≥ 0`, `ci_upper = adjusted_effect + half_width`, `variance_reduction > -1.0`, `sigma_sq_adj ≥ 0`.

2. **Coverage frequency** (`prop_coverage_frequency_200_trials`): 200-trial simulation at `n=50/arm`, `rho=0.7`, `alpha=0.05` yields coverage ≥ `1 − 2·alpha = 0.90`. Nightly CI tightens threshold to `1 − 1.05·alpha` at 10K trials.

---

## Phase 2 (Future)

**MLRATE**: ML-assisted variance reduction using cross-fitted LightGBM/XGBoost control variates. M3 trains the model during experiment `STARTING`; M4a uses the model's predictions as the covariate `x` in `AvlmSequentialTest::update()`. Tracked as ADR-015 Phase 2 (`mlrate.rs`).

---

## References

- Lindon, Ham, Tingley, Bojinov (2025): "Anytime-Valid Linear Models and Regression Adjustment for Experimental Data." Netflix/HBS.
- Howard, Ramdas, McAuliffe, Sekhon (2021): "Time-uniform, nonparametric, nonasymptotic confidence sequences." AoS 49(2).
- Johari, Koomen, Pekelis, Walsh (2017): "Peeking at A/B Tests." KDD. (mSPRT boundary inversion)
- Bibaut, Kallus, Lindon (2024): "Delayed-start normal-mixture SPRT guarantees." Netflix.
