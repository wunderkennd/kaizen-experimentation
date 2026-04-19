# ADR-020: Adaptive Sample Size Recalculation via Promising-Zone Designs

**Status**: Accepted and Implemented
- **Date**: 2026-03-19
- **Author**: Agent-4 (Analysis) / Agent-5 (Management)
- **Supersedes**: None (extends existing power analysis and GST framework)

## Context

Kaizen computes required sample size at experiment creation (during STARTING validation) based on the experimenter's specified minimum detectable effect (MDE), target power (typically 80%), and significance level. This sample size estimate uses variance from historical data or pilot experiments. But the pre-experiment variance estimate is often wrong:

1. **Overestimated variance** → the experiment reaches significance earlier than planned, wasting traffic on an already-decided question.
2. **Underestimated variance** → the experiment runs to planned duration but lacks power, producing an inconclusive result after weeks of traffic allocation.
3. **Misspecified MDE** → the experimenter guessed the effect size incorrectly. A smaller-than-expected effect needs more observations to detect.

Clinical trial methodology has addressed this with **adaptive sample size recalculation** — re-estimating required N during the experiment using accumulating data without compromising type I error control. Two approaches are directly applicable to Kaizen:

**Promising-zone designs** (Mehta and Pocock, 2011; widely adopted in clinical trials by 2024): At a pre-specified interim look, compute the conditional power — the probability of achieving significance at the final look given current data. Based on conditional power, the experiment falls into one of three zones:

- **Favorable zone** (conditional power > 90%): Continue as planned; likely to succeed.
- **Promising zone** (conditional power 30–90%): Increase sample size to boost conditional power to the target.
- **Futile zone** (conditional power < 30%): Consider early termination; effect may be too small to detect at any reasonable sample size.

**Blinded sample size re-estimation**: Uses pooled variance estimates (combining treatment and control without unblinding the treatment assignment) to re-estimate required N. This maintains experiment integrity because the experimenter does not learn the treatment effect direction at the interim look — only that the variance was different from what was assumed.

## Decision

Implement adaptive sample size recalculation as an optional feature layered on top of Kaizen's existing GST framework.

### Configuration

```protobuf
message AdaptiveSampleSizeConfig {
  // Enable adaptive recalculation. Default false.
  bool enabled = 1;

  // When to perform the interim recalculation (fraction of planned duration).
  // Default 0.5 (halfway through planned experiment duration).
  // Must be in (0.2, 0.8) — too early gives unreliable estimates,
  // too late leaves insufficient time to extend.
  double interim_fraction = 2;

  // Conditional power thresholds defining the zones.
  double promising_zone_lower = 3;  // Default 0.30 (below = futile)
  double promising_zone_upper = 4;  // Default 0.90 (above = favorable)

  // Maximum sample size multiplier. Prevents runaway experiments.
  // Default 2.0 (experiment can at most double its planned duration).
  double max_sample_multiplier = 5;

  // Whether to use blinded re-estimation (pooled variance only,
  // no treatment effect direction visible to experimenters).
  bool blinded = 6;  // Default true
}

// On Experiment message:
AdaptiveSampleSizeConfig adaptive_sample_size = 29;
```

### Recalculation Procedure

At the interim analysis point (e.g., 50% of planned duration):

1. **Compute observed pooled variance** (blinded: combines treatment and control):
   ```
   σ²_pooled = [(n_c - 1)s²_c + (n_t - 1)s²_t] / (n_c + n_t - 2)
   ```
   When `blinded = true`, M4a computes and reports only the pooled variance — M5 and M6 do not see treatment-specific means.

2. **Compute conditional power** at the originally planned sample size:
   ```
   CP = P(reject H0 at final look | current data, planned N)
   ```
   This uses the observed test statistic at interim, the planned total N, and the spending function from the GST configuration (if applicable).

3. **Classify into zone**:
   - **Favorable** (CP > 0.90): No action. Continue to planned conclusion.
   - **Promising** (0.30 ≤ CP ≤ 0.90): Compute adjusted N to achieve target power (default 80%):
     ```
     N_adjusted = N_planned × (σ²_observed / σ²_assumed) × power_correction
     ```
     Cap at `N_planned × max_sample_multiplier`.
   - **Futile** (CP < 0.30): Flag for early termination review. M5 sends notification to experiment owner with recommendation to conclude early.

4. **Update experiment configuration** (promising zone only):
   - M5 extends the planned experiment duration proportionally to the new N.
   - If the experiment uses GST, the spending function is recalculated with the new total N and remaining planned looks.
   - The traffic allocation is not changed — only the duration is extended.
   - Audit trail records the recalculation with reason, old N, new N, and conditional power.

### Type I Error Preservation

The promising-zone design preserves type I error control because:
- The recalculation depends only on the *pooled variance* (blinded), not on the treatment effect estimate.
- Under the null hypothesis (no treatment effect), the pooled variance is independent of the test statistic, so conditioning on it does not inflate the rejection probability.
- The maximum sample size cap ensures the experiment terminates in finite time.

When `blinded = false` (unblinded recalculation), type I error control requires the conditional error function approach of Müller and Schäfer (2001): the final test is adjusted to account for the information used in the recalculation. M4a implements this as a modified critical value at the final look.

### GST Interaction

For experiments with GST (O'Brien-Fleming or Pocock), the interim recalculation is aligned with a planned look:

- The recalculation occurs at the look closest to `interim_fraction × planned_looks`.
- The alpha spending up to the recalculation look is fixed.
- The remaining alpha is re-spent over the adjusted number of remaining looks (which may increase if N increases).
- Boundaries for remaining looks are recomputed using the remaining alpha budget and the new expected information at each future look.

### AVLM Interaction (ADR-015)

When AVLM is configured, the interim recalculation uses the **variance of the regression-adjusted estimator** rather than the raw outcome variance. This is crucial — CUPED/AVLM reduces the effective variance, so the sample size recalculation must account for the variance reduction. Without this adjustment, the recalculation would overestimate the needed N.

```
σ²_effective = σ²_pooled × (1 - ρ²)
```

where ρ is the observed correlation between the covariate and outcome.

### M5 Workflow

1. **STARTING**: M5 validates `AdaptiveSampleSizeConfig` parameters and computes the interim analysis date.
2. **RUNNING**: M5 schedules the interim analysis trigger at the configured fraction of planned duration.
3. **Interim trigger**: M5 requests M4a to compute conditional power. M4a returns the zone classification and (if promising) the adjusted N.
4. **Favorable zone**: M5 logs the result in audit trail. No action.
5. **Promising zone**: M5 updates the experiment's planned duration, adjusts GST boundaries if applicable, sends notification to owner ("Your experiment has been extended from 2 weeks to 3 weeks based on observed variance being higher than assumed. Conditional power at original duration was 45%; at extended duration it will be 80%."), and logs in audit trail.
6. **Futile zone**: M5 sends a notification recommending early termination. Does not auto-terminate — this requires the owner's CONCLUDE action. The notification includes the estimated effect size needed at any sample size for significance.

### M6 Rendering

- Experiment detail page shows the zone classification after interim analysis.
- A timeline indicator marks the interim analysis point and (if promising) the extended conclusion date.
- For blinded recalculation, the UI shows "Variance higher/lower than expected" without revealing treatment effect direction.
- Futile zone experiments display a prominent banner: "This experiment is unlikely to reach significance at any feasible sample size. Consider concluding early."

## Consequences

### Positive

- Experiments that would have been inconclusive (due to underestimated variance) automatically extend to reach adequate power, converting wasted traffic into actionable results.
- Experiments with overestimated variance can be identified early (favorable zone), informing the experimenter that the experiment is on track.
- Futile experiments are surfaced early rather than consuming traffic for weeks with no hope of a conclusive result.
- Blinded re-estimation preserves experiment integrity — the experimenter does not learn the treatment direction at interim, preventing biased interpretation.
- Integrates cleanly with GST spending functions — the alpha budget is preserved and re-allocated.

### Negative

- Extending experiment duration ties up traffic allocation for longer, potentially delaying other experiments in the same layer. ADR-019's traffic allocation optimizer should account for extensions.
- The max_sample_multiplier cap (default 2×) is a tradeoff: higher values give more power recovery but risk very long experiments. The 2× default is conservative.
- Blinded re-estimation is less informative than unblinded — the experimenter cannot see whether the treatment is trending positive or negative. This is a feature (prevents bias) but may frustrate impatient stakeholders.
- Adds a new state to the experiment lifecycle: "extended" is not a formal state but a configuration change during RUNNING. Audit trail captures it, but M6 must communicate clearly that the planned duration changed mid-experiment.

### Risks

- Experimenters may misinterpret the futile zone as "the treatment doesn't work" rather than "we can't detect whether it works." The UI messaging must be precise: futile means underpowered, not necessarily null effect.
- If multiple experiments in the same layer are in the promising zone simultaneously, extending all of them may exceed the layer's traffic capacity. M5 should check layer capacity before approving an extension and prioritize based on ADR-019's portfolio priorities.
- Unblinded recalculation (when blinded=false) introduces a subtle type I error inflation risk if the conditional error function is misimplemented. The Müller-Schäfer approach requires careful bookkeeping of the interim test statistic. Proptest invariant: type I error ≤ α on 10,000 simulated null experiments.

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| Fixed sample size only (status quo) | Simple | Wastes traffic on favorable experiments; produces inconclusive results on underpowered experiments | The core problem this ADR addresses |
| Always use mSPRT/AVLM (continuous monitoring) | Eliminates the fixed-N question entirely | Lower power than fixed-horizon or GST with planned looks; doesn't help when the issue is variance misestimation rather than peeking | Complementary but doesn't solve the variance mismatch problem |
| Pre-experiment variance estimation improvement | Addresses root cause | Even perfect historical variance estimation can't predict experiment-specific variance (treatment may change the variance itself) | Helps but doesn't eliminate the need for mid-experiment adjustment |
| Automatic early stopping for futile experiments | Saves traffic | Risk of stopping experiments that would have reached significance with a modest extension; requires careful calibration of futility boundary | Too aggressive; promising-zone design is more nuanced |

## References

- Mehta and Pocock: "Adaptive increase in sample size when interim results are promising" (Statistics in Medicine, 2011)
- Müller and Schäfer: "Adaptive group sequential designs for clinical trials" (Biometrika, 2001) — conditional error function
- Cui, Hung, Wang: "Modification of sample size in group sequential clinical trials" (Biometrics, 1999)
- Netflix "Return-Aware Experimentation" (TechBlog, 2024) — experiment duration optimization
- ADR-004 (GST — spending functions for planned looks)
- ADR-015 (AVLM — variance reduction affects recalculation)
- ADR-019 (Portfolio optimization — traffic allocation during extensions)
