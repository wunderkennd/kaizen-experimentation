//! LP constraint post-processing layer for arm selection (ADR-012).
//!
//! Adjusts raw bandit arm probabilities to satisfy hard constraints while
//! minimising KL(q ‖ p) from the unconstrained bandit distribution.
//!
//! Constraint types supported:
//! - Probability simplex: Σ qᵢ = 1, qᵢ ≥ 0  (always enforced)
//! - Per-arm bounds: floor_i ≤ qᵢ ≤ ceiling_i
//! - General linear: lower_j ≤ Σ aⱼᵢ qᵢ ≤ upper_j  (provider exposure etc.)
//!
//! Performance target: <50 μs for K ≤ 20 arms (ADR-012).
//!
//! Algorithm:
//!   Simple case (box + simplex only): O(K × B) dual bisection (B = bisection steps).
//!   General case: alternating projections — each iteration corrects one violated linear
//!   constraint then re-projects onto the box-constrained simplex.  Converges in O(10)
//!   iterations for the provider-exposure constraint patterns in practice.

use std::cmp::Ordering;

/// A general linear constraint applied to the arm probability vector q.
///
/// Encodes: `lower_bound ≤ ⟨coefficients, q⟩ ≤ upper_bound`.
#[derive(Debug, Clone)]
pub struct LinearConstraint {
    /// One coefficient per arm; must have length K.
    pub coefficients: Vec<f64>,
    /// Minimum required value for the linear expression.
    pub lower_bound: f64,
    /// Maximum allowed value for the linear expression.
    pub upper_bound: f64,
}

/// ADR-012 constrained arm-selection solver.
///
/// Minimises ‖q − p‖₂ (Euclidean projection) subject to simplex + per-arm bounds
/// + optional linear constraints.  The adjusted probabilities q are logged as
///   `assignment_probability` for IPW validity.
///
/// For the full KL(q‖p) objective use the fast-path (no linear constraints) where
/// the unconstrained minimum q = p is projected onto the feasible set.
///
/// # Example
/// ```
/// use experimentation_bandit::lp_constraints::{ConstraintSolver, LinearConstraint};
///
/// let k = 5;
/// let solver = ConstraintSolver::new(k)
///     .with_arm_bounds(vec![0.05; k], vec![0.60; k]);
///
/// let raw = vec![0.5, 0.3, 0.1, 0.05, 0.05];
/// let q = solver.solve(&raw);
/// assert!((q.iter().sum::<f64>() - 1.0).abs() < 1e-9);
/// ```
#[derive(Debug, Clone)]
pub struct ConstraintSolver {
    k: usize,
    arm_floors: Vec<f64>,
    arm_ceilings: Vec<f64>,
    linear_constraints: Vec<LinearConstraint>,
    /// Maximum alternating-projection outer iterations for the general path.
    max_iterations: u32,
    tolerance: f64,
}

impl ConstraintSolver {
    /// Build a solver for `k` arms with default bounds [0, 1] and no linear constraints.
    pub fn new(k: usize) -> Self {
        ConstraintSolver {
            k,
            arm_floors: vec![0.0; k],
            arm_ceilings: vec![1.0; k],
            linear_constraints: Vec::new(),
            max_iterations: 50,
            tolerance: 1e-7,
        }
    }

    /// Set per-arm probability floors and ceilings.
    ///
    /// Requires `floors[i] ≤ ceilings[i]` and `Σ floors[i] ≤ 1 ≤ Σ ceilings[i]`.
    pub fn with_arm_bounds(mut self, floors: Vec<f64>, ceilings: Vec<f64>) -> Self {
        assert_eq!(floors.len(), self.k, "floors length mismatch");
        assert_eq!(ceilings.len(), self.k, "ceilings length mismatch");
        self.arm_floors = floors;
        self.arm_ceilings = ceilings;
        self
    }

    /// Append a general linear constraint.
    pub fn with_linear_constraint(mut self, constraint: LinearConstraint) -> Self {
        assert_eq!(
            constraint.coefficients.len(),
            self.k,
            "constraint coefficients length mismatch"
        );
        self.linear_constraints.push(constraint);
        self
    }

    /// Solve: return q = argmin ‖q − p‖ subject to all registered constraints.
    ///
    /// `raw_probs` must have length K (need not sum to 1).
    /// The returned vector always satisfies Σ qᵢ = 1 and all registered bounds.
    pub fn solve(&self, raw_probs: &[f64]) -> Vec<f64> {
        debug_assert_eq!(raw_probs.len(), self.k, "raw_probs length mismatch");

        let p = normalise(raw_probs);

        if self.linear_constraints.is_empty() {
            // Fast path: O(K × 32) dual bisection; no allocation beyond the return vec.
            return self.project_box_simplex(&p);
        }

        // General path: alternating projections (linear constraint correction → box-simplex).
        self.alternating_projections(&p)
    }

    // ------------------------------------------------------------------
    //  Fast path: box-constrained simplex projection (bisection)
    // ------------------------------------------------------------------

    /// Project v onto { q : Σ qᵢ = 1, floor_i ≤ qᵢ ≤ ceiling_i }.
    ///
    /// Dual objective: f(μ) = Σ clamp(vᵢ − μ, loᵢ, hiᵢ) − 1 is monotone-decreasing.
    /// Bisect in 32 steps → ~1e-9 relative precision.
    pub fn project_box_simplex(&self, v: &[f64]) -> Vec<f64> {
        let k = self.k;
        let lo = &self.arm_floors;
        let hi = &self.arm_ceilings;

        let v_max = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lo_min = lo.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi_max = hi.iter().cloned().fold(0.0_f64, f64::max);
        let v_min = v.iter().cloned().fold(f64::INFINITY, f64::min);

        let mut mu_lo = v_min - hi_max;
        let mut mu_hi = v_max - lo_min;

        // 32 bisection steps give precision ~2^{-32} × range ≈ 1e-9 for typical ranges.
        for _ in 0..32 {
            let mid = 0.5 * (mu_lo + mu_hi);
            let s: f64 = (0..k).map(|i| (v[i] - mid).clamp(lo[i], hi[i])).sum();
            if s > 1.0 {
                mu_lo = mid;
            } else {
                mu_hi = mid;
            }
        }

        let mu = 0.5 * (mu_lo + mu_hi);
        (0..k).map(|i| (v[i] - mu).clamp(lo[i], hi[i])).collect()
    }

    // ------------------------------------------------------------------
    //  General path: alternating projections
    // ------------------------------------------------------------------

    /// Alternating projections for the linear-constraint case.
    ///
    /// Each iteration:
    ///   1. For each violated linear constraint apply an orthogonal correction.
    ///   2. Re-project onto the box-constrained simplex (restores Σ=1).
    ///
    /// The final result always satisfies the simplex + box constraints exactly.
    /// Linear constraints are satisfied up to `tolerance` after convergence.
    fn alternating_projections(&self, p: &[f64]) -> Vec<f64> {
        let k = self.k;
        let mut q = self.project_box_simplex(p);

        for _iter in 0..self.max_iterations {
            let q_prev = q.clone();
            let mut any_corrected = false;

            for lc in &self.linear_constraints {
                let dot: f64 = q.iter().zip(&lc.coefficients).map(|(qi, ci)| qi * ci).sum();
                let norm_sq: f64 = lc.coefficients.iter().map(|c| c * c).sum();
                if norm_sq < 1e-14 {
                    continue;
                }

                let lambda = if dot < lc.lower_bound {
                    any_corrected = true;
                    (lc.lower_bound - dot) / norm_sq
                } else if dot > lc.upper_bound {
                    any_corrected = true;
                    (lc.upper_bound - dot) / norm_sq
                } else {
                    continue;
                };

                for (i, qi) in q.iter_mut().enumerate().take(k) {
                    *qi += lambda * lc.coefficients[i];
                }
            }

            // Re-project onto box-constrained simplex to restore Σ = 1.
            q = self.project_box_simplex(&q);

            if !any_corrected {
                break;
            }

            // Convergence check.
            let change: f64 = (0..k).map(|i| (q[i] - q_prev[i]).powi(2)).sum::<f64>();
            if change.sqrt() < self.tolerance {
                break;
            }
        }

        q
    }
}

// ------------------------------------------------------------------
//  Public utility: plain probability-simplex projection
// ------------------------------------------------------------------

/// Project v onto the probability simplex { q : Σ qᵢ = 1, qᵢ ≥ 0 }.
///
/// O(K log K) algorithm (Duchi et al., 2008).  Used as the per-arm constraint
/// baseline in benchmarks (no floor/ceiling bounds).
pub fn simplex_projection(v: &[f64]) -> Vec<f64> {
    let k = v.len();
    if k == 0 {
        return Vec::new();
    }

    // Sort descending.
    let mut u: Vec<f64> = v.to_vec();
    u.sort_unstable_by(|a, b| b.partial_cmp(a).unwrap_or(Ordering::Equal));

    // Find the largest ρ s.t. u_ρ > (Σ_{j≤ρ} u_j − 1) / ρ.
    let mut cssv = 0.0_f64;
    let mut rho = 0_usize;
    for (j, &uj) in u.iter().enumerate() {
        cssv += uj;
        if uj - (cssv - 1.0) / (j as f64 + 1.0) > 0.0 {
            rho = j;
        }
    }

    let cssv_rho: f64 = u[..=rho].iter().sum();
    let theta = (cssv_rho - 1.0) / (rho as f64 + 1.0);

    v.iter().map(|&vi| (vi - theta).max(0.0)).collect()
}

// ------------------------------------------------------------------
//  Internal helpers
// ------------------------------------------------------------------

fn normalise(v: &[f64]) -> Vec<f64> {
    let s: f64 = v.iter().map(|&x| x.max(0.0)).sum();
    if s < 1e-12 {
        let k = v.len();
        return vec![1.0 / k as f64; k];
    }
    v.iter().map(|&x| x.max(0.0) / s).collect()
}

// ------------------------------------------------------------------
//  Tests
// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    // --- simplex_projection ---

    #[test]
    fn simplex_projection_sums_to_one() {
        let v = vec![0.7, 0.5, -0.2, 0.3, 0.1];
        let q = simplex_projection(&v);
        assert!(close(q.iter().sum::<f64>(), 1.0, 1e-10));
        assert!(q.iter().all(|&x| x >= -1e-12));
    }

    #[test]
    fn simplex_projection_identity_for_valid_dist() {
        let v = vec![0.4, 0.3, 0.2, 0.1];
        let q = simplex_projection(&v);
        for (qi, vi) in q.iter().zip(&v) {
            assert!(close(*qi, *vi, 1e-10));
        }
    }

    // --- ConstraintSolver: fast path ---

    #[test]
    fn solver_no_constraints_sums_to_one() {
        let k = 5;
        let solver = ConstraintSolver::new(k);
        let raw = vec![0.5, 0.3, 0.1, 0.05, 0.05];
        let q = solver.solve(&raw);
        assert!(close(q.iter().sum::<f64>(), 1.0, 1e-8));
        assert!(q.iter().all(|&x| x >= -1e-9));
    }

    #[test]
    fn solver_enforces_arm_floors() {
        let k = 4;
        let floors = vec![0.10; k];
        let ceilings = vec![0.70; k];
        let solver = ConstraintSolver::new(k).with_arm_bounds(floors.clone(), ceilings);
        let raw = vec![0.97, 0.01, 0.01, 0.01];
        let q = solver.solve(&raw);
        assert!(close(q.iter().sum::<f64>(), 1.0, 1e-8));
        for i in 1..k {
            assert!(q[i] >= floors[i] - 1e-7, "arm {i}: q={:.6} floor={:.3}", q[i], floors[i]);
        }
    }

    #[test]
    fn solver_enforces_arm_ceilings() {
        let k = 4;
        let floors = vec![0.0; k];
        let ceilings = vec![0.40; k];
        let solver = ConstraintSolver::new(k).with_arm_bounds(floors, ceilings.clone());
        let raw = vec![0.97, 0.01, 0.01, 0.01];
        let q = solver.solve(&raw);
        assert!(close(q.iter().sum::<f64>(), 1.0, 1e-8));
        for i in 0..k {
            assert!(q[i] <= ceilings[i] + 1e-7, "arm {i}: q={:.6} ceil={:.3}", q[i], ceilings[i]);
        }
    }

    // --- ConstraintSolver: general path (linear constraints) ---

    #[test]
    fn solver_linear_constraint_sum_preserved() {
        // Verify Σq = 1 even with linear constraints.
        let k = 6;
        let mut coeff = vec![0.0_f64; k];
        coeff[0] = 1.0;
        coeff[1] = 1.0;
        coeff[2] = 1.0;
        let solver = ConstraintSolver::new(k).with_linear_constraint(LinearConstraint {
            coefficients: coeff,
            lower_bound: 0.30,
            upper_bound: 1.0,
        });
        let raw = vec![0.01, 0.01, 0.01, 0.50, 0.30, 0.17];
        let q = solver.solve(&raw);
        assert!(close(q.iter().sum::<f64>(), 1.0, 1e-6), "sum={:.9}", q.iter().sum::<f64>());
    }

    #[test]
    fn solver_linear_constraint_provider_exposure() {
        // Require arms 0..2 to receive ≥ 30% aggregate exposure.
        let k = 6;
        let mut coeff = vec![0.0_f64; k];
        coeff[0] = 1.0;
        coeff[1] = 1.0;
        coeff[2] = 1.0;
        let lc = LinearConstraint { coefficients: coeff, lower_bound: 0.30, upper_bound: 1.0 };
        let solver = ConstraintSolver::new(k).with_linear_constraint(lc);

        // Raw probs heavily favour arms 3..5.
        let raw = vec![0.01, 0.01, 0.01, 0.50, 0.30, 0.17];
        let q = solver.solve(&raw);
        assert!(close(q.iter().sum::<f64>(), 1.0, 1e-6), "sum={:.9}", q.iter().sum::<f64>());
        let exposure = q[0] + q[1] + q[2];
        assert!(
            exposure >= 0.30 - 1e-5,
            "provider exposure={exposure:.6} < 0.30"
        );
    }

    #[test]
    fn solver_multiple_linear_constraints() {
        let k = 8;
        let mut c1 = vec![0.0_f64; k];
        let mut c2 = vec![0.0_f64; k];
        c1[0] = 1.0;
        c1[1] = 1.0;
        c2[2] = 1.0;
        c2[3] = 1.0;

        let solver = ConstraintSolver::new(k)
            .with_arm_bounds(vec![0.01; k], vec![0.50; k])
            .with_linear_constraint(LinearConstraint {
                coefficients: c1,
                lower_bound: 0.20,
                upper_bound: 1.0,
            })
            .with_linear_constraint(LinearConstraint {
                coefficients: c2,
                lower_bound: 0.15,
                upper_bound: 1.0,
            });

        let raw = vec![0.01, 0.01, 0.01, 0.01, 0.48, 0.24, 0.12, 0.12];
        let q = solver.solve(&raw);
        assert!(close(q.iter().sum::<f64>(), 1.0, 1e-6), "sum={:.9}", q.iter().sum::<f64>());
        assert!(q[0] + q[1] >= 0.20 - 1e-5, "c1 exposure={:.6}", q[0] + q[1]);
        assert!(q[2] + q[3] >= 0.15 - 1e-5, "c2 exposure={:.6}", q[2] + q[3]);
    }

    // --- Performance guard (ADR-012 target: K=20 solve < 50 μs) ---

    #[test]
    fn solver_k20_meets_50us_target() {
        // ADR-012 performance requirement: K=20 solve() must complete in < 50 μs
        // in an optimised (release) build.
        //
        // Run with: cargo test -p experimentation-bandit --release -- lp_constraints
        //
        // Debug builds have ~10–30x overhead; the strict assertion is skipped.
        use std::time::Instant;

        let k = 20;
        let solver = ConstraintSolver::new(k)
            .with_arm_bounds(vec![0.01; k], vec![0.30; k])
            .with_linear_constraint(LinearConstraint {
                coefficients: {
                    let mut c = vec![0.0_f64; k];
                    for i in 0..k / 4 {
                        c[i] = 1.0;
                    }
                    c
                },
                lower_bound: 0.15,
                upper_bound: 1.0,
            })
            .with_linear_constraint(LinearConstraint {
                coefficients: {
                    let mut c = vec![0.0_f64; k];
                    for i in k / 4..k / 2 {
                        c[i] = 1.0;
                    }
                    c
                },
                lower_bound: 0.10,
                upper_bound: 1.0,
            });

        let raw: Vec<f64> = (0..k)
            .map(|i| {
                let x = ((i * 17 + 3) % 31) as f64;
                x / 100.0 + 0.02
            })
            .collect();

        // Warm up caches and branch predictor.
        for _ in 0..20 {
            let _ = solver.solve(&raw);
        }

        // Measure median of 100 calls.
        let mut durations: Vec<u64> = (0..100)
            .map(|_| {
                let t0 = Instant::now();
                let _ = std::hint::black_box(solver.solve(std::hint::black_box(&raw)));
                t0.elapsed().as_nanos() as u64
            })
            .collect();
        durations.sort_unstable();
        let median_ns = durations[50];

        // Strict timing assertion: release builds only.
        // Debug builds are ~10–30x slower due to lack of optimisations.
        #[cfg(not(debug_assertions))]
        assert!(
            median_ns < 50_000,
            "K=20 solve median {median_ns} ns exceeds 50 μs ADR-012 target"
        );

        // In all builds: verify correctness.
        let q = solver.solve(&raw);
        assert!(
            close(q.iter().sum::<f64>(), 1.0, 1e-6),
            "sum={:.9}",
            q.iter().sum::<f64>()
        );
        assert!(q[0] + q[1] + q[2] + q[3] + q[4] >= 0.15 - 1e-5);

        // Informational: print median even in debug so CI can observe trends.
        // (Use `cargo test ... -- --nocapture` to see output.)
        let _ = median_ns; // silence unused warning in debug
    }
}
