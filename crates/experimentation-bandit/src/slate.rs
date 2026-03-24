//! Slot-wise factorized Thompson Sampling slate bandit (ADR-016).
//!
//! # Algorithm
//!
//! Each slot maintains independent Beta posteriors over all candidate arms.
//! Inference is sequential: for each slot `s`, draw once from each remaining
//! candidate's posterior for slot `s`, pick the argmax, then remove that arm
//! from the pool for slot `s+1`. This enforces item uniqueness across slots
//! and runs in **O(L × K)** where L = slots, K = candidates.
//!
//! # Reward Attribution
//!
//! Three models control how observed engagement is credited back to slot decisions:
//!
//! - [`AttributionModel::ClickedSlot`] — full credit to the clicked position only.
//! - [`AttributionModel::PositionWeighted`] — reciprocal-rank discounting across slots.
//! - [`AttributionModel::Counterfactual`] — IPS correction: reward / logging propensity.
//!
//! # Off-Policy Evaluation
//!
//! [`lips_estimate`] computes the linearized IPS (LIPS) estimator for offline
//! evaluation of slate policies against logged interaction data.
//!
//! # Persistence
//!
//! [`SlatePolicy::to_bytes`] / [`SlatePolicy::from_bytes`] serialize the full
//! policy state (all per-slot Beta posteriors) with `bincode` for RocksDB snapshots.

use crate::thompson::BetaArm;
use experimentation_core::error::assert_finite;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Arm identifier type alias (content item ID, placement ID, etc.)
pub type ArmId = String;

// ─────────────────────────────────────────────────────────────────────────────
// Attribution model
// ─────────────────────────────────────────────────────────────────────────────

/// How reward is credited to per-slot decisions after a slate interaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttributionModel {
    /// Full credit to the clicked slot only; other slots receive no update.
    ClickedSlot,
    /// Credit all slots, weighted by reciprocal rank: slot 0 → 1.0, slot 1 → 0.5, etc.
    /// The reward assigned to each slot is `reward × weight(pos)` if clicked, else `0.0`.
    PositionWeighted,
    /// Counterfactual IPS: the clicked slot receives `(reward / propensity).clamp(0.0, 1.0)`.
    /// Provides an approximately unbiased estimate of the counterfactual reward.
    Counterfactual,
}

// ─────────────────────────────────────────────────────────────────────────────
// SlateLog — used by the LIPS OPE estimator
// ─────────────────────────────────────────────────────────────────────────────

/// One logged slate interaction, used for off-policy evaluation with [`lips_estimate`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlateLog {
    /// The ordered slate shown to the user; `slate[0]` is the top-ranked slot.
    pub slate: Vec<ArmId>,
    /// Which arm was clicked (`None` if no click occurred).
    pub clicked: Option<ArmId>,
    /// Index of the clicked item in `slate` (`None` if no click).
    pub clicked_position: Option<usize>,
    /// Propensity of the logging policy for this slate.
    /// Under the factorized model this is the product of per-slot probabilities.
    pub propensity: f64,
    /// Observed reward (e.g. `1.0` for click, `0.0` for no-click).
    pub reward: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal serializable state (for bincode RocksDB snapshots)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlatePolicyState {
    experiment_id: String,
    n_slots: usize,
    slot_arms: Vec<HashMap<ArmId, BetaArm>>,
    attribution: AttributionModel,
    total_updates: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// SlatePolicy
// ─────────────────────────────────────────────────────────────────────────────

/// Slot-wise factorized Thompson Sampling slate bandit.
///
/// Maintains one Beta posterior per `(slot, arm)` pair.  At inference time
/// (`select_slate`), slots are filled left-to-right: the arm chosen for slot
/// `i` is excluded from consideration for all subsequent slots, ensuring no
/// duplicate items in the returned slate.
///
/// State is persisted to RocksDB via `bincode` through [`Self::to_bytes`] /
/// [`Self::from_bytes`].
#[derive(Debug, Clone)]
pub struct SlatePolicy {
    experiment_id: String,
    n_slots: usize,
    /// `slot_arms[s][arm_id]` → Beta posterior for `arm_id` at slot `s`.
    slot_arms: Vec<HashMap<ArmId, BetaArm>>,
    attribution: AttributionModel,
    total_updates: u64,
}

impl SlatePolicy {
    /// Create a new `SlatePolicy` with uniform Beta(1, 1) priors for every
    /// `(slot, arm)` pair.
    ///
    /// # Panics
    /// Panics if `n_slots == 0` or `arm_ids` is empty.
    pub fn new(
        experiment_id: String,
        arm_ids: Vec<ArmId>,
        n_slots: usize,
        attribution: AttributionModel,
    ) -> Self {
        assert!(n_slots > 0, "n_slots must be >= 1");
        assert!(!arm_ids.is_empty(), "arm_ids must not be empty");

        let slot_arms = (0..n_slots)
            .map(|_| {
                arm_ids
                    .iter()
                    .map(|id| (id.clone(), BetaArm::new(id.clone())))
                    .collect::<HashMap<_, _>>()
            })
            .collect();

        Self {
            experiment_id,
            n_slots,
            slot_arms,
            attribution,
            total_updates: 0,
        }
    }

    /// Experiment ID this policy is attached to.
    pub fn experiment_id(&self) -> &str {
        &self.experiment_id
    }

    /// Total reward updates processed since creation or last restore.
    pub fn total_updates(&self) -> u64 {
        self.total_updates
    }

    /// Select an ordered slate via sequential Thompson Sampling.
    ///
    /// For each slot `s` in `0..min(n_slots, candidates.len())`:
    ///   1. For every remaining candidate, draw one sample from the slot-`s` Beta posterior.
    ///   2. Select the candidate with the highest draw.
    ///   3. Remove it from the pool (context propagation — no duplicates).
    ///
    /// Unknown arms (added after policy creation) fall back to a Beta(1, 1) prior.
    ///
    /// **Complexity**: O(L × K), L = effective slot count, K = `candidates.len()`.
    ///
    /// Returns a `Vec<ArmId>` of length `min(n_slots, candidates.len())`.
    pub fn select_slate<R: Rng>(
        &self,
        candidates: &[ArmId],
        n_slots: usize,
        rng: &mut R,
    ) -> Vec<ArmId> {
        let actual_slots = n_slots.min(candidates.len()).min(self.n_slots);
        let mut remaining: Vec<ArmId> = candidates.to_vec();
        let mut selected = Vec::with_capacity(actual_slots);

        for slot_idx in 0..actual_slots {
            let slot_posteriors = &self.slot_arms[slot_idx];

            // O(K): draw once from each remaining arm's slot-posterior.
            let best_local = remaining
                .iter()
                .enumerate()
                .map(|(i, arm_id)| {
                    let draw = if let Some(arm) = slot_posteriors.get(arm_id) {
                        arm.sample(rng)
                    } else {
                        // Fall back to uniform prior for arms unknown at policy creation.
                        BetaArm::new(arm_id.clone()).sample(rng)
                    };
                    (i, draw)
                })
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .map(|(i, _)| i)
                .expect("remaining candidates is non-empty");

            let chosen = remaining.remove(best_local);
            selected.push(chosen);
        }

        selected
    }

    /// Update per-slot Beta posteriors from one observed slate interaction.
    ///
    /// `reward` must be in `[0.0, 1.0]` (required by the Beta-Bernoulli model).
    /// `propensity` is only used by [`AttributionModel::Counterfactual`].
    ///
    /// # Panics
    /// - If `reward` is not in `[0.0, 1.0]`.
    /// - If `reward` or `propensity` is non-finite.
    pub fn update(
        &mut self,
        slate: &[ArmId],
        clicked_position: Option<usize>,
        reward: f64,
        propensity: f64,
    ) {
        assert_finite(reward, "slate reward");
        assert!(
            (0.0..=1.0).contains(&reward),
            "slate reward must be in [0, 1], got {reward}"
        );

        match self.attribution.clone() {
            AttributionModel::ClickedSlot => {
                self.apply_clicked_slot(slate, clicked_position, reward);
            }
            AttributionModel::PositionWeighted => {
                self.apply_position_weighted(slate, clicked_position, reward);
            }
            AttributionModel::Counterfactual => {
                assert_finite(propensity, "slate propensity");
                self.apply_counterfactual(slate, clicked_position, reward, propensity);
            }
        }

        self.total_updates += 1;
    }

    // ── Attribution helpers ───────────────────────────────────────────────

    /// `ClickedSlot`: only the clicked position's posterior is updated.
    fn apply_clicked_slot(
        &mut self,
        slate: &[ArmId],
        clicked_position: Option<usize>,
        reward: f64,
    ) {
        if let Some(pos) = clicked_position {
            if pos < slate.len() && pos < self.n_slots {
                let arm_id = slate[pos].clone();
                if let Some(arm) = self.slot_arms[pos].get_mut(&arm_id) {
                    arm.update(reward);
                }
            }
        }
        // No click → no update (Thompson Sampling naturally handles no-reward steps
        // through unchanged posteriors; forcing a 0-reward update would over-penalize).
    }

    /// `PositionWeighted`: every slot is updated; credit is `reward × 1/(pos+1)` when
    /// clicked, and `0.0` for non-clicked slots.
    fn apply_position_weighted(
        &mut self,
        slate: &[ArmId],
        clicked_position: Option<usize>,
        reward: f64,
    ) {
        let n = slate.len().min(self.n_slots);
        for (pos, arm_id) in slate.iter().enumerate().take(n) {
            let slot_reward = if clicked_position == Some(pos) {
                (reward * position_weight(pos)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let arm_id = arm_id.clone();
            if let Some(arm) = self.slot_arms[pos].get_mut(&arm_id) {
                arm.update(slot_reward);
            }
        }
    }

    /// `Counterfactual` (IPS): clicked slot receives `(reward / propensity).clamp(0, 1)`.
    fn apply_counterfactual(
        &mut self,
        slate: &[ArmId],
        clicked_position: Option<usize>,
        reward: f64,
        propensity: f64,
    ) {
        if let Some(pos) = clicked_position {
            if pos < slate.len() && pos < self.n_slots && propensity > 0.0 {
                let ips_reward = (reward / propensity).clamp(0.0, 1.0);
                assert_finite(ips_reward, "IPS-adjusted reward");
                let arm_id = slate[pos].clone();
                if let Some(arm) = self.slot_arms[pos].get_mut(&arm_id) {
                    arm.update(ips_reward);
                }
            }
        }
    }

    // ── Serialization / RocksDB snapshot ─────────────────────────────────

    /// Serialize the full `SlatePolicy` state to bytes using `bincode`.
    ///
    /// Use this to write snapshots to RocksDB.  The byte format includes all
    /// per-slot Beta posteriors, the attribution model, and `total_updates`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let state = SlatePolicyState {
            experiment_id: self.experiment_id.clone(),
            n_slots: self.n_slots,
            slot_arms: self.slot_arms.clone(),
            attribution: self.attribution.clone(),
            total_updates: self.total_updates,
        };
        bincode::serialize(&state).expect("SlatePolicy serialization should not fail")
    }

    /// Restore a `SlatePolicy` from `bincode`-encoded bytes.
    ///
    /// Panics if the bytes are malformed or were produced by an incompatible version.
    pub fn from_bytes(data: &[u8]) -> Self {
        let state: SlatePolicyState =
            bincode::deserialize(data).expect("SlatePolicy deserialization failed");
        Self {
            experiment_id: state.experiment_id,
            n_slots: state.n_slots,
            slot_arms: state.slot_arms,
            attribution: state.attribution,
            total_updates: state.total_updates,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Position weighting
// ─────────────────────────────────────────────────────────────────────────────

/// Reciprocal-rank position weight used by `PositionWeighted` attribution.
///
/// Slot 0 (top position) → 1.0, slot 1 → 0.5, slot 2 → 0.33, …
#[inline]
pub fn position_weight(pos: usize) -> f64 {
    1.0 / (pos as f64 + 1.0)
}

// ─────────────────────────────────────────────────────────────────────────────
// LIPS OPE estimator
// ─────────────────────────────────────────────────────────────────────────────

/// Linearized Inverse Propensity Score (LIPS) estimator for slate OPE.
///
/// Given a set of logged slate interactions from the *logging policy*, returns
/// an importance-weighted estimate of the expected reward.  For each observation
/// the reward is scaled by `1 / propensity` (IPS reweighting), and the result
/// is averaged over all observations.
///
/// Under the factorized model the propensity of a slate is the *product* of
/// per-slot propensities; callers should supply that product in
/// [`SlateLog::propensity`].  Observations with `propensity <= 0` are skipped
/// (e.g. deterministic logging policy with zero probability for this slate).
///
/// Returns `0.0` when `logged` is empty.
///
/// # References
/// Kiyohara, Nomura, Saito: "Off-Policy Evaluation of Slate Bandit Policies
/// via Optimizing Abstraction" (WWW 2024).
pub fn lips_estimate(logged: &[SlateLog]) -> f64 {
    if logged.is_empty() {
        return 0.0;
    }

    let n = logged.len() as f64;
    let total: f64 = logged
        .iter()
        .filter_map(|log| {
            if log.propensity <= 0.0 {
                return None; // skip zero-propensity observations
            }
            let ips = log.reward / log.propensity;
            assert_finite(ips, "LIPS IPS reward");
            Some(ips)
        })
        .sum();

    total / n
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn seeded_rng() -> StdRng {
        StdRng::seed_from_u64(42)
    }

    fn arm_ids(n: usize) -> Vec<ArmId> {
        (0..n).map(|i| format!("arm-{i}")).collect()
    }

    // ── SlatePolicy::new ───────────────────────────────────────────────────

    #[test]
    fn test_new_creates_uniform_priors() {
        let policy = SlatePolicy::new(
            "exp-1".into(),
            arm_ids(5),
            3,
            AttributionModel::ClickedSlot,
        );
        assert_eq!(policy.n_slots, 3);
        assert_eq!(policy.slot_arms.len(), 3);
        for slot in &policy.slot_arms {
            assert_eq!(slot.len(), 5);
            for arm in slot.values() {
                assert_eq!(arm.alpha, 1.0);
                assert_eq!(arm.beta, 1.0);
            }
        }
    }

    // ── select_slate — basic properties ───────────────────────────────────

    #[test]
    fn test_select_slate_correct_length() {
        let policy = SlatePolicy::new("exp".into(), arm_ids(10), 5, AttributionModel::ClickedSlot);
        let mut rng = seeded_rng();
        let slate = policy.select_slate(&arm_ids(10), 5, &mut rng);
        assert_eq!(slate.len(), 5);
    }

    #[test]
    fn test_select_slate_no_duplicates() {
        let policy = SlatePolicy::new("exp".into(), arm_ids(20), 10, AttributionModel::ClickedSlot);
        let mut rng = seeded_rng();
        let slate = policy.select_slate(&arm_ids(20), 10, &mut rng);
        let unique: std::collections::HashSet<_> = slate.iter().collect();
        assert_eq!(unique.len(), slate.len(), "slate must not contain duplicate arms");
    }

    #[test]
    fn test_select_slate_truncates_to_candidates() {
        // Only 3 candidates, requesting 5 slots → should get 3 items.
        let policy = SlatePolicy::new("exp".into(), arm_ids(5), 5, AttributionModel::ClickedSlot);
        let mut rng = seeded_rng();
        let slate = policy.select_slate(&arm_ids(3), 5, &mut rng);
        assert_eq!(slate.len(), 3);
    }

    #[test]
    fn test_select_slate_prefers_high_alpha_arm() {
        // Arm "arm-0" has strongly updated posterior (alpha=100) → nearly always selected first.
        let arms = arm_ids(3);
        let mut policy = SlatePolicy::new("exp".into(), arms.clone(), 3, AttributionModel::ClickedSlot);
        // Force arm-0 to be dominant in slot 0.
        policy.slot_arms[0].get_mut("arm-0").unwrap().alpha = 100.0;
        policy.slot_arms[0].get_mut("arm-0").unwrap().beta = 1.0;

        let mut wins = 0u32;
        let mut rng = seeded_rng();
        for _ in 0..200 {
            let slate = policy.select_slate(&arms, 3, &mut rng);
            if slate[0] == "arm-0" {
                wins += 1;
            }
        }
        assert!(wins > 150, "dominant arm should win slot-0 most of the time, got {wins}/200");
    }

    #[test]
    fn test_select_slate_context_propagation() {
        // Once an arm is selected for slot i it must not appear in slots i+1..L.
        let policy = SlatePolicy::new("exp".into(), arm_ids(5), 5, AttributionModel::ClickedSlot);
        let mut rng = seeded_rng();
        for _ in 0..50 {
            let slate = policy.select_slate(&arm_ids(5), 5, &mut rng);
            let unique: std::collections::HashSet<_> = slate.iter().collect();
            assert_eq!(unique.len(), 5);
        }
    }

    // ── Attribution: ClickedSlot ───────────────────────────────────────────

    #[test]
    fn test_update_clicked_slot_updates_only_clicked() {
        let mut policy = SlatePolicy::new(
            "exp".into(),
            arm_ids(3),
            3,
            AttributionModel::ClickedSlot,
        );
        let slate = arm_ids(3);
        policy.update(&slate, Some(1), 1.0, 1.0);

        // Only slot 1 / arm-1 should have changed.
        assert_eq!(policy.slot_arms[0]["arm-0"].alpha, 1.0); // unchanged
        assert_eq!(policy.slot_arms[1]["arm-1"].alpha, 2.0); // updated: 1 + 1
        assert_eq!(policy.slot_arms[2]["arm-2"].alpha, 1.0); // unchanged
        assert_eq!(policy.total_updates(), 1);
    }

    #[test]
    fn test_update_clicked_slot_no_click_no_change() {
        let mut policy = SlatePolicy::new("exp".into(), arm_ids(3), 3, AttributionModel::ClickedSlot);
        let slate = arm_ids(3);
        policy.update(&slate, None, 0.0, 1.0);
        for slot in &policy.slot_arms {
            for arm in slot.values() {
                assert_eq!(arm.alpha, 1.0);
                assert_eq!(arm.beta, 1.0);
            }
        }
    }

    // ── Attribution: PositionWeighted ─────────────────────────────────────

    #[test]
    fn test_update_position_weighted_click_at_slot_0() {
        let mut policy = SlatePolicy::new(
            "exp".into(),
            arm_ids(3),
            3,
            AttributionModel::PositionWeighted,
        );
        let slate = arm_ids(3);
        policy.update(&slate, Some(0), 1.0, 1.0);

        // Slot 0 clicked: reward × 1/(0+1) = 1.0 → alpha += 1.0
        assert!((policy.slot_arms[0]["arm-0"].alpha - 2.0).abs() < 1e-10);
        // Slot 1 not clicked: reward = 0.0 → alpha unchanged
        assert_eq!(policy.slot_arms[1]["arm-1"].alpha, 1.0);
        // Slot 2 not clicked: same
        assert_eq!(policy.slot_arms[2]["arm-2"].alpha, 1.0);
    }

    #[test]
    fn test_update_position_weighted_click_at_slot_1() {
        let mut policy = SlatePolicy::new(
            "exp".into(),
            arm_ids(3),
            3,
            AttributionModel::PositionWeighted,
        );
        let slate = arm_ids(3);
        policy.update(&slate, Some(1), 1.0, 1.0);

        // Slot 0: not clicked → 0.0 update
        assert_eq!(policy.slot_arms[0]["arm-0"].alpha, 1.0);
        // Slot 1 clicked: reward × 1/2 = 0.5 → alpha += 0.5
        let expected = 1.0 + position_weight(1); // 1.5
        assert!((policy.slot_arms[1]["arm-1"].alpha - expected).abs() < 1e-10);
    }

    #[test]
    fn test_position_weight_values() {
        assert!((position_weight(0) - 1.0).abs() < 1e-12);
        assert!((position_weight(1) - 0.5).abs() < 1e-12);
        assert!((position_weight(2) - 1.0 / 3.0).abs() < 1e-12);
        assert!((position_weight(9) - 0.1).abs() < 1e-12);
    }

    // ── Attribution: Counterfactual ────────────────────────────────────────

    #[test]
    fn test_update_counterfactual_ips_scaling() {
        let mut policy = SlatePolicy::new(
            "exp".into(),
            arm_ids(3),
            3,
            AttributionModel::Counterfactual,
        );
        let slate = arm_ids(3);
        // reward=1.0, propensity=0.5 → IPS = 1.0/0.5 = 2.0, clamp to 1.0
        policy.update(&slate, Some(0), 1.0, 0.5);
        assert!((policy.slot_arms[0]["arm-0"].alpha - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_update_counterfactual_low_propensity_clamps() {
        let mut policy = SlatePolicy::new(
            "exp".into(),
            arm_ids(2),
            2,
            AttributionModel::Counterfactual,
        );
        let slate = arm_ids(2);
        // propensity=0.01 → IPS = 100.0 → clamped to 1.0
        policy.update(&slate, Some(0), 1.0, 0.01);
        assert!((policy.slot_arms[0]["arm-0"].alpha - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_update_counterfactual_no_click() {
        let mut policy = SlatePolicy::new(
            "exp".into(),
            arm_ids(2),
            2,
            AttributionModel::Counterfactual,
        );
        let slate = arm_ids(2);
        policy.update(&slate, None, 0.0, 0.5);
        // No click → no posterior change.
        assert_eq!(policy.slot_arms[0]["arm-0"].alpha, 1.0);
    }

    // ── Serialization roundtrip ────────────────────────────────────────────

    #[test]
    fn test_bincode_roundtrip_preserves_state() {
        let mut policy = SlatePolicy::new(
            "exp-roundtrip".into(),
            arm_ids(4),
            2,
            AttributionModel::PositionWeighted,
        );
        policy.update(&arm_ids(4)[..2], Some(0), 1.0, 1.0);
        policy.update(&arm_ids(4)[..2], Some(1), 0.5, 1.0);

        let bytes = policy.to_bytes();
        let restored = SlatePolicy::from_bytes(&bytes);

        assert_eq!(restored.experiment_id(), "exp-roundtrip");
        assert_eq!(restored.n_slots, 2);
        assert_eq!(restored.total_updates(), 2);
        assert_eq!(restored.attribution, AttributionModel::PositionWeighted);

        // Posteriors are preserved.
        let orig_alpha = policy.slot_arms[0]["arm-0"].alpha;
        let rest_alpha = restored.slot_arms[0]["arm-0"].alpha;
        assert!((orig_alpha - rest_alpha).abs() < 1e-12);
    }

    #[test]
    fn test_bincode_roundtrip_all_attribution_models() {
        for model in [
            AttributionModel::ClickedSlot,
            AttributionModel::PositionWeighted,
            AttributionModel::Counterfactual,
        ] {
            let policy = SlatePolicy::new("exp".into(), arm_ids(3), 2, model.clone());
            let restored = SlatePolicy::from_bytes(&policy.to_bytes());
            assert_eq!(restored.attribution, model);
        }
    }

    // ── LIPS estimator ─────────────────────────────────────────────────────

    #[test]
    fn test_lips_estimate_empty_returns_zero() {
        assert_eq!(lips_estimate(&[]), 0.0);
    }

    #[test]
    fn test_lips_estimate_click_full_propensity() {
        let log = vec![SlateLog {
            slate: vec!["arm-0".into()],
            clicked: Some("arm-0".into()),
            clicked_position: Some(0),
            propensity: 1.0,
            reward: 1.0,
        }];
        // IPS = 1.0 / 1.0 = 1.0; average = 1.0
        assert!((lips_estimate(&log) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_lips_estimate_click_with_propensity() {
        // reward=1.0, propensity=0.5 → IPS=2.0; average = 2.0/1 = 2.0
        let log = vec![SlateLog {
            slate: vec!["arm-0".into()],
            clicked: Some("arm-0".into()),
            clicked_position: Some(0),
            propensity: 0.5,
            reward: 1.0,
        }];
        assert!((lips_estimate(&log) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn test_lips_estimate_mixed_observations() {
        let logs = vec![
            SlateLog {
                slate: vec!["arm-0".into()],
                clicked: Some("arm-0".into()),
                clicked_position: Some(0),
                propensity: 0.5,
                reward: 1.0,
            },
            SlateLog {
                slate: vec!["arm-1".into()],
                clicked: None,
                clicked_position: None,
                propensity: 0.5,
                reward: 0.0,
            },
        ];
        // (1.0/0.5 + 0.0/0.5) / 2 = (2.0 + 0.0) / 2 = 1.0
        assert!((lips_estimate(&logs) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_lips_estimate_skips_zero_propensity() {
        let logs = vec![
            SlateLog {
                slate: vec!["arm-0".into()],
                clicked: Some("arm-0".into()),
                clicked_position: Some(0),
                propensity: 0.0,  // zero propensity → skipped
                reward: 1.0,
            },
            SlateLog {
                slate: vec!["arm-1".into()],
                clicked: Some("arm-1".into()),
                clicked_position: Some(0),
                propensity: 1.0,
                reward: 1.0,
            },
        ];
        // skipped observation counts in denominator; total = 0 + 1 = 1; n = 2
        assert!((lips_estimate(&logs) - 0.5).abs() < 1e-12);
    }

    // ── Convergence: dominant arm occupies slot-0 ─────────────────────────

    #[test]
    fn test_convergence_dominant_arm_occupies_slot_0() {
        let arms = arm_ids(5);
        let mut policy = SlatePolicy::new(
            "conv-exp".into(),
            arms.clone(),
            3,
            AttributionModel::ClickedSlot,
        );
        let mut rng = seeded_rng();

        // Simulate: arm-0 always clicked in slot-0 → should dominate.
        for _ in 0..500 {
            let slate = policy.select_slate(&arms, 3, &mut rng);
            let clicked = if slate[0] == "arm-0" { Some(0) } else { None };
            let reward = if clicked.is_some() { 1.0 } else { 0.0 };
            policy.update(&slate, clicked, reward, 1.0);
        }

        let mut wins = 0u32;
        for _ in 0..200 {
            let slate = policy.select_slate(&arms, 3, &mut rng);
            if slate[0] == "arm-0" {
                wins += 1;
            }
        }
        assert!(
            wins > 140,
            "arm-0 should win slot-0 >70% of the time after training, got {wins}/200"
        );
    }
}
