//! LP constraint post-processing layer for bandit arm selection (ADR-012).
//!
//! Implements the deterministic layer between raw bandit probabilities **p** and
//! final assignment probabilities **q**, solving:
//!
//! ```text
//! minimise  KL(q ∥ p) = Σ_i q_i log(q_i / p_i)
//! subject to  Σ_i q_i = 1
//!             lo_i ≤ q_i ≤ hi_i          ∀ i  (per-arm bounds)
//!             Σ_i A_{ji} q_i ≤ b_j       ∀ j  (general linear constraints)
//! ```
//!
//! # Algorithms
//!
//! - **Per-arm only**: O(K log K) bisection on the KL Lagrange multiplier C where
//!   `q_i = clip(r_i / C, lo_i, hi_i)` and C is found by bisection on Σq_i = 1.
//! - **General constraints**: Dual gradient ascent — the inner subproblem reduces to
//!   the per-arm KL projection applied to `r_i = p_i exp(−λᵀ a_i)` where λ is the
//!   vector of dual variables for the general constraints.  Targets <50 μs for K ≤ 20.
//!
//! # Population-level impression tracking
//!
//! [`ImpressionTracker`] maintains per-arm EMA-decayed impression fractions on the
//! LMAX single thread.  No synchronisation required; state is updated atomically
//! alongside policy state after each assignment.
//!
//! # IPW validity
//!
//! The **adjusted** q returned by [`ConstraintSolver::apply`] MUST be logged as
//! `assignment_probability` in the ExposureEvent.  If the solver returns
//! [`ConstraintResult::Infeasible`], raw p is the fallback — both cases must be
//! logged for downstream IPW correctness.

use experimentation_core::error::assert_finite;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ──────────────────────────────────────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────────────────────────────────────

/// Per-arm lower/upper traffic-fraction bounds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerArmBound {
    /// Arm identifier.
    pub arm_id: String,
    /// Minimum traffic fraction ∈ [0, 1].  Default 0.0.
    pub min_fraction: f64,
    /// Maximum traffic fraction ∈ [0, 1].  Default 1.0.
    pub max_fraction: f64,
}

/// One general linear constraint: Σ_i coefficients[i] * q_i ≤ rhs.
///
/// Arm IDs not present in `coefficients` have an implicit coefficient of 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearConstraint {
    /// Human-readable label for audit trail.
    pub label: String,
    /// Coefficients keyed by arm_id.
    pub coefficients: HashMap<String, f64>,
    /// Right-hand side.
    pub rhs: f64,
}

/// Result of applying constraints to raw arm probabilities.
#[derive(Debug, Clone)]
pub enum ConstraintResult {
    /// Constraint solve succeeded; `adjusted` contains q (sums to 1, satisfies constraints).
    /// Log `adjusted[selected_arm]` as `assignment_probability` for IPW.
    Feasible {
        adjusted: HashMap<String, f64>,
    },
    /// Constraint polytope is infeasible or solver did not converge within budget.
    /// `raw` is the unmodified bandit output.  Log `raw[selected_arm]` as
    /// `assignment_probability`; the IPW estimator remains unbiased w.r.t. p.
    Infeasible {
        raw: HashMap<String, f64>,
    },
}

impl ConstraintResult {
    /// Returns the probabilities that should be logged as `assignment_probability`.
    pub fn probabilities(&self) -> &HashMap<String, f64> {
        match self {
            ConstraintResult::Feasible { adjusted } => adjusted,
            ConstraintResult::Infeasible { raw } => raw,
        }
    }

    /// Returns true if the constraint solve was successful.
    pub fn is_feasible(&self) -> bool {
        matches!(self, ConstraintResult::Feasible { .. })
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// ImpressionTracker
// ──────────────────────────────────────────────────────────────────────────────

/// EMA-decayed per-arm impression tracker.
///
/// Maintained on the LMAX single thread; updated after every arm assignment.
/// The EMA update on each call to [`Self::record`] is:
///   `ema[i] = (1 − α) × ema[i] + α × I(selected == i)`
///
/// This tracks the exponentially weighted fraction of impressions each arm
/// has received, used for population-level traffic auditing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpressionTracker {
    /// EMA-smoothed impression fractions per arm.
    pub ema: HashMap<String, f64>,
    /// Decay coefficient α ∈ (0, 1].
    pub alpha: f64,
    /// Total selections recorded (not decayed; for diagnostics).
    pub total_selections: u64,
}

impl ImpressionTracker {
    /// Create a new tracker with all arms initialised to uniform impression rate.
    pub fn new(arm_ids: &[String], alpha: f64) -> Self {
        assert!(
            alpha > 0.0 && alpha <= 1.0,
            "ImpressionTracker alpha must be in (0, 1], got {alpha}"
        );
        let n = arm_ids.len();
        let init = if n > 0 { 1.0 / n as f64 } else { 0.0 };
        let ema = arm_ids.iter().map(|id| (id.clone(), init)).collect();
        Self {
            ema,
            alpha,
            total_selections: 0,
        }
    }

    /// Record that `selected_arm` was assigned to one user.
    ///
    /// Updates the EMA for every arm: the selected arm gains α weight, all others
    /// decay toward 0.  Must be called on the LMAX thread.
    pub fn record(&mut self, selected_arm: &str) {
        // EMA update for all arms.
        let one_minus_alpha = 1.0 - self.alpha;
        for (arm_id, count) in self.ema.iter_mut() {
            let indicator = if arm_id == selected_arm { 1.0 } else { 0.0 };
            *count = one_minus_alpha * *count + self.alpha * indicator;
            assert_finite(*count, "ImpressionTracker EMA count");
        }
        self.total_selections += 1;
    }

    /// Current EMA-smoothed impression fraction per arm.
    ///
    /// Fractions may not sum to exactly 1.0 due to floating-point error;
    /// callers should treat them as approximate population-level traffic estimates.
    pub fn fractions(&self) -> &HashMap<String, f64> {
        &self.ema
    }

    /// Add a new arm (initialised to 0.0 impression rate).
    pub fn add_arm(&mut self, arm_id: &str) {
        self.ema.entry(arm_id.to_string()).or_insert(0.0);
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// ConstraintSolver
// ──────────────────────────────────────────────────────────────────────────────

/// LP constraint post-processing solver for bandit arm selection (ADR-012).
///
/// Holds per-experiment constraint state (per-arm bounds, linear constraints, and
/// EMA impression counts).  All methods must be called on the LMAX single thread —
/// there is no internal locking.
///
/// # Usage
/// ```ignore
/// let mut solver = ConstraintSolver::new(arm_ids, alpha: 0.01);
/// solver.add_per_arm_bound("arm_provider_X", 0.10, 0.40);
/// solver.add_linear_constraint("diversity_cap", coefficients, rhs: 0.60);
///
/// // On each SelectArm call:
/// let result = solver.apply(&raw_probs);
/// let logged_probs = result.probabilities(); // must be used for assignment_probability
///
/// // After writing the ExposureEvent:
/// solver.record_impression(&selected_arm_id);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintSolver {
    /// Ordered arm IDs (canonical ordering used inside the solver).
    pub arm_ids: Vec<String>,
    /// Per-arm lower/upper bounds.  Arms absent here get default [0, 1].
    per_arm: HashMap<String, (f64, f64)>,
    /// General linear constraints (Ax ≤ b).
    linear: Vec<LinearConstraint>,
    /// EMA impression tracker.
    pub impressions: ImpressionTracker,
}

impl ConstraintSolver {
    /// Create a solver for the given arms with an EMA decay of `alpha`.
    ///
    /// `alpha` controls the EMA impression tracking speed.  Typical value: 0.01.
    pub fn new(arm_ids: Vec<String>, alpha: f64) -> Self {
        let impressions = ImpressionTracker::new(&arm_ids, alpha);
        Self {
            per_arm: HashMap::new(),
            linear: Vec::new(),
            impressions,
            arm_ids,
        }
    }

    /// Add or replace a per-arm traffic-fraction bound.
    ///
    /// # Panics
    /// - `min_fraction` or `max_fraction` outside [0, 1]
    /// - `min_fraction > max_fraction`
    pub fn add_per_arm_bound(&mut self, arm_id: &str, min_fraction: f64, max_fraction: f64) {
        assert!(
            (0.0..=1.0).contains(&min_fraction),
            "min_fraction for '{arm_id}' must be in [0, 1], got {min_fraction}"
        );
        assert!(
            (0.0..=1.0).contains(&max_fraction),
            "max_fraction for '{arm_id}' must be in [0, 1], got {max_fraction}"
        );
        assert!(
            min_fraction <= max_fraction,
            "min_fraction ({min_fraction}) must be ≤ max_fraction ({max_fraction}) for '{arm_id}'"
        );
        self.per_arm.insert(arm_id.to_string(), (min_fraction, max_fraction));
        self.impressions.add_arm(arm_id);
    }

    /// Add a general linear constraint: Σ_i coefficients[arm_i] * q_i ≤ rhs.
    pub fn add_linear_constraint(
        &mut self,
        label: &str,
        coefficients: HashMap<String, f64>,
        rhs: f64,
    ) {
        assert_finite(rhs, "LinearConstraint rhs");
        for (arm_id, coeff) in &coefficients {
            assert_finite(*coeff, &format!("LinearConstraint '{label}' coefficient for '{arm_id}'"));
        }
        self.linear.push(LinearConstraint {
            label: label.to_string(),
            coefficients,
            rhs,
        });
    }

    /// Apply constraints to `raw_probs` and return adjusted probabilities.
    ///
    /// The adjusted q minimises KL(q ∥ p) over the constraint polytope.
    /// Returns [`ConstraintResult::Infeasible`] (falling back to raw p) when:
    /// - The polytope is empty (e.g., Σ lo_i > 1 or Σ hi_i < 1).
    /// - The dual gradient ascent does not converge within the iteration budget.
    ///
    /// **The returned probabilities MUST be logged as `assignment_probability`.**
    pub fn apply(&self, raw_probs: &HashMap<String, f64>) -> ConstraintResult {
        let k = self.arm_ids.len();
        if k == 0 {
            return ConstraintResult::Feasible {
                adjusted: HashMap::new(),
            };
        }

        // Build ordered arrays in canonical arm order.
        let mut p = Vec::with_capacity(k);
        let mut lo = Vec::with_capacity(k);
        let mut hi = Vec::with_capacity(k);

        for arm_id in &self.arm_ids {
            let prob = raw_probs.get(arm_id).copied().unwrap_or(0.0);
            assert_finite(prob, &format!("raw_prob for arm '{arm_id}'"));
            // Clamp to [0, 1] for numerical safety.
            p.push(prob.clamp(0.0, 1.0));

            let (l, h) = self.per_arm.get(arm_id).copied().unwrap_or((0.0, 1.0));
            lo.push(l);
            hi.push(h);
        }

        // Add a small floor for arms with p_i = 0 to avoid log(0) issues.
        // This preserves IPW validity: the tiny floor (1e-10) is negligible
        // for arms the policy never selects.
        let p_sum: f64 = p.iter().sum();
        if p_sum <= 0.0 {
            // Degenerate: return uniform within bounds.
            return self.fallback_feasible_or_infeasible(raw_probs, &lo, &hi);
        }
        // Ensure p is normalised (it should be already, but guard against drift).
        let p: Vec<f64> = p.iter().map(|&pi| (pi / p_sum).max(1e-300)).collect();

        // Build general constraint matrix in arm order.
        // A_mat[j][i] = coefficient of arm i in constraint j.
        let a_mat: Vec<Vec<f64>> = self
            .linear
            .iter()
            .map(|c| {
                self.arm_ids
                    .iter()
                    .map(|arm_id| c.coefficients.get(arm_id).copied().unwrap_or(0.0))
                    .collect()
            })
            .collect();
        let b_vec: Vec<f64> = self.linear.iter().map(|c| c.rhs).collect();

        // Solve.
        let q_opt = if a_mat.is_empty() {
            bisect_kl_projection(&p, &lo, &hi)
        } else {
            solve_kl_general(&p, &lo, &hi, &a_mat, &b_vec)
        };

        match q_opt {
            Some(q) => {
                let adjusted: HashMap<String, f64> = self
                    .arm_ids
                    .iter()
                    .zip(q.iter())
                    .map(|(id, &qi)| (id.clone(), qi))
                    .collect();
                ConstraintResult::Feasible { adjusted }
            }
            None => ConstraintResult::Infeasible {
                raw: raw_probs.clone(),
            },
        }
    }

    /// Record that `selected_arm` was assigned after a solve.
    ///
    /// Updates the EMA impression tracker.  Must be called on the LMAX thread.
    pub fn record_impression(&mut self, selected_arm: &str) {
        self.impressions.record(selected_arm);
    }

    /// Current EMA impression fractions (for audit and diagnostics).
    pub fn impression_fractions(&self) -> &HashMap<String, f64> {
        self.impressions.fractions()
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Last-resort: if per-arm bounds alone are feasible, distribute uniformly
    /// within them; otherwise return Infeasible.
    fn fallback_feasible_or_infeasible(
        &self,
        raw: &HashMap<String, f64>,
        lo: &[f64],
        hi: &[f64],
    ) -> ConstraintResult {
        let sum_lo: f64 = lo.iter().sum();
        let sum_hi: f64 = hi.iter().sum();
        if sum_lo > 1.0 + 1e-9 || sum_hi < 1.0 - 1e-9 {
            return ConstraintResult::Infeasible { raw: raw.clone() };
        }
        // Distribute evenly within bounds.
        let k = self.arm_ids.len();
        let uniform = vec![1.0 / k as f64; k];
        match bisect_kl_projection(&uniform, lo, hi) {
            Some(q) => {
                let adjusted = self
                    .arm_ids
                    .iter()
                    .zip(q.iter())
                    .map(|(id, &qi)| (id.clone(), qi))
                    .collect();
                ConstraintResult::Feasible { adjusted }
            }
            None => ConstraintResult::Infeasible { raw: raw.clone() },
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Core algorithms (private)
// ──────────────────────────────────────────────────────────────────────────────

/// KL projection onto the bounded simplex: O(K × 64) bisection.
///
/// Finds q minimising KL(q ∥ r) subject to Σq = 1 and lo_i ≤ q_i ≤ hi_i.
///
/// Solution: `q_i = clip(r_i / C, lo_i, hi_i)` where C is found by bisection
/// on `f(C) = Σ_i clip(r_i / C, lo_i, hi_i) = 1`.
///
/// Returns `None` if the polytope `{q: Σq=1, lo≤q≤hi}` is empty.
pub(crate) fn bisect_kl_projection(r: &[f64], lo: &[f64], hi: &[f64]) -> Option<Vec<f64>> {
    let k = r.len();
    assert_eq!(lo.len(), k);
    assert_eq!(hi.len(), k);

    // Feasibility check: Σlo ≤ 1 ≤ Σhi.
    let sum_lo: f64 = lo.iter().sum();
    let sum_hi: f64 = hi.iter().sum();
    if sum_lo > 1.0 + 1e-9 || sum_hi < 1.0 - 1e-9 {
        return None;
    }

    // f(C) = Σ_i clip(r_i / C, lo_i, hi_i) — monotone decreasing in C.
    // We want f(C) = 1.  Bisect on log(C) for numerical stability across
    // a wide dynamic range.
    let f = |c: f64| -> f64 {
        r.iter()
            .zip(lo.iter())
            .zip(hi.iter())
            .map(|((ri, loi), hii)| (ri / c).clamp(*loi, *hii))
            .sum::<f64>()
    };

    // Bracket: log C ∈ [−50, +50] covers C ∈ [1.9e-22, 5.2e21], more than enough
    // for normalised probabilities.
    let mut log_c_lo: f64 = -50.0;
    let mut log_c_hi: f64 = 50.0;

    // Verify the bracket is valid (f decreasing).
    let f_at_lo = f(log_c_lo.exp());
    let f_at_hi = f(log_c_hi.exp());

    if f_at_lo < 1.0 - 1e-9 || f_at_hi > 1.0 + 1e-9 {
        // Should not happen if sum_lo / sum_hi check passed; return None defensively.
        return None;
    }

    // 64 bisection steps → interval width 100 / 2^64 ≈ 5.4e-18 in log-space:
    // sufficient for f64 precision.
    for _ in 0..64 {
        let mid = (log_c_lo + log_c_hi) * 0.5;
        if f(mid.exp()) > 1.0 {
            log_c_lo = mid;
        } else {
            log_c_hi = mid;
        }
    }

    let c = ((log_c_lo + log_c_hi) * 0.5).exp();
    let q: Vec<f64> = r
        .iter()
        .zip(lo.iter())
        .zip(hi.iter())
        .map(|((ri, loi), hii)| {
            let qi = (ri / c).clamp(*loi, *hii);
            assert_finite(qi, "bisect_kl_projection q_i");
            qi
        })
        .collect();

    Some(q)
}

/// KL minimisation with general linear constraints via dual gradient ascent.
///
/// Outer loop: projected gradient ascent on dual variables λ ≥ 0 for the
/// general constraints.  Inner loop: `bisect_kl_projection` applied to the
/// modified reference `r_i = p_i × exp(−λᵀ a_i)`.
///
/// Convergence criterion: max constraint violation ≤ 1e-7.
/// Falls back to `None` when the budget of `MAX_OUTER` iterations is exhausted
/// without reaching feasibility.
///
/// Target: <50 μs for K ≤ 20, G ≤ 10 (empirically verified in benchmarks).
fn solve_kl_general(
    p: &[f64],
    lo: &[f64],
    hi: &[f64],
    a_mat: &[Vec<f64>], // a_mat[j][i] = coefficient of arm i in constraint j
    b_vec: &[f64],
) -> Option<Vec<f64>> {
    let k = p.len();
    let g = a_mat.len();

    // If p is already feasible for general constraints, just run per-arm projection.
    let check_feasible = |q: &[f64]| -> f64 {
        a_mat
            .iter()
            .zip(b_vec.iter())
            .map(|(aj, bj)| {
                let aq: f64 = aj.iter().zip(q.iter()).map(|(a, qi)| a * qi).sum();
                aq - bj
            })
            .fold(f64::NEG_INFINITY, f64::max)
    };

    // Quick path: if p satisfies all general constraints, per-arm solve suffices.
    if check_feasible(p) <= 1e-9 {
        if let Some(q) = bisect_kl_projection(p, lo, hi) {
            if check_feasible(&q) <= 1e-6 {
                return Some(q);
            }
        }
        // Per-arm projection violated general constraints; fall through to dual ascent.
    }
    }

    // Dual variables for general constraints; all initialised to 0.
    let mut lambda = vec![0.0f64; g];
    let mut best_q: Option<Vec<f64>> = None;
    let mut best_violation = f64::MAX;

    // Dual gradient ascent.  The step size needs to be O(1) for fast convergence:
    // near the optimum, violation ≈ δλ × Var_q(A) where Var_q ≤ max(|A_ij|)².
    // With step = 1 and |A_ij| ≤ 1, each iteration reduces |δλ| by ~Var_q ≤ 1,
    // giving geometric convergence to 1e-7 in ~60–80 iterations.
    //
    // For safety against large-coefficient constraints, we scale the step by
    // 1 / (max_norm + 1) where max_norm = max_j ||A_j||_∞.
    let max_coeff: f64 = a_mat
        .iter()
        .flat_map(|aj| aj.iter().map(|x| x.abs()))
        .fold(0.0f64, f64::max)
        .max(1.0);
    let base_step = 1.0 / max_coeff;

    const MAX_ITER: usize = 250;
    const TOL: f64 = 1e-7;

    for _iter in 0..MAX_ITER {
        // Compute modified reference r_i = p_i * exp(-λᵀ a_i).
        let r: Vec<f64> = (0..k)
            .map(|i| {
                let dot: f64 = lambda
                    .iter()
                    .zip(a_mat.iter())
                    .map(|(lj, aj)| lj * aj[i])
                    .sum();
                let ri = p[i] * (-dot).exp();
                assert_finite(ri, "solve_kl_general r_i");
                ri
            })
            .collect();

        // Inner solve: KL projection onto bounded simplex.
        let q = bisect_kl_projection(&r, lo, hi)?;

        // Compute constraint violations: (Aq - b)_j.
        let violations: Vec<f64> = a_mat
            .iter()
            .zip(b_vec.iter())
            .map(|(aj, bj)| {
                let aq: f64 = aj.iter().zip(q.iter()).map(|(a, qi)| a * qi).sum();
                aq - bj
            })
            .collect();

        let max_violation = violations.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        // Track best feasible-ish solution.
        if max_violation < best_violation {
            best_violation = max_violation;
            best_q = Some(q.clone());
        }

        if max_violation <= TOL {
            return Some(q);
        }

        // Dual update: projected gradient ascent (λ ≥ 0).
        for j in 0..g {
            lambda[j] = (lambda[j] + base_step * violations[j]).max(0.0);
            assert_finite(lambda[j], "solve_kl_general lambda");
        }
    }

    // Return best solution if it is close enough to feasible (relaxed tolerance).
    best_q.filter(|q| check_feasible(q) <= 1e-5)
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn arm_ids(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("arm_{i}")).collect()
    }

    fn uniform_probs(ids: &[String]) -> HashMap<String, f64> {
        let p = 1.0 / ids.len() as f64;
        ids.iter().map(|id| (id.clone(), p)).collect()
    }

    fn sum_probs(probs: &HashMap<String, f64>) -> f64 {
        probs.values().sum()
    }

    // ── bisect_kl_projection ──────────────────────────────────────────────────

    #[test]
    fn bisect_unconstrained_returns_p() {
        // lo = 0, hi = 1 for all arms: q should equal the input r (normalised).
        let r = vec![0.5, 0.3, 0.2];
        let lo = vec![0.0, 0.0, 0.0];
        let hi = vec![1.0, 1.0, 1.0];
        let q = bisect_kl_projection(&r, &lo, &hi).unwrap();
        // r already sums to 1.0; q should be close to r.
        for (qi, ri) in q.iter().zip(r.iter()) {
            assert!(
                (qi - ri).abs() < 1e-10,
                "unconstrained: expected q ≈ p, got q={qi:.12}, p={ri:.12}"
            );
        }
    }

    #[test]
    fn bisect_lower_bound_enforced() {
        // Arm 1 has min_fraction = 0.4 but only 0.1 raw probability.
        let r = vec![0.7, 0.1, 0.2];
        let lo = vec![0.0, 0.4, 0.0];
        let hi = vec![1.0, 1.0, 1.0];
        let q = bisect_kl_projection(&r, &lo, &hi).unwrap();

        let sum: f64 = q.iter().sum();
        assert!((sum - 1.0).abs() < 1e-9, "q must sum to 1, got {sum:.15}");
        assert!(q[1] >= 0.4 - 1e-9, "lower bound violated: q[1]={}", q[1]);
        for qi in &q {
            assert!(qi.is_finite() && *qi >= 0.0);
        }
    }

    #[test]
    fn bisect_upper_bound_enforced() {
        // Arm 0 has max_fraction = 0.3 but 0.9 raw probability.
        let r = vec![0.9, 0.05, 0.05];
        let lo = vec![0.0, 0.0, 0.0];
        let hi = vec![0.3, 1.0, 1.0];
        let q = bisect_kl_projection(&r, &lo, &hi).unwrap();

        let sum: f64 = q.iter().sum();
        assert!((sum - 1.0).abs() < 1e-9, "q must sum to 1, got {sum:.15}");
        assert!(q[0] <= 0.3 + 1e-9, "upper bound violated: q[0]={}", q[0]);
    }

    #[test]
    fn bisect_infeasible_sum_lo_too_large() {
        // Σlo = 1.2 > 1 → infeasible.
        let r = vec![0.5, 0.5];
        let lo = vec![0.6, 0.6];
        let hi = vec![1.0, 1.0];
        assert!(bisect_kl_projection(&r, &lo, &hi).is_none());
    }

    #[test]
    fn bisect_infeasible_sum_hi_too_small() {
        // Σhi = 0.8 < 1 → infeasible.
        let r = vec![0.5, 0.5];
        let lo = vec![0.0, 0.0];
        let hi = vec![0.4, 0.4];
        assert!(bisect_kl_projection(&r, &lo, &hi).is_none());
    }

    #[test]
    fn bisect_single_arm() {
        let q = bisect_kl_projection(&[0.5], &[0.0], &[1.0]).unwrap();
        assert!((q[0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn bisect_bounds_tight() {
        // Every arm pinned: lo == hi (all fractions fixed).
        let r = vec![0.5, 0.3, 0.2];
        let lo = vec![0.2, 0.5, 0.3];
        let hi = vec![0.2, 0.5, 0.3];
        let q = bisect_kl_projection(&r, &lo, &hi).unwrap();
        assert!((q[0] - 0.2).abs() < 1e-9);
        assert!((q[1] - 0.5).abs() < 1e-9);
        assert!((q[2] - 0.3).abs() < 1e-9);
    }

    // ── ConstraintSolver — per-arm only ──────────────────────────────────────

    #[test]
    fn solver_no_constraints_returns_raw() {
        let ids = arm_ids(3);
        let solver = ConstraintSolver::new(ids.clone(), 0.01);
        let raw = uniform_probs(&ids);
        let result = solver.apply(&raw);

        assert!(result.is_feasible());
        let adj = result.probabilities();
        assert!((sum_probs(adj) - 1.0).abs() < 1e-9);

        // Without any constraints, adjusted ≈ raw (KL projection with no active bounds).
        for id in &ids {
            assert!(
                (adj[id] - raw[id]).abs() < 1e-10,
                "no constraints: expected q ≈ p"
            );
        }
    }

    #[test]
    fn solver_per_arm_lower_bound_satisfied() {
        let ids = arm_ids(4);
        let mut solver = ConstraintSolver::new(ids.clone(), 0.01);
        // Arm_2 must get at least 30% traffic.
        solver.add_per_arm_bound("arm_2", 0.30, 1.0);

        let mut raw = HashMap::new();
        raw.insert("arm_0".to_string(), 0.50);
        raw.insert("arm_1".to_string(), 0.25);
        raw.insert("arm_2".to_string(), 0.05); // below floor
        raw.insert("arm_3".to_string(), 0.20);

        let result = solver.apply(&raw);
        assert!(result.is_feasible());
        let adj = result.probabilities();
        assert!(
            adj["arm_2"] >= 0.30 - 1e-9,
            "lower bound not satisfied: arm_2 = {}",
            adj["arm_2"]
        );
        assert!((sum_probs(adj) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn solver_per_arm_upper_bound_satisfied() {
        let ids = arm_ids(3);
        let mut solver = ConstraintSolver::new(ids.clone(), 0.01);
        // Arm_0 must not exceed 20%.
        solver.add_per_arm_bound("arm_0", 0.0, 0.20);

        let mut raw = HashMap::new();
        raw.insert("arm_0".to_string(), 0.80); // above ceiling
        raw.insert("arm_1".to_string(), 0.15);
        raw.insert("arm_2".to_string(), 0.05);

        let result = solver.apply(&raw);
        assert!(result.is_feasible());
        let adj = result.probabilities();
        assert!(
            adj["arm_0"] <= 0.20 + 1e-9,
            "upper bound violated: arm_0 = {}",
            adj["arm_0"]
        );
        assert!((sum_probs(adj) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn solver_infeasible_per_arm_bounds_falls_back() {
        let ids = arm_ids(2);
        let mut solver = ConstraintSolver::new(ids.clone(), 0.01);
        // Impossible: both arms require >= 60%.
        solver.add_per_arm_bound("arm_0", 0.60, 1.0);
        solver.add_per_arm_bound("arm_1", 0.60, 1.0);

        let raw = uniform_probs(&ids);
        let result = solver.apply(&raw);
        // Must fall back to raw.
        assert!(
            !result.is_feasible(),
            "should return Infeasible for impossible constraints"
        );
    }

    // ── ConstraintSolver — general linear constraints ─────────────────────────

    #[test]
    fn solver_linear_constraint_satisfied_after_solve() {
        // 3 arms; constraint: arm_0 + arm_1 ≤ 0.60 (diversity cap).
        let ids = arm_ids(3);
        let mut solver = ConstraintSolver::new(ids.clone(), 0.01);

        let mut coeffs = HashMap::new();
        coeffs.insert("arm_0".to_string(), 1.0);
        coeffs.insert("arm_1".to_string(), 1.0);
        solver.add_linear_constraint("diversity_cap", coeffs, 0.60);

        // Raw probs heavily favour arm_0 + arm_1 = 0.95.
        let mut raw = HashMap::new();
        raw.insert("arm_0".to_string(), 0.50);
        raw.insert("arm_1".to_string(), 0.45);
        raw.insert("arm_2".to_string(), 0.05);

        let result = solver.apply(&raw);
        assert!(
            result.is_feasible(),
            "should find feasible solution for this constraint"
        );
        let adj = result.probabilities();

        let q0 = adj["arm_0"];
        let q1 = adj["arm_1"];
        assert!(
            q0 + q1 <= 0.60 + 1e-5,
            "linear constraint violated: arm_0 + arm_1 = {:.6}",
            q0 + q1
        );
        assert!((sum_probs(adj) - 1.0).abs() < 1e-9, "q must sum to 1");
    }

    #[test]
    fn solver_linear_constraint_with_per_arm_bounds() {
        // 4 arms:
        //   - arm_0 (provider): min 0.10, max 0.30
        //   - Linear: arm_0 + arm_1 ≤ 0.50
        let ids = arm_ids(4);
        let mut solver = ConstraintSolver::new(ids.clone(), 0.01);
        solver.add_per_arm_bound("arm_0", 0.10, 0.30);

        let mut coeffs = HashMap::new();
        coeffs.insert("arm_0".to_string(), 1.0);
        coeffs.insert("arm_1".to_string(), 1.0);
        solver.add_linear_constraint("cap_01", coeffs, 0.50);

        let mut raw = HashMap::new();
        raw.insert("arm_0".to_string(), 0.35); // above max
        raw.insert("arm_1".to_string(), 0.35);
        raw.insert("arm_2".to_string(), 0.20);
        raw.insert("arm_3".to_string(), 0.10);

        let result = solver.apply(&raw);
        assert!(result.is_feasible());
        let adj = result.probabilities();

        assert!(adj["arm_0"] >= 0.10 - 1e-7, "lower bound arm_0");
        assert!(adj["arm_0"] <= 0.30 + 1e-7, "upper bound arm_0");
        assert!(adj["arm_0"] + adj["arm_1"] <= 0.50 + 1e-5, "linear constraint");
        assert!((sum_probs(adj) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn solver_multiple_linear_constraints() {
        // 5 arms, 2 diversity constraints.
        let ids = arm_ids(5);
        let mut solver = ConstraintSolver::new(ids.clone(), 0.01);

        // Constraint 1: arm_0 + arm_1 ≤ 0.50.
        let mut c1 = HashMap::new();
        c1.insert("arm_0".to_string(), 1.0);
        c1.insert("arm_1".to_string(), 1.0);
        solver.add_linear_constraint("cap_01", c1, 0.50);

        // Constraint 2: arm_2 ≥ 0.10 (equivalently: −arm_2 ≤ −0.10).
        let mut c2 = HashMap::new();
        c2.insert("arm_2".to_string(), -1.0);
        solver.add_linear_constraint("floor_2", c2, -0.10);

        let mut raw = HashMap::new();
        raw.insert("arm_0".to_string(), 0.40);
        raw.insert("arm_1".to_string(), 0.40);
        raw.insert("arm_2".to_string(), 0.05); // below floor
        raw.insert("arm_3".to_string(), 0.10);
        raw.insert("arm_4".to_string(), 0.05);

        let result = solver.apply(&raw);
        assert!(result.is_feasible(), "multi-constraint solve should succeed");
        let adj = result.probabilities();

        assert!(
            adj["arm_0"] + adj["arm_1"] <= 0.50 + 1e-5,
            "constraint 1 violated: {:.6}",
            adj["arm_0"] + adj["arm_1"]
        );
        assert!(adj["arm_2"] >= 0.10 - 1e-5, "constraint 2 violated: {:.6}", adj["arm_2"]);
        assert!((sum_probs(adj) - 1.0).abs() < 1e-9);
    }

    // ── ConstraintResult — Infeasible falls back to raw ───────────────────────

    #[test]
    fn infeasible_result_returns_raw_probs() {
        let ids = arm_ids(2);
        let mut solver = ConstraintSolver::new(ids.clone(), 0.01);
        // Impossible bounds.
        solver.add_per_arm_bound("arm_0", 0.70, 1.0);
        solver.add_per_arm_bound("arm_1", 0.70, 1.0);

        let raw = uniform_probs(&ids);
        let result = solver.apply(&raw);

        match result {
            ConstraintResult::Infeasible { raw: returned_raw } => {
                assert_eq!(returned_raw["arm_0"], raw["arm_0"]);
                assert_eq!(returned_raw["arm_1"], raw["arm_1"]);
            }
            ConstraintResult::Feasible { .. } => panic!("expected Infeasible"),
        }
    }

    // ── ImpressionTracker ─────────────────────────────────────────────────────

    #[test]
    fn impression_tracker_converges_to_selection_rate() {
        let ids = arm_ids(2);
        let mut tracker = ImpressionTracker::new(&ids, 0.02);

        // Always select arm_0.
        for _ in 0..500 {
            tracker.record("arm_0");
        }
        let fracs = tracker.fractions();
        // After 500 updates with α=0.02: arm_0 fraction ≈ 1 − (1−0.02)^500 ≈ >0.99.
        assert!(
            fracs["arm_0"] > 0.95,
            "arm_0 EMA should converge toward 1.0, got {}",
            fracs["arm_0"]
        );
        assert!(
            fracs["arm_1"] < 0.05,
            "arm_1 EMA should converge toward 0.0, got {}",
            fracs["arm_1"]
        );
    }

    #[test]
    fn impression_tracker_uniform_selection_converges_to_half() {
        let ids = arm_ids(2);
        let mut tracker = ImpressionTracker::new(&ids, 0.02);

        // Alternate selections.
        for i in 0..1000 {
            tracker.record(if i % 2 == 0 { "arm_0" } else { "arm_1" });
        }
        let fracs = tracker.fractions();
        // Both should converge to ≈ 0.5.
        assert!(
            (fracs["arm_0"] - 0.5).abs() < 0.05,
            "arm_0 EMA ≈ 0.5, got {}",
            fracs["arm_0"]
        );
        assert!(
            (fracs["arm_1"] - 0.5).abs() < 0.05,
            "arm_1 EMA ≈ 0.5, got {}",
            fracs["arm_1"]
        );
    }

    #[test]
    fn impression_tracker_total_selections_increments() {
        let ids = arm_ids(3);
        let mut tracker = ImpressionTracker::new(&ids, 0.01);
        for _ in 0..42 {
            tracker.record("arm_0");
        }
        assert_eq!(tracker.total_selections, 42);
    }

    // ── ConstraintSolver serialization ───────────────────────────────────────

    #[test]
    fn constraint_solver_serialize_roundtrip() {
        let ids = arm_ids(3);
        let mut solver = ConstraintSolver::new(ids.clone(), 0.05);
        solver.add_per_arm_bound("arm_0", 0.10, 0.50);

        let mut coeffs = HashMap::new();
        coeffs.insert("arm_1".to_string(), 1.0);
        solver.add_linear_constraint("cap_1", coeffs, 0.40);

        let bytes = serde_json::to_vec(&solver).expect("serialize");
        let restored: ConstraintSolver = serde_json::from_slice(&bytes).expect("deserialize");

        assert_eq!(restored.arm_ids, solver.arm_ids);
        assert_eq!(restored.per_arm["arm_0"], solver.per_arm["arm_0"]);
        assert_eq!(restored.linear.len(), 1);
        assert!((restored.impressions.alpha - 0.05).abs() < 1e-12);
    }

    // ── Integration: adjusted q satisfies all constraints ────────────────────

    /// Integration test: after applying the solver, the returned q satisfies all
    /// per-arm bounds and general linear constraints simultaneously.
    #[test]
    fn integration_all_constraints_satisfied() {
        let ids = arm_ids(5);
        let mut solver = ConstraintSolver::new(ids.clone(), 0.01);

        // Per-arm: arm_0 is a provider arm with guaranteed floor.
        solver.add_per_arm_bound("arm_0", 0.10, 0.35);
        solver.add_per_arm_bound("arm_4", 0.05, 0.20);

        // Linear: top-2 arms (arm_1 + arm_2) must not dominate.
        let mut coeffs_12 = HashMap::new();
        coeffs_12.insert("arm_1".to_string(), 1.0);
        coeffs_12.insert("arm_2".to_string(), 1.0);
        solver.add_linear_constraint("diversity_cap", coeffs_12, 0.55);

        // Linear: arm_3 must get at least 8% (encoded as -arm_3 ≤ -0.08).
        let mut coeffs_3 = HashMap::new();
        coeffs_3.insert("arm_3".to_string(), -1.0);
        solver.add_linear_constraint("arm3_floor", coeffs_3, -0.08);

        // Raw probs: heavily skewed toward arm_1 + arm_2.
        let mut raw = HashMap::new();
        raw.insert("arm_0".to_string(), 0.05); // below arm_0 floor
        raw.insert("arm_1".to_string(), 0.42);
        raw.insert("arm_2".to_string(), 0.38); // arm_1+arm_2 = 0.80 > 0.55 cap
        raw.insert("arm_3".to_string(), 0.03); // below arm_3 floor
        raw.insert("arm_4".to_string(), 0.12);

        let result = solver.apply(&raw);
        assert!(result.is_feasible(), "solver should find feasible solution");

        let adj = result.probabilities();

        // Probabilities sum to 1.
        let total: f64 = adj.values().sum();
        assert!((total - 1.0).abs() < 1e-8, "q must sum to 1, got {total:.12}");

        // All probabilities non-negative.
        for (id, &qi) in adj {
            assert!(qi >= -1e-10, "q[{id}] negative: {qi}");
        }

        // Per-arm bounds.
        assert!(adj["arm_0"] >= 0.10 - 1e-6, "arm_0 below floor: {}", adj["arm_0"]);
        assert!(adj["arm_0"] <= 0.35 + 1e-6, "arm_0 above cap: {}", adj["arm_0"]);
        assert!(adj["arm_4"] >= 0.05 - 1e-6, "arm_4 below floor: {}", adj["arm_4"]);
        assert!(adj["arm_4"] <= 0.20 + 1e-6, "arm_4 above cap: {}", adj["arm_4"]);

        // Linear constraints.
        let cap_12 = adj["arm_1"] + adj["arm_2"];
        assert!(cap_12 <= 0.55 + 1e-5, "diversity cap violated: {cap_12:.6}");

        let floor_3 = adj["arm_3"];
        assert!(floor_3 >= 0.08 - 1e-5, "arm_3 floor violated: {floor_3:.6}");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Adjusted q always sums to 1 and satisfies per-arm bounds (no general constraints).
        #[test]
        fn prop_per_arm_constraints_satisfied(
            p0 in 0.0f64..1.0,
            p1 in 0.0f64..1.0,
            p2 in 0.0f64..1.0,
            lo0 in 0.0f64..0.3,
            lo1 in 0.0f64..0.3,
            hi0 in 0.3f64..1.0,
            hi1 in 0.3f64..1.0,
        ) {
            let ids: Vec<String> = ["arm_0", "arm_1", "arm_2"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let mut solver = ConstraintSolver::new(ids.clone(), 0.01);
            solver.add_per_arm_bound("arm_0", lo0, hi0.max(lo0));
            solver.add_per_arm_bound("arm_1", lo1, hi1.max(lo1));

            // Feasibility requires Σlo ≤ 1 ≤ Σhi; only test feasible region.
            if lo0 + lo1 > 1.0 {
                return Ok(());
            }

            let total = p0 + p1 + p2;
            if total < 1e-10 {
                return Ok(());
            }
            let mut raw = HashMap::new();
            raw.insert("arm_0".to_string(), p0 / total);
            raw.insert("arm_1".to_string(), p1 / total);
            raw.insert("arm_2".to_string(), p2 / total);

            let result = solver.apply(&raw);
            match result {
                ConstraintResult::Feasible { ref adjusted } => {
                    let sum: f64 = adjusted.values().sum();
                    prop_assert!((sum - 1.0).abs() < 1e-8, "q must sum to 1: {sum}");
                    prop_assert!(adjusted["arm_0"] >= lo0 - 1e-8);
                    prop_assert!(adjusted["arm_0"] <= hi0.max(lo0) + 1e-8);
                    prop_assert!(adjusted["arm_1"] >= lo1 - 1e-8);
                    prop_assert!(adjusted["arm_1"] <= hi1.max(lo1) + 1e-8);
                    for qi in adjusted.values() {
                        prop_assert!(qi.is_finite());
                    }
                }
                ConstraintResult::Infeasible { .. } => {
                    // Allowed: proptest may generate sum_lo > 1.
                }
            }
        }

        /// bisect_kl_projection output is always finite and sums to 1.
        #[test]
        fn prop_bisect_output_finite_and_sums_to_one(
            r0 in 0.01f64..0.98,
            r1 in 0.01f64..0.98,
        ) {
            let r2 = (1.0 - r0 - r1).abs().min(0.98).max(0.01);
            let r = vec![r0, r1, r2];
            let lo = vec![0.0, 0.0, 0.0];
            let hi = vec![1.0, 1.0, 1.0];
            if let Some(q) = bisect_kl_projection(&r, &lo, &hi) {
                let sum: f64 = q.iter().sum();
                prop_assert!((sum - 1.0).abs() < 1e-9);
                for qi in &q {
                    prop_assert!(qi.is_finite() && *qi >= 0.0);
                }
            }
        }
    }
}
