# ADR-023: Synthetic Control Methods for Quasi-Experimental Evaluation

**Status**: Accepted and Implemented
- **Date**: 2026-03-19
- **Author**: Agent-4 (Analysis) / Agent-3 (Metrics)
- **Supersedes**: None (new analysis capability)

## Context

Kaizen is designed around randomized experiments — A/B tests, interleaving, bandits. But SVOD platforms regularly need to evaluate interventions that cannot be randomized:

1. **Market-level launches**: Rolling out a new content tier to one country before others. The question is "what would engagement in Germany have been if we hadn't launched the new tier?" — there is no within-country randomization.

2. **Content events**: A major title launch (e.g., a new season of a flagship series) affects all users on the platform. Netflix Games uses synthetic control for exactly this — evaluating game events and updates where user-level A/B testing is infeasible.

3. **Policy changes**: Changing recommendation algorithm defaults for an entire region, modifying autoplay behavior, or adjusting content moderation thresholds. These are often deployed all-at-once due to regulatory or operational constraints.

4. **Competitive disruptions**: When a competitor launches a competing service in a specific market, the platform needs to estimate the causal effect on its own metrics — no randomization possible.

5. **Post-hoc evaluation**: Sometimes an operational change was deployed without an experiment, and the platform needs to retroactively estimate its causal impact.

**Synthetic control methods (SCM)** address this by constructing a weighted combination of "donor" units (untreated regions, time periods, user segments) that closely matches the treated unit's pre-treatment trajectory, then using the synthetic control's post-treatment trajectory as the counterfactual.

Three advances make SCM practical for SVOD experimentation platforms:

- **Augmented Synthetic Control** (Ben-Michael, Feller, Rothstein): Solves the key limitation of standard SCM — imperfect pre-treatment fit — using Ridge regression de-biasing. Provides conformal confidence intervals (valid under weaker assumptions than traditional SCM). Implemented in the R package `augsynth`.
- **Synthetic Difference-in-Differences (SDiD)** (Arkhangelsky et al.): Combines SCM unit weights with DiD time-period weights, offering double robustness — consistent if either the SCM weights or the parallel trends assumption is correct.
- **CausalImpact** (Google, Brodersen et al.): Bayesian structural time series approach to causal inference from time-series data. Widely used in industry for marketing and product launch evaluation. Python implementation via `tfcausalimpact`.

Kaizen has no quasi-experimental analysis capability. Every evaluation requires a prospective randomized design. This forces teams into either (a) running experiments they shouldn't (randomizing pricing in a single market creates customer confusion) or (b) making decisions without any causal evidence (launching a content tier and hoping it works).

## Decision

Add synthetic control analysis as a new analysis mode in M4a, alongside the existing randomized experiment analysis methods. This is not a new experiment type — it is an analysis method applied to non-randomized interventions after the fact.

### Analysis Configuration

```protobuf
enum ExperimentType {
  // ... existing types ...
  EXPERIMENT_TYPE_QUASI = 11;  // NEW: quasi-experimental (non-randomized)
}

message QuasiExperimentConfig {
  // The unit that received the intervention.
  // e.g., "region:germany", "platform:ios", "content_tier:premium"
  string treated_unit_id = 1;

  // Donor pool: units available to construct the synthetic control.
  // e.g., ["region:france", "region:uk", "region:spain", ...]
  repeated string donor_unit_ids = 2;

  // Pre-treatment period for fitting the synthetic control.
  google.protobuf.Timestamp pre_treatment_start = 3;

  // Treatment onset.
  google.protobuf.Timestamp treatment_start = 4;

  // Post-treatment observation period end (optional; defaults to current).
  google.protobuf.Timestamp post_treatment_end = 5;

  // Time granularity for the analysis.
  TimeGranularity granularity = 6;

  // Analysis method.
  SyntheticControlMethod method = 7;

  // Covariates for matching (optional, used by augmented SCM).
  repeated string covariate_metric_ids = 8;
}

enum TimeGranularity {
  TIME_GRANULARITY_DAILY = 0;
  TIME_GRANULARITY_WEEKLY = 1;
  TIME_GRANULARITY_MONTHLY = 2;
}

enum SyntheticControlMethod {
  SYNTHETIC_CONTROL_CLASSIC = 0;       // Abadie, Diamond, Hainmueller (2010)
  SYNTHETIC_CONTROL_AUGMENTED = 1;     // Ben-Michael, Feller, Rothstein (default)
  SYNTHETIC_CONTROL_SDID = 2;          // Synthetic Difference-in-Differences
  SYNTHETIC_CONTROL_CAUSAL_IMPACT = 3; // Bayesian structural time series
}
```

### Data Requirements

Synthetic control requires panel data: the outcome metric observed for multiple units over multiple time periods. M3 computes this from existing data:

```sql
-- Panel data view for synthetic control analysis
CREATE VIEW quasi_experiment_panel AS
SELECT
    unit_id,          -- e.g., region, platform, segment
    time_period,      -- e.g., date, week
    metric_value,     -- outcome of interest
    covariate_1,      -- optional: pre-treatment covariates
    covariate_2
FROM metric_summaries
WHERE experiment_id = $1
GROUP BY unit_id, time_period;
```

The unit definition is flexible — it can be a geographic region, a device platform, a user segment, or any categorical attribute present in the exposure/metric data. M3 aggregates user-level metrics to the unit-time level.

### Analysis Implementation (M4a)

```rust
/// Synthetic Control Method implementation.
pub struct SyntheticControlAnalyzer {
    /// Panel data: (unit_id, time_period) -> metric_value
    panel: HashMap<(String, Date), f64>,
    /// Treated unit identifier
    treated_unit: String,
    /// Donor pool unit identifiers
    donors: Vec<String>,
    /// Treatment onset date
    treatment_start: Date,
    /// Covariates for augmentation (optional)
    covariates: Option<HashMap<(String, Date), Vec<f64>>>,
}

pub struct SyntheticControlResult {
    /// Estimated treatment effect (treated - synthetic control, post-treatment average).
    pub ate: f64,
    /// Confidence interval.
    pub ci_lower: f64,
    pub ci_upper: f64,
    /// P-value.
    /// - Classic/Augmented: placebo test (rank-based inference).
    /// - SDiD: jackknife over time periods.
    /// - CausalImpact: Bayesian posterior probability.
    pub p_value: f64,
    /// Pre-treatment fit quality (RMSPE: root mean squared prediction error).
    pub pre_treatment_rmspe: f64,
    /// Post/pre RMSPE ratio (key diagnostic; > 2 suggests real effect).
    pub rmspe_ratio: f64,
    /// Donor weights (which units comprise the synthetic control).
    pub donor_weights: HashMap<String, f64>,
    /// Time series: treated vs. synthetic control.
    pub treated_series: Vec<(Date, f64)>,
    pub synthetic_series: Vec<(Date, f64)>,
    /// Pointwise treatment effects over time.
    pub pointwise_effects: Vec<(Date, f64, f64, f64)>,  // (date, effect, ci_lower, ci_upper)
    /// Cumulative effect.
    pub cumulative_effect: f64,
    pub cumulative_ci_lower: f64,
    pub cumulative_ci_upper: f64,
    /// Placebo test results (one per donor unit).
    pub placebo_effects: Vec<PlaceboResult>,
}

pub struct PlaceboResult {
    pub unit_id: String,
    pub effect: f64,
    pub pre_rmspe: f64,
    pub post_pre_ratio: f64,
}

impl SyntheticControlAnalyzer {
    /// Classic SCM: minimize pre-treatment MSE via constrained optimization.
    /// Weights: w_i >= 0, Σ w_i = 1 (convex combination).
    pub fn classic(&self) -> SyntheticControlResult;

    /// Augmented SCM: Ridge-regularized bias correction on top of classic SCM.
    /// Handles imperfect pre-treatment fit.
    pub fn augmented(&self) -> SyntheticControlResult;

    /// Synthetic DiD: combines SCM unit weights with DiD time weights.
    /// Doubly robust: consistent if either weights or parallel trends is correct.
    pub fn sdid(&self) -> SyntheticControlResult;

    /// CausalImpact: Bayesian structural time series.
    /// Uses local linear trend + seasonal + regression on donor series.
    pub fn causal_impact(&self) -> SyntheticControlResult;

    /// Placebo tests: apply SCM to each donor unit (pretending it was treated).
    /// If treated unit's effect is extreme relative to placebos, it's likely real.
    pub fn placebo_tests(&self) -> Vec<PlaceboResult>;
}
```

### Inference Strategy

| Method | Inference Approach | Assumptions | Best For |
|--------|-------------------|-------------|----------|
| Classic SCM | Placebo-based permutation inference | Exact pre-treatment fit; no extrapolation | Few donors, good fit |
| Augmented SCM | Conformal inference (valid under mis-specification) | Approximate linearity of bias | Default recommendation |
| SDiD | Jackknife over time + placebo | Parallel trends OR correct SCM weights | When parallel trends is plausible |
| CausalImpact | Bayesian posterior | Structural time series model correct | Single treated unit, rich time series |

Default recommendation: Augmented SCM. It provides valid confidence intervals even when pre-treatment fit is imperfect (the key failure mode of classic SCM) and doesn't require the parallel trends assumption of DiD.

### Placebo Validation

The placebo test is the gold standard for SCM inference: apply the synthetic control procedure to each donor unit (pretending it was treated) and check whether the treated unit's effect is extreme. If the treated unit's post/pre RMSPE ratio ranks in the top 5% of placebo ratios, the effect is significant at the 5% level.

This is a distribution-free, exact test — no parametric assumptions needed. M4a runs placebo tests automatically for all SCM analyses and reports the rank-based p-value alongside parametric confidence intervals.

### M5 Management

`EXPERIMENT_TYPE_QUASI` has a simplified lifecycle:
- **DRAFT**: Configure `QuasiExperimentConfig` (treated unit, donors, time windows).
- **STARTING**: M3 validates that panel data exists for all specified units and time periods. No traffic allocation — this is observational.
- **RUNNING**: Placeholder state while post-treatment data accumulates. No assignment serving.
- **CONCLUDING**: M4a runs SCM analysis.
- **CONCLUDED**: Results available.

No bucket allocation, no guardrails, no bandit policies. The experiment is purely analytical.

### M6 Rendering

Dedicated quasi-experiment results page:

1. **Treated vs. Synthetic Control plot**: Two time series with a vertical treatment onset line. Shaded confidence band around the synthetic control series. This is the most important visualization — it shows whether the treated unit diverged from its expected trajectory.

2. **Pointwise effect plot**: Time series of (treated - synthetic) with confidence bands. Shows when the effect emerged and whether it persists or decays.

3. **Cumulative effect plot**: Running sum of pointwise effects — total accumulated impact since treatment.

4. **Donor weight table**: Which units contribute to the synthetic control, and with what weights. Useful for interpretation ("Germany's synthetic control is 40% France + 30% UK + 20% Spain + 10% Italy").

5. **Placebo test panel**: Small-multiple plots showing the treated unit's effect overlaid on all donor placebo effects. The treated unit should be visually extreme if the effect is real.

6. **Pre-treatment fit diagnostic**: RMSPE and visual assessment of how well the synthetic control matches the treated unit before treatment. Poor fit → unreliable results (warning displayed).

### Integration with Existing Capabilities

- **Surrogate metrics**: SCM can use surrogate metrics as covariates for matching. If a region's short-term engagement pattern matches another's, the surrogate framework provides additional features for the pre-treatment fit.
- **Lifecycle segments**: SCM can be applied per-lifecycle-segment by disaggregating the panel data by segment, estimating segment-level treatment effects.
- **Provider-side metrics (ADR-014)**: SCM on catalog-level metrics (coverage, provider exposure) enables evaluation of content strategy changes across markets.

## Consequences

### Positive

- Kaizen can now provide causal evidence for interventions that cannot be randomized — filling a major capability gap for market-level launches, pricing experiments, and post-hoc evaluations.
- Placebo-based inference provides distribution-free validity without parametric assumptions.
- Augmented SCM handles the practical problem of imperfect pre-treatment fit that makes classic SCM unreliable in many real-world applications.
- Teams that previously made decisions without any causal evidence ("we launched in Germany and engagement went up, so it must have worked") now have a principled counterfactual estimation method.
- The panel data infrastructure (unit × time aggregation in M3) is reusable for other analytical purposes — trend analysis, anomaly detection, cross-market benchmarking.

### Negative

- SCM requires sufficient donor units to construct a good synthetic control. For SVOD platforms operating in a small number of markets (< 10), the donor pool may be too small. Minimum recommendation: 5+ donors.
- Pre-treatment fit quality is the key validity diagnostic. If the synthetic control doesn't match the treated unit before treatment, post-treatment inferences are unreliable. M4a reports RMSPE, but interpreting fit quality requires domain judgment.
- SCM assumes no spillover from the treated unit to donors. If Germany's content tier launch affects French users (e.g., through word-of-mouth or content licensing changes), the synthetic control is contaminated. This is inherent to SCM, not a platform design issue.
- Quasi-experiments provide weaker causal evidence than randomized experiments. The "no unmeasured confounders" assumption is untestable. M6 should clearly label quasi-experimental results as observational evidence, not experimental.
- CausalImpact (Bayesian structural time series) requires a good time-series model. Misspecified seasonality or trend components produce misleading posterior intervals. Recommendation: default to Augmented SCM unless the experimenter has strong reasons for CausalImpact.

### Risks

- Experimenters may use quasi-experiments as a substitute for proper randomized experiments when randomization is merely inconvenient rather than impossible. M5 should enforce a justification field on quasi-experiments ("Why can't this be randomized?") and display a prominent warning that quasi-experimental evidence is weaker than experimental.
- SCM optimization (finding donor weights that minimize pre-treatment MSE) can overfit to noise in the pre-treatment period, producing a synthetic control that matches the treated unit's random fluctuations rather than its underlying trend. Augmented SCM's Ridge regularization mitigates this, but the risk remains with classic SCM.
- Placebo tests have low power when the number of donors is small. With 5 donors, the minimum achievable p-value from placebo inference is 1/6 ≈ 0.17. This means small effects will never reach significance. For small donor pools, supplement with CausalImpact's Bayesian posterior.

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| No quasi-experimental capability (status quo) | Simple; avoids weaker causal claims | Teams make decisions without causal evidence; or force inappropriate randomization | Worse than providing quasi-experimental evidence with appropriate caveats |
| Difference-in-differences only | Simple, well-understood | Requires parallel trends assumption (often violated for SVOD markets); no pre-treatment fit validation; no distribution-free inference | SCM is strictly more general; SDiD includes DiD as a special case |
| Interrupted time series only | Simple for single units | No donor-based counterfactual; confounded by temporal trends; no randomization-based inference | Weak identification; SCM provides a much stronger counterfactual |
| Bayesian structural time series only (CausalImpact) | Good for single-unit, time-series-rich settings | Requires correct time-series model; no placebo-based inference; no donor weights for interpretability | CausalImpact is included as one of four methods; it should not be the only option |

## References

- Abadie, Diamond, Hainmueller: "Synthetic Control Methods for Comparative Case Studies" (JASA, 2010)
- Ben-Michael, Feller, Rothstein: "The Augmented Synthetic Control Method" (JASA, 2021) — `augsynth` R package
- Arkhangelsky et al.: "Synthetic Difference-in-Differences" (AER, 2021)
- Brodersen et al.: "Inferring causal impact using Bayesian structural time series models" (Google, Annals of Applied Statistics, 2015) — `CausalImpact` R package
- Netflix Games: Synthetic control for game event evaluation (2024)
- Doudchenko and Imbens: "Balancing, Regression, Difference-In-Differences and Synthetic Control Methods" (2016) — theoretical unification
- ADR-014 (Provider-side metrics — SCM on catalog-level metrics)
- ADR-022 (Switchback designs — complementary quasi-experimental design for temporal treatments)
