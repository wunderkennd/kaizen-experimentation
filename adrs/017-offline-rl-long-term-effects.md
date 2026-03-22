# ADR-017: Offline Reinforcement Learning for Long-Term Causal Effect Estimation

- **Status**: Proposed
- **Date**: 2026-03-19
- **Author**: Agent-4 (Analysis) / Agent-3 (Metrics)
- **Supersedes**: None (corrects a theoretical limitation of the existing surrogate metric framework)

## Context

Kaizen's surrogate metric framework (M3 + MLflow) projects long-term outcomes (90-day churn) from short-term signals (7-day watch time, session frequency) using trained regression models with R²-based calibration. This approach has a **fundamental theoretical limitation** identified by Tran, Bibaut, and Kallus (Netflix, ICML 2024).

For "long-term treatments" — continual interventions like recommendation algorithm changes — the surrogacy assumption is violated by construction. The surrogacy assumption requires that the treatment's effect on the long-term outcome is *fully mediated* through the short-term surrogate. But a recommendation algorithm change has ongoing direct effects on user behavior throughout the entire observation period, not just during the first 7 days. The algorithm continues to shape what users see on day 30, day 60, day 90 — these direct effects bypass the 7-day surrogate entirely.

This is not an edge case. **Every recommendation experiment on an SVOD platform is a continual treatment.** The algorithm doesn't apply once and stop; it runs continuously for the duration of the user's subscription. The surrogate R² may look high in historical validation (because surrogates and outcomes are both influenced by stable user characteristics), but the *causal* surrogacy relationship is confounded.

Netflix's own work exposed additional problems with surrogate calibration:

1. **Bibaut, Chou, Ejdemyr, and Kallus (KDD 2024)**: User-level correlations between surrogates and outcomes are confounded by stable user characteristics. OLS regression of treatment effects across historical experiments suffers from correlated measurement error bias. They propose **Treatment-effect Correlation (TC)** and **Jackknife Instrumental Variables Estimation (JIVE)** — cross-fold procedures that eliminate this bias. Applied to 200+ Netflix A/B tests, this enabled data-driven proxy metric selection that agrees with full-duration decisions ~95% of the time.

2. **Kallus and Mao (JRSS-B 2025)**: Relaxed the strong surrogacy condition entirely, deriving efficiency bounds for what can be gained from surrogate information without assuming full mediation.

3. **Gao, Gilbert, and Han (December 2024)**: Demonstrated that surrogates good for average treatment effects may be poor surrogates for personalized treatment decisions — directly relevant since Kaizen uses CATE for lifecycle segmentation.

The solution from Tran et al. is an **offline reinforcement learning (ORL)** approach: model the user-platform interaction as a Markov Decision Process, estimate Q-functions and density ratios from logged experimental data, and infer long-term causal effects using doubly-robust estimators that account for the sequential nature of the treatment.

## Decision

### Phase 1: Fix Surrogate Calibration (TC/JIVE)

Replace the current R²-based surrogate calibration with the TC/JIVE procedure from Bibaut et al. (KDD 2024):

**Current (biased)**: Calibrate surrogate by regressing observed long-term treatment effects on surrogate-predicted effects across historical experiments. This OLS regression suffers from correlated measurement error.

**Proposed (de-biased)**: Use Jackknife Instrumental Variables Estimation:
1. Split historical experiments into K folds.
2. For each fold k, estimate the surrogate's predictive model on the other K-1 folds.
3. Predict the left-out fold's surrogate effects using the cross-fold model.
4. Use the cross-fold predictions as instruments for the actual surrogate effects in a 2SLS regression against long-term outcomes.

This eliminates the correlated measurement error bias because the instrument (cross-fold prediction) is independent of the measurement error in the left-out fold.

New fields on `SurrogateModelConfig`:

```protobuf
message SurrogateModelConfig {
  // ... existing fields ...

  // NEW: De-biased calibration metrics (replace raw R²).
  double treatment_effect_correlation = 10;  // TC: correlation of predicted vs. actual treatment effects
  double jive_coefficient = 11;              // JIVE slope: debiased predictive coefficient
  double jive_r_squared = 12;               // JIVE R²: debiased explanatory power
  int32 calibration_experiment_count = 13;   // Number of historical experiments used for calibration
}
```

M6's confidence badge updates:
- **Green**: JIVE R² > 0.6 AND calibration_experiment_count >= 20
- **Yellow**: JIVE R² 0.4–0.6 OR calibration_experiment_count 10–20
- **Red**: JIVE R² < 0.4 OR calibration_experiment_count < 10

### Phase 2: MDP-Based Long-Term Effect Estimation (ORL)

For experiments where surrogate projection is insufficient (high-stakes decisions, novel algorithm classes), provide an offline RL estimator that directly estimates long-term causal effects.

Model the user-platform interaction as an MDP:
- **State**: User's engagement history, content consumption pattern, subscription status.
- **Action**: The recommendation policy's output (which content to show).
- **Transition**: How the user's state evolves given the recommendation and their response.
- **Reward**: Long-term outcome of interest (retention at T days, LTV).

The ORL estimator uses doubly-robust (DR) estimation combining:
1. A **direct method** (Q-function): Fit a model predicting cumulative future reward from current state and action.
2. An **importance-weighted method** (density ratio): Estimate the ratio of the target policy's state-action visitation frequency to the logging policy's.
3. **DR combination**: The Q-function provides a control variate that reduces the variance of the importance-weighted estimator while maintaining unbiasedness.

```rust
/// Offline RL estimator for long-term treatment effects.
/// Implements Tran, Bibaut, Kallus (ICML 2024) doubly-robust MDP estimator.
pub struct OrlEstimator {
    /// Fitted Q-function (state, action) -> expected cumulative reward.
    q_function: QFunction,
    /// Estimated density ratio: π_target / π_logging.
    density_ratio: DensityRatioEstimator,
    /// Discount factor for future rewards (default 0.99).
    gamma: f64,
    /// Maximum horizon (days) for MDP rollout.
    max_horizon_days: u32,
}

pub struct OrlResult {
    /// Estimated long-term treatment effect (DR estimator).
    pub effect: f64,
    /// Standard error (sandwich variance).
    pub se: f64,
    /// Confidence interval.
    pub ci_lower: f64,
    pub ci_upper: f64,
    /// Effective sample size (accounting for importance weighting).
    pub effective_n: f64,
    /// Diagnostics.
    pub max_density_ratio: f64,  // high values indicate distribution mismatch
    pub q_function_r_squared: f64,
}
```

### Data Requirements

The ORL estimator requires sequential logged data that Kaizen already collects:
- Exposure events with timestamps (user state at decision time)
- Metric events with timestamps (user response to recommendation)
- Assignment probabilities (for density ratio estimation)

M3 restructures this data into MDP trajectories:

```sql
-- New Delta Lake table: user-level MDP trajectories
CREATE TABLE user_trajectories (
    experiment_id STRING,
    user_id STRING,
    trajectory_step INT,         -- time step within the experiment
    state_features ARRAY<DOUBLE>,  -- user state vector at this step
    action_id STRING,             -- recommendation shown
    reward DOUBLE,                -- immediate reward (engagement)
    next_state_features ARRAY<DOUBLE>,
    logging_probability DOUBLE,    -- P(action | state) under logging policy
    timestamp TIMESTAMP
)
USING DELTA
PARTITIONED BY (experiment_id);
```

### When to Use ORL vs. Surrogates

| Scenario | Recommended Method | Rationale |
|----------|-------------------|-----------|
| Standard A/B test, 2-week duration, engagement metric | TC/JIVE-calibrated surrogate | Fast, low-variance, sufficient for most decisions |
| Novel algorithm class, retention impact uncertain | ORL | Surrogate may not capture new mechanism of action |
| High-stakes decision (e.g., algorithm affects 100% of users) | ORL + surrogate (cross-validate) | Belt-and-suspenders for consequential decisions |
| Meta-experiment (ADR-013) on objective weights | ORL | Objective function changes affect the *mechanism*, invalidating surrogacy |
| Cumulative holdout analysis (long-running) | ORL | Holdout measures total lift over months — inherently an MDP problem |

M5 should recommend ORL for experiments flagged as high-stakes or involving novel algorithm types during STARTING validation.

### Interaction with CATE

The Gao et al. (2024) finding that surrogates good for ATE may be poor for CATE means Kaizen's lifecycle segmentation analysis (ADR-005, CATE + Cochran Q) should not rely on surrogate projections for segment-level decisions. When CATE is enabled and surrogate projections are used:
- M4a runs both ATE-level and CATE-level surrogate projections.
- M6 displays a warning when segment-level surrogate projections diverge significantly from the ATE-level projection (indicating the surrogate is not equally predictive across segments).
- For high-stakes segment-level decisions, recommend ORL with segment-level state features.

## Consequences

### Positive

- Corrects a fundamental theoretical limitation in Kaizen's surrogate framework for continual treatments — every SVOD recommendation experiment.
- TC/JIVE calibration (Phase 1) is a low-complexity fix that immediately improves surrogate reliability using existing infrastructure (historical experiment data + MLflow models).
- ORL (Phase 2) provides a principled estimator for long-term effects that accounts for the sequential nature of recommendation exposure, enabling confident decisions about algorithm changes that affect user behavior over months.
- The ORL estimator can also be used for cumulative holdout analysis, providing a causal estimate of total algorithmic lift that's more principled than the current trend-based approach.

### Negative

- ORL requires fitting Q-functions and density ratios — both of which are noisy in practice. The Q-function must be expressive enough to capture the state-action-reward relationship but not so complex that it overfits. Default: gradient-boosted trees (XGBoost) with early stopping.
- Density ratios can be extreme when the target and logging policies differ significantly, inflating variance. Mitigation: clip ratios at 10× (configurable) and report clipping frequency.
- MDP trajectory construction requires M3 to join exposure, metric, and assignment data along the time axis per user, producing a new Delta Lake table. This adds ~30 minutes to the daily metric computation job for experiments with ORL enabled.
- ORL results are harder to interpret than surrogate projections. "The estimated 90-day retention effect is +0.3% ± 0.2% (DR estimator, effective N = 12,000)" is less intuitive than "the surrogate projects +0.5% retention lift (R² = 0.72)." M6 must present ORL results with clear explanations.

### Risks

- Markov assumption violation: the MDP model assumes the current state is sufficient to predict future transitions. If important state variables are missing (e.g., user's real-world life events), the Q-function will be misspecified. Mitigation: include rich state features (watch history, session patterns, subscription tenure) and validate Q-function out-of-sample.
- Computational cost: Q-function fitting on large user trajectories (millions of users × dozens of time steps) requires significant compute. Recommend Spark-based fitting in M3, not in-memory in M4a.
- Adoption barrier: ORL is conceptually harder than surrogates. Teams may avoid it even when it's the correct method. Mitigation: automate the recommendation (M5 suggests ORL for eligible experiments) and provide clear "when to use which" guidance in M6.

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| Status quo (R²-calibrated surrogates) | Simple, fast, familiar | Theoretically invalid for continual treatments; biased calibration | The core problem this ADR corrects |
| Longer experiments (avoid surrogates entirely) | No surrogate model needed | 90-day experiments are operationally impractical; ties up traffic for months | Business reality requires short-term decisions |
| Difference-in-differences | Well-understood causal method | Requires parallel trends assumption; doesn't account for sequential treatment | Not suited for continual treatments with dynamic user responses |
| Synthetic control | Good for aggregate outcomes | Requires donor pool of similar untreated units; typically used for market-level interventions, not user-level | Wrong granularity for user-level experimentation |
| Bayesian structural time series | Handles sequential data | Not designed for counterfactual policy evaluation; estimates effect of an event, not a policy change | Doesn't leverage the logged action-reward structure |

## References

- Tran, Bibaut, Kallus: "Long-term causal effect estimation via offline RL" (Netflix, ICML 2024)
- Bibaut, Chou, Ejdemyr, Kallus: "Improve Your Next Experiment by Learning Better Proxy Metrics" (Netflix, KDD 2024)
- Kallus and Mao: "Relaxed surrogacy bounds" (JRSS-B, 2025)
- Gao, Gilbert, Han: "SCIENCE framework for individual treatment effect surrogacy" (December 2024)
- Netflix "Return-Aware Experimentation" (TechBlog, 2024)
- ADR-004 (Sequential testing — interaction with ORL timeline)
- ADR-013 (Meta-experiments — ORL recommended for objective function testing)
