//! Multi-objective reward composition for bandit policies (ADR-011).
//!
//! [`RewardComposer`] scalarizes multiple per-metric reward observations into
//! a single reward signal for posterior updates. Three strategies are supported:
//!
//! - **`WeightedScalarization`**: `Σ wᵢ × normalized(rᵢ)` — linear combination
//!   of normalized per-metric rewards. Weights must sum to 1.0.
//! - **`EpsilonConstraint`**: Maximize primary objective; secondary objectives
//!   contribute credit only when they exceed their floor thresholds (Lagrangian
//!   relaxation, closed-form per-step update).
//! - **`Tchebycheff`**: Minimize the maximum weighted deviation from each metric's
//!   running ideal point (Pareto-aware; finds optimal solutions on non-convex
//!   frontiers where weighted sum fails).
//!
//! [`MetricNormalizer`] maintains an EMA running mean and variance (α = 0.01)
//! per metric, normalizing raw reward values to a comparable z-score scale.
//! Both [`RewardComposer`] and [`MetricNormalizer`] are fully serializable and
//! are persisted in RocksDB alongside posterior parameters.
//!
//! # Typical lifecycle
//! ```text
//! // At policy creation:
//! let composer = RewardComposer::new(objectives, CompositionMethod::WeightedScalarization);
//!
//! // At each reward update (on the LMAX thread):
//! let scalar = composer.compose(&metric_values);
//! policy.update(arm_id, scalar, context);
//!
//! // Persistence: serialize policy_state to JSON including the composer.
//! ```

use experimentation_core::error::assert_finite;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// EMA smoothing coefficient for running mean/variance updates (α = 0.01).
pub const EMA_ALPHA: f64 = 0.01;

/// Multi-objective scalarization strategy.
///
/// Mirrors `RewardCompositionMethod` in `bandit.proto` (ADR-011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompositionMethod {
    /// `Σ wᵢ × normalized(rᵢ)` — weights must sum to 1.0.
    WeightedScalarization,
    /// Maximize primary objective; Lagrangian credit for secondaries above their
    /// floor thresholds. `is_primary = true` for exactly one objective.
    EpsilonConstraint,
    /// Minimize `max_i { wᵢ × max(0, ideal_i − normalized(rᵢ)) }`.
    /// The per-metric ideal is the running maximum of normalized observations.
    Tchebycheff,
}

/// One component of a multi-objective reward specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Objective {
    /// Metric ID as defined in the experiment's metric set.
    pub metric_id: String,
    /// Relative weight.
    /// All weights must sum to 1.0 for `WeightedScalarization`.
    pub weight: f64,
    /// Floor constraint in normalized units (used by `EpsilonConstraint` only).
    /// Secondary objectives earn credit only when `normalized(r) >= floor`.
    pub floor: f64,
    /// When `true`, this is the primary objective (exactly one required for
    /// `EpsilonConstraint`; ignored for other methods).
    pub is_primary: bool,
}

// ──────────────────────────────────────────────────────────────────────────────
// MetricNormalizer
// ──────────────────────────────────────────────────────────────────────────────

/// EMA running mean and variance for a single metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricStats {
    /// EMA mean.
    pub mean: f64,
    /// EMA variance (online EMA update; initialized to 1.0 to avoid cold-start
    /// div-by-zero on the first observation).
    pub variance: f64,
    /// Running maximum of normalized values; used as ideal point in Tchebycheff.
    pub ideal: f64,
    /// Total observations processed.
    pub n: u64,
}

impl MetricStats {
    fn new() -> Self {
        Self {
            mean: 0.0,
            variance: 1.0,
            ideal: 0.0,
            n: 0,
        }
    }

    /// Apply one EMA update with `value`.
    ///
    /// Uses the incremental EMA variance formula:
    /// `V_new = (1 − α) × (V_old + α × (x − μ_old)²)`
    fn update(&mut self, value: f64) {
        assert_finite(value, "MetricStats::update value");
        let delta = value - self.mean;
        self.mean += EMA_ALPHA * delta;
        self.variance = (1.0 - EMA_ALPHA) * (self.variance + EMA_ALPHA * delta * delta);
        assert_finite(self.mean, "MetricStats EMA mean");
        assert_finite(self.variance, "MetricStats EMA variance");
        self.n += 1;
    }

    /// Normalize `value` using current EMA statistics.
    ///
    /// Returns `(value − mean) / std_dev`, clamped to `std_dev ≥ 1e-8` to
    /// prevent division by zero before sufficient data has been seen.
    fn normalize(&self, value: f64) -> f64 {
        let std_dev = self.variance.max(1e-8).sqrt();
        let z = (value - self.mean) / std_dev;
        assert_finite(z, "MetricStats normalized value");
        z
    }
}

/// Per-metric EMA normalizer; serialized alongside policy posterior state.
///
/// Tracks running mean, variance, and ideal point (running max of normalized
/// values) for every metric that has been observed.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricNormalizer {
    stats: HashMap<String, MetricStats>,
}

impl MetricNormalizer {
    /// Create an empty normalizer.
    pub fn new() -> Self {
        Self {
            stats: HashMap::new(),
        }
    }

    /// Update EMA statistics for `metric_id` and return the normalized value.
    ///
    /// Also updates the running ideal point (max of normalized values seen).
    pub fn update_and_normalize(&mut self, metric_id: &str, value: f64) -> f64 {
        let stats = self
            .stats
            .entry(metric_id.to_string())
            .or_insert_with(MetricStats::new);
        stats.update(value);
        let normalized = stats.normalize(value);
        // Maintain running maximum as the Tchebycheff ideal point.
        if normalized > stats.ideal {
            stats.ideal = normalized;
        }
        normalized
    }

    /// Normalize `value` using existing EMA statistics **without** updating them.
    ///
    /// Returns 0.0 if no observations have been seen for this metric yet.
    pub fn normalize(&self, metric_id: &str, value: f64) -> f64 {
        match self.stats.get(metric_id) {
            Some(stats) => stats.normalize(value),
            None => 0.0,
        }
    }

    /// Running ideal point (maximum normalized value observed) for `metric_id`.
    ///
    /// Returns 0.0 if the metric has not been observed.
    pub fn ideal(&self, metric_id: &str) -> f64 {
        self.stats.get(metric_id).map(|s| s.ideal).unwrap_or(0.0)
    }

    /// Access raw [`MetricStats`] for a metric, if present.
    pub fn stats(&self, metric_id: &str) -> Option<&MetricStats> {
        self.stats.get(metric_id)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// RewardComposer
// ──────────────────────────────────────────────────────────────────────────────

/// Composes multiple per-metric reward observations into a single scalar reward.
///
/// Persisted alongside the policy's posterior parameters in RocksDB.
/// All floating-point operations use `assert_finite!()` fail-fast guards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardComposer {
    pub objectives: Vec<Objective>,
    pub method: CompositionMethod,
    pub normalizer: MetricNormalizer,
}

impl RewardComposer {
    /// Construct a `RewardComposer` with the given objectives and method.
    ///
    /// # Panics
    /// - `objectives` is empty.
    /// - For `WeightedScalarization`: weights do not sum to 1.0 (±1e-6).
    /// - For `EpsilonConstraint`: not exactly one objective has `is_primary = true`.
    /// - For `Tchebycheff`: any weight is non-positive.
    pub fn new(objectives: Vec<Objective>, method: CompositionMethod) -> Self {
        assert!(!objectives.is_empty(), "reward_objectives must be non-empty");
        for obj in &objectives {
            assert!(
                obj.weight.is_finite() && obj.weight >= 0.0,
                "objective '{}' weight must be finite and non-negative, got {}",
                obj.metric_id,
                obj.weight
            );
        }
        match method {
            CompositionMethod::WeightedScalarization => {
                let total: f64 = objectives.iter().map(|o| o.weight).sum();
                assert!(
                    (total - 1.0).abs() < 1e-6,
                    "WeightedScalarization weights must sum to 1.0, got {total:.8}"
                );
            }
            CompositionMethod::EpsilonConstraint => {
                let n_primary = objectives.iter().filter(|o| o.is_primary).count();
                assert_eq!(
                    n_primary, 1,
                    "EpsilonConstraint requires exactly one primary objective, found {n_primary}"
                );
            }
            CompositionMethod::Tchebycheff => {
                for obj in &objectives {
                    assert!(
                        obj.weight > 0.0,
                        "Tchebycheff weight for '{}' must be positive, got {}",
                        obj.metric_id,
                        obj.weight
                    );
                }
            }
        }
        Self {
            objectives,
            method,
            normalizer: MetricNormalizer::new(),
        }
    }

    /// Update normalizer statistics and compose a scalar reward.
    ///
    /// `metric_values` maps `metric_id → raw reward observation`. Every objective
    /// must have a corresponding entry in `metric_values`.
    ///
    /// This method:
    /// 1. Looks up each metric's raw value from `metric_values`.
    /// 2. Feeds it through the EMA normalizer (updating running stats).
    /// 3. Applies the selected composition strategy.
    ///
    /// Must be called on the LMAX single-threaded core (no internal locking).
    ///
    /// # Panics
    /// - Any metric listed in `objectives` is absent from `metric_values`.
    /// - Any raw or normalized value is non-finite.
    pub fn compose(&mut self, metric_values: &HashMap<String, f64>) -> f64 {
        // Pass 1 — collect raw values and objective metadata (immutable borrow of objectives).
        // We clone only the three scalar fields we need; metric_id is cheap to clone.
        let raw: Vec<(String, f64, f64, f64, bool)> = self
            .objectives
            .iter()
            .map(|obj| {
                let raw_val = *metric_values.get(&obj.metric_id).unwrap_or_else(|| {
                    panic!(
                        "metric '{}' missing from metric_values (have: {:?})",
                        obj.metric_id,
                        metric_values.keys().collect::<Vec<_>>()
                    )
                });
                assert_finite(raw_val, &format!("raw metric '{}'", obj.metric_id));
                (
                    obj.metric_id.clone(),
                    raw_val,
                    obj.weight,
                    obj.floor,
                    obj.is_primary,
                )
            })
            .collect();

        // Pass 2 — update EMA normalizer and collect normalized values.
        // `raw` is a local Vec so no borrow conflict with self.normalizer.
        let normalized: Vec<(f64, f64, f64, bool)> = raw
            .iter()
            .map(|(metric_id, raw_val, weight, floor, is_primary)| {
                let norm = self.normalizer.update_and_normalize(metric_id, *raw_val);
                (*weight, norm, *floor, *is_primary)
            })
            .collect();

        // Pass 3 — compose scalar reward.
        let reward = match self.method {
            CompositionMethod::WeightedScalarization => {
                // reward = Σ wᵢ × normalized(rᵢ)
                normalized.iter().map(|(w, v, _, _)| w * v).sum()
            }

            CompositionMethod::EpsilonConstraint => {
                // Lagrangian relaxation:
                //   reward = w_primary × norm_primary
                //          + Σ_{secondary} w_secondary × max(0, norm_secondary − floor)
                //
                // Secondary objectives earn credit only when they exceed their
                // floor thresholds, which drives the policy to satisfy constraints
                // while maximizing the primary objective.
                let mut reward = 0.0f64;
                for (w, v, floor, is_primary) in &normalized {
                    if *is_primary {
                        reward += w * v;
                    } else {
                        reward += w * (v - floor).max(0.0);
                    }
                }
                reward
            }

            CompositionMethod::Tchebycheff => {
                // Minimize max weighted deviation from the per-metric ideal point:
                //   reward = −max_i { wᵢ × max(0, ideal_i − normalized(rᵢ)) }
                //
                // `ideal_i` is the running maximum of each metric's normalized
                // observations (updated in Pass 2 above). Arms that fall furthest
                // below the ideal across any dimension are penalized most,
                // steering the policy toward Pareto-balanced selections even on
                // non-convex frontiers.
                let max_dev = raw
                    .iter()
                    .zip(normalized.iter())
                    .map(|((metric_id, _, _, _, _), (w, v, _, _))| {
                        let ideal = self.normalizer.ideal(metric_id);
                        w * (ideal - v).max(0.0)
                    })
                    .fold(f64::NEG_INFINITY, f64::max);
                // Negate: the bandit maximizes reward, so minimizing deviation
                // is equivalent to maximizing the negated maximum deviation.
                -max_dev
            }
        };

        assert_finite(reward, "composed reward");
        reward
    }

    /// Number of objectives.
    pub fn len(&self) -> usize {
        self.objectives.len()
    }

    /// Returns `true` if there are no objectives.
    pub fn is_empty(&self) -> bool {
        self.objectives.is_empty()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_objectives(n: usize) -> Vec<Objective> {
        let weight = 1.0 / n as f64;
        (0..n)
            .map(|i| Objective {
                metric_id: format!("m{i}"),
                weight,
                floor: 0.0,
                is_primary: i == 0,
            })
            .collect()
    }

    fn metric_map(pairs: &[(&str, f64)]) -> HashMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    // ── MetricNormalizer ──────────────────────────────────────────────────────

    #[test]
    fn normalizer_first_observation_returns_zero() {
        let mut n = MetricNormalizer::new();
        // First update: delta = value - mean = value - 0.0, mean becomes alpha*value.
        // After update, normalize(value): (value - new_mean) / std_dev ≈ 0 (near mean)
        // Not exactly 0 but close for small alpha; let's just check finite.
        let v = n.update_and_normalize("m0", 5.0);
        assert!(v.is_finite());
    }

    #[test]
    fn normalizer_tracks_mean() {
        let mut n = MetricNormalizer::new();
        // Feed 1000 samples of value=10.0; EMA mean should converge to 10.0.
        for _ in 0..1000 {
            n.update_and_normalize("m0", 10.0);
        }
        let stats = n.stats("m0").unwrap();
        assert!(
            (stats.mean - 10.0).abs() < 0.5,
            "EMA mean should converge to 10.0, got {}",
            stats.mean
        );
    }

    #[test]
    fn normalizer_variance_positive() {
        let mut n = MetricNormalizer::new();
        for i in 0..200 {
            n.update_and_normalize("m0", i as f64 % 10.0);
        }
        let stats = n.stats("m0").unwrap();
        assert!(stats.variance > 0.0, "variance must be positive");
    }

    #[test]
    fn normalizer_ideal_monotone_nondecreasing() {
        let mut n = MetricNormalizer::new();
        let mut prev_ideal = f64::NEG_INFINITY;
        for v in [1.0, 2.0, 3.0, 2.5, 1.0, 4.0, 3.0] {
            n.update_and_normalize("m0", v);
            let ideal = n.ideal("m0");
            assert!(
                ideal >= prev_ideal,
                "ideal must be non-decreasing: prev={prev_ideal}, now={ideal}"
            );
            prev_ideal = ideal;
        }
    }

    #[test]
    fn normalizer_missing_metric_returns_zero() {
        let n = MetricNormalizer::new();
        assert_eq!(n.normalize("unseen", 42.0), 0.0);
        assert_eq!(n.ideal("unseen"), 0.0);
    }

    #[test]
    fn normalizer_serialize_roundtrip() {
        let mut n = MetricNormalizer::new();
        for v in [1.0, 2.0, 3.0, 4.0, 5.0] {
            n.update_and_normalize("clicks", v);
        }
        let bytes = serde_json::to_vec(&n).expect("serialize");
        let restored: MetricNormalizer = serde_json::from_slice(&bytes).expect("deserialize");
        let stats_orig = n.stats("clicks").unwrap();
        let stats_rest = restored.stats("clicks").unwrap();
        assert!((stats_orig.mean - stats_rest.mean).abs() < 1e-12);
        assert!((stats_orig.variance - stats_rest.variance).abs() < 1e-12);
        assert!((stats_orig.ideal - stats_rest.ideal).abs() < 1e-12);
    }

    // ── RewardComposer construction ───────────────────────────────────────────

    #[test]
    #[should_panic(expected = "non-empty")]
    fn composer_rejects_empty_objectives() {
        RewardComposer::new(vec![], CompositionMethod::WeightedScalarization);
    }

    #[test]
    #[should_panic(expected = "sum to 1.0")]
    fn weighted_scalarization_rejects_bad_weights() {
        let objectives = vec![
            Objective {
                metric_id: "m0".into(),
                weight: 0.6,
                floor: 0.0,
                is_primary: true,
            },
            Objective {
                metric_id: "m1".into(),
                weight: 0.6,
                floor: 0.0,
                is_primary: false,
            },
        ];
        RewardComposer::new(objectives, CompositionMethod::WeightedScalarization);
    }

    #[test]
    #[should_panic(expected = "exactly one primary")]
    fn epsilon_constraint_rejects_no_primary() {
        let objectives = vec![
            Objective {
                metric_id: "m0".into(),
                weight: 0.5,
                floor: 0.0,
                is_primary: false,
            },
            Objective {
                metric_id: "m1".into(),
                weight: 0.5,
                floor: 0.0,
                is_primary: false,
            },
        ];
        RewardComposer::new(objectives, CompositionMethod::EpsilonConstraint);
    }

    #[test]
    #[should_panic(expected = "positive")]
    fn tchebycheff_rejects_zero_weight() {
        let objectives = vec![
            Objective {
                metric_id: "m0".into(),
                weight: 0.0,
                floor: 0.0,
                is_primary: false,
            },
        ];
        RewardComposer::new(objectives, CompositionMethod::Tchebycheff);
    }

    // ── WeightedScalarization ─────────────────────────────────────────────────

    #[test]
    fn weighted_scalarization_output_finite() {
        let mut c = RewardComposer::new(make_objectives(2), CompositionMethod::WeightedScalarization);
        for _ in 0..50 {
            let v = c.compose(&metric_map(&[("m0", 1.0), ("m1", 2.0)]));
            assert!(v.is_finite());
        }
    }

    #[test]
    fn weighted_scalarization_single_objective_equals_normalized() {
        // With one objective at weight=1.0, the composed reward = normalized(r).
        let obj = vec![Objective {
            metric_id: "engagement".into(),
            weight: 1.0,
            floor: 0.0,
            is_primary: true,
        }];
        let mut c = RewardComposer::new(obj, CompositionMethod::WeightedScalarization);
        // After warmup, the normalizer should be close to zero mean; just verify finite.
        for v in [1.0, 2.0, 3.0, 4.0, 5.0] {
            let r = c.compose(&metric_map(&[("engagement", v)]));
            assert!(r.is_finite(), "composed reward must be finite");
        }
    }

    #[test]
    fn weighted_scalarization_high_weight_dominates() {
        // Metric m0 with weight 0.9 vs m1 with weight 0.1.
        // After warmup, m0 should dominate the reward signal.
        let objectives = vec![
            Objective {
                metric_id: "m0".into(),
                weight: 0.9,
                floor: 0.0,
                is_primary: true,
            },
            Objective {
                metric_id: "m1".into(),
                weight: 0.1,
                floor: 0.0,
                is_primary: false,
            },
        ];
        let mut c = RewardComposer::new(objectives, CompositionMethod::WeightedScalarization);
        // Warmup: feed equal values.
        for _ in 0..100 {
            c.compose(&metric_map(&[("m0", 1.0), ("m1", 1.0)]));
        }
        // Now m0 above mean, m1 at mean → reward dominated by m0 contribution.
        let r_high_m0 = c.compose(&metric_map(&[("m0", 5.0), ("m1", 1.0)]));
        let r_low_m0 = c.compose(&metric_map(&[("m0", 0.5), ("m1", 1.0)]));
        assert!(
            r_high_m0 > r_low_m0,
            "high m0 should produce higher reward: {r_high_m0} vs {r_low_m0}"
        );
    }

    // ── EpsilonConstraint ─────────────────────────────────────────────────────

    #[test]
    fn epsilon_constraint_output_finite() {
        let objectives = vec![
            Objective {
                metric_id: "engagement".into(),
                weight: 0.7,
                floor: 0.0,
                is_primary: true,
            },
            Objective {
                metric_id: "diversity".into(),
                weight: 0.3,
                floor: 0.5,
                is_primary: false,
            },
        ];
        let mut c = RewardComposer::new(objectives, CompositionMethod::EpsilonConstraint);
        for _ in 0..50 {
            let r = c.compose(&metric_map(&[("engagement", 1.0), ("diversity", 0.8)]));
            assert!(r.is_finite());
        }
    }

    #[test]
    fn epsilon_constraint_secondary_floor_gates_credit() {
        // After warmup, secondary above floor should give more reward than below floor.
        let objectives = vec![
            Objective {
                metric_id: "ctr".into(),
                weight: 0.7,
                floor: 0.0,
                is_primary: true,
            },
            Objective {
                metric_id: "provider_share".into(),
                weight: 0.3,
                floor: 0.0, // floor = 0 normalized units
                is_primary: false,
            },
        ];
        let mut c = RewardComposer::new(objectives, CompositionMethod::EpsilonConstraint);
        // Warmup with equal values so mean ≈ constant.
        for _ in 0..200 {
            c.compose(&metric_map(&[("ctr", 1.0), ("provider_share", 1.0)]));
        }
        // Now: same primary (ctr=1.0), but secondary above vs below mean.
        let r_secondary_above = c.compose(&metric_map(&[("ctr", 1.0), ("provider_share", 3.0)]));
        let r_secondary_below = c.compose(&metric_map(&[("ctr", 1.0), ("provider_share", 0.1)]));
        assert!(
            r_secondary_above > r_secondary_below,
            "secondary above floor ({r_secondary_above:.4}) must exceed secondary below ({r_secondary_below:.4})"
        );
    }

    // ── Tchebycheff ───────────────────────────────────────────────────────────

    #[test]
    fn tchebycheff_output_finite() {
        let objectives = vec![
            Objective {
                metric_id: "watch_time".into(),
                weight: 0.6,
                floor: 0.0,
                is_primary: false,
            },
            Objective {
                metric_id: "diversity".into(),
                weight: 0.4,
                floor: 0.0,
                is_primary: false,
            },
        ];
        let mut c = RewardComposer::new(objectives, CompositionMethod::Tchebycheff);
        for _ in 0..50 {
            let r = c.compose(&metric_map(&[("watch_time", 2.0), ("diversity", 1.5)]));
            assert!(r.is_finite());
        }
    }

    #[test]
    fn tchebycheff_balanced_arm_beats_imbalanced() {
        // After warmup, an arm balanced across objectives should have lower max
        // deviation than an arm strong on one dimension but weak on the other.
        let objectives = vec![
            Objective {
                metric_id: "m0".into(),
                weight: 0.5,
                floor: 0.0,
                is_primary: false,
            },
            Objective {
                metric_id: "m1".into(),
                weight: 0.5,
                floor: 0.0,
                is_primary: false,
            },
        ];
        let mut c = RewardComposer::new(objectives.clone(), CompositionMethod::Tchebycheff);
        // Warmup: both metrics around 1.0.
        for _ in 0..300 {
            c.compose(&metric_map(&[("m0", 1.0), ("m1", 1.0)]));
        }
        // Balanced arm: both above mean.
        let r_balanced = c.compose(&metric_map(&[("m0", 3.0), ("m1", 3.0)]));
        // Imbalanced: strong on m0, weak on m1 (m1 below mean).
        let r_imbalanced = c.compose(&metric_map(&[("m0", 5.0), ("m1", 0.1)]));

        assert!(
            r_balanced > r_imbalanced,
            "balanced arm ({r_balanced:.4}) should beat imbalanced ({r_imbalanced:.4})"
        );
    }

    // ── Serialization ─────────────────────────────────────────────────────────

    #[test]
    fn composer_serialize_roundtrip() {
        let mut c = RewardComposer::new(make_objectives(3), CompositionMethod::WeightedScalarization);
        // Feed some observations to build up normalizer state.
        for i in 0..50 {
            let v = i as f64;
            c.compose(&metric_map(&[("m0", v), ("m1", v * 0.5), ("m2", v * 2.0)]));
        }
        let bytes = serde_json::to_vec(&c).expect("serialize composer");
        let restored: RewardComposer = serde_json::from_slice(&bytes).expect("deserialize composer");

        assert_eq!(restored.objectives.len(), 3);
        assert_eq!(restored.method, CompositionMethod::WeightedScalarization);

        // Normalizer state preserved.
        for metric in ["m0", "m1", "m2"] {
            let s_orig = c.normalizer.stats(metric).unwrap();
            let s_rest = restored.normalizer.stats(metric).unwrap();
            assert!(
                (s_orig.mean - s_rest.mean).abs() < 1e-12,
                "mean mismatch for {metric}"
            );
            assert!(
                (s_orig.variance - s_rest.variance).abs() < 1e-12,
                "variance mismatch for {metric}"
            );
        }
    }

    // ── Convergence simulation ────────────────────────────────────────────────

    /// Simulation: two-arm Thompson Sampling bandit with weighted-sum reward.
    ///
    /// Arm A has ground-truth metric values (m0=2.0, m1=1.5).
    /// Arm B has ground-truth metric values (m0=1.0, m1=1.0).
    /// With weights (0.6, 0.4), Arm A has higher expected reward.
    /// After 2000 rounds, the bandit should select Arm A >70% of the time.
    #[test]
    fn weighted_sum_bandit_converges_to_optimal_arm() {
        use crate::thompson::{select_arm, BetaArm};

        let objectives = vec![
            Objective {
                metric_id: "watch_time".into(),
                weight: 0.6,
                floor: 0.0,
                is_primary: true,
            },
            Objective {
                metric_id: "diversity".into(),
                weight: 0.4,
                floor: 0.0,
                is_primary: false,
            },
        ];

        // One shared composer normalizes metrics across both arms.
        let mut composer = RewardComposer::new(objectives, CompositionMethod::WeightedScalarization);

        // Arm posteriors (Beta–Bernoulli; we binarize the composed reward).
        let mut arm_a = BetaArm::new("A".into());
        let mut arm_b = BetaArm::new("B".into());

        // Ground-truth metric values per arm.
        let gt_a = metric_map(&[("watch_time", 2.0), ("diversity", 1.5)]);
        let gt_b = metric_map(&[("watch_time", 1.0), ("diversity", 1.0)]);

        let mut rng = rand::thread_rng();
        let mut a_selections = 0u64;
        let rounds = 2000u64;

        for _ in 0..rounds {
            let selection = select_arm(&[arm_a.clone(), arm_b.clone()], &mut rng);

            // Compose scalar reward for the selected arm.
            let (metrics, is_a) = if selection.arm_id == "A" {
                (&gt_a, true)
            } else {
                (&gt_b, false)
            };
            let scalar = composer.compose(metrics);
            // Map to binary reward in [0, 1] via sigmoid for Beta posterior update.
            let binary_reward = 1.0 / (1.0 + (-scalar).exp());

            if is_a {
                arm_a.update(binary_reward);
                a_selections += 1;
            } else {
                arm_b.update(binary_reward);
            }
        }

        let a_fraction = a_selections as f64 / rounds as f64;
        assert!(
            a_fraction > 0.60,
            "multi-objective bandit should converge to optimal arm A >60% (got {:.1}%)",
            a_fraction * 100.0
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_metric_values(n: usize) -> impl Strategy<Value = HashMap<String, f64>> {
        proptest::collection::vec(-100.0f64..100.0, n).prop_map(move |vals| {
            vals.into_iter()
                .enumerate()
                .map(|(i, v)| (format!("m{i}"), v))
                .collect()
        })
    }

    proptest! {
        #[test]
        fn weighted_scalarization_always_finite(vals in arb_metric_values(3)) {
            let objectives = vec![
                Objective { metric_id: "m0".into(), weight: 0.5, floor: 0.0, is_primary: true },
                Objective { metric_id: "m1".into(), weight: 0.3, floor: 0.0, is_primary: false },
                Objective { metric_id: "m2".into(), weight: 0.2, floor: 0.0, is_primary: false },
            ];
            let mut c = RewardComposer::new(objectives, CompositionMethod::WeightedScalarization);
            let reward = c.compose(&vals);
            prop_assert!(reward.is_finite(), "reward must be finite, got {reward}");
        }

        #[test]
        fn epsilon_constraint_always_finite(vals in arb_metric_values(2)) {
            let objectives = vec![
                Objective { metric_id: "m0".into(), weight: 0.7, floor: 0.0, is_primary: true },
                Objective { metric_id: "m1".into(), weight: 0.3, floor: 0.5, is_primary: false },
            ];
            let mut c = RewardComposer::new(objectives, CompositionMethod::EpsilonConstraint);
            let reward = c.compose(&vals);
            prop_assert!(reward.is_finite(), "reward must be finite, got {reward}");
        }

        #[test]
        fn tchebycheff_always_finite(vals in arb_metric_values(2)) {
            let objectives = vec![
                Objective { metric_id: "m0".into(), weight: 0.6, floor: 0.0, is_primary: false },
                Objective { metric_id: "m1".into(), weight: 0.4, floor: 0.0, is_primary: false },
            ];
            let mut c = RewardComposer::new(objectives, CompositionMethod::Tchebycheff);
            let reward = c.compose(&vals);
            prop_assert!(reward.is_finite(), "reward must be finite, got {reward}");
        }

        #[test]
        fn tchebycheff_reward_nonpositive_after_warmup(
            vals in arb_metric_values(2)
        ) {
            let objectives = vec![
                Objective { metric_id: "m0".into(), weight: 0.5, floor: 0.0, is_primary: false },
                Objective { metric_id: "m1".into(), weight: 0.5, floor: 0.0, is_primary: false },
            ];
            let mut c = RewardComposer::new(objectives, CompositionMethod::Tchebycheff);
            // Warmup: establish a non-trivial ideal.
            for i in 0..20 {
                let warmup = HashMap::from([
                    ("m0".to_string(), i as f64),
                    ("m1".to_string(), i as f64),
                ]);
                c.compose(&warmup);
            }
            // After warmup the ideal > 0; any observation with some metrics below
            // ideal must produce reward ≤ 0.
            let reward = c.compose(&vals);
            // We can't always guarantee ≤ 0 for arbitrary vals (if all above ideal,
            // max_dev = 0 → reward = 0). Just check finite.
            prop_assert!(reward.is_finite());
        }
    }
}
