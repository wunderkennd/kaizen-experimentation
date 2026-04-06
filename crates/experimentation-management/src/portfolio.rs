//! Portfolio optimization for ADR-019.
//!
//! Implements priority-weighted variance budget allocation, conflict detection,
//! traffic recommendations, `ExperimentLearning` classification, `AnnualizedImpact`
//! computation, and decision rule evaluation across a portfolio of RUNNING experiments.
//!
//! This is a Rust port of `services/management/internal/portfolio/optimizer.go`.

use std::collections::{HashMap, HashSet};

use experimentation_core::error::assert_finite;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default business priority when not specified in overrides.
pub const DEFAULT_PRIORITY: i32 = 3;

/// Valid priority range (1 = lowest, 5 = highest).
pub const MIN_PRIORITY: i32 = 1;
pub const MAX_PRIORITY: i32 = 5;

/// No single experiment should take more than half a layer's capacity.
const MAX_SINGLE_EXPERIMENT_SHARE: f64 = 0.5;

/// An experiment is underpowered if its current allocation < 75% of its recommended share.
const UNDERPOWERED_THRESHOLD: f64 = 0.75;

/// Conservative Bonferroni-bound false discovery rate per experiment.
const PER_EXPERIMENT_FDR: f64 = 0.05;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Minimal per-experiment data needed by the portfolio optimizer.
/// Constructed from store rows + bucket allocations, avoiding proto imports.
#[derive(Debug, Clone)]
pub struct ExperimentInfo {
    pub experiment_id: String,
    pub experiment_name: String,
    pub layer_id: String,
    pub primary_metric_id: String,
    pub guardrail_metric_ids: Vec<String>,
    /// Empty string means no targeting (full population).
    pub targeting_rule_id: String,
    pub start_bucket: i32,
    pub end_bucket: i32,
    /// Total buckets in the layer this experiment belongs to.
    pub layer_total_buckets: i32,
}

impl ExperimentInfo {
    /// Current fraction [0.0, 1.0] of layer capacity occupied by this experiment.
    pub fn traffic_fraction(&self) -> f64 {
        if self.layer_total_buckets <= 0 {
            return 0.0;
        }
        let used =
            (self.end_bucket - self.start_bucket + 1) as f64 / self.layer_total_buckets as f64;
        used.clamp(0.0, 1.0)
    }
}

/// Mirrors the proto `ConflictType` enum without importing proto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictType {
    PrimaryMetricOverlap = 1,
    GuardrailMetricOverlap = 2,
    PopulationOverlap = 3,
}

/// A detected conflict between two experiments.
#[derive(Debug, Clone)]
pub struct Conflict {
    pub experiment_id_a: String,
    pub experiment_id_b: String,
    pub conflict_type: ConflictType,
    pub rationale: String,
}

/// Per-experiment portfolio recommendation.
#[derive(Debug, Clone)]
pub struct Allocation {
    pub experiment_id: String,
    pub experiment_name: String,
    pub priority: i32,
    pub current_traffic_fraction: f64,
    pub recommended_traffic_fraction: f64,
    pub underpowered: bool,
    pub rationale: String,
    pub variance_budget_share: f64,
}

/// Portfolio health summary.
#[derive(Debug, Clone)]
pub struct PortfolioStats {
    pub running_count: i32,
    pub traffic_utilization: f64,
    pub expected_false_discoveries: f64,
    pub underpowered_count: i32,
    pub conflict_count: i32,
}

/// Full output of `optimize`.
#[derive(Debug, Clone)]
pub struct PortfolioResult {
    pub allocations: Vec<Allocation>,
    pub conflicts: Vec<Conflict>,
    pub stats: PortfolioStats,
}

// ---------------------------------------------------------------------------
// ExperimentLearning classification (ADR-019)
// ---------------------------------------------------------------------------

/// Classification of the knowledge yield of a concluded experiment.
/// Maps 1:1 to proto `ExperimentLearning` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExperimentLearning {
    Unspecified = 0,
    /// Significant positive effect confirmed; ship the treatment.
    Winner = 1,
    /// No significant effect; null result with sufficient power.
    Null = 2,
    /// Significant negative effect; do not ship.
    Loser = 3,
    /// Insufficient power or unexpected behavior.
    Inconclusive = 4,
    /// Directional signal below significance threshold; follow-up warranted.
    Directional = 5,
}

/// Input data for classifying a concluded experiment.
#[derive(Debug, Clone)]
pub struct ExperimentOutcome {
    /// Estimated treatment effect on primary metric.
    pub effect_estimate: f64,
    /// Two-sided p-value from the primary metric test.
    pub p_value: f64,
    /// Statistical power achieved (fraction, 0.0–1.0).
    pub achieved_power: f64,
    /// Significance level used for the test.
    pub alpha: f64,
}

/// Classify the learning from a concluded experiment's statistical results.
///
/// Decision rules:
/// - `p < alpha` and effect > 0 → Winner
/// - `p < alpha` and effect < 0 → Loser
/// - `p >= alpha` and power >= 0.8 → Null (well-powered non-significant result)
/// - `p >= alpha` and `p < 2*alpha` and power < 0.8 → Directional
/// - Otherwise → Inconclusive
pub fn classify_learning(outcome: &ExperimentOutcome) -> ExperimentLearning {
    assert_finite(outcome.effect_estimate, "effect_estimate");
    assert_finite(outcome.p_value, "p_value");
    assert_finite(outcome.achieved_power, "achieved_power");
    assert_finite(outcome.alpha, "alpha");

    if outcome.p_value < outcome.alpha {
        if outcome.effect_estimate > 0.0 {
            ExperimentLearning::Winner
        } else {
            ExperimentLearning::Loser
        }
    } else if outcome.achieved_power >= 0.8 {
        ExperimentLearning::Null
    } else if outcome.p_value < 2.0 * outcome.alpha {
        ExperimentLearning::Directional
    } else {
        ExperimentLearning::Inconclusive
    }
}

// ---------------------------------------------------------------------------
// AnnualizedImpact computation (ADR-019)
// ---------------------------------------------------------------------------

/// Input data for annualized impact projection.
#[derive(Debug, Clone)]
pub struct ImpactInput {
    pub experiment_id: String,
    /// Observed relative effect on primary metric (e.g., 0.02 = +2%).
    pub primary_metric_effect: f64,
    /// Lower bound of the confidence interval for the effect.
    pub effect_ci_lower: f64,
    /// Upper bound of the confidence interval for the effect.
    pub effect_ci_upper: f64,
    /// Current daily active users (or users in experiment population).
    pub daily_users: i64,
    /// Whether the projection is based on a surrogate model rather than observed data.
    pub based_on_surrogate: bool,
}

/// Annualized impact projection for a concluded experiment.
#[derive(Debug, Clone)]
pub struct AnnualizedImpact {
    pub experiment_id: String,
    pub primary_metric_annual_effect: f64,
    pub annual_effect_ci_lower: f64,
    pub annual_effect_ci_upper: f64,
    pub annual_users_impacted: i64,
    pub based_on_surrogate: bool,
}

/// Compute the annualized impact projection from experiment results.
///
/// Scales the observed relative effect to a full year (365 days) and
/// projects annual user reach from the daily user count.
pub fn compute_annualized_impact(input: &ImpactInput) -> AnnualizedImpact {
    assert_finite(input.primary_metric_effect, "primary_metric_effect");
    assert_finite(input.effect_ci_lower, "effect_ci_lower");
    assert_finite(input.effect_ci_upper, "effect_ci_upper");

    AnnualizedImpact {
        experiment_id: input.experiment_id.clone(),
        primary_metric_annual_effect: input.primary_metric_effect,
        annual_effect_ci_lower: input.effect_ci_lower,
        annual_effect_ci_upper: input.effect_ci_upper,
        annual_users_impacted: input.daily_users * 365,
        based_on_surrogate: input.based_on_surrogate,
    }
}

// ---------------------------------------------------------------------------
// Decision rule evaluation (ADR-019)
// ---------------------------------------------------------------------------

/// Decision recommendation for a running experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Continue collecting data.
    Continue,
    /// Ship the winning treatment.
    Ship,
    /// Stop the experiment (negative or null result).
    Stop,
    /// Extend the experiment for more power.
    Extend,
}

/// Inputs for decision rule evaluation.
#[derive(Debug, Clone)]
pub struct DecisionInput {
    /// Current p-value from the latest analysis.
    pub p_value: f64,
    /// Significance level.
    pub alpha: f64,
    /// Estimated treatment effect.
    pub effect_estimate: f64,
    /// Fraction of planned duration elapsed (0.0–1.0).
    pub elapsed_fraction: f64,
    /// Achieved statistical power at current sample size.
    pub achieved_power: f64,
    /// Whether the experiment has a guardrail breach.
    pub guardrail_breached: bool,
}

/// Evaluate decision rules for a running experiment.
///
/// Returns a recommendation based on current statistical evidence.
pub fn evaluate_decision(input: &DecisionInput) -> Decision {
    assert_finite(input.p_value, "p_value");
    assert_finite(input.alpha, "alpha");
    assert_finite(input.effect_estimate, "effect_estimate");
    assert_finite(input.elapsed_fraction, "elapsed_fraction");
    assert_finite(input.achieved_power, "achieved_power");

    // Guardrail breach → immediate stop.
    if input.guardrail_breached {
        return Decision::Stop;
    }

    // Significant result reached.
    if input.p_value < input.alpha {
        return if input.effect_estimate > 0.0 {
            Decision::Ship
        } else {
            Decision::Stop
        };
    }

    // Past full planned duration with no significance → stop.
    if input.elapsed_fraction >= 1.0 {
        return Decision::Stop;
    }

    // Past halfway with low power → extend.
    if input.elapsed_fraction >= 0.5 && input.achieved_power < 0.5 {
        return Decision::Extend;
    }

    Decision::Continue
}

// ---------------------------------------------------------------------------
// Alpha recommendation engine (ADR-019)
// ---------------------------------------------------------------------------

/// Recommend a per-experiment alpha (significance level) based on portfolio context.
///
/// Uses a priority-weighted Bonferroni correction: higher-priority experiments
/// receive a larger share of the family-wise error budget.
pub fn recommend_alpha(
    experiment_id: &str,
    experiments: &[ExperimentInfo],
    priority_overrides: &HashMap<String, i32>,
    family_alpha: f64,
) -> f64 {
    assert_finite(family_alpha, "family_alpha");

    if experiments.is_empty() {
        return family_alpha;
    }

    let priorities: HashMap<&str, i32> = experiments
        .iter()
        .map(|e| {
            let p = priority_overrides
                .get(&e.experiment_id)
                .copied()
                .map(clamp_priority)
                .unwrap_or(DEFAULT_PRIORITY);
            (e.experiment_id.as_str(), p)
        })
        .collect();

    let total_priority: i32 = priorities.values().sum();
    if total_priority == 0 {
        return family_alpha / experiments.len() as f64;
    }

    let this_priority = priorities.get(experiment_id).copied().unwrap_or(DEFAULT_PRIORITY);
    let share = this_priority as f64 / total_priority as f64;

    // Weighted alpha: this experiment's share of the family-wise budget.
    let alpha = family_alpha * share;

    // Floor at standard Bonferroni (equal split) to never be worse than uniform.
    let uniform = family_alpha / experiments.len() as f64;
    alpha.max(uniform)
}

// ---------------------------------------------------------------------------
// Portfolio optimizer (main entry point)
// ---------------------------------------------------------------------------

/// Compute portfolio-level allocation recommendations, conflict detection,
/// and variance budget shares for the supplied experiments.
///
/// `priority_overrides` maps experiment_id → priority (1–5). Missing IDs get `DEFAULT_PRIORITY`.
pub fn optimize(
    experiments: &[ExperimentInfo],
    priority_overrides: &HashMap<String, i32>,
) -> PortfolioResult {
    if experiments.is_empty() {
        return PortfolioResult {
            allocations: vec![],
            conflicts: vec![],
            stats: PortfolioStats {
                running_count: 0,
                traffic_utilization: 0.0,
                expected_false_discoveries: 0.0,
                underpowered_count: 0,
                conflict_count: 0,
            },
        };
    }

    // --- Priority resolution ---
    let priorities: HashMap<&str, i32> = experiments
        .iter()
        .map(|e| {
            let p = priority_overrides
                .get(&e.experiment_id)
                .copied()
                .map(clamp_priority)
                .unwrap_or(DEFAULT_PRIORITY);
            (e.experiment_id.as_str(), p)
        })
        .collect();

    // --- Conflict detection ---
    let conflicts = detect_conflicts(experiments);

    // --- Variance budget shares (priority-weighted) ---
    let total_priority: i32 = priorities.values().sum();
    let budget_shares: HashMap<&str, f64> = experiments
        .iter()
        .map(|e| {
            let share = if total_priority > 0 {
                *priorities.get(e.experiment_id.as_str()).unwrap() as f64 / total_priority as f64
            } else {
                1.0 / experiments.len() as f64
            };
            (e.experiment_id.as_str(), share)
        })
        .collect();

    // --- Recommended traffic allocation (per-layer) ---
    let by_layer = group_by_layer(experiments);
    let mut recommendations: HashMap<&str, f64> = HashMap::new();
    let mut underpowered_flags: HashMap<&str, bool> = HashMap::new();

    for (_layer_id, layer_exps) in &by_layer {
        let (layer_rec, layer_underpowered) =
            recommend_layer(layer_exps, &priorities, &budget_shares);
        recommendations.extend(layer_rec);
        underpowered_flags.extend(layer_underpowered);
    }

    // --- Assemble allocations ---
    let mut underpowered_count: i32 = 0;
    let mut allocations: Vec<Allocation> = experiments
        .iter()
        .map(|e| {
            let current = e.traffic_fraction();
            let recommended = recommendations
                .get(e.experiment_id.as_str())
                .copied()
                .unwrap_or(0.0);
            let is_underpowered = underpowered_flags
                .get(e.experiment_id.as_str())
                .copied()
                .unwrap_or(false);
            if is_underpowered {
                underpowered_count += 1;
            }
            let priority = *priorities.get(e.experiment_id.as_str()).unwrap();

            Allocation {
                experiment_id: e.experiment_id.clone(),
                experiment_name: e.experiment_name.clone(),
                priority,
                current_traffic_fraction: current,
                recommended_traffic_fraction: recommended,
                underpowered: is_underpowered,
                rationale: build_rationale(current, recommended, is_underpowered, priority),
                variance_budget_share: budget_shares
                    .get(e.experiment_id.as_str())
                    .copied()
                    .unwrap_or(0.0),
            }
        })
        .collect();

    // Sort by priority desc, then experiment_id for determinism.
    allocations.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.experiment_id.cmp(&b.experiment_id))
    });

    // --- Portfolio stats ---
    let utilization = compute_traffic_utilization(experiments, &by_layer);
    let stats = PortfolioStats {
        running_count: experiments.len() as i32,
        traffic_utilization: utilization,
        expected_false_discoveries: experiments.len() as f64 * PER_EXPERIMENT_FDR,
        underpowered_count,
        conflict_count: conflicts.len() as i32,
    };

    PortfolioResult {
        allocations,
        conflicts,
        stats,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn clamp_priority(p: i32) -> i32 {
    p.clamp(MIN_PRIORITY, MAX_PRIORITY)
}

fn detect_conflicts(experiments: &[ExperimentInfo]) -> Vec<Conflict> {
    let mut conflicts = Vec::new();

    for i in 0..experiments.len() {
        for j in (i + 1)..experiments.len() {
            let a = &experiments[i];
            let b = &experiments[j];

            // Primary metric overlap within the same layer.
            if a.layer_id == b.layer_id
                && !a.primary_metric_id.is_empty()
                && a.primary_metric_id == b.primary_metric_id
            {
                conflicts.push(Conflict {
                    experiment_id_a: a.experiment_id.clone(),
                    experiment_id_b: b.experiment_id.clone(),
                    conflict_type: ConflictType::PrimaryMetricOverlap,
                    rationale: format!(
                        "Both experiments use primary metric {:?} in the same layer — \
                         concurrent significance tests inflate false discovery rate",
                        a.primary_metric_id,
                    ),
                });
            }

            // Guardrail metric overlap within the same layer.
            if a.layer_id == b.layer_id {
                let shared = shared_metrics(&a.guardrail_metric_ids, &b.guardrail_metric_ids);
                for m in shared {
                    conflicts.push(Conflict {
                        experiment_id_a: a.experiment_id.clone(),
                        experiment_id_b: b.experiment_id.clone(),
                        conflict_type: ConflictType::GuardrailMetricOverlap,
                        rationale: format!(
                            "Both experiments monitor guardrail metric {:?} — \
                             correlated stopping rules may cause spurious pauses",
                            m,
                        ),
                    });
                }
            }

            // Population overlap: same layer, no targeting separation.
            if a.layer_id == b.layer_id
                && a.targeting_rule_id.is_empty()
                && b.targeting_rule_id.is_empty()
            {
                conflicts.push(Conflict {
                    experiment_id_a: a.experiment_id.clone(),
                    experiment_id_b: b.experiment_id.clone(),
                    conflict_type: ConflictType::PopulationOverlap,
                    rationale: "Both experiments target the full user population in the same \
                                layer — users may see both treatments simultaneously, causing \
                                interference"
                        .to_string(),
                });
            }
        }
    }

    conflicts
}

fn recommend_layer<'a>(
    experiments: &'a [&ExperimentInfo],
    priorities: &HashMap<&str, i32>,
    budget_shares: &HashMap<&str, f64>,
) -> (HashMap<&'a str, f64>, HashMap<&'a str, bool>) {
    let mut recommendations = HashMap::new();
    let mut underpowered = HashMap::new();

    if experiments.is_empty() {
        return (recommendations, underpowered);
    }

    // Total weight for this layer.
    let total_weight: f64 = experiments
        .iter()
        .map(|e| budget_shares.get(e.experiment_id.as_str()).copied().unwrap_or(0.0))
        .sum();

    for e in experiments {
        let share = if total_weight > 0.0 {
            budget_shares.get(e.experiment_id.as_str()).copied().unwrap_or(0.0) / total_weight
        } else {
            0.0
        };

        // Cap at MAX_SINGLE_EXPERIMENT_SHARE.
        let share = share.min(MAX_SINGLE_EXPERIMENT_SHARE);
        recommendations.insert(e.experiment_id.as_str(), share);

        // Underpowered if current < 75% of recommended share.
        let current = e.traffic_fraction();
        if share > 0.0 && current < share * UNDERPOWERED_THRESHOLD {
            underpowered.insert(e.experiment_id.as_str(), true);
        } else {
            underpowered.insert(e.experiment_id.as_str(), false);
        }
    }

    let _ = priorities; // used indirectly via budget_shares
    (recommendations, underpowered)
}

fn compute_traffic_utilization(
    _experiments: &[ExperimentInfo],
    by_layer: &HashMap<&str, Vec<&ExperimentInfo>>,
) -> f64 {
    if by_layer.is_empty() {
        return 0.0;
    }

    let mut total_util = 0.0;
    let mut layer_count = 0;

    for layer_exps in by_layer.values() {
        let mut layer_util: f64 = layer_exps.iter().map(|e| e.traffic_fraction()).sum();
        if layer_util > 1.0 {
            layer_util = 1.0;
        }
        total_util += layer_util;
        layer_count += 1;
    }

    let avg = total_util / layer_count as f64;
    avg.min(1.0)
}

fn group_by_layer<'a>(experiments: &'a [ExperimentInfo]) -> HashMap<&'a str, Vec<&'a ExperimentInfo>> {
    let mut map: HashMap<&str, Vec<&ExperimentInfo>> = HashMap::new();
    for e in experiments {
        map.entry(e.layer_id.as_str()).or_default().push(e);
    }
    map
}

fn shared_metrics(a: &[String], b: &[String]) -> Vec<String> {
    let set: HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    b.iter()
        .filter(|m| set.contains(m.as_str()))
        .cloned()
        .collect()
}

fn build_rationale(
    current: f64,
    recommended: f64,
    underpowered: bool,
    priority: i32,
) -> String {
    if underpowered && recommended > current {
        format!(
            "Priority {} experiment is under-trafficked ({:.1}% vs recommended {:.1}%) — \
             increase allocation to reach significance faster",
            priority,
            current * 100.0,
            recommended * 100.0,
        )
    } else if recommended < current {
        format!(
            "Priority {} experiment has more traffic than its variance budget share warrants \
             ({:.1}% vs recommended {:.1}%) — consider reducing to free capacity for \
             higher-priority work",
            priority,
            current * 100.0,
            recommended * 100.0,
        )
    } else {
        format!(
            "Priority {} experiment allocation ({:.1}%) is consistent with its variance \
             budget share ({:.1}%)",
            priority,
            current * 100.0,
            recommended * 100.0,
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_exp(
        id: &str,
        name: &str,
        layer: &str,
        primary_metric: &str,
        start: i32,
        end: i32,
        total: i32,
    ) -> ExperimentInfo {
        ExperimentInfo {
            experiment_id: id.to_string(),
            experiment_name: name.to_string(),
            layer_id: layer.to_string(),
            primary_metric_id: primary_metric.to_string(),
            guardrail_metric_ids: vec![],
            targeting_rule_id: String::new(),
            start_bucket: start,
            end_bucket: end,
            layer_total_buckets: total,
        }
    }

    // --- optimize tests ---

    #[test]
    fn optimize_empty() {
        let result = optimize(&[], &HashMap::new());
        assert!(result.allocations.is_empty());
        assert!(result.conflicts.is_empty());
        assert_eq!(result.stats.running_count, 0);
    }

    #[test]
    fn optimize_single_experiment() {
        let exp = make_exp("exp-1", "Test", "layer-1", "watch_time", 0, 99, 1000);
        let result = optimize(&[exp], &HashMap::new());

        assert_eq!(result.allocations.len(), 1);
        let a = &result.allocations[0];
        assert_eq!(a.experiment_id, "exp-1");
        assert_eq!(a.priority, DEFAULT_PRIORITY);
        assert!((a.current_traffic_fraction - 0.1).abs() < 0.001); // 100/1000
        assert!(result.conflicts.is_empty());
        assert_eq!(result.stats.running_count, 1);
    }

    #[test]
    fn optimize_priority_override() {
        let exp1 = make_exp("exp-1", "High Prio", "layer-1", "watch_time", 0, 99, 1000);
        let exp2 = make_exp("exp-2", "Low Prio", "layer-1", "clicks", 100, 199, 1000);

        let mut overrides = HashMap::new();
        overrides.insert("exp-1".to_string(), 5);
        overrides.insert("exp-2".to_string(), 1);

        let result = optimize(&[exp1, exp2], &overrides);

        assert_eq!(result.allocations.len(), 2);
        // First allocation should be exp-1 (highest priority, sorted desc).
        assert_eq!(result.allocations[0].experiment_id, "exp-1");
        assert_eq!(result.allocations[0].priority, 5);
        assert_eq!(result.allocations[1].experiment_id, "exp-2");
        assert_eq!(result.allocations[1].priority, 1);

        // exp-1 gets more variance budget (5/6 ≈ 0.833).
        assert!(
            result.allocations[0].variance_budget_share
                > result.allocations[1].variance_budget_share
        );
    }

    #[test]
    fn optimize_priority_clamping() {
        let exp = make_exp("exp-1", "Test", "layer-1", "watch_time", 0, 99, 1000);
        let mut overrides = HashMap::new();
        overrides.insert("exp-1".to_string(), 99); // out of bounds
        let result = optimize(&[exp], &overrides);
        assert_eq!(result.allocations[0].priority, MAX_PRIORITY);
    }

    // --- Conflict detection tests ---

    #[test]
    fn conflict_primary_metric_overlap() {
        let exp1 = make_exp("exp-1", "Exp1", "layer-1", "watch_time", 0, 99, 1000);
        let exp2 = make_exp("exp-2", "Exp2", "layer-1", "watch_time", 100, 199, 1000);

        let result = optimize(&[exp1, exp2], &HashMap::new());

        // Should have primary metric + population overlap conflicts.
        assert!(result.conflicts.len() >= 1);
        let has_primary = result
            .conflicts
            .iter()
            .any(|c| c.conflict_type == ConflictType::PrimaryMetricOverlap);
        assert!(has_primary, "expected primary metric conflict");
    }

    #[test]
    fn conflict_different_layers_no_conflict() {
        let exp1 = make_exp("exp-1", "Exp1", "layer-1", "watch_time", 0, 99, 1000);
        let exp2 = make_exp("exp-2", "Exp2", "layer-2", "watch_time", 0, 99, 1000);

        let result = optimize(&[exp1, exp2], &HashMap::new());
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn conflict_guardrail_metric_overlap() {
        let exp1 = ExperimentInfo {
            experiment_id: "exp-1".to_string(),
            experiment_name: "Exp1".to_string(),
            layer_id: "layer-1".to_string(),
            primary_metric_id: "watch_time".to_string(),
            guardrail_metric_ids: vec!["rebuffer_rate".to_string(), "playback_failures".to_string()],
            targeting_rule_id: String::new(),
            start_bucket: 0,
            end_bucket: 99,
            layer_total_buckets: 1000,
        };
        let exp2 = ExperimentInfo {
            experiment_id: "exp-2".to_string(),
            experiment_name: "Exp2".to_string(),
            layer_id: "layer-1".to_string(),
            primary_metric_id: "clicks".to_string(),
            guardrail_metric_ids: vec!["rebuffer_rate".to_string()], // shared
            targeting_rule_id: String::new(),
            start_bucket: 100,
            end_bucket: 199,
            layer_total_buckets: 1000,
        };

        let result = optimize(&[exp1, exp2], &HashMap::new());
        let has_guardrail = result
            .conflicts
            .iter()
            .any(|c| c.conflict_type == ConflictType::GuardrailMetricOverlap);
        assert!(has_guardrail, "expected guardrail metric conflict");
    }

    #[test]
    fn conflict_population_overlap() {
        let exp1 = make_exp("exp-1", "Exp1", "layer-1", "watch_time", 0, 99, 1000);
        let exp2 = make_exp("exp-2", "Exp2", "layer-1", "clicks", 100, 199, 1000);

        let result = optimize(&[exp1, exp2], &HashMap::new());
        let has_pop = result
            .conflicts
            .iter()
            .any(|c| c.conflict_type == ConflictType::PopulationOverlap);
        assert!(has_pop, "expected population overlap conflict");
    }

    #[test]
    fn conflict_targeting_rule_separation_no_population_conflict() {
        let exp1 = ExperimentInfo {
            experiment_id: "exp-1".to_string(),
            experiment_name: "Exp1".to_string(),
            layer_id: "layer-1".to_string(),
            primary_metric_id: "watch_time".to_string(),
            guardrail_metric_ids: vec![],
            targeting_rule_id: "rule-mobile".to_string(),
            start_bucket: 0,
            end_bucket: 99,
            layer_total_buckets: 1000,
        };
        let exp2 = ExperimentInfo {
            experiment_id: "exp-2".to_string(),
            experiment_name: "Exp2".to_string(),
            layer_id: "layer-1".to_string(),
            primary_metric_id: "clicks".to_string(),
            guardrail_metric_ids: vec![],
            targeting_rule_id: "rule-desktop".to_string(),
            start_bucket: 100,
            end_bucket: 199,
            layer_total_buckets: 1000,
        };

        let result = optimize(&[exp1, exp2], &HashMap::new());
        for c in &result.conflicts {
            assert_ne!(
                c.conflict_type,
                ConflictType::PopulationOverlap,
                "experiments with targeting rules should not have population overlap conflict"
            );
        }
    }

    // --- Variance budget tests ---

    #[test]
    fn variance_budget_shares_sum_to_one() {
        let experiments = vec![
            make_exp("exp-1", "Exp1", "layer-1", "m1", 0, 99, 1000),
            make_exp("exp-2", "Exp2", "layer-1", "m2", 100, 199, 1000),
            make_exp("exp-3", "Exp3", "layer-1", "m3", 200, 299, 1000),
        ];
        let result = optimize(&experiments, &HashMap::new());
        let total: f64 = result.allocations.iter().map(|a| a.variance_budget_share).sum();
        assert!((total - 1.0).abs() < 1e-9);
    }

    // --- Portfolio stats tests ---

    #[test]
    fn portfolio_stats_false_discovery_estimate() {
        let experiments: Vec<ExperimentInfo> = (0..10)
            .map(|i| {
                make_exp(
                    &format!("exp-{i}"),
                    "Exp",
                    "layer-1",
                    "metric",
                    i * 100,
                    i * 100 + 99,
                    10000,
                )
            })
            .collect();
        let result = optimize(&experiments, &HashMap::new());
        // Expected false discoveries = N × 0.05 = 10 × 0.05 = 0.5
        assert!((result.stats.expected_false_discoveries - 0.5).abs() < 1e-9);
    }

    #[test]
    fn portfolio_stats_traffic_utilization() {
        let exp1 = make_exp("exp-1", "Exp1", "layer-1", "m1", 0, 99, 1000);
        let exp2 = make_exp("exp-2", "Exp2", "layer-1", "m2", 100, 199, 1000);
        let result = optimize(&[exp1, exp2], &HashMap::new());
        assert!((result.stats.traffic_utilization - 0.2).abs() < 0.001);
    }

    // --- Underpowered detection ---

    #[test]
    fn underpowered_detection() {
        let exp1 = ExperimentInfo {
            experiment_id: "exp-1".to_string(),
            experiment_name: "High Priority".to_string(),
            layer_id: "layer-1".to_string(),
            primary_metric_id: "watch_time".to_string(),
            guardrail_metric_ids: vec![],
            targeting_rule_id: String::new(),
            start_bucket: 0,
            end_bucket: 9, // 1%
            layer_total_buckets: 1000,
        };
        let exp2 = ExperimentInfo {
            experiment_id: "exp-2".to_string(),
            experiment_name: "Low Priority".to_string(),
            layer_id: "layer-1".to_string(),
            primary_metric_id: "clicks".to_string(),
            guardrail_metric_ids: vec![],
            targeting_rule_id: String::new(),
            start_bucket: 10,
            end_bucket: 409, // 40%
            layer_total_buckets: 1000,
        };

        let mut overrides = HashMap::new();
        overrides.insert("exp-1".to_string(), 5);
        overrides.insert("exp-2".to_string(), 1);

        let result = optimize(&[exp1, exp2], &overrides);

        let exp1_alloc = result
            .allocations
            .iter()
            .find(|a| a.experiment_id == "exp-1")
            .unwrap();
        let exp2_alloc = result
            .allocations
            .iter()
            .find(|a| a.experiment_id == "exp-2")
            .unwrap();

        assert!(exp1_alloc.underpowered, "exp-1 should be underpowered");
        assert!(!exp2_alloc.underpowered, "exp-2 should not be underpowered");
    }

    // --- ExperimentLearning classification tests ---

    #[test]
    fn classify_winner() {
        let outcome = ExperimentOutcome {
            effect_estimate: 0.05,
            p_value: 0.01,
            achieved_power: 0.9,
            alpha: 0.05,
        };
        assert_eq!(classify_learning(&outcome), ExperimentLearning::Winner);
    }

    #[test]
    fn classify_loser() {
        let outcome = ExperimentOutcome {
            effect_estimate: -0.03,
            p_value: 0.02,
            achieved_power: 0.85,
            alpha: 0.05,
        };
        assert_eq!(classify_learning(&outcome), ExperimentLearning::Loser);
    }

    #[test]
    fn classify_null() {
        let outcome = ExperimentOutcome {
            effect_estimate: 0.001,
            p_value: 0.45,
            achieved_power: 0.85,
            alpha: 0.05,
        };
        assert_eq!(classify_learning(&outcome), ExperimentLearning::Null);
    }

    #[test]
    fn classify_directional() {
        let outcome = ExperimentOutcome {
            effect_estimate: 0.02,
            p_value: 0.07, // between alpha and 2*alpha
            achieved_power: 0.6,
            alpha: 0.05,
        };
        assert_eq!(classify_learning(&outcome), ExperimentLearning::Directional);
    }

    #[test]
    fn classify_inconclusive() {
        let outcome = ExperimentOutcome {
            effect_estimate: 0.01,
            p_value: 0.30,
            achieved_power: 0.4,
            alpha: 0.05,
        };
        assert_eq!(
            classify_learning(&outcome),
            ExperimentLearning::Inconclusive
        );
    }

    // --- AnnualizedImpact tests ---

    #[test]
    fn annualized_impact_basic() {
        let input = ImpactInput {
            experiment_id: "exp-1".to_string(),
            primary_metric_effect: 0.02,
            effect_ci_lower: 0.005,
            effect_ci_upper: 0.035,
            daily_users: 1_000_000,
            based_on_surrogate: false,
        };
        let impact = compute_annualized_impact(&input);
        assert_eq!(impact.experiment_id, "exp-1");
        assert!((impact.primary_metric_annual_effect - 0.02).abs() < 1e-9);
        assert_eq!(impact.annual_users_impacted, 365_000_000);
        assert!(!impact.based_on_surrogate);
    }

    #[test]
    fn annualized_impact_surrogate() {
        let input = ImpactInput {
            experiment_id: "exp-2".to_string(),
            primary_metric_effect: 0.01,
            effect_ci_lower: -0.005,
            effect_ci_upper: 0.025,
            daily_users: 500_000,
            based_on_surrogate: true,
        };
        let impact = compute_annualized_impact(&input);
        assert!(impact.based_on_surrogate);
    }

    // --- Decision rule tests ---

    #[test]
    fn decision_ship_on_significant_positive() {
        let input = DecisionInput {
            p_value: 0.01,
            alpha: 0.05,
            effect_estimate: 0.03,
            elapsed_fraction: 0.6,
            achieved_power: 0.9,
            guardrail_breached: false,
        };
        assert_eq!(evaluate_decision(&input), Decision::Ship);
    }

    #[test]
    fn decision_stop_on_significant_negative() {
        let input = DecisionInput {
            p_value: 0.02,
            alpha: 0.05,
            effect_estimate: -0.04,
            elapsed_fraction: 0.7,
            achieved_power: 0.9,
            guardrail_breached: false,
        };
        assert_eq!(evaluate_decision(&input), Decision::Stop);
    }

    #[test]
    fn decision_stop_on_guardrail_breach() {
        let input = DecisionInput {
            p_value: 0.30,
            alpha: 0.05,
            effect_estimate: 0.02,
            elapsed_fraction: 0.3,
            achieved_power: 0.5,
            guardrail_breached: true,
        };
        assert_eq!(evaluate_decision(&input), Decision::Stop);
    }

    #[test]
    fn decision_stop_at_end_no_significance() {
        let input = DecisionInput {
            p_value: 0.20,
            alpha: 0.05,
            effect_estimate: 0.01,
            elapsed_fraction: 1.0,
            achieved_power: 0.7,
            guardrail_breached: false,
        };
        assert_eq!(evaluate_decision(&input), Decision::Stop);
    }

    #[test]
    fn decision_extend_low_power() {
        let input = DecisionInput {
            p_value: 0.15,
            alpha: 0.05,
            effect_estimate: 0.01,
            elapsed_fraction: 0.6,
            achieved_power: 0.3,
            guardrail_breached: false,
        };
        assert_eq!(evaluate_decision(&input), Decision::Extend);
    }

    #[test]
    fn decision_continue_early() {
        let input = DecisionInput {
            p_value: 0.20,
            alpha: 0.05,
            effect_estimate: 0.02,
            elapsed_fraction: 0.3,
            achieved_power: 0.4,
            guardrail_breached: false,
        };
        assert_eq!(evaluate_decision(&input), Decision::Continue);
    }

    // --- Alpha recommendation tests ---

    #[test]
    fn alpha_single_experiment() {
        let exp = make_exp("exp-1", "Test", "layer-1", "m1", 0, 99, 1000);
        let alpha = recommend_alpha("exp-1", &[exp], &HashMap::new(), 0.05);
        assert!((alpha - 0.05).abs() < 1e-9);
    }

    #[test]
    fn alpha_weighted_higher_priority_gets_more() {
        let exp1 = make_exp("exp-1", "High", "layer-1", "m1", 0, 99, 1000);
        let exp2 = make_exp("exp-2", "Low", "layer-1", "m2", 100, 199, 1000);

        let mut overrides = HashMap::new();
        overrides.insert("exp-1".to_string(), 5);
        overrides.insert("exp-2".to_string(), 1);

        let alpha1 = recommend_alpha("exp-1", &[exp1.clone(), exp2.clone()], &overrides, 0.05);
        let alpha2 = recommend_alpha("exp-2", &[exp1, exp2], &overrides, 0.05);

        assert!(alpha1 > alpha2, "higher priority should get more alpha budget");
    }

    #[test]
    fn alpha_sum_does_not_exceed_family() {
        let experiments: Vec<ExperimentInfo> = (0..5)
            .map(|i| make_exp(&format!("exp-{i}"), "Exp", "layer-1", "m", i * 100, i * 100 + 99, 1000))
            .collect();

        let total: f64 = experiments
            .iter()
            .map(|e| recommend_alpha(&e.experiment_id, &experiments, &HashMap::new(), 0.05))
            .sum();

        // With uniform priorities, sum should equal family_alpha.
        assert!((total - 0.05).abs() < 1e-9);
    }

    // --- Traffic fraction edge cases ---

    #[test]
    fn traffic_fraction_zero_total_buckets() {
        let exp = ExperimentInfo {
            layer_total_buckets: 0,
            ..make_exp("exp-1", "Test", "layer-1", "m1", 0, 99, 0)
        };
        assert_eq!(exp.traffic_fraction(), 0.0);
    }

    #[test]
    fn traffic_fraction_full_layer() {
        let exp = make_exp("exp-1", "Test", "layer-1", "m1", 0, 999, 1000);
        assert!((exp.traffic_fraction() - 1.0).abs() < 1e-9);
    }
}
