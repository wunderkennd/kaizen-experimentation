# ADR-021: Feedback Loop Interference Detection and Mitigation

- **Status**: Proposed
- **Date**: 2026-03-19
- **Author**: Agent-3 (Metrics) / Agent-4 (Analysis)
- **Supersedes**: None (extends existing interference detection: JSD, Jaccard, Gini)

## Context

Kaizen detects content interference via Jensen-Shannon divergence, Jaccard similarity, and Gini coefficient on consumption distributions. These metrics capture *observational* interference — whether treatment and control groups are watching different content. But they miss a more insidious form of interference: **feedback loop contamination**.

Two 2024–2025 papers identify this mechanism:

1. **"Tackling Interference Induced by Data Training Loops in A/B Tests"** (arXiv 2310.17496v4, updated 2024): When a recommendation model is retrained during an A/B test, the training data includes behavioral data from *both* treatment and control groups. This contaminated training data alters the model for both groups, diluting the measured treatment effect. The treatment effect estimate is biased toward zero because the control group's model has been partially trained on treatment-influenced data.

2. **"Symbiosis bias in recommendation experiments"** (Brennan et al., WWW 2025): Formalizes the bias from recommendation systems adapting to experimental context. The recommendation model learns to optimize for the *mixture* of treatment and control user behaviors rather than either one purely. This "symbiosis" between the model and the experimental design produces a biased estimate of what would happen if the treatment were deployed to 100% of users.

For an SVOD platform, this is not a theoretical concern. Recommendation models are retrained daily or weekly on logged user interactions. An A/B test running for 4 weeks goes through 4+ model retraining cycles, each one progressively contaminating the control group's experience with treatment-influenced training data.

Kaizen's existing interference detection cannot capture this because:
- JSD/Jaccard/Gini measure consumption distribution *differences*, not the causal mechanism of contamination.
- Feedback loop interference makes treatment and control look *more similar* over time (because the shared model converges to a compromise), which would actually *decrease* JSD — the opposite of what the interference detector looks for.
- The effect is temporal: it grows with each model retraining cycle, but Kaizen's interference analysis is a single snapshot, not a time series.

## Decision

Add feedback loop interference detection as a new analysis capability in M4a, with mitigation recommendations surfaced through M5 and M6.

### Detection: Model Retraining Event Tracking

The first requirement is knowing *when* the recommendation model was retrained. Add a new event type to M2's ingestion pipeline:

```protobuf
message ModelRetrainingEvent {
  string event_id = 1;
  // Identifier for the recommendation model that was retrained.
  string model_id = 2;
  // Training data window: start and end timestamps.
  google.protobuf.Timestamp training_data_start = 3;
  google.protobuf.Timestamp training_data_end = 4;
  // Whether the training data included both experiment groups.
  bool includes_experiment_data = 5;
  // Experiments that were RUNNING during the training data window.
  repeated string active_experiment_ids = 6;
  google.protobuf.Timestamp retrained_at = 7;
}
```

This event is published to a new Kafka topic `model_retraining_events` by the recommendation model training pipeline (external to Kaizen). M3 consumes these events and correlates them with active experiments.

### Detection: Treatment Effect Drift Analysis

M4a computes daily treatment effects (already available via `daily_treatment_effects` Delta Lake table) and tests for systematic drift that correlates with model retraining events:

```rust
/// Feedback loop interference detector.
/// Tests whether treatment effects systematically decay after
/// model retraining events that used contaminated training data.
pub struct FeedbackLoopDetector {
    /// Daily treatment effect time series.
    daily_effects: Vec<(Date, f64)>,
    /// Model retraining timestamps.
    retraining_events: Vec<DateTime>,
}

pub struct FeedbackLoopResult {
    /// Whether feedback loop interference is detected.
    pub interference_detected: bool,
    /// Average treatment effect in periods between retraining events.
    pub pre_retrain_effect: f64,
    /// Average treatment effect in periods after retraining events.
    pub post_retrain_effect: f64,
    /// Percentage dilution: (pre - post) / pre.
    pub dilution_percentage: f64,
    /// p-value for the difference (paired test across retraining events).
    pub dilution_p_value: f64,
    /// Number of retraining events observed during the experiment.
    pub retraining_count: u32,
    /// Estimated bias in the final treatment effect estimate
    /// due to feedback loop contamination.
    pub estimated_bias: f64,
}

impl FeedbackLoopDetector {
    /// Test for systematic post-retraining effect dilution.
    ///
    /// Method: For each retraining event, compare the treatment effect
    /// in the 3-day window before vs. after retraining. Test whether
    /// the post-retraining effect is systematically lower using a
    /// paired t-test across retraining events.
    pub fn detect(&self) -> FeedbackLoopResult;

    /// Estimate the bias-corrected treatment effect by extrapolating
    /// the pre-retraining trend.
    pub fn bias_corrected_effect(&self) -> (f64, f64, f64);  // (estimate, ci_lower, ci_upper)
}
```

Detection criteria:
- `interference_detected = true` when `dilution_p_value < 0.05` AND `dilution_percentage > 10%` AND `retraining_count >= 2` (need multiple retraining events to establish the pattern).

### Detection: Training Data Overlap Quantification

M3 computes the degree of training data contamination for each retraining event:

```sql
-- Fraction of training data that came from experiment users
-- (both treatment and control contribute contamination)
SELECT
    r.model_id,
    r.retrained_at,
    COUNT(DISTINCT CASE WHEN e.experiment_id IS NOT NULL THEN e.user_id END) AS experiment_users_in_training,
    COUNT(DISTINCT e_train.user_id) AS total_training_users,
    experiment_users_in_training / total_training_users AS contamination_fraction
FROM model_retraining_events r
JOIN training_data e_train ON e_train.timestamp BETWEEN r.training_data_start AND r.training_data_end
LEFT JOIN exposures e ON e.user_id = e_train.user_id AND e.experiment_id IN (r.active_experiment_ids)
GROUP BY r.model_id, r.retrained_at;
```

High contamination fraction (> 20% of training data from experiment users) is a risk indicator.

### Mitigation Recommendations

When feedback loop interference is detected, M5 surfaces mitigation options through M6:

| Mitigation | Complexity | Effectiveness | Description |
|------------|-----------|---------------|-------------|
| **Data diversion** | Medium | High | Exclude experiment users' data from model retraining. The model is trained only on non-experiment user data. Requires coordination with the ML training pipeline. |
| **Holdout retraining** | Low | Medium | Retrain the control group's model variant on control-only data, treatment variant on treatment-only data. Prevents cross-contamination but requires separate model variants. |
| **Retraining freeze** | Low | High | Freeze the recommendation model during the experiment. Simple but prevents legitimate model improvements. |
| **Bias correction** | Low | Medium | Use `FeedbackLoopDetector.bias_corrected_effect()` to estimate the uncontaminated treatment effect. Statistical correction, no operational change. |
| **Weighted training** (arXiv 2310.17496v4) | High | High | Weight training examples by inverse probability of being in the experiment. Formally de-biases the model retraining process. |

M6 renders these as a decision matrix on the interference analysis tab when feedback loop interference is detected.

### Proto Extension

```protobuf
// New fields on InterferenceAnalysisResult:
message InterferenceAnalysisResult {
  // ... existing fields (jsd, jaccard, gini, spillover titles) ...

  // NEW: Feedback loop interference.
  bool feedback_loop_detected = 11;
  double pre_retrain_effect = 12;
  double post_retrain_effect = 13;
  double dilution_percentage = 14;
  double dilution_p_value = 15;
  int32 retraining_count = 16;
  double estimated_bias = 17;
  double bias_corrected_effect = 18;
  double bias_corrected_ci_lower = 19;
  double bias_corrected_ci_upper = 20;
  // Training data contamination fraction per retraining event.
  repeated RetrainingContamination retraining_contaminations = 21;
}

message RetrainingContamination {
  string model_id = 1;
  google.protobuf.Timestamp retrained_at = 2;
  double contamination_fraction = 3;
}
```

### M6 Rendering

The interference analysis tab gains a new panel: "Feedback Loop Analysis" (shown only when `model_retraining_events` data is available):

- **Retraining timeline**: Vertical lines on the daily treatment effect time series marking each model retraining event.
- **Pre/post retraining comparison**: Box plots of treatment effects in pre- vs. post-retraining windows.
- **Contamination indicator**: Bar chart showing training data contamination fraction per retraining event.
- **Bias-corrected estimate**: Side-by-side display of raw treatment effect and bias-corrected estimate.
- **Mitigation recommendations**: Decision matrix (table above) with links to documentation.

### Guardrail Integration

A new guardrail metric `feedback_loop_dilution` can be configured to auto-pause experiments when contamination exceeds a threshold:

```json
{
  "metric_id": "feedback_loop_dilution",
  "threshold": 0.20,
  "consecutive_breaches_required": 1
}
```

This triggers when `dilution_percentage > 20%` — meaning the treatment effect has been diluted by more than 20% due to model retraining contamination.

## Consequences

### Positive

- Detects a form of interference that Kaizen's existing metrics (JSD, Jaccard, Gini) fundamentally cannot capture — and that makes treatment effects look *smaller* rather than different.
- Bias-corrected estimates let experimenters recover the "true" treatment effect even when contamination has occurred, without re-running the experiment.
- Mitigation recommendations give actionable guidance rather than just flagging the problem.
- The `model_retraining_events` ingestion path provides infrastructure for broader model-experiment coordination in the future.

### Negative

- Requires the recommendation model training pipeline (external to Kaizen) to publish `ModelRetrainingEvent` events. This is a cross-system integration dependency that Kaizen cannot enforce unilaterally.
- Detection requires 2+ model retraining events during the experiment — experiments shorter than 2 retraining cycles (often 1–2 weeks) cannot be analyzed. This is inherent to the detection method.
- Bias correction is an estimate, not a guarantee. The extrapolation from pre-retraining trends to counterfactual uncontaminated effects assumes the treatment effect would have been stable without retraining — an untestable assumption.
- Data diversion and weighted training mitigations require engineering effort in the ML training pipeline, outside Kaizen's direct control.

### Risks

- False positives: natural effect decay (novelty wearing off) can be misattributed to feedback loop interference if retraining events happen to coincide with the novelty decay period. Mitigation: the novelty detector (existing) should be run first; if novelty is detected, the feedback loop detector adjusts for the expected decay trend before testing for retraining-correlated drops.
- Model retraining events may not be available in all deployments. When `model_retraining_events` topic has no data, the feedback loop analysis panel is hidden and the detector is skipped. Graceful degradation.
- The "retraining freeze" mitigation can be counterproductive if the experiment is testing an algorithm that improves with retraining. Experimenters must understand that freezing prevents both contamination and legitimate learning.

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| Ignore feedback loop interference (status quo) | No additional complexity | Treatment effect estimates systematically biased toward zero; shipping decisions based on diluted effects | Undetected bias is the worst kind |
| Always freeze model retraining during experiments | Eliminates contamination entirely | Prevents legitimate model improvements; operationally burdensome at scale with many concurrent experiments | Too restrictive for a platform running 50+ concurrent experiments |
| Run experiments only between retraining cycles | Avoids contamination | Severely constrains experiment scheduling; retraining cycles are often weekly, limiting experiments to < 7 days | Impractical for experiments needing 2-4 weeks |
| Use only observational interference metrics (JSD, Jaccard, Gini) | Already implemented | Cannot detect feedback loop interference — it manifests as effect dilution, not distribution divergence | Structurally unable to detect this mechanism |

## References

- arXiv 2310.17496v4: "Tackling Interference Induced by Data Training Loops in A/B Tests" (2024) — weighted training approach
- Brennan et al.: "Symbiosis bias in recommendation experiments" (WWW 2025) — data-diverted designs
- Farias et al.: "Creator-side interference in recommendation platforms" (2023) — provider-side feedback effects
- Netflix "Heterogeneous Treatment Effects" (TechBlog, November 2025) — interaction between model retraining and CATE
- ADR-013 (Meta-experiments — particularly susceptible to feedback loop interference due to parallel bandit policies)
- ADR-014 (Provider-side metrics — feedback loops affect provider exposure distributions)
