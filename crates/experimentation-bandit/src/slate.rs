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
//! Three models control how observed engagement is credited back to slot decisions
//! (mirrors `SlateInteractionModel.AttributionModel` proto enum):
//!
//! - [`AttributionModel::ClickedSlotOnly`] — full credit to the clicked position only.
//! - [`AttributionModel::PositionWeighted`] — reciprocal-rank discounting across slots.
//! - [`AttributionModel::LeaveOneOut`] — counterfactual leave-one-out: examined-but-not-clicked
//!   slots get a failure update; post-click slots are unobserved under cascade.
//!
//! # Position Bias Correction
//!
//! [`PositionBiasModel`] adjusts reward updates to separate true item quality
//! from positional examination effects (mirrors `PositionBiasModel` proto enum):
//!
//! - [`PositionBiasModel::None`] — no correction.
//! - [`PositionBiasModel::Cascade`] — examination probability decreases geometrically:
//!   `P(examine slot s) = γ^s` where `γ` is a persistence parameter.
//! - [`PositionBiasModel::Examination`] — position-specific examination probabilities
//!   estimated from data.
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
///
/// Maps to `SlateInteractionModel.AttributionModel` in `bandit.proto`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttributionModel {
    /// Full credit to the clicked slot only; other slots receive no update.
    /// Proto: `ATTRIBUTION_MODEL_CLICKED_SLOT_ONLY`.
    ClickedSlotOnly,
    /// Credit all slots, weighted by reciprocal rank: slot 0 → 1.0, slot 1 → 0.5, etc.
    /// The reward assigned to each slot is `reward × weight(pos)` if clicked, else `0.0`.
    /// Proto: `ATTRIBUTION_MODEL_POSITION_WEIGHTED`.
    PositionWeighted,
    /// Counterfactual leave-one-out: under a cascade browsing assumption, the clicked
    /// slot gets full reward credit, slots *before* the click are updated with `0.0`
    /// (examined but not clicked = failure signal), and slots *after* the click receive
    /// no update (unobserved under cascade).  When no click occurs, all slots are
    /// updated with `0.0` (all examined, none converted).
    /// Proto: `ATTRIBUTION_MODEL_LEAVE_ONE_OUT`.
    LeaveOneOut,
}

// ─────────────────────────────────────────────────────────────────────────────
// Position bias model
// ─────────────────────────────────────────────────────────────────────────────

/// Bias correction for positional examination effects in slate interactions.
///
/// Maps to `PositionBiasModel` in `bandit.proto`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PositionBiasModel {
    /// No position bias correction.  Raw rewards are used directly.
    /// Proto: `POSITION_BIAS_MODEL_NONE`.
    None,
    /// Cascade model: examination probability decreases geometrically with position.
    /// `P(examine slot s) = γ^s` where `γ ∈ (0, 1]` is the persistence parameter.
    /// Reward is divided by the examination probability to debias:
    /// `adjusted_reward = reward / γ^s`, clamped to `[0, 1]`.
    /// Proto: `POSITION_BIAS_MODEL_CASCADE`.
    Cascade {
        /// Persistence parameter γ ∈ (0, 1].  γ = 1.0 means no position decay;
        /// γ = 0.5 means each subsequent position has half the examination probability.
        gamma: f64,
    },
    /// Examination hypothesis: position-specific examination probabilities estimated
    /// from logged data.  Each position has an independent examination probability.
    /// Reward is divided by the position's examination probability:
    /// `adjusted_reward = reward / exam_prob[s]`, clamped to `[0, 1]`.
    /// Proto: `POSITION_BIAS_MODEL_EXAMINATION`.
    Examination {
        /// Per-position examination probabilities.  `exam_probs[s]` is the
        /// probability that position `s` is examined.  Must be in `(0, 1]`.
        /// If a position index exceeds `exam_probs.len()`, falls back to the last value.
        exam_probs: Vec<f64>,
    },
}

impl PositionBiasModel {
    /// Returns the examination probability for a given position.
    fn exam_probability(&self, position: usize) -> f64 {
        match self {
            PositionBiasModel::None => 1.0,
            PositionBiasModel::Cascade { gamma } => gamma.powi(position as i32),
            PositionBiasModel::Examination { exam_probs } => {
                if exam_probs.is_empty() {
                    1.0
                } else if position < exam_probs.len() {
                    exam_probs[position]
                } else {
                    *exam_probs.last().unwrap()
                }
            }
        }
    }

    /// Debias a reward by dividing by the examination probability at the given position.
    /// Result is clamped to `[0.0, 1.0]` for the Beta-Bernoulli model.
    fn debias_reward(&self, reward: f64, position: usize) -> f64 {
        let exam_prob = self.exam_probability(position);
        if exam_prob <= 0.0 {
            return reward;
        }
        (reward / exam_prob).clamp(0.0, 1.0)
    }
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
    position_bias: PositionBiasModel,
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
    position_bias: PositionBiasModel,
    total_updates: u64,
}

impl SlatePolicy {
    /// Create a new `SlatePolicy` with uniform Beta(1, 1) priors for every
    /// `(slot, arm)` pair and no position bias correction.
    ///
    /// # Panics
    /// Panics if `n_slots == 0` or `arm_ids` is empty.
    pub fn new(
        experiment_id: String,
        arm_ids: Vec<ArmId>,
        n_slots: usize,
        attribution: AttributionModel,
    ) -> Self {
        Self::with_position_bias(
            experiment_id,
            arm_ids,
            n_slots,
            attribution,
            PositionBiasModel::None,
        )
    }

    /// Create a new `SlatePolicy` with uniform Beta(1, 1) priors and the
    /// specified position bias correction model.
    ///
    /// # Panics
    /// Panics if `n_slots == 0`, `arm_ids` is empty, or bias model parameters
    /// are invalid (e.g. `gamma <= 0` for Cascade).
    pub fn with_position_bias(
        experiment_id: String,
        arm_ids: Vec<ArmId>,
        n_slots: usize,
        attribution: AttributionModel,
        position_bias: PositionBiasModel,
    ) -> Self {
        assert!(n_slots > 0, "n_slots must be >= 1");
        assert!(!arm_ids.is_empty(), "arm_ids must not be empty");

        match &position_bias {
            PositionBiasModel::Cascade { gamma } => {
                assert!(*gamma > 0.0 && *gamma <= 1.0, "gamma must be in (0, 1], got {gamma}");
            }
            PositionBiasModel::Examination { exam_probs } => {
                for (i, p) in exam_probs.iter().enumerate() {
                    assert!(
                        *p > 0.0 && *p <= 1.0,
                        "exam_probs[{i}] must be in (0, 1], got {p}"
                    );
                }
            }
            PositionBiasModel::None => {}
        }

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
            position_bias,
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

    /// Returns a reference to the position bias model.
    pub fn position_bias(&self) -> &PositionBiasModel {
        &self.position_bias
    }

    /// Returns a reference to the attribution model.
    pub fn attribution(&self) -> &AttributionModel {
        &self.attribution
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

            let best_local = remaining
                .iter()
                .enumerate()
                .map(|(i, arm_id)| {
                    let draw = if let Some(arm) = slot_posteriors.get(arm_id) {
                        arm.sample(rng)
                    } else {
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
    /// `propensity` is only used for logging context — not currently consumed
    /// by any attribution model but preserved for API compatibility.
    ///
    /// Position bias correction (if configured) is applied before the posterior
    /// update, debiasing the reward by the examination probability at each position.
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
        assert_finite(propensity, "slate propensity");

        match self.attribution.clone() {
            AttributionModel::ClickedSlotOnly => {
                self.apply_clicked_slot_only(slate, clicked_position, reward);
            }
            AttributionModel::PositionWeighted => {
                self.apply_position_weighted(slate, clicked_position, reward);
            }
            AttributionModel::LeaveOneOut => {
                self.apply_leave_one_out(slate, clicked_position, reward);
            }
        }

        self.total_updates += 1;
    }

    // ── Attribution helpers ───────────────────────────────────────────────

    /// `ClickedSlotOnly`: only the clicked position's posterior is updated.
    fn apply_clicked_slot_only(
        &mut self,
        slate: &[ArmId],
        clicked_position: Option<usize>,
        reward: f64,
    ) {
        if let Some(pos) = clicked_position {
            if pos < slate.len() && pos < self.n_slots {
                let debiased = self.position_bias.debias_reward(reward, pos);
                let arm_id = slate[pos].clone();
                if let Some(arm) = self.slot_arms[pos].get_mut(&arm_id) {
                    arm.update(debiased);
                }
            }
        }
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
                let weighted = reward * position_weight(pos);
                self.position_bias.debias_reward(weighted.clamp(0.0, 1.0), pos)
            } else {
                0.0
            };
            let arm_id = arm_id.clone();
            if let Some(arm) = self.slot_arms[pos].get_mut(&arm_id) {
                arm.update(slot_reward);
            }
        }
    }

    /// `LeaveOneOut`: counterfactual leave-one-out attribution under cascade browsing.
    ///
    /// Under the cascade assumption, users examine slots top-to-bottom and stop
    /// after clicking.  Each slot's marginal contribution:
    /// - **Clicked slot**: gets `reward` (removing this item eliminates the click).
    /// - **Slots before click**: examined but not clicked → updated with `0.0` (failure).
    /// - **Slots after click**: not examined under cascade → no posterior update.
    /// - **No click**: all slots updated with `0.0` (all examined, none converted).
    fn apply_leave_one_out(
        &mut self,
        slate: &[ArmId],
        clicked_position: Option<usize>,
        reward: f64,
    ) {
        let n = slate.len().min(self.n_slots);

        match clicked_position {
            Some(click_pos) if click_pos < n => {
                // Slots before click: examined but not clicked → failure update.
                for pos in 0..click_pos {
                    let arm_id = slate[pos].clone();
                    let debiased_zero = self.position_bias.debias_reward(0.0, pos);
                    if let Some(arm) = self.slot_arms[pos].get_mut(&arm_id) {
                        arm.update(debiased_zero);
                    }
                }
                // Clicked slot: full reward credit.
                let debiased = self.position_bias.debias_reward(reward, click_pos);
                let arm_id = slate[click_pos].clone();
                if let Some(arm) = self.slot_arms[click_pos].get_mut(&arm_id) {
                    arm.update(debiased);
                }
                // Slots after click: not examined → no update (cascade assumption).
            }
            _ => {
                // No click (or click_pos out of range): all slots examined, none clicked.
                for pos in 0..n {
                    let arm_id = slate[pos].clone();
                    let debiased_zero = self.position_bias.debias_reward(0.0, pos);
                    if let Some(arm) = self.slot_arms[pos].get_mut(&arm_id) {
                        arm.update(debiased_zero);
                    }
                }
            }
        }
    }

    // ── Serialization / RocksDB snapshot ─────────────────────────────────

    /// Serialize the full `SlatePolicy` state to bytes using `bincode`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let state = SlatePolicyState {
            experiment_id: self.experiment_id.clone(),
            n_slots: self.n_slots,
            slot_arms: self.slot_arms.clone(),
            attribution: self.attribution.clone(),
            position_bias: self.position_bias.clone(),
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
            position_bias: state.position_bias,
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
                return None;
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
    use rand::rngs::StdRng;
    use rand::SeedableRng;

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
            AttributionModel::ClickedSlotOnly,
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
        let policy =
            SlatePolicy::new("exp".into(), arm_ids(10), 5, AttributionModel::ClickedSlotOnly);
        let mut rng = seeded_rng();
        let slate = policy.select_slate(&arm_ids(10), 5, &mut rng);
        assert_eq!(slate.len(), 5);
    }

    #[test]
    fn test_select_slate_no_duplicates() {
        let policy =
            SlatePolicy::new("exp".into(), arm_ids(20), 10, AttributionModel::ClickedSlotOnly);
        let mut rng = seeded_rng();
        let slate = policy.select_slate(&arm_ids(20), 10, &mut rng);
        let unique: std::collections::HashSet<_> = slate.iter().collect();
        assert_eq!(unique.len(), slate.len(), "slate must not contain duplicate arms");
    }

    #[test]
    fn test_select_slate_truncates_to_candidates() {
        let policy =
            SlatePolicy::new("exp".into(), arm_ids(5), 5, AttributionModel::ClickedSlotOnly);
        let mut rng = seeded_rng();
        let slate = policy.select_slate(&arm_ids(3), 5, &mut rng);
        assert_eq!(slate.len(), 3);
    }

    #[test]
    fn test_select_slate_prefers_high_alpha_arm() {
        let arms = arm_ids(3);
        let mut policy =
            SlatePolicy::new("exp".into(), arms.clone(), 3, AttributionModel::ClickedSlotOnly);
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
        assert!(
            wins > 150,
            "dominant arm should win slot-0 most of the time, got {wins}/200"
        );
    }

    #[test]
    fn test_select_slate_context_propagation() {
        let policy =
            SlatePolicy::new("exp".into(), arm_ids(5), 5, AttributionModel::ClickedSlotOnly);
        let mut rng = seeded_rng();
        for _ in 0..50 {
            let slate = policy.select_slate(&arm_ids(5), 5, &mut rng);
            let unique: std::collections::HashSet<_> = slate.iter().collect();
            assert_eq!(unique.len(), 5);
        }
    }

    // ── Attribution: ClickedSlotOnly ──────────────────────────────────────

    #[test]
    fn test_update_clicked_slot_only_updates_only_clicked() {
        let mut policy = SlatePolicy::new(
            "exp".into(),
            arm_ids(3),
            3,
            AttributionModel::ClickedSlotOnly,
        );
        let slate = arm_ids(3);
        policy.update(&slate, Some(1), 1.0, 1.0);

        assert_eq!(policy.slot_arms[0]["arm-0"].alpha, 1.0); // unchanged
        assert_eq!(policy.slot_arms[1]["arm-1"].alpha, 2.0); // updated: 1 + 1
        assert_eq!(policy.slot_arms[2]["arm-2"].alpha, 1.0); // unchanged
        assert_eq!(policy.total_updates(), 1);
    }

    #[test]
    fn test_update_clicked_slot_only_no_click_no_change() {
        let mut policy =
            SlatePolicy::new("exp".into(), arm_ids(3), 3, AttributionModel::ClickedSlotOnly);
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

        assert!((policy.slot_arms[0]["arm-0"].alpha - 2.0).abs() < 1e-10);
        // Slot 1 not clicked
        assert_eq!(policy.slot_arms[1]["arm-1"].alpha, 1.0);
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

        assert_eq!(policy.slot_arms[0]["arm-0"].alpha, 1.0);
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

    // ── Attribution: LeaveOneOut ───────────────────────────────────────────

    #[test]
    fn test_leave_one_out_clicked_slot_gets_full_reward() {
        let mut policy =
            SlatePolicy::new("exp".into(), arm_ids(4), 4, AttributionModel::LeaveOneOut);
        let slate = arm_ids(4);
        policy.update(&slate, Some(2), 1.0, 1.0);

        // Clicked slot 2: full reward → alpha += 1.0
        assert!((policy.slot_arms[2]["arm-2"].alpha - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_leave_one_out_pre_click_slots_get_failure() {
        let mut policy =
            SlatePolicy::new("exp".into(), arm_ids(4), 4, AttributionModel::LeaveOneOut);
        let slate = arm_ids(4);
        policy.update(&slate, Some(2), 1.0, 1.0);

        // Slots 0, 1: examined but not clicked → updated with 0.0
        // alpha stays 1.0, beta increases by 1.0
        assert_eq!(policy.slot_arms[0]["arm-0"].alpha, 1.0);
        assert!((policy.slot_arms[0]["arm-0"].beta - 2.0).abs() < 1e-10);
        assert_eq!(policy.slot_arms[1]["arm-1"].alpha, 1.0);
        assert!((policy.slot_arms[1]["arm-1"].beta - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_leave_one_out_post_click_slots_unchanged() {
        let mut policy =
            SlatePolicy::new("exp".into(), arm_ids(4), 4, AttributionModel::LeaveOneOut);
        let slate = arm_ids(4);
        policy.update(&slate, Some(2), 1.0, 1.0);

        // Slot 3: after click → no update (cascade)
        assert_eq!(policy.slot_arms[3]["arm-3"].alpha, 1.0);
        assert_eq!(policy.slot_arms[3]["arm-3"].beta, 1.0);
    }

    #[test]
    fn test_leave_one_out_no_click_all_get_failure() {
        let mut policy =
            SlatePolicy::new("exp".into(), arm_ids(3), 3, AttributionModel::LeaveOneOut);
        let slate = arm_ids(3);
        policy.update(&slate, None, 0.0, 1.0);

        // No click: all slots examined, none clicked → all get 0.0 update
        for (pos, arm_name) in ["arm-0", "arm-1", "arm-2"].iter().enumerate() {
            assert_eq!(policy.slot_arms[pos][*arm_name].alpha, 1.0);
            assert!((policy.slot_arms[pos][*arm_name].beta - 2.0).abs() < 1e-10);
        }
    }

    // ── Position bias correction ──────────────────────────────────────────

    #[test]
    fn test_cascade_bias_correction() {
        let mut policy = SlatePolicy::with_position_bias(
            "exp".into(),
            arm_ids(3),
            3,
            AttributionModel::ClickedSlotOnly,
            PositionBiasModel::Cascade { gamma: 0.8 },
        );
        let slate = arm_ids(3);
        // Click at slot 1: exam_prob = 0.8^1 = 0.8, reward = 1.0 → debiased = 1.0/0.8 = 1.25, clamp to 1.0
        policy.update(&slate, Some(1), 1.0, 1.0);
        assert!((policy.slot_arms[1]["arm-1"].alpha - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_cascade_bias_partial_reward() {
        let mut policy = SlatePolicy::with_position_bias(
            "exp".into(),
            arm_ids(3),
            3,
            AttributionModel::ClickedSlotOnly,
            PositionBiasModel::Cascade { gamma: 0.5 },
        );
        let slate = arm_ids(3);
        // Click at slot 0: exam_prob = 0.5^0 = 1.0, reward = 0.4 → debiased = 0.4/1.0 = 0.4
        policy.update(&slate, Some(0), 0.4, 1.0);
        assert!((policy.slot_arms[0]["arm-0"].alpha - 1.4).abs() < 1e-10);
    }

    #[test]
    fn test_cascade_bias_lower_slot() {
        let mut policy = SlatePolicy::with_position_bias(
            "exp".into(),
            arm_ids(3),
            3,
            AttributionModel::ClickedSlotOnly,
            PositionBiasModel::Cascade { gamma: 0.5 },
        );
        let slate = arm_ids(3);
        // Click at slot 2: exam_prob = 0.5^2 = 0.25, reward = 0.2 → debiased = 0.2/0.25 = 0.8
        policy.update(&slate, Some(2), 0.2, 1.0);
        assert!((policy.slot_arms[2]["arm-2"].alpha - 1.8).abs() < 1e-10);
    }

    #[test]
    fn test_examination_bias_correction() {
        let mut policy = SlatePolicy::with_position_bias(
            "exp".into(),
            arm_ids(3),
            3,
            AttributionModel::ClickedSlotOnly,
            PositionBiasModel::Examination {
                exam_probs: vec![1.0, 0.6, 0.3],
            },
        );
        let slate = arm_ids(3);
        // Click at slot 2: exam_prob = 0.3, reward = 0.3 → debiased = 0.3/0.3 = 1.0
        policy.update(&slate, Some(2), 0.3, 1.0);
        assert!((policy.slot_arms[2]["arm-2"].alpha - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_examination_bias_fallback_to_last() {
        let bias = PositionBiasModel::Examination {
            exam_probs: vec![1.0, 0.5],
        };
        // Position 5 exceeds exam_probs length → falls back to last (0.5)
        assert!((bias.exam_probability(5) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn test_no_bias_returns_one() {
        let bias = PositionBiasModel::None;
        assert!((bias.exam_probability(0) - 1.0).abs() < 1e-12);
        assert!((bias.exam_probability(100) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_leave_one_out_with_cascade_bias() {
        let mut policy = SlatePolicy::with_position_bias(
            "exp".into(),
            arm_ids(3),
            3,
            AttributionModel::LeaveOneOut,
            PositionBiasModel::Cascade { gamma: 0.5 },
        );
        let slate = arm_ids(3);
        // Click at slot 1: exam_prob at slot 0 = 1.0, slot 1 = 0.5
        // Slot 0: debias(0.0, pos=0) = 0.0/1.0 = 0.0 → failure update
        // Slot 1: debias(1.0, pos=1) = 1.0/0.5 = 2.0 → clamped to 1.0
        // Slot 2: no update (cascade)
        policy.update(&slate, Some(1), 1.0, 1.0);

        assert_eq!(policy.slot_arms[0]["arm-0"].alpha, 1.0);
        assert!((policy.slot_arms[0]["arm-0"].beta - 2.0).abs() < 1e-10); // failure
        assert!((policy.slot_arms[1]["arm-1"].alpha - 2.0).abs() < 1e-10); // success
        assert_eq!(policy.slot_arms[2]["arm-2"].alpha, 1.0); // untouched
        assert_eq!(policy.slot_arms[2]["arm-2"].beta, 1.0);
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
        assert_eq!(restored.position_bias, PositionBiasModel::None);

        let orig_alpha = policy.slot_arms[0]["arm-0"].alpha;
        let rest_alpha = restored.slot_arms[0]["arm-0"].alpha;
        assert!((orig_alpha - rest_alpha).abs() < 1e-12);
    }

    #[test]
    fn test_bincode_roundtrip_all_attribution_models() {
        for model in [
            AttributionModel::ClickedSlotOnly,
            AttributionModel::PositionWeighted,
            AttributionModel::LeaveOneOut,
        ] {
            let policy = SlatePolicy::new("exp".into(), arm_ids(3), 2, model.clone());
            let restored = SlatePolicy::from_bytes(&policy.to_bytes());
            assert_eq!(restored.attribution, model);
        }
    }

    #[test]
    fn test_bincode_roundtrip_with_position_bias() {
        for bias in [
            PositionBiasModel::None,
            PositionBiasModel::Cascade { gamma: 0.7 },
            PositionBiasModel::Examination {
                exam_probs: vec![1.0, 0.8, 0.5],
            },
        ] {
            let policy = SlatePolicy::with_position_bias(
                "exp".into(),
                arm_ids(3),
                3,
                AttributionModel::ClickedSlotOnly,
                bias.clone(),
            );
            let restored = SlatePolicy::from_bytes(&policy.to_bytes());
            assert_eq!(restored.position_bias, bias);
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
        assert!((lips_estimate(&log) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_lips_estimate_click_with_propensity() {
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
        assert!((lips_estimate(&logs) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_lips_estimate_skips_zero_propensity() {
        let logs = vec![
            SlateLog {
                slate: vec!["arm-0".into()],
                clicked: Some("arm-0".into()),
                clicked_position: Some(0),
                propensity: 0.0,
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
            AttributionModel::ClickedSlotOnly,
        );
        let mut rng = seeded_rng();

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

    #[test]
    fn test_convergence_leave_one_out_penalizes_non_clicked() {
        let arms = arm_ids(3);
        let mut policy =
            SlatePolicy::new("conv".into(), arms.clone(), 3, AttributionModel::LeaveOneOut);
        let mut rng = seeded_rng();

        // Always click slot 0 (arm that ends up there) with reward 1.0
        for _ in 0..300 {
            let slate = policy.select_slate(&arms, 3, &mut rng);
            policy.update(&slate, Some(0), 1.0, 1.0);
        }

        // After many iterations, the best arm for slot 0 should have high alpha
        // and arms that frequently appear before the click should have higher beta
        let slot0_alphas: Vec<f64> = arms
            .iter()
            .map(|a| policy.slot_arms[0][a].alpha)
            .collect();
        let max_alpha = slot0_alphas.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!(max_alpha > 50.0, "winning arm should have high alpha, got {max_alpha}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Proptest invariants
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn arb_arm_ids(max_arms: usize) -> impl Strategy<Value = Vec<ArmId>> {
        (2..=max_arms).prop_flat_map(|n| {
            Just((0..n).map(|i| format!("arm-{i}")).collect::<Vec<_>>())
        })
    }

    fn arb_attribution() -> impl Strategy<Value = AttributionModel> {
        prop_oneof![
            Just(AttributionModel::ClickedSlotOnly),
            Just(AttributionModel::PositionWeighted),
            Just(AttributionModel::LeaveOneOut),
        ]
    }

    fn arb_position_bias() -> impl Strategy<Value = PositionBiasModel> {
        prop_oneof![
            Just(PositionBiasModel::None),
            (0.1f64..=1.0).prop_map(|gamma| PositionBiasModel::Cascade { gamma }),
            proptest::collection::vec(0.1f64..=1.0, 1..=10)
                .prop_map(|exam_probs| PositionBiasModel::Examination { exam_probs }),
        ]
    }

    proptest! {
        /// Slate selection never returns duplicates and respects length bounds.
        #[test]
        fn prop_select_slate_no_duplicates_correct_length(
            arms in arb_arm_ids(15),
            n_slots in 1usize..=10,
            seed in any::<u64>(),
        ) {
            let actual_slots = n_slots.min(arms.len());
            let policy = SlatePolicy::new("exp".into(), arms.clone(), n_slots, AttributionModel::ClickedSlotOnly);
            let mut rng = StdRng::seed_from_u64(seed);
            let slate = policy.select_slate(&arms, n_slots, &mut rng);

            prop_assert_eq!(slate.len(), actual_slots);
            let unique: std::collections::HashSet<_> = slate.iter().collect();
            prop_assert_eq!(unique.len(), slate.len(), "duplicates in slate: {:?}", slate);
        }

        /// All selected arms come from the candidate pool.
        #[test]
        fn prop_select_slate_items_from_candidates(
            arms in arb_arm_ids(10),
            n_slots in 1usize..=8,
            seed in any::<u64>(),
        ) {
            let policy = SlatePolicy::new("exp".into(), arms.clone(), n_slots, AttributionModel::ClickedSlotOnly);
            let mut rng = StdRng::seed_from_u64(seed);
            let slate = policy.select_slate(&arms, n_slots, &mut rng);

            let arm_set: std::collections::HashSet<_> = arms.iter().collect();
            for item in &slate {
                prop_assert!(arm_set.contains(item), "selected arm {item} not in candidates");
            }
        }

        /// Beta posteriors remain valid (alpha > 0, beta > 0) after arbitrary updates.
        #[test]
        fn prop_posteriors_valid_after_updates(
            n_arms in 2usize..=8,
            n_slots in 1usize..=5,
            attribution in arb_attribution(),
            bias in arb_position_bias(),
            rewards in proptest::collection::vec(0.0f64..=1.0, 1..=20),
            seed in any::<u64>(),
        ) {
            let arms = (0..n_arms).map(|i| format!("arm-{i}")).collect::<Vec<_>>();
            let actual_slots = n_slots.min(n_arms);
            let mut policy = SlatePolicy::with_position_bias(
                "exp".into(),
                arms.clone(),
                actual_slots,
                attribution,
                bias,
            );
            let mut rng = StdRng::seed_from_u64(seed);

            for reward in &rewards {
                let slate = policy.select_slate(&arms, actual_slots, &mut rng);
                let clicked = if *reward > 0.5 && !slate.is_empty() {
                    Some(rng.gen_range(0..slate.len()))
                } else {
                    None
                };
                policy.update(&slate, clicked, *reward, 1.0);
            }

            // All posteriors must remain valid.
            for (s, slot) in policy.slot_arms.iter().enumerate() {
                for (arm_id, arm) in slot {
                    prop_assert!(arm.alpha > 0.0, "alpha <= 0 for slot {s}, arm {arm_id}");
                    prop_assert!(arm.beta > 0.0, "beta <= 0 for slot {s}, arm {arm_id}");
                    prop_assert!(arm.alpha.is_finite(), "alpha non-finite for slot {s}, arm {arm_id}");
                    prop_assert!(arm.beta.is_finite(), "beta non-finite for slot {s}, arm {arm_id}");
                }
            }
        }

        /// Serialization roundtrip preserves all policy state.
        #[test]
        fn prop_bincode_roundtrip_preserves_state(
            n_arms in 2usize..=6,
            n_slots in 1usize..=4,
            attribution in arb_attribution(),
            bias in arb_position_bias(),
            n_updates in 0usize..=10,
            seed in any::<u64>(),
        ) {
            let arms = (0..n_arms).map(|i| format!("arm-{i}")).collect::<Vec<_>>();
            let actual_slots = n_slots.min(n_arms);
            let mut policy = SlatePolicy::with_position_bias(
                "exp".into(),
                arms.clone(),
                actual_slots,
                attribution.clone(),
                bias.clone(),
            );
            let mut rng = StdRng::seed_from_u64(seed);

            for _ in 0..n_updates {
                let slate = policy.select_slate(&arms, actual_slots, &mut rng);
                if !slate.is_empty() {
                    let clicked = Some(rng.gen_range(0..slate.len()));
                    policy.update(&slate, clicked, 0.5, 1.0);
                }
            }

            let bytes = policy.to_bytes();
            let restored = SlatePolicy::from_bytes(&bytes);

            prop_assert_eq!(restored.experiment_id(), "exp");
            prop_assert_eq!(restored.n_slots, actual_slots);
            prop_assert_eq!(restored.total_updates(), policy.total_updates());
            prop_assert_eq!(&restored.attribution, &attribution);
            prop_assert_eq!(&restored.position_bias, &bias);

            // Verify posteriors match.
            for (s, slot) in policy.slot_arms.iter().enumerate() {
                for (arm_id, arm) in slot {
                    let restored_arm = &restored.slot_arms[s][arm_id];
                    prop_assert!(
                        (arm.alpha - restored_arm.alpha).abs() < 1e-12,
                        "alpha mismatch at slot {s}, arm {arm_id}"
                    );
                    prop_assert!(
                        (arm.beta - restored_arm.beta).abs() < 1e-12,
                        "beta mismatch at slot {s}, arm {arm_id}"
                    );
                }
            }
        }

        /// LIPS estimator always returns finite values and non-negative for non-negative rewards.
        #[test]
        fn prop_lips_always_finite(
            n_logs in 1usize..=20,
            seed in any::<u64>(),
        ) {
            let mut rng = StdRng::seed_from_u64(seed);
            let logs: Vec<SlateLog> = (0..n_logs)
                .map(|i| {
                    let propensity = if rng.gen_bool(0.1) {
                        0.0 // occasionally zero
                    } else {
                        rng.gen_range(0.01..=1.0)
                    };
                    let reward = rng.gen_range(0.0..=1.0);
                    SlateLog {
                        slate: vec![format!("arm-{i}")],
                        clicked: Some(format!("arm-{i}")),
                        clicked_position: Some(0),
                        propensity,
                        reward,
                    }
                })
                .collect();

            let estimate = lips_estimate(&logs);
            prop_assert!(estimate.is_finite(), "LIPS returned non-finite: {estimate}");
        }

        /// Position bias debias_reward always returns a value in [0, 1].
        #[test]
        fn prop_debias_reward_in_unit_interval(
            reward in 0.0f64..=1.0,
            position in 0usize..=20,
            bias in arb_position_bias(),
        ) {
            let debiased = bias.debias_reward(reward, position);
            prop_assert!(debiased >= 0.0, "debiased reward < 0: {debiased}");
            prop_assert!(debiased <= 1.0, "debiased reward > 1: {debiased}");
            prop_assert!(debiased.is_finite(), "debiased reward non-finite: {debiased}");
        }

        /// total_updates increments correctly regardless of attribution/bias model.
        #[test]
        fn prop_total_updates_increments(
            n_arms in 2usize..=6,
            n_slots in 1usize..=4,
            attribution in arb_attribution(),
            n_updates in 1usize..=15,
            seed in any::<u64>(),
        ) {
            let arms = (0..n_arms).map(|i| format!("arm-{i}")).collect::<Vec<_>>();
            let actual_slots = n_slots.min(n_arms);
            let mut policy = SlatePolicy::new("exp".into(), arms.clone(), actual_slots, attribution);
            let mut rng = StdRng::seed_from_u64(seed);

            for _ in 0..n_updates {
                let slate = policy.select_slate(&arms, actual_slots, &mut rng);
                policy.update(&slate, Some(0), 0.5, 1.0);
            }

            prop_assert_eq!(policy.total_updates(), n_updates as u64);
        }
    }
}
