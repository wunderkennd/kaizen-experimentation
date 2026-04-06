//! LinUCB with Sherman-Morrison rank-1 updates (Li et al., 2010).
//!
//! Per-arm linear upper confidence bound bandit. Maintains `A_inv` (inverse of
//! the design matrix) directly and updates it via Sherman-Morrison, achieving
//! O(d²) per update instead of O(d³) for full inversion.

use crate::policy::Policy;
use crate::ArmSelection;
use experimentation_core::error::assert_finite;
use nalgebra::{DMatrix, DVector};
use std::collections::HashMap;

/// State for a single LinUCB arm.
#[derive(Debug, Clone)]
pub struct LinUcbArm {
    pub arm_id: String,
    /// Inverse of the design matrix A. Initialized to I_d.
    pub a_inv: DMatrix<f64>,
    /// Reward-weighted feature accumulator. b = sum(reward_t * x_t).
    pub b: DVector<f64>,
    /// Number of times this arm has been pulled.
    pub n_pulls: u64,
}

impl LinUcbArm {
    /// Create a new arm with identity A_inv and zero b vector.
    pub fn new(arm_id: String, dim: usize) -> Self {
        Self {
            arm_id,
            a_inv: DMatrix::identity(dim, dim),
            b: DVector::zeros(dim),
            n_pulls: 0,
        }
    }

    /// Compute the UCB score for this arm given context vector x.
    ///
    /// UCB_a = θ_a^T · x + α · sqrt(x^T · A_inv_a · x)
    /// where θ_a = A_inv_a · b_a
    pub fn ucb_score(&self, x: &DVector<f64>, alpha: f64) -> f64 {
        let theta = &self.a_inv * &self.b;
        let exploitation = theta.dot(x);
        assert_finite(exploitation, "LinUCB exploitation term");

        let exploration_var = x.dot(&(&self.a_inv * x));
        assert_finite(exploration_var, "LinUCB exploration variance");

        // Numerical safety: exploration_var should be non-negative but
        // floating-point can produce tiny negatives.
        let exploration = alpha * exploration_var.max(0.0).sqrt();
        assert_finite(exploration, "LinUCB exploration term");

        exploitation + exploration
    }

    /// Sherman-Morrison rank-1 update: A_inv <- A_inv - (A_inv * x * x^T * A_inv) / (1 + x^T * A_inv * x)
    ///
    /// Also updates b += reward * x.
    pub fn update(&mut self, x: &DVector<f64>, reward: f64) {
        assert_finite(reward, "LinUCB reward");

        let a_inv_x = &self.a_inv * x;
        let denominator = 1.0 + x.dot(&a_inv_x);
        assert_finite(denominator, "LinUCB Sherman-Morrison denominator");

        if denominator.abs() < 1e-10 {
            tracing::warn!(
                arm_id = %self.arm_id,
                denominator,
                "Sherman-Morrison denominator near zero, skipping A_inv update"
            );
        } else {
            // A_inv -= (A_inv * x * x^T * A_inv) / denominator
            let numerator = &a_inv_x * a_inv_x.transpose();
            self.a_inv -= numerator / denominator;

            // Single O(1) finiteness check via Frobenius norm instead of
            // iterating over all d² elements. The norm is NaN/Inf if any
            // element is non-finite.
            let norm = self.a_inv.norm();
            assert_finite(norm, "LinUCB A_inv Frobenius norm after Sherman-Morrison");
        }

        self.b += reward * x;
        let b_norm = self.b.norm();
        assert_finite(b_norm, "LinUCB b vector norm after update");

        self.n_pulls += 1;
    }
}

/// Serializable representation of a LinUCB arm for snapshots.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SerializableArm {
    arm_id: String,
    /// Flattened column-major A_inv matrix.
    a_inv_data: Vec<f64>,
    a_inv_rows: usize,
    a_inv_cols: usize,
    /// b vector elements.
    b_data: Vec<f64>,
    n_pulls: u64,
}

impl From<&LinUcbArm> for SerializableArm {
    fn from(arm: &LinUcbArm) -> Self {
        Self {
            arm_id: arm.arm_id.clone(),
            a_inv_data: arm.a_inv.as_slice().to_vec(),
            a_inv_rows: arm.a_inv.nrows(),
            a_inv_cols: arm.a_inv.ncols(),
            b_data: arm.b.as_slice().to_vec(),
            n_pulls: arm.n_pulls,
        }
    }
}

impl SerializableArm {
    fn into_arm(self) -> LinUcbArm {
        LinUcbArm {
            arm_id: self.arm_id,
            a_inv: DMatrix::from_vec(self.a_inv_rows, self.a_inv_cols, self.a_inv_data),
            b: DVector::from_vec(self.b_data),
            n_pulls: self.n_pulls,
        }
    }
}

/// Serializable representation of LinUcbPolicy state for snapshots.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PolicyState {
    experiment_id: String,
    arms: Vec<SerializableArm>,
    feature_keys: Vec<String>,
    alpha: f64,
    min_exploration_fraction: f64,
    total_rewards: u64,
}

/// LinUCB contextual bandit policy.
///
/// Uses per-arm linear models with upper confidence bounds. Requires context
/// features on every `select_arm` call (panics on `None`).
#[derive(Debug, Clone)]
pub struct LinUcbPolicy {
    experiment_id: String,
    arms: Vec<LinUcbArm>,
    /// Ordered list of feature keys for stable HashMap → DVector conversion.
    feature_keys: Vec<String>,
    /// Exploration parameter α — higher values explore more.
    alpha: f64,
    /// Minimum probability for any arm (ensures exploration floor).
    min_exploration_fraction: f64,
    total_rewards: u64,
}

impl LinUcbPolicy {
    /// Create a new LinUCB policy.
    ///
    /// # Arguments
    /// * `experiment_id` — Unique experiment identifier
    /// * `arm_ids` — Arm identifiers
    /// * `feature_keys` — Ordered list of context feature names
    /// * `alpha` — Exploration parameter (typical: 0.1–2.0)
    /// * `min_exploration_fraction` — Minimum probability per arm (e.g., 0.01)
    pub fn new(
        experiment_id: String,
        arm_ids: Vec<String>,
        feature_keys: Vec<String>,
        alpha: f64,
        min_exploration_fraction: f64,
    ) -> Self {
        assert!(!arm_ids.is_empty(), "must have at least one arm");
        assert!(!feature_keys.is_empty(), "must have at least one feature");
        assert!(alpha > 0.0, "alpha must be positive");
        assert!(
            (0.0..1.0).contains(&min_exploration_fraction),
            "min_exploration_fraction must be in [0, 1)"
        );
        let k = arm_ids.len() as f64;
        assert!(
            k * min_exploration_fraction <= 1.0,
            "K * min_exploration_fraction must not exceed 1.0"
        );

        let dim = feature_keys.len();
        let arms = arm_ids
            .into_iter()
            .map(|id| LinUcbArm::new(id, dim))
            .collect();

        Self {
            experiment_id,
            arms,
            feature_keys,
            alpha,
            min_exploration_fraction,
            total_rewards: 0,
        }
    }

    /// Get experiment ID.
    pub fn experiment_id(&self) -> &str {
        &self.experiment_id
    }

    /// Get the feature keys.
    pub fn feature_keys(&self) -> &[String] {
        &self.feature_keys
    }

    /// Get the arms.
    pub fn arms(&self) -> &[LinUcbArm] {
        &self.arms
    }

    /// Compute the predicted reward for a given arm and context.
    ///
    /// Returns θ_a^T · x where θ_a = A_inv_a · b_a.
    /// This is the exploitation component (without exploration bonus).
    pub fn predicted_reward(&self, arm_id: &str, context: &HashMap<String, f64>) -> f64 {
        let x = self.context_to_vector(context);
        let arm = self
            .arms
            .iter()
            .find(|a| a.arm_id == arm_id)
            .unwrap_or_else(|| panic!("unknown arm_id: {arm_id}"));
        let theta = &arm.a_inv * &arm.b;
        let reward = theta.dot(&x);
        assert_finite(reward, "LinUCB predicted reward");
        reward
    }

    /// Convert a context HashMap to a DVector using the stable feature_keys ordering.
    /// Missing keys default to 0.0.
    fn context_to_vector(&self, context: &HashMap<String, f64>) -> DVector<f64> {
        let mut data = Vec::with_capacity(self.feature_keys.len());
        for key in &self.feature_keys {
            let val = context.get(key).copied().unwrap_or(0.0);
            if !val.is_finite() {
                // Only allocate the format string on the error path.
                assert_finite(val, &format!("context feature '{key}'"));
            }
            data.push(val);
        }
        DVector::from_vec(data)
    }

    /// Compute arm probabilities from UCB scores.
    ///
    /// The arm with the highest UCB gets `min_expl + (1 - K * min_expl)`,
    /// all others get `min_expl` each. Probabilities sum to 1.0.
    fn compute_probabilities(&self, ucb_scores: &[(usize, f64)]) -> (usize, HashMap<String, f64>) {
        let best_idx = ucb_scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap()
            .0;

        let k = self.arms.len() as f64;
        let min_expl = self.min_exploration_fraction;
        let best_prob = min_expl + (1.0 - k * min_expl);

        let probs: HashMap<String, f64> = self
            .arms
            .iter()
            .enumerate()
            .map(|(i, arm)| {
                let p = if i == best_idx { best_prob } else { min_expl };
                (arm.arm_id.clone(), p)
            })
            .collect();

        (best_idx, probs)
    }
}

impl Policy for LinUcbPolicy {
    fn select_arm(&self, context: Option<&HashMap<String, f64>>) -> ArmSelection {
        let context = context.expect("LinUCB requires context features (got None)");
        let x = self.context_to_vector(context);

        let ucb_scores: Vec<(usize, f64)> = self
            .arms
            .iter()
            .enumerate()
            .map(|(i, arm)| (i, arm.ucb_score(&x, self.alpha)))
            .collect();

        let (best_idx, all_arm_probabilities) = self.compute_probabilities(&ucb_scores);

        let arm_id = self.arms[best_idx].arm_id.clone();
        let assignment_probability = all_arm_probabilities[&arm_id];

        ArmSelection {
            arm_id,
            assignment_probability,
            all_arm_probabilities,
            is_uniform_random: false,
        }
    }

    fn update(&mut self, arm_id: &str, reward: f64, context: Option<&HashMap<String, f64>>) {
        let context = context.expect("LinUCB requires context features for update (got None)");
        let x = self.context_to_vector(context);

        let arm = self
            .arms
            .iter_mut()
            .find(|a| a.arm_id == arm_id)
            .unwrap_or_else(|| panic!("unknown arm_id: {arm_id}"));

        arm.update(&x, reward);
        self.total_rewards += 1;
    }

    fn serialize(&self) -> Vec<u8> {
        let state = PolicyState {
            experiment_id: self.experiment_id.clone(),
            arms: self.arms.iter().map(SerializableArm::from).collect(),
            feature_keys: self.feature_keys.clone(),
            alpha: self.alpha,
            min_exploration_fraction: self.min_exploration_fraction,
            total_rewards: self.total_rewards,
        };
        serde_json::to_vec(&state).expect("LinUCB policy state serialization should not fail")
    }

    fn deserialize(data: &[u8]) -> Self {
        let state: PolicyState =
            serde_json::from_slice(data).expect("LinUCB policy state deserialization failed");
        Self {
            experiment_id: state.experiment_id,
            arms: state.arms.into_iter().map(|a| a.into_arm()).collect(),
            feature_keys: state.feature_keys,
            alpha: state.alpha,
            min_exploration_fraction: state.min_exploration_fraction,
            total_rewards: state.total_rewards,
        }
    }

    fn total_rewards(&self) -> u64 {
        self.total_rewards
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_context(keys: &[&str], values: &[f64]) -> HashMap<String, f64> {
        keys.iter()
            .zip(values.iter())
            .map(|(k, v)| (k.to_string(), *v))
            .collect()
    }

    fn make_policy(n_arms: usize, n_features: usize) -> LinUcbPolicy {
        let arm_ids: Vec<String> = (0..n_arms).map(|i| format!("arm_{i}")).collect();
        let feature_keys: Vec<String> = (0..n_features).map(|i| format!("f{i}")).collect();
        LinUcbPolicy::new("test-exp".into(), arm_ids, feature_keys, 1.0, 0.05)
    }

    #[test]
    fn test_context_vector_construction() {
        let policy = make_policy(2, 3);
        let ctx = sample_context(&["f0", "f1", "f2"], &[1.0, 2.0, 3.0]);
        let v = policy.context_to_vector(&ctx);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], 1.0);
        assert_eq!(v[1], 2.0);
        assert_eq!(v[2], 3.0);
    }

    #[test]
    fn test_context_vector_missing_keys_default_zero() {
        let policy = make_policy(2, 3);
        let ctx = sample_context(&["f0"], &[5.0]);
        let v = policy.context_to_vector(&ctx);
        assert_eq!(v[0], 5.0);
        assert_eq!(v[1], 0.0);
        assert_eq!(v[2], 0.0);
    }

    #[test]
    fn test_sherman_morrison_matches_naive() {
        let dim = 4;
        let mut arm = LinUcbArm::new("a".into(), dim);

        // Apply several updates and compare against naive inversion.
        let vectors: Vec<DVector<f64>> = vec![
            DVector::from_vec(vec![1.0, 0.5, 0.2, 0.1]),
            DVector::from_vec(vec![0.3, 1.0, 0.7, 0.4]),
            DVector::from_vec(vec![0.1, 0.2, 1.0, 0.8]),
        ];

        // Build A = I + sum(x_i * x_i^T) via direct accumulation.
        let mut a_direct = DMatrix::identity(dim, dim);

        for x in &vectors {
            arm.update(x, 1.0);
            a_direct += x * x.transpose();
        }

        // Naive inverse.
        let a_inv_naive = a_direct.try_inverse().expect("A should be invertible");

        // Compare element-wise.
        let diff = (&arm.a_inv - &a_inv_naive).norm();
        assert!(
            diff < 1e-10,
            "Sherman-Morrison A_inv diverged from naive: diff = {diff}"
        );
    }

    #[test]
    #[should_panic(expected = "LinUCB requires context features")]
    fn test_select_arm_requires_context() {
        let policy = make_policy(2, 3);
        policy.select_arm(None);
    }

    #[test]
    fn test_ucb_exploration_bonus() {
        // With high alpha, UCB scores should be higher (wider exploration).
        let feature_keys = vec!["f0".into()];
        let arm_ids = vec!["a".into(), "b".into()];

        let low_alpha =
            LinUcbPolicy::new("exp".into(), arm_ids.clone(), feature_keys.clone(), 0.1, 0.0);
        let high_alpha = LinUcbPolicy::new("exp".into(), arm_ids, feature_keys, 10.0, 0.0);

        let ctx = sample_context(&["f0"], &[1.0]);
        let x = low_alpha.context_to_vector(&ctx);

        let score_low = low_alpha.arms[0].ucb_score(&x, 0.1);
        let score_high = high_alpha.arms[0].ucb_score(&x, 10.0);

        // Both start with zero exploitation (no data), so exploration dominates.
        assert!(
            score_high > score_low,
            "high alpha ({score_high}) should produce higher UCB than low alpha ({score_low})"
        );
    }

    #[test]
    fn test_convergence_known_linear_model() {
        // Ground truth: arm 0 has θ* = [1, 2], arm 1 has θ* = [0.5, 0.5].
        // Context always [1, 1] → arm 0 expected reward = 3, arm 1 = 1.
        let feature_keys = vec!["f0".into(), "f1".into()];
        let arm_ids = vec!["optimal".into(), "suboptimal".into()];
        let mut policy = LinUcbPolicy::new("exp".into(), arm_ids, feature_keys, 1.0, 0.01);

        let ctx = sample_context(&["f0", "f1"], &[1.0, 1.0]);

        let theta_optimal = DVector::from_vec(vec![1.0, 2.0]);
        let theta_suboptimal = DVector::from_vec(vec![0.5, 0.5]);
        let x = policy.context_to_vector(&ctx);

        let mut optimal_selections = 0u64;
        let rounds = 2000;

        for _ in 0..rounds {
            let selection = policy.select_arm(Some(&ctx));

            // Simulate reward from the true model (no noise for determinism).
            let reward = if selection.arm_id == "optimal" {
                theta_optimal.dot(&x)
            } else {
                theta_suboptimal.dot(&x)
            };

            if selection.arm_id == "optimal" {
                optimal_selections += 1;
            }

            policy.update(&selection.arm_id, reward, Some(&ctx));
        }

        let optimal_fraction = optimal_selections as f64 / rounds as f64;
        assert!(
            optimal_fraction > 0.70,
            "LinUCB should converge to optimal arm >70% of the time, got {:.1}%",
            optimal_fraction * 100.0
        );
    }

    #[test]
    fn test_min_exploration_enforcement() {
        let policy = make_policy(3, 2);
        let ctx = sample_context(&["f0", "f1"], &[1.0, 0.5]);

        let selection = policy.select_arm(Some(&ctx));
        for (_arm_id, prob) in &selection.all_arm_probabilities {
            assert!(
                *prob >= policy.min_exploration_fraction - 1e-12,
                "arm probability {prob} below min_exploration_fraction {}",
                policy.min_exploration_fraction
            );
        }
    }

    #[test]
    fn test_all_probabilities_sum_to_one() {
        let mut policy = make_policy(4, 3);
        let ctx = sample_context(&["f0", "f1", "f2"], &[0.5, 1.0, 0.3]);

        // Test before any updates.
        let selection = policy.select_arm(Some(&ctx));
        let sum: f64 = selection.all_arm_probabilities.values().sum();
        assert!(
            (sum - 1.0).abs() < 1e-10,
            "probabilities should sum to 1.0, got {sum}"
        );

        // Test after some updates.
        for _ in 0..20 {
            let sel = policy.select_arm(Some(&ctx));
            policy.update(&sel.arm_id, 1.0, Some(&ctx));
        }
        let selection = policy.select_arm(Some(&ctx));
        let sum: f64 = selection.all_arm_probabilities.values().sum();
        assert!(
            (sum - 1.0).abs() < 1e-10,
            "probabilities should sum to 1.0 after updates, got {sum}"
        );
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let mut policy = make_policy(2, 3);
        let ctx = sample_context(&["f0", "f1", "f2"], &[1.0, 0.5, 0.2]);

        // Apply some updates.
        for _ in 0..10 {
            policy.update("arm_0", 1.0, Some(&ctx));
            policy.update("arm_1", 0.5, Some(&ctx));
        }

        let data = policy.serialize();
        let restored = LinUcbPolicy::deserialize(&data);

        assert_eq!(restored.experiment_id(), "test-exp");
        assert_eq!(restored.total_rewards(), 20);
        assert_eq!(restored.arms.len(), 2);
        assert_eq!(restored.feature_keys, policy.feature_keys);
        assert!((restored.alpha - policy.alpha).abs() < 1e-12);

        // Verify matrices match.
        for (orig, rest) in policy.arms.iter().zip(restored.arms.iter()) {
            let diff = (&orig.a_inv - &rest.a_inv).norm();
            assert!(diff < 1e-12, "A_inv mismatch after roundtrip: {diff}");
            let b_diff = (&orig.b - &rest.b).norm();
            assert!(b_diff < 1e-12, "b mismatch after roundtrip: {b_diff}");
            assert_eq!(orig.n_pulls, rest.n_pulls);
        }

        // Verify same selection behavior.
        let sel_orig = policy.select_arm(Some(&ctx));
        let sel_rest = restored.select_arm(Some(&ctx));
        assert_eq!(sel_orig.arm_id, sel_rest.arm_id);
    }

    #[test]
    fn test_single_arm() {
        let feature_keys = vec!["f0".into()];
        let arm_ids = vec!["only".into()];
        let policy = LinUcbPolicy::new("exp".into(), arm_ids, feature_keys, 1.0, 0.0);

        let ctx = sample_context(&["f0"], &[1.0]);
        let selection = policy.select_arm(Some(&ctx));
        assert_eq!(selection.arm_id, "only");
        assert!((selection.assignment_probability - 1.0).abs() < 1e-12);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_features(dim: usize) -> impl Strategy<Value = HashMap<String, f64>> {
        proptest::collection::vec(-10.0f64..10.0, dim).prop_map(move |vals| {
            vals.into_iter()
                .enumerate()
                .map(|(i, v)| (format!("f{i}"), v))
                .collect()
        })
    }

    proptest! {
        #[test]
        fn all_outputs_finite(
            features in arb_features(3),
        ) {
            let policy = LinUcbPolicy::new(
                "exp".into(),
                vec!["a".into(), "b".into()],
                vec!["f0".into(), "f1".into(), "f2".into()],
                1.0,
                0.05,
            );
            let selection = policy.select_arm(Some(&features));
            for (_, p) in &selection.all_arm_probabilities {
                prop_assert!(p.is_finite(), "non-finite probability: {p}");
            }
            prop_assert!(selection.assignment_probability.is_finite());
        }

        #[test]
        fn probabilities_sum_to_one(
            features in arb_features(3),
        ) {
            let policy = LinUcbPolicy::new(
                "exp".into(),
                vec!["a".into(), "b".into(), "c".into()],
                vec!["f0".into(), "f1".into(), "f2".into()],
                1.0,
                0.05,
            );
            let selection = policy.select_arm(Some(&features));
            let sum: f64 = selection.all_arm_probabilities.values().sum();
            prop_assert!(
                (sum - 1.0).abs() < 1e-10,
                "probabilities sum = {sum}, expected ≈ 1.0"
            );
        }

        #[test]
        fn selected_arm_probability_ge_min(
            features in arb_features(3),
        ) {
            let min_expl = 0.05;
            let policy = LinUcbPolicy::new(
                "exp".into(),
                vec!["a".into(), "b".into(), "c".into()],
                vec!["f0".into(), "f1".into(), "f2".into()],
                1.0,
                min_expl,
            );
            let selection = policy.select_arm(Some(&features));
            prop_assert!(
                selection.assignment_probability >= min_expl - 1e-12,
                "selected arm probability {} < min_expl {min_expl}",
                selection.assignment_probability,
            );
        }
    }
}
