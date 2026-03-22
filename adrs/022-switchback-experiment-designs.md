# ADR-022: Switchback Experiment Designs for Interference-Prone Treatments

- **Status**: Proposed
- **Date**: 2026-03-19
- **Author**: Agent-4 (Analysis) / Agent-1 (Assignment)
- **Supersedes**: None (new experiment type)

## Context

Kaizen's randomization unit is always the user (or session). This works when treatments are independent across users — my recommendation algorithm doesn't affect your experience. But SVOD recommendation has a class of treatments where this independence assumption is systematically violated:

1. **Shared content inventory**: Two users assigned to different recommendation algorithms draw from the same finite catalog. If algorithm A aggressively promotes title X to treatment users, the "trending" and "popular" signals for title X change for control users too. This is exactly the content interference Kaizen detects (JSD, Jaccard, Gini), but detection is not the same as a design that accounts for it.

2. **Content licensing and availability**: Catalog-level decisions (adding a content tier, changing licensing terms, modifying content rotation schedules) cannot be randomized at the user level — all users see the same catalog.

3. **Pricing and plan changes**: Subscription plan experiments (pricing tiers, ad-supported options, bundling) affect all users in a region simultaneously. User-level randomization either can't show different prices to different users (legal/UX constraints) or creates marketplace distortions.

4. **CDN and infrastructure changes**: Streaming quality experiments (CDN routing, encoding profiles, ABR algorithm changes) often operate at the server/region level, not the user level. All users hitting the same edge node are affected.

**Switchback designs** address this by alternating the entire platform (or a cluster) between treatment and control over time, using the temporal variation as the source of randomization rather than user-level assignment. The foundational framework from Bojinov, Simchi-Levi, and Zhao (Management Science, 2023) provides valid inference under temporal carryover effects.

Recent advances make switchback designs practical for SVOD:

- **Data-driven block length selection** (Xiong, Chin, Taylor, 2024): Uses empirical Bayes to choose optimal switchback interval lengths, balancing carryover bias (shorter intervals → more carryover from previous treatment period) against statistical efficiency (more switches → more independent comparisons).
- **Clustered switchback designs** (Jia et al., updated March 2025): Handles spatio-temporal interference by clustering users into geographic or behavioral groups and switching groups independently.
- **Regular Balanced Switchback Designs** (Amazon Science, Masoero et al., 2025): Achieves 40–60% standard error reduction over simple item randomization for item-level experiments.
- **Distribution-free randomization tests** (arXiv, February 2026): Provides valid inference without parametric assumptions on the data-generating process.
- **DoorDash implementation blueprints**: Published detailed sandwich variance estimators for switchback analysis.

Kaizen currently has no mechanism to define, run, or analyze switchback experiments.

## Decision

Add `EXPERIMENT_TYPE_SWITCHBACK` as a new experiment type with dedicated assignment logic in M1, analysis methods in M4a, and management support in M5/M6.

### Experiment Configuration

```protobuf
enum ExperimentType {
  // ... existing types ...
  EXPERIMENT_TYPE_SWITCHBACK = 10;  // NEW: temporal alternation design
}

message SwitchbackConfig {
  // Duration of each treatment block.
  // "block" = contiguous period where all units receive the same treatment.
  google.protobuf.Duration block_duration = 1;

  // Randomization unit for clustered switchback.
  // GLOBAL = entire platform switches together.
  // CLUSTER = each cluster switches independently.
  SwitchbackUnit unit = 2;

  // Cluster definition (when unit = CLUSTER).
  // Attribute name in user context (e.g., "region", "device_type").
  string cluster_attribute = 3;

  // Number of complete cycles (treatment → control → treatment → ...).
  // Minimum 4 for valid inference. More cycles = more power.
  int32 planned_cycles = 4;

  // Whether to use data-driven block length adaptation.
  // When true, M5 adjusts block_duration after the first 2 cycles
  // based on observed autocorrelation in the outcome metric.
  bool adaptive_block_length = 5;

  // Washout period between treatment switches.
  // Users in washout are excluded from analysis to mitigate carryover.
  google.protobuf.Duration washout_duration = 6;

  // Design type.
  SwitchbackDesign design = 7;
}

enum SwitchbackUnit {
  SWITCHBACK_UNIT_GLOBAL = 0;
  SWITCHBACK_UNIT_CLUSTER = 1;
}

enum SwitchbackDesign {
  SWITCHBACK_DESIGN_SIMPLE = 0;       // Alternating A-B-A-B
  SWITCHBACK_DESIGN_BALANCED = 1;     // Regular balanced (Masoero et al.)
  SWITCHBACK_DESIGN_RANDOMIZED = 2;   // Random assignment per block
}
```

### Assignment Logic (M1)

For switchback experiments, M1's assignment is time-based rather than hash-based:

```rust
fn get_switchback_assignment(
    experiment: &Experiment,
    config: &SwitchbackConfig,
    now: DateTime<Utc>,
    user_context: &UserContext,
) -> Option<VariantId> {
    let experiment_start = experiment.start_time;
    let elapsed = now - experiment_start;
    let block_duration = config.block_duration;
    let washout_duration = config.washout_duration;
    let cycle_duration = block_duration * 2 + washout_duration * 2;

    // Determine current position in cycle
    let position_in_cycle = elapsed % cycle_duration;

    // Check if in washout period
    if is_in_washout(position_in_cycle, block_duration, washout_duration) {
        return None;  // Exclude from experiment during washout
    }

    // Determine cluster (if clustered design)
    let cluster_id = match config.unit {
        SwitchbackUnit::Global => "global",
        SwitchbackUnit::Cluster => user_context.get(&config.cluster_attribute)?,
    };

    // Determine block assignment
    let block_index = (elapsed.as_secs() / block_duration.as_secs()) as u64;

    match config.design {
        SwitchbackDesign::Simple => {
            // Alternating: even blocks = treatment, odd = control
            if block_index % 2 == 0 { treatment } else { control }
        }
        SwitchbackDesign::Balanced => {
            // Regular balanced design: pre-computed sequence
            // ensuring equal treatment-control balance within each
            // sub-sequence of length 2k for variance minimization
            balanced_sequence(block_index, cluster_id)
        }
        SwitchbackDesign::Randomized => {
            // Pseudo-random per block, seeded by (experiment_salt, block_index, cluster_id)
            let seed = hash(experiment.salt, block_index, cluster_id);
            if seed % 2 == 0 { treatment } else { control }
        }
    }
}
```

Key difference from user-level assignment: the same user may be in treatment during block 1 and control during block 2. The exposure event records `block_index` alongside `variant_id`.

### Analysis Methods (M4a)

Switchback analysis requires methods that account for temporal autocorrelation and carryover effects. Add to `experimentation-stats`:

```rust
/// Switchback experiment analysis.
/// Implements Bojinov, Simchi-Levi, Zhao (Management Science, 2023).
pub struct SwitchbackAnalyzer {
    /// Block-level aggregated outcomes.
    block_outcomes: Vec<BlockOutcome>,
    /// Design matrix (which blocks received treatment).
    design_matrix: Vec<bool>,
    /// Cluster assignments (for clustered designs).
    cluster_ids: Option<Vec<String>>,
}

pub struct BlockOutcome {
    pub block_index: u64,
    pub cluster_id: String,
    pub variant: VariantId,
    pub metric_value: f64,      // Aggregate outcome for this block
    pub user_count: u64,        // Users observed in this block
    pub in_washout: bool,       // Excluded from analysis
}

pub struct SwitchbackResult {
    /// Estimated treatment effect (difference in block-level means).
    pub effect: f64,
    /// HAC (heteroskedasticity and autocorrelation consistent) standard error.
    /// Uses Newey-West estimator with automatic bandwidth selection.
    pub hac_se: f64,
    /// Confidence interval (using HAC SE).
    pub ci_lower: f64,
    pub ci_upper: f64,
    /// Randomization inference p-value (distribution-free).
    pub randomization_p_value: f64,
    /// Number of blocks used (excluding washout).
    pub effective_blocks: u32,
    /// Estimated autocorrelation at lag 1 (for carryover assessment).
    pub lag1_autocorrelation: f64,
    /// Carryover test p-value (tests whether washout is sufficient).
    pub carryover_test_p_value: f64,
}

impl SwitchbackAnalyzer {
    /// Primary analysis: HAC-adjusted treatment effect.
    pub fn analyze(&self) -> SwitchbackResult;

    /// Randomization inference: exact test using all possible
    /// treatment-control assignment permutations.
    /// For small numbers of blocks, exact; for large, Monte Carlo (10,000 permutations).
    pub fn randomization_test(&self, n_permutations: u32) -> f64;

    /// Carryover test: compare outcomes in first half vs. second half
    /// of each treatment block. Significant difference indicates
    /// insufficient washout.
    pub fn carryover_test(&self) -> (f64, f64);  // (test_statistic, p_value)
}
```

The HAC standard error (Newey-West) is critical — naive standard errors assuming independence across blocks would be wildly optimistic due to temporal autocorrelation. The automatic bandwidth selection uses the Andrews (1991) data-dependent method.

### Data-Driven Block Length Adaptation

When `adaptive_block_length = true`, M5 adjusts block duration after the first 2 complete cycles:

1. M4a estimates the lag-1 autocorrelation of block-level outcomes.
2. If autocorrelation > 0.3 (high carryover), increase block duration by 50%.
3. If autocorrelation < 0.1 (low carryover), decrease block duration by 25% (more switches = more power).
4. The adaptation follows the empirical Bayes framework of Xiong et al. (2024).
5. Only one adaptation is allowed per experiment (to preserve validity).

### M5 Validation

During STARTING, M5 validates:
- `planned_cycles >= 4` (minimum for valid inference).
- `block_duration >= 1 hour` (shorter blocks have excessive carryover for most SVOD treatments).
- `washout_duration >= 0` (can be zero if carryover is assumed negligible).
- If `unit = CLUSTER`, `cluster_attribute` must resolve in user context.
- Primary metric must be an engagement metric computable at the block level (not a long-term metric like 90-day retention — switchback experiments measure within-block effects).

### M6 Rendering

Switchback experiments get a dedicated analysis tab:
- **Block timeline**: Alternating colored bands showing treatment/control blocks with washout periods grayed out.
- **Block-level outcome plot**: Time series of block-level metric values, colored by treatment assignment.
- **Carryover diagnostic**: Autocorrelation function (ACF) plot of block-level residuals.
- **Randomization distribution**: Histogram of permutation test distribution with observed test statistic marked.

## Consequences

### Positive

- Kaizen can now run experiments on interventions that affect all users simultaneously (pricing, catalog, CDN), which was previously impossible.
- Switchback designs properly account for the content interference that user-level experiments only detect post-hoc. For treatments with strong cross-user spillover, switchback is the correct design — not a workaround.
- Randomization inference provides distribution-free validity, complementing the parametric HAC approach.
- Adaptive block length optimization balances carryover bias against statistical efficiency without requiring experimenters to guess the right interval.

### Negative

- Switchback experiments require longer calendar duration than user-level experiments for equivalent power, because the unit of analysis is the time block (typically hours or days) rather than the user. With 4 cycles of 2-day blocks, the minimum experiment duration is 16 days.
- All users are affected during treatment blocks — there is no untreated control group at any given time. This makes switchback inappropriate for high-risk treatments where a permanent control is needed.
- Temporal confounds (day-of-week effects, content release schedules, holidays) can bias results if block length doesn't align with or explicitly model these cycles. Recommendation: use block durations that are multiples of 7 days, or model day-of-week effects in the analysis.
- HAC standard errors with small numbers of blocks (< 20) can be unreliable. The clustered design with multiple clusters partially mitigates this by increasing the effective number of independent observations.

### Risks

- Carryover effects that persist longer than the washout period will bias the treatment effect estimate. The carryover test helps detect this, but cannot eliminate it. Conservative washout periods (≥ 1 block duration) are recommended for treatments with unknown carryover.
- Switchback experiments are visible to users — they may notice the platform behaving differently on different days. For treatments that change the UI or recommendation style noticeably, this could affect behavior (Hawthorne effect). Mitigation: use longer block durations to reduce switch frequency.
- Interaction with other concurrent user-level experiments: if a user-level experiment is running simultaneously, the switchback's temporal variation adds a confound. M5 should warn when switchback and user-level experiments target overlapping metrics.

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| User-level randomization with interference detection (status quo) | Simple, well-understood | Cannot properly estimate treatment effects under interference; detection ≠ correction | Detects the problem but doesn't solve it |
| Cluster-randomized design (geographic regions) | Each region is independent | Requires many independent clusters for power; SVOD regions are heterogeneous, introducing variance | Switchback with temporal randomization is more efficient than geographic randomization for most SVOD treatments |
| Difference-in-differences | Well-established | Requires parallel trends assumption; needs a "treated" and "untreated" group, which doesn't exist for platform-wide changes | Switchback provides the temporal variation DiD needs without requiring a permanent control group |
| Interrupted time series | Simple | No randomization; confounded by temporal trends; single treatment onset provides weak identification | Not an experimental design; purely observational |

## References

- Bojinov, Simchi-Levi, Zhao: "Design and Analysis of Switchback Experiments" (Management Science, 2023)
- Xiong, Chin, Taylor: "Data-driven block length selection for switchback experiments" (empirical Bayes, 2024)
- Jia et al.: "Clustered Switchback Designs for Experimentation Under Spatio-temporal Interference" (arXiv 2312.15574, updated March 2025)
- Masoero et al.: "Regular Balanced Switchback Designs for Robust Multi-Unit Online Experimentation" (Amazon Science, 2025)
- arXiv February 2026: Distribution-free randomization tests for switchback designs
- DoorDash: Switchback implementation blueprints with sandwich variance estimators
- Andrews (1991): automatic bandwidth selection for HAC estimators
- ADR-014 (Provider-side metrics — switchback analysis on catalog-level metrics)
- ADR-021 (Feedback loop interference — switchback can mitigate by keeping all users in same treatment)
