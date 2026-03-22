# ADR-015: Anytime-Valid Regression Adjustment (Sequential CUPED)

- **Status**: Proposed
- **Date**: 2026-03-19
- **Author**: Agent-4 (Analysis)
- **Supersedes**: None (unifies ADR-004 sequential testing with existing CUPED implementation)

## Context

Kaizen forces a choice that no experimentation platform should require: use CUPED for variance reduction (fixed-horizon only) or use mSPRT for continuous monitoring (no variance reduction). These are the platform's two most valuable statistical capabilities, and they cannot be combined.

This is the single highest-ROI gap in the platform. CUPED reduces confidence interval width by 30–50% for metrics with strong pre-period correlation. mSPRT enables safe continuous monitoring with arbitrary peeking. An experiment that needs both — which is most experiments — must pick one and sacrifice the other.

Lindon, Ham, Tingley, and Bojinov (Netflix/HBS, 2025) solved this problem. Their anytime-valid linear model (AVLM) framework provides:

1. **Anytime-valid F-tests** and confidence sequences for linear models, including regression adjustment (CUPED) as a special case.
2. **Optimal** confidence sequences in the GROW/REGROW sense — they cannot be uniformly tightened.
3. **Closed-form expressions** using standard OLS estimators, making implementation straightforward.
4. **Production deployment** at Netflix for streaming quality metrics (sequential tests on play-delay, buffer rate) as of early 2024.

The companion work by Bibaut, Kallus, and Lindon (Netflix, 2024) provides the first type-I-error and expected-rejection-time guarantees for delayed-start normal-mixture SPRTs under general nonparametric data-generating processes. This bridges the gap between parametric sequential tests (which under-cover in practice) and concentration-bound sequences (which over-cover with suboptimal rejection times).

Additionally, the MLRATE framework (Meta, NeurIPS 2021, widely adopted 2024–2025) extends beyond linear CUPED to use ML models with cross-fitting as control variates, delivering ~19% lower variance than linear CUPED across 48 Facebook metrics. Etsy's CUPAC (December 2025) automates this with LightGBM on 100+ pre-experiment features, achieving 27% average variance reduction (4× more than vanilla CUPED's 7%). A separate Etsy team (KDD 2024) showed how to safely incorporate in-experiment covariates alongside pre-experiment data for additional gains.

## Decision

### Phase 1: Anytime-Valid Regression Adjustment (AVLM)

Implement the Lindon et al. (2025) AVLM framework in `experimentation-stats` as a new sequential testing method that subsumes both mSPRT and CUPED:

```protobuf
enum SequentialMethod {
  // ... existing values ...
  SEQUENTIAL_METHOD_AVLM = 4;  // NEW: anytime-valid linear model (sequential CUPED)
}
```

When `SEQUENTIAL_METHOD_AVLM` is selected and `cuped_covariate_metric_id` is configured on the primary metric, M4a constructs an anytime-valid confidence sequence for the regression-adjusted treatment effect:

```
Y_adj = Y - θ̂ * X    (standard CUPED adjustment)
CS_t = β̂_t ± f(t, α, V_t)   (confidence sequence width function)
```

Where `f(t, α, V_t)` is the GROW/REGROW mixing boundary that shrinks with accumulated information `V_t` while maintaining α-level anytime validity. The key insight from Lindon et al. is that the OLS estimator `β̂_t` and its variance `V_t` can be computed incrementally as data accumulates, and the confidence sequence boundary uses a normal-mixture martingale:

```rust
/// Anytime-valid confidence sequence for regression-adjusted ATE.
/// Implements Lindon et al. (2025) Algorithm 1.
pub struct AvlmSequentialTest {
    /// Running sufficient statistics (incrementally updated).
    sum_x: f64,      // Σ X_i (covariate)
    sum_y: f64,      // Σ Y_i (outcome)
    sum_xy: f64,     // Σ X_i * Y_i
    sum_xx: f64,     // Σ X_i^2
    sum_yy: f64,     // Σ Y_i^2
    n_control: u64,
    n_treatment: u64,
    /// Mixing distribution variance parameter (tuning knob).
    /// Larger = more power at large effects, less at small effects.
    /// Default: unit-information prior (1/n_planned).
    rho: f64,
    /// Overall significance level.
    alpha: f64,
}

impl AvlmSequentialTest {
    /// Update with a new observation. O(1) per observation.
    pub fn update(&mut self, y: f64, x: f64, is_treatment: bool);

    /// Compute current confidence sequence bounds. O(1).
    pub fn confidence_sequence(&self) -> (f64, f64, f64);  // (estimate, lower, upper)

    /// Whether the null (zero effect) is excluded from the CS.
    pub fn is_significant(&self) -> bool;
}
```

The implementation requires only running sufficient statistics (6 scalars) and produces confidence sequences in O(1) per update. This is dramatically simpler than bootstrap-based sequential methods and fits naturally into M4a's incremental analysis pipeline.

### Phase 2: ML-Assisted Variance Reduction (MLRATE)

Extend the CUPED covariate from a single pre-experiment metric to an ML-predicted control variate:

```protobuf
message VarianceReductionConfig {
  // Existing: single linear covariate.
  string cuped_covariate_metric_id = 1;

  // NEW: ML-predicted control variate.
  // When set, M3 trains a model predicting the outcome metric
  // from pre-experiment features, then uses the prediction as
  // the control variate instead of the raw covariate.
  bool enable_ml_covariate = 2;

  // Features for ML covariate model (pre-experiment user attributes).
  // If empty and enable_ml_covariate=true, uses all available
  // pre-experiment metric values as features.
  repeated string ml_covariate_feature_ids = 3;

  // Cross-fitting folds for MLRATE (default 5).
  // Ensures the ML model prediction is independent of the
  // observation it predicts, avoiding overfitting bias.
  int32 cross_fitting_folds = 4;
}
```

M3 computes the ML covariate during the STARTING phase:
1. Fit a LightGBM/XGBoost model predicting the primary metric from pre-experiment features using historical data.
2. Apply K-fold cross-fitting: for each fold, train on K-1 folds and predict on the held-out fold.
3. Store cross-fitted predictions as a new column in `metric_summaries`.
4. M4a uses the cross-fitted prediction as `X` in the AVLM framework (or standard CUPED for fixed-horizon).

Cross-fitting is essential — without it, the ML model's in-sample predictions would artificially inflate variance reduction estimates.

### Phase 3: In-Experiment Covariates

Following Etsy (KDD 2024), allow covariates measured *during* the experiment (device type, time-of-day, content category) to be incorporated alongside pre-experiment data:

```protobuf
message VarianceReductionConfig {
  // ... existing fields ...

  // NEW: In-experiment covariates (measured during the experiment
  // but causally prior to the outcome, e.g., device type).
  repeated string in_experiment_covariate_ids = 5;
}
```

Safety requirement: in-experiment covariates must be *causally prior* to the outcome (e.g., device type at session start, not session duration which is post-treatment). M5 cannot enforce this automatically, but should warn experimenters that including post-treatment covariates will bias results.

### Interaction with GST

AVLM also supports planned-look schedules. When `SEQUENTIAL_METHOD_AVLM` is combined with `planned_looks > 0`, M4a applies AVLM at each planned look rather than continuously, recovering GST-like power while maintaining the regression adjustment. This subsumes the current GST + no-CUPED configuration.

## Consequences

### Positive

- Eliminates the forced choice between variance reduction and sequential monitoring — the platform's two most valuable statistical features work together.
- AVLM confidence sequences are strictly narrower than mSPRT (which doesn't use covariates) and valid at any stopping time (unlike CUPED which requires fixed horizon). This is a Pareto improvement.
- MLRATE with cross-fitting captures nonlinear relationships in SVOD behavioral data (genre preferences, viewing patterns, session history) that linear CUPED misses, potentially doubling the variance reduction.
- Incremental computation (O(1) per observation for AVLM) fits M4a's batch and streaming analysis patterns.
- Netflix's R package `avlm` provides a reference implementation for validation.

### Negative

- The mixing distribution parameter `ρ` (rho) controls the power profile: larger ρ gives more power at large effects, less at small effects. Experimenters must choose this (or accept the unit-information default), adding a configuration decision.
- MLRATE requires a model training step during STARTING, adding 5–15 minutes to the STARTING → RUNNING transition. The model must be trained on historical data, which may not exist for new metrics.
- In-experiment covariates introduce a risk of bias if experimenters include post-treatment variables. The platform cannot prevent this automatically — only warn.
- Three variance reduction strategies (linear CUPED, AVLM, MLRATE) increase the method-selection burden. Recommendation: default to AVLM when sequential testing is configured; default to MLRATE when pre-experiment features are available and fixed-horizon is used.

### Risks

- AVLM assumes asymptotic normality of the OLS estimator. For highly skewed metrics (e.g., revenue with heavy-tailed distributions), the confidence sequence may under-cover at small sample sizes. Mitigation: winsorize at the 99.5th percentile (already standard practice) and validate coverage on Kaizen's own metric distributions via the existing proptest infrastructure.
- MLRATE model quality degrades if pre-experiment and in-experiment periods have distribution shift (e.g., holiday season data used to predict non-holiday outcomes). Mitigation: restrict training data to same-calendar-period historical windows.

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| Status quo (CUPED XOR mSPRT) | Simple, well-tested | Leaves the #1 gap unaddressed | The core problem this ADR solves |
| Bootstrap-based sequential CUPED | Nonparametric; no distributional assumptions | O(B × n) per update (B=10,000 resamples); computationally infeasible for real-time sequential monitoring | Too slow for continuous monitoring |
| Stratified sequential tests | Some variance reduction via stratification | Much weaker than regression adjustment; requires discrete strata | Strictly dominated by AVLM |
| Always use MLRATE (skip linear CUPED/AVLM) | Maximum variance reduction | Requires model training infrastructure; overkill for metrics with strong linear pre-period correlation; harder to validate | Linear AVLM should be the default; MLRATE is an upgrade for teams with rich feature sets |

## References

- Lindon, Ham, Tingley, Bojinov: "Anytime-Valid Linear Models and Regression Adjusted Causal Inference in Randomized Experiments" (arXiv 2210.08589, 2025; Netflix production deployment 2024)
- Bibaut, Kallus, Lindon: delayed-start normal-mixture SPRT guarantees (Netflix, 2024)
- Guo et al.: "Machine Learning for Variance Reduction in Online Experiments" (MLRATE, NeurIPS 2021; Meta production 2024–2025)
- Etsy CUPAC: LightGBM control variates (December 2025)
- Etsy KDD 2024: in-experiment covariates for variance reduction
- `avlm` R package (michaellindon.r-universe.dev/avlm)
- ADR-004 (GST alongside mSPRT)
