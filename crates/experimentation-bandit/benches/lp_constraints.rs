//! Criterion benchmarks for the LP constraint solver (ADR-012).
//!
//! Performance target: ConstraintSolver::solve() < 50 μs for K ≤ 20 arms.
//!
//! Run: cargo bench -p experimentation-bandit -- lp_constraints

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use experimentation_bandit::lp_constraints::{
    ConstraintSolver, LinearConstraint, simplex_projection,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Deterministic pseudo-random probabilities for arm count k (unnormalised).
fn make_raw_probs(k: usize) -> Vec<f64> {
    (0..k)
        .map(|i| {
            let x = ((i * 31 + 7) % 53) as f64;
            x / 100.0 + 0.02
        })
        .collect()
}

/// Build a ConstraintSolver with per-arm bounds and `n_linear` global linear constraints.
///
/// Per-arm floors: 1/(4K), ceilings: 0.40 (ensures feasibility for K ≥ 3).
/// Linear constraints: first quarter of arms must hold ≥ 0.15 total exposure.
fn make_solver(k: usize, n_linear: usize) -> ConstraintSolver {
    let floor = 1.0 / (4.0 * k as f64);
    let floors = vec![floor; k];
    let ceilings = vec![0.40_f64; k];

    let mut solver = ConstraintSolver::new(k).with_arm_bounds(floors, ceilings);

    // Add independent block-style linear constraints (provider exposure groups).
    let block = (k / 4).max(1);
    for j in 0..n_linear {
        let mut coeff = vec![0.0_f64; k];
        let start = (j * block) % k;
        for i in start..(start + block).min(k) {
            coeff[i] = 1.0;
        }
        solver = solver.with_linear_constraint(LinearConstraint {
            coefficients: coeff,
            lower_bound: 0.10,
            upper_bound: 1.0,
        });
    }

    solver
}

// ---------------------------------------------------------------------------
// Benchmarks: ConstraintSolver::solve() for K = 5, 10, 20, 50
// ---------------------------------------------------------------------------

fn bench_solve_box_only(c: &mut Criterion) {
    let mut group = c.benchmark_group("lp_constraints/solve_box_only");

    for &k in &[5usize, 10, 20, 50] {
        let solver = make_solver(k, 0); // no linear constraints → fast path
        let raw = make_raw_probs(k);

        group.bench_with_input(BenchmarkId::new("K", k), &k, |b, _| {
            b.iter(|| solver.solve(black_box(&raw)))
        });
    }

    group.finish();
}

fn bench_solve_with_linear_constraints(c: &mut Criterion) {
    let mut group = c.benchmark_group("lp_constraints/solve_linear");

    for &k in &[5usize, 10, 20, 50] {
        // Two general linear constraints — realistic for provider exposure.
        let solver = make_solver(k, 2);
        let raw = make_raw_probs(k);

        group.bench_with_input(BenchmarkId::new("K", k), &k, |b, _| {
            b.iter(|| solver.solve(black_box(&raw)))
        });
    }

    group.finish();
}

fn bench_solve_many_linear_constraints(c: &mut Criterion) {
    let mut group = c.benchmark_group("lp_constraints/solve_5_linear");

    for &k in &[5usize, 10, 20, 50] {
        // Five linear constraints — stress test for Dykstra iterations.
        let n_linear = 5.min(k);
        let solver = make_solver(k, n_linear);
        let raw = make_raw_probs(k);

        group.bench_with_input(BenchmarkId::new("K", k), &k, |b, _| {
            b.iter(|| solver.solve(black_box(&raw)))
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmarks: simplex_projection (per-arm constraint baseline)
// ---------------------------------------------------------------------------

fn bench_simplex_projection(c: &mut Criterion) {
    let mut group = c.benchmark_group("lp_constraints/simplex_projection");

    for &k in &[5usize, 10, 20, 50] {
        let v = make_raw_probs(k);

        group.bench_with_input(BenchmarkId::new("K", k), &k, |b, _| {
            b.iter(|| simplex_projection(black_box(&v)))
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmarks: project_box_simplex (box-constrained simplex — bisection path)
// ---------------------------------------------------------------------------

fn bench_project_box_simplex(c: &mut Criterion) {
    let mut group = c.benchmark_group("lp_constraints/project_box_simplex");

    for &k in &[5usize, 10, 20, 50] {
        let solver = make_solver(k, 0);
        let v = make_raw_probs(k);
        // Normalise so we're benchmarking projection, not normalisation noise.
        let s: f64 = v.iter().sum();
        let p: Vec<f64> = v.iter().map(|&x| x / s).collect();

        group.bench_with_input(BenchmarkId::new("K", k), &k, |b, _| {
            b.iter(|| solver.project_box_simplex(black_box(&p)))
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
criterion_group!(
    benches,
    bench_solve_box_only,
    bench_solve_with_linear_constraints,
    bench_solve_many_linear_constraints,
    bench_simplex_projection,
    bench_project_box_simplex,
);
criterion_main!(benches);
