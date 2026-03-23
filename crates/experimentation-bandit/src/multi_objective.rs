//! Multi-objective reward composition (ADR-011).
//!
//! Composes multiple per-metric reward signals into a single scalar that the
//! bandit algorithm can update with. Three strategies:
//!
//! - **WeightedSum** — weighted average of normalised metric values.
//! - **EpsilonConstraint** — optimise primary objective (highest weight), apply
//!   Lagrangian penalty for each secondary objective that violates its slack bound.
//! - **Tchebycheff** — minimise the maximum weighted deviation from per-metric
//!   reference values; negated so higher = better.
//!
//! [`MetricNormalizer`] maintains EMA running mean / variance per metric
//! (α = 0.01) so cross-metric scale differences are handled automatically.
//!
//! # RocksDB persistence
//! The full [`RewardComposer`] state (objectives + method + normaliser) is
//! serialisable via `serde_json` and can be embedded in a `SnapshotEnvelope`
//! alongside the bandit posterior parameters.

use experimentation_core::error::assert_finite;
use std::collections::HashMap;

/// EMA decay for running mean / variance.
const EMA_ALPHA: f64 = 0.01;

/// Small regularisation constant added to variance before taking sqrt.
const VAR_EPS: f64 = 1e-8;

// ──────────────────────────────────────────────────────────────────────────────
// MetricStats
// ──────────────────────────────────────────────────────────────────────────────

/// EMA running mean and variance for a single metric.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetricStats {
    /// EMA mean.
    pub mean: f64,
    /// EMA variance.
    pub variance: f64,
    /// Total observations processed.
    pub n_obs: u64,
}

impl MetricStats {
    fn new() -> Self {
        // Neutral initialisation: mean = 0, variance = 1 (unit scale).
        Self {
            mean: 0.0,
            variance: 1.0,
            n_obs: 0,
        }
    }

    /// Incorporate one observation, updating EMA statistics in place.
    pub fn update(&mut self, value: f64) {
        assert_finite(value, "MetricStats::update value");
        // Welford-style EMA: delta uses old mean.
        let delta = value - self.mean;
        self.mean = (1.0 - EMA_ALPHA) * self.mean + EMA_ALPHA * value;
        self.variance = (1.0 - EMA_ALPHA) * self.variance + EMA_ALPHA * delta * delta;
        self.n_obs += 1;
        assert_finite(self.mean, "MetricStats mean after update");
        assert_finite(self.variance, "MetricStats variance after update");
    }

    /// Return z-score of `value` relative to current running statistics.
    pub fn normalize(&self, value: f64) -> f64 {
        let std = (self.variance + VAR_EPS).sqrt();
        (value - self.mean) / std
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// MetricNormalizer
// ──────────────────────────────────────────────────────────────────────────────

/// EMA running normaliser for an arbitrary set of named metrics.
///
/// Maintains independent [`MetricStats`] per metric key. α = 0.01.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct MetricNormalizer {
    pub stats: HashMap<String, MetricStats>,
}

impl MetricNormalizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the running stats for `metric` with `value`, then return its
    /// z-score.
    pub fn update_and_normalize(&mut self, metric: &str, value: f64) -> f64 {
        let stats = self
            .stats
            .entry(metric.to_string())
            .or_insert_with(MetricStats::new);
        stats.update(value);
        let z = stats.normalize(value);
        assert_finite(z, "MetricNormalizer z-score");
        z
    }

    /// Return the z-score of `value` for `metric` without updating stats.
    /// Returns `value` unchanged if no history exists yet.
    pub fn normalize_only(&self, metric: &str, value: f64) -> f64 {
        self.stats
            .get(metric)
            .map(|s| s.normalize(value))
            .unwrap_or(value)
    }

    /// Serialize normaliser state for RocksDB persistence.
    pub fn serialize(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("MetricNormalizer serialization should not fail")
    }

    /// Deserialize normaliser state from RocksDB snapshot bytes.
    pub fn deserialize(data: &[u8]) -> Self {
        serde_json::from_slice(data).expect("MetricNormalizer deserialization failed")
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// RewardObjective
// ──────────────────────────────────────────────────────────────────────────────

/// One metric participating in multi-objective reward composition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RewardObjective {
    /// Key in the observed `metric_values` map.
    pub metric_name: String,
    /// Non-negative weight (used by WeightedSum and Tchebycheff).
    pub weight: f64,
    /// Allowable slack for EpsilonConstraint: violation occurs when
    /// `normalised_value < -constraint_slack`.  `None` means unconstrained
    /// (the objective is treated as a secondary optimisation target without a
    /// hard bound).
    pub constraint_slack: Option<f64>,
    /// Reference value for Tchebycheff (normalised units, typically 0.0).
    pub reference_value: f64,
}

impl RewardObjective {
    /// Construct a simple weighted objective with no constraint.
    pub fn new(metric_name: String, weight: f64) -> Self {
        assert_finite(weight, "RewardObjective weight");
        assert!(weight >= 0.0, "RewardObjective weight must be non-negative");
        Self {
            metric_name,
            weight,
            constraint_slack: None,
            reference_value: 0.0,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// CompositionMethod
// ──────────────────────────────────────────────────────────────────────────────

/// Scalarisation strategy for multi-objective reward composition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CompositionMethod {
    /// Weighted average of normalised metric values.
    WeightedSum,
    /// Optimise primary objective (highest weight) with Lagrangian penalty for
    /// secondary objectives that violate their `constraint_slack` bounds.
    EpsilonConstraint,
    /// Tchebycheff scalarisation: reward = −max_i(w_i · |z_i − r_i|).
    /// Pareto-aware — avoids rewarding extreme trade-offs.
    Tchebycheff,
}

// ──────────────────────────────────────────────────────────────────────────────
// RewardComposer
// ──────────────────────────────────────────────────────────────────────────────

/// Composes per-metric observations into a scalar reward for bandit updates.
///
/// The `compose` method:
/// 1. Updates the running [`MetricNormalizer`] with every observed value.
/// 2. Applies the configured [`CompositionMethod`] to the z-scored values.
/// 3. Returns a real-valued scalar (not yet bounded to [0, 1] — callers that
///    feed Beta-Bernoulli bandits should apply [`sigmoid`]).
///
/// The full struct is serialisable: pass `serialize()` bytes to
/// `SnapshotEnvelope::reward_composer_state` for RocksDB persistence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RewardComposer {
    pub objectives: Vec<RewardObjective>,
    pub method: CompositionMethod,
    pub normalizer: MetricNormalizer,
}

impl RewardComposer {
    /// Create a new composer.
    ///
    /// # Panics
    /// Panics if `objectives` is empty or any weight is non-finite / negative.
    pub fn new(objectives: Vec<RewardObjective>, method: CompositionMethod) -> Self {
        assert!(
            !objectives.is_empty(),
            "RewardComposer requires at least one objective"
        );
        for obj in &objectives {
            assert_finite(obj.weight, "RewardObjective weight");
            assert!(
                obj.weight >= 0.0,
                "RewardObjective weight must be non-negative"
            );
        }
        Self {
            objectives,
            method,
            normalizer: MetricNormalizer::new(),
        }
    }

    /// Compose `metric_values` into a scalar reward, updating running statistics.
    ///
    /// Metrics not present in `metric_values` are silently skipped (partial
    /// observation is allowed).  Returns `0.0` if no objectives have matching
    /// keys.
    pub fn compose(&mut self, metric_values: &HashMap<String, f64>) -> f64 {
        // Phase 1: collect (objective_index, metric_name, raw_value) for
        // objectives that have an observation in this event.
        let observed: Vec<(usize, String, f64)> = self
            .objectives
            .iter()
            .enumerate()
            .filter_map(|(i, obj)| {
                metric_values
                    .get(&obj.metric_name)
                    .map(|&v| (i, obj.metric_name.clone(), v))
            })
            .collect();
        // self.objectives borrow released — `observed` owns cloned strings.

        if observed.is_empty() {
            return 0.0;
        }

        // Phase 2: update normaliser and collect z-scores (sequential borrows,
        // no overlap with objectives).
        let normalized: Vec<(usize, f64)> = observed
            .into_iter()
            .map(|(i, name, v)| {
                let z = self.normalizer.update_and_normalize(&name, v);
                (i, z)
            })
            .collect();
        // self.normalizer borrow released.

        // Phase 3: scalarise.
        let reward = match &self.method {
            CompositionMethod::WeightedSum => {
                let total_w: f64 = normalized.iter().map(|(i, _)| self.objectives[*i].weight).sum();
                if total_w == 0.0 {
                    return 0.0;
                }
                normalized
                    .iter()
                    .map(|(i, z)| self.objectives[*i].weight * z)
                    .sum::<f64>()
                    / total_w
            }

            CompositionMethod::EpsilonConstraint => {
                // Primary = objective with highest weight.
                let primary_idx = normalized
                    .iter()
                    .max_by(|(i, _), (j, _)| {
                        self.objectives[*i]
                            .weight
                            .partial_cmp(&self.objectives[*j].weight)
                            .unwrap()
                    })
                    .map(|(i, _)| *i)
                    .unwrap();

                let primary_z = normalized
                    .iter()
                    .find(|(i, _)| *i == primary_idx)
                    .map(|(_, z)| *z)
                    .unwrap_or(0.0);

                // Penalty per violated constraint (unit Lagrange multiplier).
                let penalty: f64 = normalized
                    .iter()
                    .filter(|(i, _)| *i != primary_idx)
                    .map(|(i, z)| {
                        if let Some(slack) = self.objectives[*i].constraint_slack {
                            // Violation = amount by which z falls below -slack.
                            (-slack - z).max(0.0)
                        } else {
                            0.0 // unconstrained objective
                        }
                    })
                    .sum();

                primary_z - penalty
            }

            CompositionMethod::Tchebycheff => {
                // Reward = −max_i(w_i · |z_i − r_i|).  Higher is better.
                let max_dev = normalized
                    .iter()
                    .map(|(i, z)| {
                        let w = self.objectives[*i].weight;
                        let r = self.objectives[*i].reference_value;
                        w * (z - r).abs()
                    })
                    .fold(f64::NEG_INFINITY, f64::max);
                -max_dev
            }
        };

        assert_finite(reward, "RewardComposer composed reward");
        // Clamp to ±10 σ for numerical safety before passing to bandit.
        reward.clamp(-10.0, 10.0)
    }

    /// Serialize composer state (objectives + method + normaliser) for RocksDB.
    pub fn serialize(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("RewardComposer serialization should not fail")
    }

    /// Deserialize composer state from RocksDB snapshot bytes.
    pub fn deserialize(data: &[u8]) -> Self {
        serde_json::from_slice(data).expect("RewardComposer deserialization failed")
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Utilities
// ──────────────────────────────────────────────────────────────────────────────

/// Map a real-valued composed reward to (0, 1) via the logistic function.
///
/// Required when feeding composed rewards into Beta-Bernoulli (Thompson
/// Sampling) bandits that require rewards in [0, 1].
#[inline]
pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thompson::{select_arm, BetaArm};
    use rand::Rng;

    fn make_objectives() -> Vec<RewardObjective> {
        vec![
            RewardObjective {
                metric_name: "engagement".into(),
                weight: 0.7,
                constraint_slack: None,
                reference_value: 0.0,
            },
            RewardObjective {
                metric_name: "quality".into(),
                weight: 0.3,
                constraint_slack: None,
                reference_value: 0.0,
            },
        ]
    }

    // ── MetricStats ──────────────────────────────────────────────────────────

    #[test]
    fn test_metric_stats_constant_sequence() {
        let mut stats = MetricStats::new();
        for _ in 0..500 {
            stats.update(1.0);
        }
        // Mean should converge to 1.0, variance to ~0 (decays at rate 0.99^t).
        // After 500 updates: var ≈ 0.99^500 ≈ 0.007, but initial value is 1.0
        // and EMA decay is slow; realistic bound is < 0.05.
        assert!((stats.mean - 1.0).abs() < 0.05, "mean={}", stats.mean);
        assert!(stats.variance < 0.05, "variance={}", stats.variance);
    }

    #[test]
    fn test_metric_stats_normalization_direction() {
        let mut stats = MetricStats::new();
        // Warm up with values near 0.5.
        for _ in 0..200 {
            stats.update(0.5);
        }
        // A value above the mean should have a positive z-score.
        let z_high = stats.normalize(0.9);
        let z_low = stats.normalize(0.1);
        assert!(z_high > 0.0, "z_high={z_high}");
        assert!(z_low < 0.0, "z_low={z_low}");
    }

    // ── MetricNormalizer ─────────────────────────────────────────────────────

    #[test]
    fn test_normalizer_serialize_roundtrip() {
        let mut n = MetricNormalizer::new();
        for v in [0.1, 0.5, 0.8, 0.3] {
            n.update_and_normalize("engagement", v);
        }
        let bytes = n.serialize();
        let restored = MetricNormalizer::deserialize(&bytes);
        let orig_stats = &n.stats["engagement"];
        let rest_stats = &restored.stats["engagement"];
        assert_eq!(orig_stats.n_obs, rest_stats.n_obs);
        assert!((orig_stats.mean - rest_stats.mean).abs() < 1e-12);
    }

    // ── WeightedSum ──────────────────────────────────────────────────────────

    #[test]
    fn test_weighted_sum_single_metric() {
        let objs = vec![RewardObjective::new("eng".into(), 1.0)];
        let mut composer = RewardComposer::new(objs, CompositionMethod::WeightedSum);

        // Warm up so normaliser has a stable mean.
        let mut metrics = HashMap::new();
        for _ in 0..100 {
            metrics.insert("eng".into(), 0.5_f64);
            composer.compose(&metrics);
        }
        // Above-mean observation should yield positive composed reward.
        metrics.insert("eng".into(), 0.9);
        let r_high = composer.compose(&metrics);

        metrics.insert("eng".into(), 0.1);
        let r_low = composer.compose(&metrics);

        assert!(r_high > r_low, "r_high={r_high} r_low={r_low}");
    }

    #[test]
    fn test_weighted_sum_missing_metric_returns_zero() {
        let objs = make_objectives();
        let mut composer = RewardComposer::new(objs, CompositionMethod::WeightedSum);
        let metrics = HashMap::new(); // No matching keys.
        assert_eq!(composer.compose(&metrics), 0.0);
    }

    // ── EpsilonConstraint ────────────────────────────────────────────────────

    #[test]
    fn test_epsilon_constraint_penalty_applied() {
        let objs = vec![
            RewardObjective {
                metric_name: "primary".into(),
                weight: 1.0,
                constraint_slack: None,
                reference_value: 0.0,
            },
            RewardObjective {
                metric_name: "secondary".into(),
                weight: 0.5,
                constraint_slack: Some(0.5), // Must stay ≥ −0.5 σ
                reference_value: 0.0,
            },
        ];
        let mut composer = RewardComposer::new(objs, CompositionMethod::EpsilonConstraint);

        // Warm up normaliser with mid-range values.
        for _ in 0..200 {
            let mut m = HashMap::new();
            m.insert("primary".into(), 0.5_f64);
            m.insert("secondary".into(), 0.5_f64);
            composer.compose(&m);
        }

        // Unconstrained call: secondary satisfies constraint.
        let mut good = HashMap::new();
        good.insert("primary".into(), 0.9_f64);
        good.insert("secondary".into(), 0.9_f64);
        let r_good = composer.compose(&good);

        // Constrained call: secondary violates constraint (very low value).
        let mut bad = HashMap::new();
        bad.insert("primary".into(), 0.9_f64);
        bad.insert("secondary".into(), 0.01_f64); // Far below mean → penalty
        let r_bad = composer.compose(&bad);

        assert!(
            r_good > r_bad,
            "Constraint violation should reduce reward: r_good={r_good} r_bad={r_bad}"
        );
    }

    // ── Tchebycheff ──────────────────────────────────────────────────────────

    #[test]
    fn test_tchebycheff_balanced_preferred() {
        let objs = make_objectives();
        let mut composer_a = RewardComposer::new(objs.clone(), CompositionMethod::Tchebycheff);
        let mut composer_b = RewardComposer::new(objs, CompositionMethod::Tchebycheff);

        // Warm up both with balanced values.
        for _ in 0..200 {
            let mut m = HashMap::new();
            m.insert("engagement".into(), 0.5_f64);
            m.insert("quality".into(), 0.5_f64);
            composer_a.compose(&m);
            composer_b.compose(&m);
        }

        // Balanced outcome: both metrics near mean.
        let mut balanced = HashMap::new();
        balanced.insert("engagement".into(), 0.55_f64);
        balanced.insert("quality".into(), 0.55_f64);
        let r_balanced = composer_a.compose(&balanced);

        // Extreme outcome: high engagement, low quality.
        let mut extreme = HashMap::new();
        extreme.insert("engagement".into(), 0.99_f64);
        extreme.insert("quality".into(), 0.01_f64);
        let r_extreme = composer_b.compose(&extreme);

        // Tchebycheff should penalise extreme trade-offs.
        assert!(
            r_balanced > r_extreme,
            "Balanced={r_balanced} should beat Extreme={r_extreme}"
        );
    }

    // ── Serialize / Deserialize ──────────────────────────────────────────────

    #[test]
    fn test_composer_serialize_roundtrip() {
        let objs = make_objectives();
        let mut composer = RewardComposer::new(objs, CompositionMethod::WeightedSum);

        // Update with some observations.
        let mut m = HashMap::new();
        m.insert("engagement".into(), 0.7_f64);
        m.insert("quality".into(), 0.4_f64);
        for _ in 0..50 {
            composer.compose(&m);
        }

        let bytes = composer.serialize();
        let restored = RewardComposer::deserialize(&bytes);

        // Normaliser state preserved.
        let orig_n = &composer.normalizer.stats["engagement"];
        let rest_n = &restored.normalizer.stats["engagement"];
        assert_eq!(orig_n.n_obs, rest_n.n_obs);
        assert!((orig_n.mean - rest_n.mean).abs() < 1e-12);
    }

    // ── Sigmoid ──────────────────────────────────────────────────────────────

    #[test]
    fn test_sigmoid_properties() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-12);
        assert!(sigmoid(10.0) > 0.99);
        assert!(sigmoid(-10.0) < 0.01);
        assert!(sigmoid(1.0) > sigmoid(-1.0));
    }

    // ── Convergence simulation ───────────────────────────────────────────────

    /// Verify that a multi-objective bandit converges toward the arm with the
    /// higher weighted-sum reward within 1 000 rounds.
    ///
    /// Ground truth:
    ///   arm_a: E[engagement] = 0.80, E[quality] = 0.60
    ///   arm_b: E[engagement] = 0.30, E[quality] = 0.70
    ///
    /// Weighted sum (w_eng=0.7, w_qual=0.3):
    ///   arm_a expected: 0.74  >  arm_b expected: 0.42  → arm_a should win.
    #[test]
    fn test_weighted_sum_convergence() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let objs = make_objectives();
        let mut composer = RewardComposer::new(objs, CompositionMethod::WeightedSum);

        let mut arms = vec![
            BetaArm::new("arm_a".into()),
            BetaArm::new("arm_b".into()),
        ];

        const N_ROUNDS: usize = 1_000;
        // Track arm_a selection rate over the final 200 rounds.
        let mut arm_a_late_count = 0usize;
        const LATE_START: usize = 800;

        for round in 0..N_ROUNDS {
            // Select arm via Thompson Sampling.
            let selection = select_arm(&arms, &mut rng);

            // Sample true metric values from Bernoulli distributions.
            let (engagement, quality) = if selection.arm_id == "arm_a" {
                (
                    if rng.gen::<f64>() < 0.80 { 1.0 } else { 0.0 },
                    if rng.gen::<f64>() < 0.60 { 1.0 } else { 0.0 },
                )
            } else {
                (
                    if rng.gen::<f64>() < 0.30 { 1.0 } else { 0.0 },
                    if rng.gen::<f64>() < 0.70 { 1.0 } else { 0.0 },
                )
            };

            // Compose into a scalar and map to (0, 1) for Beta-Bernoulli.
            let mut metrics = HashMap::new();
            metrics.insert("engagement".into(), engagement);
            metrics.insert("quality".into(), quality);
            let composed = composer.compose(&metrics);
            let reward = sigmoid(composed);

            // Update the selected arm's posterior.
            let arm = arms
                .iter_mut()
                .find(|a| a.arm_id == selection.arm_id)
                .unwrap();
            arm.update(reward);

            if round >= LATE_START && selection.arm_id == "arm_a" {
                arm_a_late_count += 1;
            }
        }

        let arm_a_late_rate = arm_a_late_count as f64 / (N_ROUNDS - LATE_START) as f64;
        assert!(
            arm_a_late_rate > 0.60,
            "Expected arm_a late-phase selection rate > 60%, got {arm_a_late_rate:.2}"
        );
    }
}
